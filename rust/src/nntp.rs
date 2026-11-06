//! NntpConnection's protocol (NntpConnection.cpp): a request with the
//! re-login a 480 answer asks for, the AUTHINFO USER/PASS exchange, the
//! greeting on connect and changing the group, as the C++ did them. The
//! connection's I/O, its error reporting and the line buffer stay C++:
//! answers are the C++ line buffer, which the exchange cuts at its last CR
//! for error messages, as before.

use std::ffi::{c_char, CStr};

/// A C++ callback failed (it threw): stop, the C++ wrapper rethrows.
#[derive(Debug, PartialEq)]
pub struct Abort;

/// The connection, as the exchange uses it.
///
/// # Safety
/// A non-null `read_line` result must point to writable, NUL-terminated storage
/// valid until the next read and through the return to the caller. Only
/// `read_line` may overwrite it; callbacks must not reenter the exchange or
/// retain borrowed strings. A connection must have only one exchange at a time.
pub unsafe trait Io {
    fn write_line(&mut self, line: &CStr) -> Result<(), Abort>;
    /// The next line in the C++ line buffer (NUL-terminated), or null.
    fn read_line(&mut self) -> Result<*mut c_char, Abort>;
    /// Connection::ReportError(prefix, arg, false, 0)
    fn report_error(&mut self, prefix: &CStr, arg: &CStr) -> Result<(), Abort>;
    fn debug(&mut self, msg: &CStr) -> Result<(), Abort>;
    /// Whether debug messages are kept (a Release build of the C++ drops them,
    /// so they aren't even built).
    fn debug_enabled(&self) -> bool {
        true
    }
    /// GetStatus() == csCancelled
    fn cancelled(&mut self) -> Result<bool, Abort>;
    fn user(&self) -> &CStr;
    fn password(&self) -> &CStr;
    /// the news server's name and host, and the connection's host
    fn name(&self) -> &CStr;
    fn host(&self) -> &CStr;
    fn conn_host(&self) -> &CStr;
}

/// The login state NntpConnection keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Auth {
    pub error: bool,
    pub rejected: bool,
}

/// BString<1024>'s capacity: 1023 bytes and the NUL.
const BSTRING_MAX: usize = 1023;

/// Appends up to the BString capacity (never copying more of an input).
fn push_capped(out: &mut Vec<u8>, part: &[u8]) {
    let room = BSTRING_MAX.saturating_sub(out.len());
    out.extend_from_slice(&part[..part.len().min(room)]);
}

/// BString<1024> on the stack: printf's output cut to 1023 bytes, as a C
/// string. Built incrementally: an input (a group name from an NZB) is never
/// copied whole.
struct BStr {
    buf: [u8; BSTRING_MAX + 1],
    len: usize,
}

impl std::ops::Deref for BStr {
    type Target = CStr;
    fn deref(&self) -> &CStr {
        // the parts are C strings (no NUL inside) and a NUL follows
        CStr::from_bytes_with_nul(&self.buf[..=self.len]).expect("no NUL inside")
    }
}

fn bstring(parts: &[&[u8]]) -> BStr {
    let mut b = BStr { buf: [0; BSTRING_MAX + 1], len: 0 };
    for part in parts {
        let n = part.len().min(BSTRING_MAX - b.len);
        b.buf[b.len..b.len + n].copy_from_slice(&part[..n]);
        b.len += n;
    }
    b
}

/// debug(...): built and passed only when the C++ keeps debug messages.
macro_rules! debug {
    ($io:expr, $parts:expr) => {
        if $io.debug_enabled() {
            let msg = bstring($parts);
            $io.debug(&msg)?;
        }
    };
}

/// A message format with "%s" fields, filled as printf would (a null
/// argument printing as "(null)", extra arguments ignored).
/// The output stops at the BString capacity (it is only used through one).
fn format(fmt: &[u8], args: &[Option<&[u8]>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(BSTRING_MAX + 1);
    let mut args = args.iter();
    let mut i = 0;
    while i < fmt.len() && out.len() < BSTRING_MAX {
        if fmt[i] == b'%' && fmt.get(i + 1) == Some(&b's') {
            push_capped(&mut out, args.next().copied().flatten().unwrap_or(b"(null)"));
            i += 2;
        } else {
            out.push(fmt[i]);
            i += 1;
        }
    }
    out
}

unsafe fn answer_bytes<'a>(answer: *mut c_char) -> &'a [u8] {
    // the C++ line buffer: NUL-terminated
    unsafe { CStr::from_ptr(answer) }.to_bytes()
}

/// ReportErrorAnswer
fn report_answer(io: &mut dyn Io, prefix: &[u8], answer: Option<&[u8]>) -> Result<(), Abort> {
    let text = format(prefix, &[Some(io.name().to_bytes()), Some(io.host().to_bytes()), answer]);
    let msg = bstring(&[&text]);
    io.report_error(c"%s", &msg)
}

/// Cuts the answer at its last CR (for the message), notes a 502 refusal
/// and reports it unless the connection was cancelled.
fn failed_answer(io: &mut dyn Io, auth: &mut Auth, answer: *mut c_char) -> Result<bool, Abort> {
    let bytes = unsafe { answer_bytes(answer) };
    if let Some(cr) = bytes.iter().rposition(|&b| b == b'\r') {
        // inside the C++ buffer, before its NUL
        unsafe { *answer.add(cr) = 0 };
    }
    let bytes = unsafe { answer_bytes(answer) };
    auth.rejected = bytes.starts_with(b"502");
    if !io.cancelled()? {
        report_answer(io, b"Authorization for %s (%s) failed: %s", Some(bytes))?;
    }
    Ok(false)
}

fn auth_info_pass(io: &mut dyn Io, auth: &mut Auth, mut recur: i32) -> Result<bool, Abort> {
    loop {
        if recur > 10 {
            return Ok(false);
        }
        let line = bstring(&[b"AUTHINFO PASS ", io.password().to_bytes(), b"\r\n"]);
        io.write_line(&line)?;
        let answer = io.read_line()?;
        if answer.is_null() {
            report_answer(io, b"Authorization failed for %s (%s): Connection closed by remote host", None)?;
            return Ok(false);
        }
        let bytes = unsafe { answer_bytes(answer) };
        if bytes.starts_with(b"2") {
            debug!(io, &[b"Authorization for ", io.conn_host().to_bytes(), b" successful"]);
            return Ok(true);
        }
        if bytes.starts_with(b"381") {
            recur += 1;
            continue;
        }
        return failed_answer(io, auth, answer);
    }
}

fn auth_info_user(io: &mut dyn Io, auth: &mut Auth, mut recur: i32) -> Result<bool, Abort> {
    loop {
        if recur > 10 {
            return Ok(false);
        }
        let line = bstring(&[b"AUTHINFO USER ", io.user().to_bytes(), b"\r\n"]);
        io.write_line(&line)?;
        let answer = io.read_line()?;
        if answer.is_null() {
            report_answer(io, b"Authorization for %s (%s) failed: Connection closed by remote host", None)?;
            return Ok(false);
        }
        let bytes = unsafe { answer_bytes(answer) };
        if bytes.starts_with(b"281") {
            debug!(io, &[b"Authorization for ", io.conn_host().to_bytes(), b" successful"]);
            return Ok(true);
        }
        if bytes.starts_with(b"381") {
            recur += 1;
            return auth_info_pass(io, auth, recur);
        }
        if bytes.starts_with(b"480") {
            recur += 1;
            continue;
        }
        return failed_answer(io, auth, answer);
    }
}

/// NntpConnection::Authenticate
pub fn authenticate(io: &mut dyn Io, auth: &mut Auth) -> Result<bool, Abort> {
    if io.user().is_empty() || io.password().is_empty() {
        let host = io.host().to_owned();
        io.report_error(
            c"Could not connect to %s: server requested authorization but username/password are not set in settings",
            &host,
        )?;
        auth.error = true;
        return Ok(false);
    }
    auth.error = !auth_info_user(io, auth, 0)?;
    Ok(!auth.error)
}

/// NntpConnection::Request: the answer (the C++ line buffer) or null.
///
/// # Safety
/// `req` must remain a valid NUL-terminated string throughout the exchange.
/// It may point into the line buffer, which reads can overwrite.
pub unsafe fn request(io: &mut dyn Io, auth: &mut Auth, req: *const c_char) -> Result<*mut c_char, Abort> {
    *auth = Auth::default();
    // Borrow only during the write: a read may change both contents and length.
    io.write_line(unsafe { CStr::from_ptr(req) })?;
    let answer = io.read_line()?;
    if answer.is_null() {
        return Ok(answer);
    }
    if unsafe { answer_bytes(answer) }.starts_with(b"480") {
        debug!(io, &[io.conn_host().to_bytes(), b" requested authorization"]);
        if !authenticate(io, auth)? {
            return Ok(std::ptr::null_mut());
        }
        // try again
        // Borrow only during the write: a read may change both contents and length.
        io.write_line(unsafe { CStr::from_ptr(req) })?;
        return io.read_line();
    }
    Ok(answer)
}

/// NntpConnection::Connect after the socket connected: 0 connected, 1 the
/// greeting failed (the C++ disconnects), 2 the login failed.
pub fn handshake(io: &mut dyn Io, auth: &mut Auth) -> Result<i32, Abort> {
    let answer = io.read_line()?;
    if answer.is_null() {
        report_answer(io, b"Connection to %s (%s) failed: Connection closed by remote host", None)?;
        return Ok(1);
    }
    let bytes = unsafe { answer_bytes(answer) };
    if !bytes.starts_with(b"2") {
        report_answer(io, b"Connection to %s (%s) failed: %s", Some(bytes))?;
        return Ok(1);
    }
    if !io.user().is_empty() && !io.password().is_empty() && !authenticate(io, auth)? {
        return Ok(2);
    }
    debug!(io, &[b"Connection to ", io.conn_host().to_bytes(), b" established"]);
    Ok(0)
}

/// NntpConnection::JoinGroup when not in `group` yet: the answer, and
/// whether the group changed.
///
/// # Safety
/// `group` must remain a valid NUL-terminated string throughout the exchange.
/// As in C++, it may point into the line buffer and change during the request.
pub unsafe fn join_group(io: &mut dyn Io, auth: &mut Auth, group: *const c_char) -> Result<(*mut c_char, bool), Abort> {
    let req = bstring(&[b"GROUP ", unsafe { CStr::from_ptr(group) }.to_bytes(), b"\r\n"]);
    let answer = unsafe { request(io, auth, req.as_ptr()) }?;
    if !answer.is_null() && unsafe { answer_bytes(answer) }.starts_with(b"2") {
        debug!(io, &[b"Changed group to ", unsafe { CStr::from_ptr(group) }.to_bytes(), b" on ", io.conn_host().to_bytes()]);
        return Ok((answer, true));
    }
    let text = if answer.is_null() { &b"(null)"[..] } else { unsafe { answer_bytes(answer) } };
    debug!(io, &[b"Error changing group on ", io.conn_host().to_bytes(), b" to ", unsafe { CStr::from_ptr(group) }.to_bytes(), b": ", text, b"."]);
    Ok((answer, false))
}

// ============================================================================
// ArticleDownloader (`daemon/nntp/ArticleDownloader.cpp`)
// ============================================================================

/// Evaluates level exhaustion in `ArticleDownloader::Run`.
/// Returns `(advance_level, new_level, break_all_failed)`.
pub fn article_dl_eval_level_exhaustion(
    all_failed: bool,
    level: i32,
    max_norm_level: i32,
) -> (bool, i32, bool) {
    if !all_failed {
        return (false, level, false);
    }
    if level < max_norm_level {
        (true, level.saturating_add(1), false)
    } else {
        (false, level, true)
    }
}

/// Evaluates content-rejected handling in `ArticleDownloader::Run`.
/// Returns `(handled, should_break, next_status, next_content_rejected, next_remained_retries)`.
#[inline]
pub fn article_dl_handle_content_rejected(
    content_rejected: i32,
    dupe_fallback_round: i32,
    cur_status: i32,
    ad_failed: i32,
    remained_retries: i32,
) -> (bool, bool, i32, bool, i32) {
    if content_rejected != 0 {
        if dupe_fallback_round > 0 {
            (true, true, ad_failed, true, remained_retries)
        } else {
            (true, false, ad_failed, false, 0)
        }
    } else {
        (false, false, cur_status, false, remained_retries)
    }
}

/// Classifies a write failure in `ArticleDownloader::Download` into
/// `(status, content_rejected)`.
#[inline]
pub fn article_dl_classify_write_failure(
    local_write_error: i32,
    ad_fatal_error: i32,
    ad_failed: i32,
) -> (i32, bool) {
    if local_write_error != 0 {
        (ad_fatal_error, false)
    } else {
        (ad_failed, true)
    }
}

/// Matches the server retention check in `ArticleDownloader::Run`:
/// returns `(is_failure, age_days)`.
pub fn article_dl_is_retention_failure(
    retention_days: i32,
    file_time: i64,
    cur_time: i64,
) -> (bool, i32) {
    if retention_days <= 0 || file_time <= 0 {
        return (false, 0);
    }
    let age_days = (cur_time.wrapping_sub(file_time) / 86400) as i32;
    (age_days > retention_days, age_days)
}

/// Matches the `allServersOnLevelFailed` loop in `ArticleDownloader::Run`.
pub fn article_dl_all_servers_on_level_failed(
    level: i32,
    cand_ids: &[i32],
    cand_levels: &[i32],
    cand_groups: &[i32],
    cand_avail: &[u8],
    failed_ids: &[i32],
    failed_levels: &[i32],
    failed_groups: &[i32],
) -> bool {
    let n = cand_ids
        .len()
        .min(cand_levels.len())
        .min(cand_groups.len())
        .min(cand_avail.len());
    let f = failed_ids
        .len()
        .min(failed_levels.len())
        .min(failed_groups.len());

    for i in 0..n {
        if cand_levels[i] != level {
            continue;
        }
        if cand_avail[i] == 0 {
            continue;
        }
        let cid = cand_ids[i];
        let clvl = cand_levels[i];
        let cgrp = cand_groups[i];
        let mut server_failed = false;
        for j in 0..f {
            if failed_ids[j] == cid
                || (failed_groups[j] > 0 && failed_groups[j] == cgrp && failed_levels[j] == clvl)
            {
                server_failed = true;
                break;
            }
        }
        if !server_failed {
            return false;
        }
    }
    true
}

/// Matches the post-loop status normalization in `ArticleDownloader::Run`.
pub fn article_dl_resolve_final_status(
    mut status: i32,
    is_duplicate: bool,
    is_stopped: bool,
    ad_finished: i32,
    ad_failed: i32,
    ad_retry: i32,
    ad_fatal_error: i32,
) -> i32 {
    if is_duplicate {
        status = ad_finished;
    }
    if status != ad_finished && status != ad_retry && status != ad_fatal_error {
        status = ad_failed;
    }
    if is_stopped {
        status = ad_retry;
    }
    status
}

/// Matches `ArticleDownloader::DecodeCheck()`.
pub fn article_dl_classify_decode_status(
    raw_article: bool,
    decoder_status: i32,
    is_yenc: bool,
    range_exceeded: bool,
) -> i32 {
    if raw_article {
        return 0;
    }
    if decoder_status == 1 && is_yenc && range_exceeded {
        return 2;
    }
    match decoder_status {
        1 => 1,
        3 => 3,
        2 => 4,
        4 => 5,
        5 => 6,
        _ => 7,
    }
}

/// Evaluates `adConnectError` normalization and `remainedRetries` decrement in `ArticleDownloader::Run`.
pub fn article_dl_normalize_connect_and_decrement_retries(
    connected: bool,
    status: i32,
    ad_connect_error: i32,
    ad_failed: i32,
    remained_retries: i32,
) -> (bool, i32, i32) {
    let (new_connected, new_status) = if status == ad_connect_error {
        (false, ad_failed)
    } else {
        (connected, status)
    };
    let new_retries = if new_connected && new_status == ad_failed {
        remained_retries.wrapping_sub(1)
    } else {
        remained_retries
    };
    (new_connected, new_status, new_retries)
}

/// `if (response && !strncmp(response, "2", 1))`
pub fn article_dl_is_join_group_ok(response: Option<&[u8]>) -> bool {
    matches!(response, Some(r) if r.first() == Some(&b'2'))
}

/// Classifies an article download attempt status into `(record, success, failed)` server stats.
#[inline]
pub fn article_dl_classify_attempt_stat(
    status: i32,
    ad_finished: i32,
    ad_failed: i32,
    ad_not_found: i32,
    ad_crc_error: i32,
) -> (bool, i32, i32) {
    let record = status == ad_finished
        || status == ad_failed
        || status == ad_not_found
        || status == ad_crc_error;
    if !record {
        (false, 0, 0)
    } else if status == ad_finished {
        (true, 1, 0)
    } else {
        (true, 0, 1)
    }
}

/// Determines whether `ArticleDownloader::Run` should advance through the server list and whether `lastServer` should be added to `failedServers`.
#[inline]
pub fn article_dl_should_advance_server_list(
    has_want_server: bool,
    connected: bool,
    retention_failure: bool,
    optional_blocked: bool,
    auth_rejected: bool,
) -> (bool, bool) {
    let advance =
        !has_want_server && (connected || retention_failure || optional_blocked || auth_rejected);
    (advance, advance && !optional_blocked)
}

/// `ServerPool::BlockServer`:
/// Determines whether the block timestamp changed (`new_block`) and whether to log a warning
/// (`should_warn = new_block && retry_interval > 0`).
#[inline]
pub fn server_pool_eval_block_server(
    prev_block_time: i64,
    cur_time: i64,
    retry_interval: i32,
) -> (bool, bool) {
    let new_block = prev_block_time != cur_time;
    let should_warn = new_block && retry_interval > 0;
    (new_block, should_warn)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Script {
        answers: Vec<Option<Vec<u8>>>,
        buf: Vec<u8>,
        written: Vec<Vec<u8>>,
        errors: Vec<Vec<u8>>,
    }

    unsafe impl Io for Script {
        fn write_line(&mut self, line: &CStr) -> Result<(), Abort> {
            self.written.push(line.to_bytes().to_vec());
            Ok(())
        }
        fn read_line(&mut self) -> Result<*mut c_char, Abort> {
            match self.answers.remove(0) {
                None => Ok(std::ptr::null_mut()),
                Some(a) => {
                    self.buf = a;
                    self.buf.push(0);
                    Ok(self.buf.as_mut_ptr().cast())
                }
            }
        }
        fn report_error(&mut self, prefix: &CStr, arg: &CStr) -> Result<(), Abort> {
            self.errors.push([prefix.to_bytes(), b"|", arg.to_bytes()].concat());
            Ok(())
        }
        fn debug(&mut self, _: &CStr) -> Result<(), Abort> {
            Ok(())
        }
        fn cancelled(&mut self) -> Result<bool, Abort> {
            Ok(false)
        }
        fn user(&self) -> &CStr {
            c"me"
        }
        fn password(&self) -> &CStr {
            c"pw"
        }
        fn name(&self) -> &CStr {
            c"news"
        }
        fn host(&self) -> &CStr {
            c"news.example"
        }
        fn conn_host(&self) -> &CStr {
            c"news.example"
        }
    }

    fn script(answers: &[Option<&[u8]>]) -> Script {
        Script { answers: answers.iter().map(|a| a.map(<[u8]>::to_vec)).collect(), buf: Vec::new(), written: Vec::new(), errors: Vec::new() }
    }

    #[test]
    fn capped_strings() {
        let long = vec![b'g'; 1 << 20];
        let s = bstring(&[b"GROUP ", &long, b"\r\n"]);
        assert_eq!(s.to_bytes().len(), BSTRING_MAX);
        assert!(s.to_bytes().starts_with(b"GROUP ggg"));
        assert_eq!(format(b"%s (%s)", &[Some(&long), None]).len(), BSTRING_MAX);
        assert_eq!(format(b"%s (%s): %s", &[Some(b"a"), None, Some(b"c")]), b"a ((null)): c");
    }

    #[test]
    fn relogin() {
        let mut io = script(&[Some(b"480 auth"), Some(b"381 pass"), Some(b"281 ok"), Some(b"222 body")]);
        let mut auth = Auth::default();
        let a = unsafe { request(&mut io, &mut auth, c"BODY <x>\r\n".as_ptr()) }.unwrap();
        assert_eq!(unsafe { answer_bytes(a) }, b"222 body");
        assert_eq!(io.written, [b"BODY <x>\r\n".to_vec(), b"AUTHINFO USER me\r\n".to_vec(), b"AUTHINFO PASS pw\r\n".to_vec(), b"BODY <x>\r\n".to_vec()]);
    }

    #[test]
    fn rejected() {
        let mut io = script(&[Some(b"200 hi"), Some(b"502 no\r\n")]);
        let mut auth = Auth::default();
        assert_eq!(handshake(&mut io, &mut auth), Ok(2));
        assert!(auth.error && auth.rejected);
        assert_eq!(io.errors, [b"%s|Authorization for news (news.example) failed: 502 no".to_vec()]);
    }
}
