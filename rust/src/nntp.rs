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
    /// the connection closed or timed out during the login (no answer at all)
    pub dropped: bool,
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
    let mut b = BStr {
        buf: [0; BSTRING_MAX + 1],
        len: 0,
    };
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
            push_capped(
                &mut out,
                args.next().copied().flatten().unwrap_or(b"(null)"),
            );
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
    let text = format(
        prefix,
        &[
            Some(io.name().to_bytes()),
            Some(io.host().to_bytes()),
            answer,
        ],
    );
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
            report_answer(
                io,
                b"Authorization failed for %s (%s): Connection closed by remote host",
                None,
            )?;
            auth.dropped = true;
            return Ok(false);
        }
        let bytes = unsafe { answer_bytes(answer) };
        if bytes.starts_with(b"2") {
            debug!(
                io,
                &[
                    b"Authorization for ",
                    io.conn_host().to_bytes(),
                    b" successful"
                ]
            );
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
            report_answer(
                io,
                b"Authorization for %s (%s) failed: Connection closed by remote host",
                None,
            )?;
            auth.dropped = true;
            return Ok(false);
        }
        let bytes = unsafe { answer_bytes(answer) };
        if bytes.starts_with(b"281") {
            debug!(
                io,
                &[
                    b"Authorization for ",
                    io.conn_host().to_bytes(),
                    b" successful"
                ]
            );
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
pub unsafe fn request(
    io: &mut dyn Io,
    auth: &mut Auth,
    req: *const c_char,
) -> Result<*mut c_char, Abort> {
    *auth = Auth::default();
    // Borrow only during the write: a read may change both contents and length.
    io.write_line(unsafe { CStr::from_ptr(req) })?;
    let answer = io.read_line()?;
    if answer.is_null() {
        return Ok(answer);
    }
    if unsafe { answer_bytes(answer) }.starts_with(b"480") {
        debug!(
            io,
            &[io.conn_host().to_bytes(), b" requested authorization"]
        );
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
        report_answer(
            io,
            b"Connection to %s (%s) failed: Connection closed by remote host",
            None,
        )?;
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
    debug!(
        io,
        &[
            b"Connection to ",
            io.conn_host().to_bytes(),
            b" established"
        ]
    );
    Ok(0)
}

/// NntpConnection::JoinGroup when not in `group` yet: the answer, and
/// whether the group changed.
///
/// # Safety
/// `group` must remain a valid NUL-terminated string throughout the exchange.
/// As in C++, it may point into the line buffer and change during the request.
pub unsafe fn join_group(
    io: &mut dyn Io,
    auth: &mut Auth,
    group: *const c_char,
) -> Result<(*mut c_char, bool), Abort> {
    let req = bstring(&[
        b"GROUP ",
        unsafe { CStr::from_ptr(group) }.to_bytes(),
        b"\r\n",
    ]);
    let answer = unsafe { request(io, auth, req.as_ptr()) }?;
    if !answer.is_null() && unsafe { answer_bytes(answer) }.starts_with(b"2") {
        debug!(
            io,
            &[
                b"Changed group to ",
                unsafe { CStr::from_ptr(group) }.to_bytes(),
                b" on ",
                io.conn_host().to_bytes()
            ]
        );
        return Ok((answer, true));
    }
    let text = if answer.is_null() {
        &b"(null)"[..]
    } else {
        unsafe { answer_bytes(answer) }
    };
    debug!(
        io,
        &[
            b"Error changing group on ",
            io.conn_host().to_bytes(),
            b" to ",
            unsafe { CStr::from_ptr(group) }.to_bytes(),
            b": ",
            text,
            b"."
        ]
    );
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

/// One server as `article_dl_all_servers_on_level_failed` sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelCandidate {
    pub id: i32,
    pub level: i32,
    pub group: i32,
    pub retention: i32,
    /// active, has connections, and not an optional server that is blocked
    pub available: bool,
}

/// Matches the `allServersOnLevelFailed` loop in `ArticleDownloader::Run`: every
/// available server on `level` either failed this article itself or is on a backbone that answered 430 for it (see [`BackboneMiss`]).
pub fn article_dl_all_servers_on_level_failed(
    level: i32,
    candidates: &[LevelCandidate],
    failed_ids: &[i32],
    missing: &[BackboneMiss],
) -> bool {
    candidates
        .iter()
        .filter(|c| c.level == level && c.available)
        .all(|c| failed_ids.contains(&c.id) || backbone_missing(missing, c.group, c.retention))
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransientServerState {
    server_key: usize,
    attempts: i32,
    retry_time: i64,
}

/// Counts temporary NNTP errors one article met on each server and tracks per-server
/// ordinary retry budgets so temporary errors switching servers do not consume another
/// server's ordinary retry budget.
#[derive(Debug, Clone)]
pub struct TransientRetries {
    attempts_limit: i32,
    interval_sec: i32,
    exhausted: bool,
    servers: Vec<TransientServerState>,
    ordinary_retries: Vec<(usize, i32)>,
}

impl TransientRetries {
    pub const MAX_INTERVAL_SEC: i32 = 60;

    pub fn new(attempts: i32, interval_sec: i32) -> Self {
        Self {
            attempts_limit: attempts,
            interval_sec,
            exhausted: false,
            servers: Vec::new(),
            ordinary_retries: Vec::new(),
        }
    }

    /// Registers a temporary error on `server_key` at `now`.
    /// Returns `true` if the server has an attempt left (and schedules a growing wait
    /// up to `MAX_INTERVAL_SEC`), or `false` if attempts on that server are exhausted.
    pub fn register(&mut self, server_key: usize, now: i64) -> bool {
        let idx = if let Some(pos) = self.servers.iter().position(|s| s.server_key == server_key) {
            pos
        } else {
            self.servers.push(TransientServerState {
                server_key,
                attempts: 0,
                retry_time: 0,
            });
            self.servers.len() - 1
        };

        let state = &mut self.servers[idx];
        state.attempts = state.attempts.saturating_add(1);
        if state.attempts >= self.attempts_limit {
            self.exhausted = true;
            return false;
        }

        let mut interval = self.interval_sec;
        let mut i = 1;
        while i < state.attempts && interval < Self::MAX_INTERVAL_SEC {
            interval = interval.saturating_mul(2);
            i += 1;
        }
        let wait = interval.min(Self::MAX_INTERVAL_SEC) as i64;
        state.retry_time = now.saturating_add(wait);
        true
    }

    pub fn waiting_servers(&self, now: i64) -> Vec<usize> {
        self.servers
            .iter()
            .filter(|s| now < s.retry_time)
            .map(|s| s.server_key)
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    pub fn remained_retries(&self, server_key: usize) -> i32 {
        self.ordinary_retries
            .iter()
            .find(|&&(k, _)| k == server_key)
            .map_or(self.attempts_limit, |&(_, r)| r)
    }

    pub fn set_remained_retries(&mut self, server_key: usize, remained: i32) {
        if let Some(entry) = self
            .ordinary_retries
            .iter_mut()
            .find(|(k, _)| *k == server_key)
        {
            entry.1 = remained;
        } else {
            self.ordinary_retries.push((server_key, remained));
        }
    }
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

/// `ArticleBatchFetcher::ClaimNext`:
/// Checks whether a worker can claim the next request in the batch window.
pub fn article_batch_can_claim_next(
    batch_id: i32,
    cur_batch_id: i32,
    cancelled: bool,
    stopped: bool,
    next_claim: usize,
    requests_len: usize,
    next_deliver: usize,
    window_parts: i32,
    buffered_bytes: i64,
    max_buffered_bytes: i64,
) -> bool {
    batch_id == cur_batch_id
        && !cancelled
        && !stopped
        && next_claim < requests_len
        && next_claim.saturating_sub(next_deliver) < (window_parts.max(0) as usize)
        && buffered_bytes < max_buffered_bytes
}

/// Formats an NNTP message-id in angle brackets (`<id>`) if not already bracketed.
pub fn nntp_health_format_bracketed_id(message_id: &[u8]) -> Vec<u8> {
    if message_id.first() == Some(&b'<') && message_id.last() == Some(&b'>') {
        message_id.to_vec()
    } else {
        let mut out = Vec::with_capacity(message_id.len() + 2);
        out.push(b'<');
        out.extend_from_slice(message_id);
        out.push(b'>');
        out
    }
}

/// Returns true when `ArticleBatchFetcher::StoreResult` should accept and store the fetched result.
pub fn article_batch_can_store_result(
    batch_id: i32,
    current_batch_id: i32,
    cancelled: bool,
) -> bool {
    batch_id == current_batch_id && !cancelled
}

/// Returns true when `ArticleBatchFetcher::Next` can deliver the next slot.
pub fn article_batch_can_deliver_next(
    cancelled: bool,
    stopped: bool,
    next_deliver: usize,
    total_requests: usize,
) -> bool {
    !cancelled && !stopped && next_deliver < total_requests
}

/// Returns true when `ServerPool` has a valid level index with free connection slots (`level >= 0 && (level as usize) < levels_len && level_free_count > 0`).
pub fn server_pool_has_available_level_conns(
    level: i32,
    levels_len: usize,
    level_free_count: i32,
) -> bool {
    level >= 0 && (level as usize) < levels_len && level_free_count > 0
}

/// Computes `rand_val % candidates_len` for `ServerPool::LockedGetConnection`.
pub fn server_pool_pick_candidate_index(rand_val: i32, candidates_len: usize) -> usize {
    if candidates_len == 0 {
        0
    } else {
        (rand_val.unsigned_abs() as usize) % candidates_len
    }
}

/// Returns true when `ServerPool::CloseUnusedConnections` should disconnect a pooled connection on an idle level: it is connected and its server is on that level.
pub const fn server_pool_should_disconnect_idle_level_conn(
    conn_norm_level: i32,
    level: i32,
    connected: bool,
) -> bool {
    conn_norm_level == level && connected
}

/// Returns true when `ArticleBatchFetcher::Begin` should call `EnsureWorkers()` (`worker_count > 0 && !is_stopped`).
pub fn article_batch_should_ensure_workers(worker_count: i32, is_stopped: bool) -> bool {
    worker_count > 0 && !is_stopped
}

/// Returns true when `Network::SpeedTest::RunTest` should count an iteration as a failure (`elapsed_sec == 0.0 || downloaded < target_size`).
pub fn speed_test_is_iteration_failed(elapsed_sec: f64, downloaded: u32, target_size: u32) -> bool {
    elapsed_sec == 0.0 || downloaded < target_size
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
            self.errors
                .push([prefix.to_bytes(), b"|", arg.to_bytes()].concat());
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
        Script {
            answers: answers.iter().map(|a| a.map(<[u8]>::to_vec)).collect(),
            buf: Vec::new(),
            written: Vec::new(),
            errors: Vec::new(),
        }
    }

    #[test]
    fn capped_strings() {
        let long = vec![b'g'; 1 << 20];
        let s = bstring(&[b"GROUP ", &long, b"\r\n"]);
        assert_eq!(s.to_bytes().len(), BSTRING_MAX);
        assert!(s.to_bytes().starts_with(b"GROUP ggg"));
        assert_eq!(format(b"%s (%s)", &[Some(&long), None]).len(), BSTRING_MAX);
        assert_eq!(
            format(b"%s (%s): %s", &[Some(b"a"), None, Some(b"c")]),
            b"a ((null)): c"
        );
    }

    #[test]
    fn relogin() {
        let mut io = script(&[
            Some(b"480 auth"),
            Some(b"381 pass"),
            Some(b"281 ok"),
            Some(b"222 body"),
        ]);
        let mut auth = Auth::default();
        let a = unsafe { request(&mut io, &mut auth, c"BODY <x>\r\n".as_ptr()) }.unwrap();
        assert_eq!(unsafe { answer_bytes(a) }, b"222 body");
        assert_eq!(
            io.written,
            [
                b"BODY <x>\r\n".to_vec(),
                b"AUTHINFO USER me\r\n".to_vec(),
                b"AUTHINFO PASS pw\r\n".to_vec(),
                b"BODY <x>\r\n".to_vec()
            ]
        );
    }

    #[test]
    fn rejected() {
        let mut io = script(&[Some(b"200 hi"), Some(b"502 no\r\n")]);
        let mut auth = Auth::default();
        assert_eq!(handshake(&mut io, &mut auth), Ok(2));
        assert!(auth.error && auth.rejected);
        assert_eq!(
            io.errors,
            [b"%s|Authorization for news (news.example) failed: 502 no".to_vec()]
        );
    }
}

// ============================================================================
// NntpHealthServer, NntpConnection, ServerPool, and Connection Helpers
// ============================================================================

/// Returns whether `ServerPool::GetConnection` can advance past `level` given a server's state.
pub fn server_pool_can_advance_level(
    norm_level: i32,
    level: i32,
    active: bool,
    optional: bool,
    blocked: bool,
) -> bool {
    !(norm_level == level && active && !(optional && blocked))
}

/// Returns whether a candidate server matches `wantServer`. Only that server: another
/// server on its backbone is another provider, with its own account and retention.
pub fn server_pool_match_want_server(has_want: bool, cand_id: i32, want_id: i32) -> bool {
    !has_want || cand_id == want_id
}

/// An article a server answered `430 no such article` for. The other servers on
/// the same backbone read from the same article store, so the ones that keep
/// articles no longer than that server are not asked for it, at any priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackboneMiss {
    /// the backbone id (`group` of the server, 0 = independent)
    pub group: i32,
    /// retention of the server that answered 430, in days (0 = not set)
    pub retention: i32,
}

/// Whether `miss` rules out a server on backbone `cand_group` with retention
/// `cand_retention`. A sibling that may keep articles longer is still asked:
/// both retentions must be set and the sibling's no longer, or both unset.
pub fn backbone_miss_covers(miss: &BackboneMiss, cand_group: i32, cand_retention: i32) -> bool {
    miss.group > 0
        && miss.group == cand_group
        && ((miss.retention <= 0 && cand_retention <= 0)
            || (miss.retention > 0 && cand_retention > 0 && cand_retention <= miss.retention))
}

/// Whether any of `misses` rules out the candidate server.
pub fn backbone_missing(misses: &[BackboneMiss], cand_group: i32, cand_retention: i32) -> bool {
    misses
        .iter()
        .any(|m| backbone_miss_covers(m, cand_group, cand_retention))
}

/// Returns whether a candidate server is one the article already failed on.
/// Only the server itself: a login error, "too many connections" or a timeout
/// on one server says nothing about its backbone siblings.
pub fn server_pool_match_ignore_server(cand_id: i32, ign_id: i32) -> bool {
    cand_id == ign_id
}

// ============================================================================
// ArticleBatchFetcher (`daemon/nntp/ArticleFetcher.cpp`)
// ============================================================================

pub const ARTICLE_BATCH_MAX_WINDOW_PARTS: i32 = 8;
pub const ARTICLE_BATCH_MAX_BUFFERED_BYTES: i64 = 128 * 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub struct BatchRequest {
    pub message_id: Vec<u8>,
    pub groups: std::sync::Arc<Vec<Vec<u8>>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchFetchedArticle {
    pub data: Vec<u8>,
    pub offset: i64,
    pub file_size: i64,
    pub success: bool,
    pub retry: bool,
    pub transient: bool,
}

#[derive(Clone, Debug, Default)]
struct BatchSlot {
    result: BatchFetchedArticle,
    ready: bool,
}

#[derive(Default)]
struct BatchInner {
    requests: Vec<BatchRequest>,
    slots: Vec<BatchSlot>,
    next_claim: usize,
    next_deliver: usize,
    buffered_bytes: i64,
    batch_id: i32,
    cancelled: bool,
    worker_count: i32,
}

impl BatchInner {
    fn window_parts(&self) -> i32 {
        self.worker_count
            .saturating_mul(2)
            .max(ARTICLE_BATCH_MAX_WINDOW_PARTS)
    }

    fn claim_next(&mut self, batch_id: i32, stopped: bool) -> Option<(usize, BatchRequest)> {
        if !article_batch_can_claim_next(
            batch_id,
            self.batch_id,
            self.cancelled,
            stopped,
            self.next_claim,
            self.requests.len(),
            self.next_deliver,
            self.window_parts(),
            self.buffered_bytes,
            ARTICLE_BATCH_MAX_BUFFERED_BYTES,
        ) {
            return None;
        }
        let index = self.next_claim;
        self.next_claim += 1;
        let req = std::mem::take(&mut self.requests[index]);
        Some((index, req))
    }

    fn store_result(&mut self, batch_id: i32, index: usize, result: BatchFetchedArticle) {
        if !article_batch_can_store_result(batch_id, self.batch_id, self.cancelled) {
            return;
        }
        if index < self.slots.len() {
            self.buffered_bytes += result.data.len() as i64;
            self.slots[index].result = result;
            self.slots[index].ready = true;
        }
    }
}

/// Locks `m`, carrying on with the data if a worker panicked while holding it:
/// the batch state stays consistent between statements, and a panic in one
/// fetch must not take down delivery for the rest.
fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub type BatchFetchFn =
    dyn Fn(usize, &BatchRequest) -> Result<BatchFetchedArticle, String> + Send + Sync;

pub struct ArticleBatchFetcher {
    inner: std::sync::Arc<std::sync::Mutex<BatchInner>>,
    stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    workers: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>>,
    fetch_fn: std::sync::Arc<BatchFetchFn>,
    stop_hook: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl ArticleBatchFetcher {
    pub fn new(
        fetch_fn: impl Fn(usize, &BatchRequest) -> Result<BatchFetchedArticle, String>
            + Send
            + Sync
            + 'static,
        stop_hook: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(BatchInner::default())),
            stopped: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            workers: std::sync::Mutex::new(Vec::new()),
            fetch_fn: std::sync::Arc::new(fetch_fn),
            stop_hook,
        }
    }

    pub fn set_worker_count(&self, worker_count: i32) {
        let mut guard = lock(&self.inner);
        guard.worker_count = worker_count;
    }

    /// Raises the worker count of the batch running now: the added workers start on its
    /// unclaimed requests at once instead of waiting for the next batch.
    pub fn raise_worker_count(&self, worker_count: i32) {
        use std::sync::atomic::Ordering;
        self.set_worker_count(worker_count);
        if article_batch_should_ensure_workers(worker_count, self.stopped.load(Ordering::SeqCst)) {
            self.ensure_workers(worker_count);
        }
    }

    pub fn get_window_parts(&self) -> i32 {
        let guard = lock(&self.inner);
        guard.window_parts()
    }

    fn ensure_workers(&self, target_count: i32) {
        use std::sync::atomic::Ordering;
        let mut workers = lock(&self.workers);
        let target = usize::try_from(target_count).unwrap_or(0);
        while workers.len() < target {
            let worker_idx = workers.len();
            let inner = std::sync::Arc::clone(&self.inner);
            let stopped = std::sync::Arc::clone(&self.stopped);
            let fetch_fn = std::sync::Arc::clone(&self.fetch_fn);
            let handle = std::thread::spawn(move || {
                while !stopped.load(Ordering::SeqCst) {
                    // A worker past a lowered count idles: threads are never shrunk, so this
                    // keeps a later batch to the count it was given.
                    let (batch_id, idle) = {
                        let g = lock(&inner);
                        (g.batch_id, worker_idx >= usize::try_from(g.worker_count).unwrap_or(0))
                    };
                    if idle {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        continue;
                    }
                    let claimed = lock(&inner).claim_next(batch_id, stopped.load(Ordering::SeqCst));
                    if let Some((index, req)) = claimed {
                        let fetched = (fetch_fn)(worker_idx, &req).unwrap_or_default();
                        lock(&inner).store_result(batch_id, index, fetched);
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
            });
            workers.push(handle);
        }
    }

    pub fn begin(&self, requests: Vec<BatchRequest>) {
        use std::sync::atomic::Ordering;
        let worker_count = {
            let mut guard = lock(&self.inner);
            guard.batch_id = guard.batch_id.wrapping_add(1);
            guard.slots = vec![BatchSlot::default(); requests.len()];
            guard.requests = requests;
            guard.next_claim = 0;
            guard.next_deliver = 0;
            guard.buffered_bytes = 0;
            guard.cancelled = false;
            guard.worker_count
        };
        if article_batch_should_ensure_workers(worker_count, self.stopped.load(Ordering::SeqCst)) {
            self.ensure_workers(worker_count);
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&self) -> Option<BatchFetchedArticle> {
        use std::sync::atomic::Ordering;
        let (index, worker_count) = {
            let guard = lock(&self.inner);
            if !article_batch_can_deliver_next(
                guard.cancelled,
                self.stopped.load(Ordering::SeqCst),
                guard.next_deliver,
                guard.requests.len(),
            ) {
                return None;
            }
            (guard.next_deliver, guard.worker_count)
        };

        if worker_count == 0 {
            let req = {
                let guard = lock(&self.inner);
                guard.requests[index].clone()
            };
            let result = (self.fetch_fn)(usize::MAX, &req).unwrap_or_default();
            let mut guard = lock(&self.inner);
            guard.next_deliver += 1;
            if self.stopped.load(Ordering::SeqCst) {
                return None;
            }
            return Some(result);
        }

        while !self.stopped.load(Ordering::SeqCst) {
            {
                let mut guard = lock(&self.inner);
                if guard.cancelled {
                    return None;
                }
                if guard.slots[index].ready {
                    let result = std::mem::take(&mut guard.slots[index].result);
                    guard.slots[index].ready = false;
                    guard.buffered_bytes -= result.data.len() as i64;
                    guard.next_deliver += 1;
                    return Some(result);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        None
    }

    pub fn cancel_remaining(&self) {
        let mut guard = lock(&self.inner);
        guard.cancelled = true;
        guard.buffered_bytes = 0;
    }

    pub fn stop(&self) {
        use std::sync::atomic::Ordering;
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(ref hook) = self.stop_hook {
            hook();
        }
    }
}

impl Drop for ArticleBatchFetcher {
    fn drop(&mut self) {
        self.stop();
        let mut workers = lock(&self.workers);
        for worker in workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerPoolServerView {
    pub id: i32,
    pub group: i32,
    pub norm_level: i32,
    pub active: bool,
    pub optional: bool,
    pub blocked: bool,
    /// retention in days (0 = not set), for [`BackboneMiss`]
    pub retention: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerPoolConnView {
    pub server_index: usize,
    pub in_use: bool,
    pub connected: bool,
}

/// Pure Rust implementation of `ServerPool::GetConnection` + `ServerPool::LockedGetConnection`.
/// Scans levels starting at `start_level`, unblocking any servers whose connections pass the
/// availability/want/active/blocked check (recording their indices in `out_unblock_server_indices`),
/// filtering against `ignore_servers` (ids) and backbones in `missing`, and selecting a candidate or advancing to the next level if all active servers
/// on the current level are optional and blocked. Without a wanted server, a level whose every
/// active server is on a backbone that answered 430 for this article is passed over, so an
/// article never waits on a level it can't use. Servers in `ignore_servers` (failed, or waiting
/// out a temporary error) never cause a level skip: the downloader decides when to move on.
#[allow(clippy::too_many_arguments)]
pub fn server_pool_select_connection(
    start_level: i32,
    levels: &[i32],
    servers: &[ServerPoolServerView],
    conns: &[ServerPoolConnView],
    want_server: Option<(i32, i32)>,
    ignore_servers: &[i32],
    missing: &[BackboneMiss],
    rand_val: i32,
    out_unblock_server_indices: &mut Vec<usize>,
) -> Option<(usize, i32)> {
    out_unblock_server_indices.clear();
    if start_level < 0 {
        return None;
    }

    let mut server_blocked: Vec<bool> = servers.iter().map(|s| s.blocked).collect();
    let mut unblocked_set = vec![false; servers.len()];
    let ruled_out = |srv: &ServerPoolServerView| {
        want_server.is_none()
            && (ignore_servers
                .iter()
                .any(|&ig| server_pool_match_ignore_server(srv.id, ig))
                || backbone_missing(missing, srv.group, srv.retention))
    };

    let mut level = start_level;
    loop {
        if level >= 0
            && (level as usize) < levels.len()
            && want_server.is_none()
            && level_ruled_out(servers, level, &|srv: &ServerPoolServerView| {
                backbone_missing(missing, srv.group, srv.retention)
            })
        {
            level += 1;
            continue;
        }
        if !server_pool_has_available_level_conns(
        level,
        levels.len(),
        if level >= 0 && (level as usize) < levels.len() {
            levels[level as usize]
        } else {
            0
        },
    ) {
            break;
        }
        let mut candidates = Vec::new();
        for (conn_idx, conn) in conns.iter().enumerate() {
            if conn.server_index >= servers.len() {
                continue;
            }
            let srv = &servers[conn.server_index];
            let (has_want, want_id) = match want_server {
                Some((id, _)) => (true, id),
                None => (false, 0),
            };
            let match_want = server_pool_match_want_server(has_want, srv.id, want_id);
            let is_blocked = !conn.connected && server_blocked[conn.server_index];
            if !conn.in_use
                && srv.active
                && srv.norm_level == level
                && match_want
                && (conn.connected || !is_blocked)
            {
                let use_conn = !ruled_out(srv);
                server_blocked[conn.server_index] = false;
                if !unblocked_set[conn.server_index] {
                    unblocked_set[conn.server_index] = true;
                    out_unblock_server_indices.push(conn.server_index);
                }
                if use_conn {
                    candidates.push(conn_idx);
                }
            }
        }

        if !candidates.is_empty() {
            let pick = server_pool_pick_candidate_index(rand_val, candidates.len());
            return Some((candidates[pick], level));
        }

        for (srv_idx, srv) in servers.iter().enumerate() {
            if !server_pool_can_advance_level(
                srv.norm_level,
                level,
                srv.active,
                srv.optional,
                server_blocked[srv_idx],
            ) {
                return None;
            }
        }

        level += 1;
    }

    None
}

/// Whether every active server on `level` is ruled out for the article, and
/// the level has at least one active server (an empty level is not skipped
/// here: the scan stops at it as before).
fn level_ruled_out(
    servers: &[ServerPoolServerView],
    level: i32,
    ruled_out: &dyn Fn(&ServerPoolServerView) -> bool,
) -> bool {
    let mut any = false;
    for srv in servers.iter().filter(|s| s.norm_level == level && s.active) {
        if !ruled_out(srv) {
            return false;
        }
        any = true;
    }
    any
}

#[derive(Debug, Clone)]
pub struct RsNewsServer {
    pub id: i32,
    pub active: bool,
    pub level: i32,
    pub norm_level: i32,
    pub optional: bool,
    pub group: i32,
    pub max_connections: i32,
    pub block_time: i64,
}

#[derive(Debug, Clone)]
pub struct RsPooledConnection {
    pub server_id: i32,
    pub in_use: bool,
    pub connected: bool,
    pub free_time: i64,
}

/// Pure Rust domain model of `ServerPool` exercising the exact Rust production algorithms
/// (`server_pool_normalize_levels`, `server_pool_select_connection`, `server_pool_is_blocked`,
/// `server_pool_eval_block_server`, `server_pool_level_should_close`,
/// `server_pool_should_disconnect_idle_level_conn`).
#[derive(Debug, Default)]
pub struct ServerPoolModel {
    pub servers: Vec<RsNewsServer>,
    pub sorted_server_ids: Vec<i32>,
    pub connections: Vec<RsPooledConnection>,
    pub levels: Vec<i32>,
    pub max_norm_level: i32,
    pub retry_interval: i32,
    pub generation: i32,
}

impl ServerPoolModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_server(
        &mut self,
        id: i32,
        active: bool,
        level: i32,
        optional: bool,
        group: i32,
        max_connections: i32,
    ) {
        self.servers.push(RsNewsServer {
            id,
            active,
            level,
            norm_level: 0,
            optional,
            group,
            max_connections,
            block_time: 0,
        });
        self.sorted_server_ids.push(id);
    }

    pub fn get_server_by_id(&self, id: i32) -> Option<&RsNewsServer> {
        self.servers.iter().find(|s| s.id == id)
    }

    pub fn get_server_by_id_mut(&mut self, id: i32) -> Option<&mut RsNewsServer> {
        self.servers.iter_mut().find(|s| s.id == id)
    }

    pub fn normalize_levels(&mut self) {
        if self.servers.is_empty() {
            return;
        }
        let orig_ids = self.sorted_server_ids.clone();
        let inputs: Vec<crate::dupefallback::NzbgetRsServerNormInput> = orig_ids
            .iter()
            .map(|&id| {
                let s = self.servers.iter().find(|srv| srv.id == id).unwrap();
                crate::dupefallback::NzbgetRsServerNormInput {
                    level: s.level,
                    active: i32::from(s.active),
                    max_connections: s.max_connections,
                }
            })
            .collect();
        let mut sorted_indices = vec![0usize; inputs.len()];
        let mut norm_levels = vec![0i32; inputs.len()];
        self.max_norm_level = crate::dupefallback::server_pool_normalize_levels(
            &inputs,
            &mut sorted_indices,
            &mut norm_levels,
        );
        for (i, &id) in orig_ids.iter().enumerate() {
            if let Some(s) = self.servers.iter_mut().find(|srv| srv.id == id) {
                s.norm_level = norm_levels[i];
            }
            self.sorted_server_ids[i] = orig_ids[sorted_indices[i]];
        }
    }

    pub fn init_connections(&mut self) {
        self.normalize_levels();
        self.levels.clear();
        let sorted_ids = self.sorted_server_ids.clone();
        for id in sorted_ids {
            let (norm_level, active, max_conns) = {
                let srv = self.servers.iter_mut().find(|s| s.id == id).unwrap();
                srv.block_time = 0;
                (srv.norm_level, srv.active, srv.max_connections)
            };
            if norm_level > -1 {
                if (self.levels.len() as i32) <= norm_level {
                    self.levels.push(0);
                }
                if active {
                    let mut conns = self
                        .connections
                        .iter()
                        .filter(|c| c.server_id == id)
                        .count() as i32;
                    while conns < max_conns {
                        self.connections.push(RsPooledConnection {
                            server_id: id,
                            in_use: false,
                            connected: false,
                            free_time: 0,
                        });
                        conns += 1;
                    }
                    self.levels[norm_level as usize] += conns;
                }
            }
        }
        self.generation += 1;
    }

    pub fn block_server(&mut self, server_id: i32, cur_time: i64) {
        if let Some(srv) = self.servers.iter_mut().find(|s| s.id == server_id) {
            let (_new_block, _warn) =
                server_pool_eval_block_server(srv.block_time, cur_time, self.retry_interval);
            srv.block_time = cur_time;
        }
    }

    pub fn is_server_blocked(&self, server_id: i32, cur_time: i64) -> bool {
        if let Some(srv) = self.servers.iter().find(|s| s.id == server_id) {
            if srv.block_time == 0 {
                return false;
            }
            crate::dupefallback::server_pool_is_blocked(
                srv.block_time,
                cur_time,
                self.retry_interval,
            )
        } else {
            false
        }
    }

    pub fn get_connection(
        &mut self,
        level: i32,
        want_server_id: Option<i32>,
        ignore_server_ids: &[i32],
        cur_time: i64,
    ) -> Option<usize> {
        self.get_connection_missing(level, want_server_id, ignore_server_ids, &[], cur_time)
    }

    /// [`Self::get_connection`] for an article some backbones answered 430 for.
    pub fn get_connection_missing(
        &mut self,
        level: i32,
        want_server_id: Option<i32>,
        ignore_server_ids: &[i32],
        missing: &[BackboneMiss],
        cur_time: i64,
    ) -> Option<usize> {
        let srv_views: Vec<ServerPoolServerView> = self
            .sorted_server_ids
            .iter()
            .map(|&id| {
                let s = self.servers.iter().find(|srv| srv.id == id).unwrap();
                let blocked = s.block_time != 0
                    && crate::dupefallback::server_pool_is_blocked(
                        s.block_time,
                        cur_time,
                        self.retry_interval,
                    );
                ServerPoolServerView {
                    id: s.id,
                    group: s.group,
                    norm_level: s.norm_level,
                    active: s.active,
                    optional: s.optional,
                    blocked,
                    retention: 0,
                }
            })
            .collect();
        let conn_views: Vec<ServerPoolConnView> = self
            .connections
            .iter()
            .map(|c| {
                let server_index = self
                    .sorted_server_ids
                    .iter()
                    .position(|&id| id == c.server_id)
                    .unwrap();
                ServerPoolConnView {
                    server_index,
                    in_use: c.in_use,
                    connected: c.connected,
                }
            })
            .collect();
        let want_tuple = want_server_id.and_then(|wid| {
            self.servers
                .iter()
                .find(|s| s.id == wid)
                .map(|s| (s.id, s.group))
        });
        let mut unblock_indices = Vec::new();
        let result = server_pool_select_connection(
            level,
            &self.levels,
            &srv_views,
            &conn_views,
            want_tuple,
            ignore_server_ids,
            missing,
            0,
            &mut unblock_indices,
        );
        for idx in unblock_indices {
            let sid = self.sorted_server_ids[idx];
            if let Some(s) = self.servers.iter_mut().find(|srv| srv.id == sid) {
                s.block_time = 0;
            }
        }
        if let Some((conn_idx, chosen_level)) = result {
            self.connections[conn_idx].in_use = true;
            self.levels[chosen_level as usize] -= 1;
            Some(conn_idx)
        } else {
            None
        }
    }

    pub fn free_connection(&mut self, conn_idx: usize, used: bool, cur_time: i64) {
        self.connections[conn_idx].in_use = false;
        if used {
            self.connections[conn_idx].free_time = cur_time;
        }
        let sid = self.connections[conn_idx].server_id;
        if let Some(srv) = self.servers.iter().find(|s| s.id == sid) {
            if srv.norm_level > -1 && srv.active {
                self.levels[srv.norm_level as usize] += 1;
            }
        }
    }

    pub fn close_unused_connections(&mut self, cur_time: i64) {
        let servers = &self.servers;
        self.connections.retain(|c| {
            let srv = servers.iter().find(|s| s.id == c.server_id).unwrap();
            !(!c.in_use && (srv.norm_level == -1 || !srv.active))
        });
        for level in 0..=self.max_norm_level {
            let mut in_use = Vec::new();
            let mut connected = Vec::new();
            let mut free_times = Vec::new();
            for c in &self.connections {
                let srv = self.servers.iter().find(|s| s.id == c.server_id).unwrap();
                if srv.norm_level == level {
                    in_use.push(u8::from(c.in_use));
                    connected.push(u8::from(c.connected));
                    free_times.push(c.free_time);
                }
            }
            if crate::dupefallback::server_pool_level_should_close(
                &in_use,
                &connected,
                &free_times,
                cur_time,
                5,
            ) {
                for c in &mut self.connections {
                    let srv = self.servers.iter().find(|s| s.id == c.server_id).unwrap();
                    if server_pool_should_disconnect_idle_level_conn(
                        srv.norm_level,
                        level,
                        c.connected,
                    ) {
                        c.connected = false;
                    }
                }
            }
        }
    }

    pub fn changed(&mut self, cur_time: i64) {
        self.init_connections();
        self.close_unused_connections(cur_time);
    }
}

#[cfg(test)]
mod backbone_tests {
    use super::*;

    fn cand(id: i32, level: i32, group: i32, retention: i32) -> LevelCandidate {
        LevelCandidate {
            id,
            level,
            group,
            retention,
            available: true,
        }
    }

    #[test]
    fn a_430_rules_out_siblings_that_keep_articles_no_longer() {
        let unset = BackboneMiss { group: 4, retention: 0 };
        assert!(backbone_miss_covers(&unset, 4, 0));
        // the sibling may keep articles longer: still asked
        assert!(!backbone_miss_covers(&unset, 4, 3000));
        let short = BackboneMiss { group: 4, retention: 1000 };
        assert!(backbone_miss_covers(&short, 4, 1000));
        assert!(backbone_miss_covers(&short, 4, 900));
        assert!(!backbone_miss_covers(&short, 4, 4000));
        assert!(!backbone_miss_covers(&short, 4, 0));
        // other backbones and independent servers are never ruled out
        assert!(!backbone_miss_covers(&short, 5, 900));
        let independent = BackboneMiss { group: 0, retention: 0 };
        assert!(!backbone_miss_covers(&independent, 0, 0));
    }

    #[test]
    fn level_counts_as_failed_with_backbone_misses() {
        // priority 1: servers 1 and 2 on backbone 7; priority 2: server 3 on backbone 7, server 4 on 8
        let mut c = vec![cand(1, 0, 7, 0), cand(2, 0, 7, 0), cand(3, 1, 7, 0), cand(4, 1, 8, 0)];
        let miss = [BackboneMiss { group: 7, retention: 0 }];
        // server 1 answered 430: its sibling at the same and at the next priority are done too
        assert!(article_dl_all_servers_on_level_failed(0, &c, &[1], &miss));
        assert!(!article_dl_all_servers_on_level_failed(1, &c, &[1], &miss));
        assert!(article_dl_all_servers_on_level_failed(1, &c, &[1, 4], &miss));
        // server 1 refused the login or had too many connections (no 430): its
        // sibling is still to be tried
        assert!(!article_dl_all_servers_on_level_failed(0, &c, &[1], &[]));
        // unavailable servers don't hold a level open
        c[1].available = false;
        assert!(article_dl_all_servers_on_level_failed(0, &c, &[1], &[]));
    }

    #[test]
    fn select_passes_over_a_level_made_only_of_missing_backbones() {
        let mut pool = ServerPoolModel::new();
        pool.add_server(1, true, 0, false, 7, 1);
        pool.add_server(2, true, 0, false, 7, 1);
        pool.add_server(3, true, 1, false, 7, 1);
        pool.add_server(4, true, 2, false, 8, 1);
        pool.init_connections();
        let miss = [BackboneMiss { group: 7, retention: 0 }];
        // levels 0 and 1 are all backbone 7: the article goes straight to level 2
        let con = pool.get_connection_missing(0, None, &[1], &miss, 1000).unwrap();
        assert_eq!(pool.connections[con].server_id, 4);
        // nothing left: no connection, and no wait on a level it can't use
        assert!(pool.get_connection_missing(0, None, &[1], &miss, 1000).is_none());
        // a wanted server (a retry of the same article on it) is not filtered
        let mut pool = ServerPoolModel::new();
        pool.add_server(1, true, 0, false, 7, 1);
        pool.init_connections();
        assert!(pool.get_connection_missing(0, Some(1), &[], &miss, 1000).is_some());
    }

    #[test]
    fn an_ignored_server_never_moves_the_article_to_the_fill_level() {
        // server 1 (priority 1) is waiting out a 481/502; server 2 is a priority-2 fill
        // server: the article waits for server 1 instead of going to the fill server
        let mut pool = ServerPoolModel::new();
        pool.add_server(1, true, 0, false, 0, 1);
        pool.add_server(2, true, 1, false, 0, 1);
        pool.init_connections();
        assert!(pool.get_connection_missing(0, None, &[1], &[], 1000).is_none());
        // the same with both on one backbone and no 430
        let mut pool = ServerPoolModel::new();
        pool.add_server(1, true, 0, false, 7, 1);
        pool.add_server(2, true, 1, false, 7, 1);
        pool.init_connections();
        assert!(pool.get_connection_missing(0, None, &[1], &[], 1000).is_none());
    }

    #[test]
    fn a_wanted_server_is_only_that_server() {
        // servers 1 and 2 share backbone 7 at one priority: a retry wanting server 1
        // never lands on server 2 (another provider, another account)
        let mut pool = ServerPoolModel::new();
        pool.add_server(1, true, 0, false, 7, 1);
        pool.add_server(2, true, 0, false, 7, 1);
        pool.init_connections();
        for _ in 0..2 {
            if let Some(con) = pool.get_connection_missing(0, Some(1), &[], &[], 1000) {
                assert_eq!(pool.connections[con].server_id, 1);
            }
        }
    }
}

#[cfg(test)]
mod article_fetcher_tests {
    use super::*;
    use crate::dupefallback::{
        article_fetch_add_decoded_bytes, article_fetch_add_raw_bytes,
        article_fetch_header_within_limit, ARTICLE_FETCH_MAX_DECODED_BYTES,
        ARTICLE_FETCH_MAX_HEADER_BYTES, ARTICLE_FETCH_MAX_RAW_BYTES,
    };
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn make_result(tag: u8, size: usize) -> BatchFetchedArticle {
        BatchFetchedArticle {
            data: vec![tag; size],
            offset: 0,
            file_size: size as i64,
            success: true,
            retry: false,
            transient: false,
        }
    }

    #[test]
    fn article_fetch_limits_suite_port_from_cpp() {
        // 1. ArticleFetchLimitsRejectUnboundedHeader
        let mut raw = 0i64;
        assert!(article_fetch_add_raw_bytes(
            &mut raw,
            ARTICLE_FETCH_MAX_HEADER_BYTES
        ));
        assert!(article_fetch_header_within_limit(raw, false));
        assert!(article_fetch_add_raw_bytes(&mut raw, 1));
        assert!(!article_fetch_header_within_limit(raw, false));
        assert!(article_fetch_header_within_limit(raw, true));

        // 2. ArticleFetchLimitsRejectUnboundedWireBody
        let mut raw = 0i64;
        assert!(article_fetch_add_raw_bytes(
            &mut raw,
            ARTICLE_FETCH_MAX_RAW_BYTES
        ));
        assert!(!article_fetch_add_raw_bytes(&mut raw, 1));

        // 3. ArticleFetchLimitsEnforceDeclaredDecodedRange
        let mut dec = 0i64;
        assert!(!article_fetch_add_decoded_bytes(&mut dec, 1, 0, 10, 100));
        assert!(!article_fetch_add_decoded_bytes(&mut dec, 1, 11, 10, 100));
        assert!(!article_fetch_add_decoded_bytes(&mut dec, 1, 91, 101, 100));
        assert!(article_fetch_add_decoded_bytes(&mut dec, 4, 11, 20, 100));
        assert!(article_fetch_add_decoded_bytes(&mut dec, 6, 11, 20, 100));
        assert!(!article_fetch_add_decoded_bytes(&mut dec, 1, 11, 20, 100));

        // 4. ArticleFetchLimitsEnforceHardDecodedCap
        let mut dec = 0i64;
        let too_large_end = ARTICLE_FETCH_MAX_DECODED_BYTES + 1;
        assert!(!article_fetch_add_decoded_bytes(
            &mut dec,
            1,
            1,
            too_large_end,
            too_large_end
        ));
        assert!(article_fetch_add_decoded_bytes(
            &mut dec,
            ARTICLE_FETCH_MAX_DECODED_BYTES,
            1,
            ARTICLE_FETCH_MAX_DECODED_BYTES,
            ARTICLE_FETCH_MAX_DECODED_BYTES
        ));
        assert!(!article_fetch_add_decoded_bytes(
            &mut dec,
            1,
            1,
            ARTICLE_FETCH_MAX_DECODED_BYTES,
            ARTICLE_FETCH_MAX_DECODED_BYTES
        ));
    }

    #[test]
    fn raising_the_worker_count_joins_the_running_batch() {
        let no_groups = Arc::new(Vec::<Vec<u8>>::new());
        let seen = Arc::new(std::sync::Mutex::new(std::collections::BTreeSet::new()));
        let seen_cb = Arc::clone(&seen);
        let raised = Arc::new(AtomicBool::new(false));
        let raised_cb = Arc::clone(&raised);
        let batch = ArticleBatchFetcher::new(
            move |worker, _| {
                seen_cb.lock().unwrap().insert(worker);
                // Once raised, worker 0 waits for the others so it cannot drain the batch
                // alone on a loaded machine.
                let until = std::time::Instant::now() + Duration::from_secs(5);
                while raised_cb.load(Ordering::SeqCst)
                    && seen_cb.lock().unwrap().len() < 4
                    && std::time::Instant::now() < until
                {
                    std::thread::sleep(Duration::from_millis(2));
                }
                std::thread::sleep(Duration::from_millis(20));
                Ok(make_result(b'r', 4))
            },
            None,
        );
        batch.set_worker_count(1);
        let requests = (0..24)
            .map(|_| BatchRequest {
                message_id: b"m".to_vec(),
                groups: Arc::clone(&no_groups),
            })
            .collect();
        batch.begin(requests);
        assert!(batch.next().is_some());
        // Mid-batch: no new batch begins, yet the added workers take part in this one.
        raised.store(true, Ordering::SeqCst);
        batch.raise_worker_count(4);
        let mut delivered = 1;
        while batch.next().is_some() {
            delivered += 1;
        }
        assert_eq!(delivered, 24);
        assert_eq!(seen.lock().unwrap().len(), 4);
    }

    #[test]
    fn article_batch_fetcher_suite_port_from_cpp() {
        let no_groups = Arc::new(Vec::<Vec<u8>>::new());

        // 5. ArticleBatchFetcherDeliversInSubmissionOrder
        {
            let batch = ArticleBatchFetcher::new(
                |_, req| {
                    let index: usize = std::str::from_utf8(&req.message_id)
                        .unwrap()
                        .parse()
                        .unwrap();
                    std::thread::sleep(Duration::from_millis((40 - index * 10) as u64));
                    Ok(make_result(b'a' + (index as u8), 4))
                },
                None,
            );
            batch.set_worker_count(3);
            let requests = (0..4)
                .map(|i| BatchRequest {
                    message_id: format!("{i}").into_bytes(),
                    groups: Arc::clone(&no_groups),
                })
                .collect();
            batch.begin(requests);
            for i in 0..4u8 {
                let fetched = batch.next().unwrap();
                assert_eq!(fetched.data[0], b'a' + i);
            }
            assert!(batch.next().is_none());
        }

        // 6. ArticleBatchFetcherHonorsWindowBound
        for workers in [3, ARTICLE_BATCH_MAX_WINDOW_PARTS * 2] {
            let window = ARTICLE_BATCH_MAX_WINDOW_PARTS.max(workers * 2);
            let started = Arc::new(AtomicI32::new(0));
            let started_cb = Arc::clone(&started);
            let batch = ArticleBatchFetcher::new(
                move |_, _| {
                    started_cb.fetch_add(1, Ordering::SeqCst);
                    Ok(make_result(b'w', 4))
                },
                None,
            );
            batch.set_worker_count(workers);
            assert_eq!(batch.get_window_parts(), window);
            let requests = (0..(window * 3))
                .map(|_| BatchRequest {
                    message_id: b"m".to_vec(),
                    groups: Arc::clone(&no_groups),
                })
                .collect();
            batch.begin(requests);
            std::thread::sleep(Duration::from_millis(120));
            assert_eq!(started.load(Ordering::SeqCst), window);
            let mut delivered = 0;
            while batch.next().is_some() {
                delivered += 1;
            }
            assert_eq!(delivered, window * 3);
        }

        // 7. ArticleBatchFetcherByteCapThrottlesClaims
        {
            let started = Arc::new(AtomicI32::new(0));
            let started_cb = Arc::clone(&started);
            let batch = ArticleBatchFetcher::new(
                move |_, _| {
                    started_cb.fetch_add(1, Ordering::SeqCst);
                    Ok(make_result(
                        b'b',
                        (ARTICLE_BATCH_MAX_BUFFERED_BYTES as usize) + 1,
                    ))
                },
                None,
            );
            batch.set_worker_count(1);
            let requests = (0..3)
                .map(|_| BatchRequest {
                    message_id: b"m".to_vec(),
                    groups: Arc::clone(&no_groups),
                })
                .collect();
            batch.begin(requests);
            std::thread::sleep(Duration::from_millis(150));
            assert_eq!(started.load(Ordering::SeqCst), 1);
            assert!(batch.next().is_some());
            std::thread::sleep(Duration::from_millis(150));
            assert_eq!(started.load(Ordering::SeqCst), 2);
            batch.cancel_remaining();
        }

        // 8. ArticleBatchFetcherCancelStopsClaims
        {
            let started = Arc::new(AtomicI32::new(0));
            let started_cb = Arc::clone(&started);
            let batch = ArticleBatchFetcher::new(
                move |_, _| {
                    started_cb.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(40));
                    Ok(make_result(b'c', 4))
                },
                None,
            );
            batch.set_worker_count(2);
            let requests = (0..20)
                .map(|_| BatchRequest {
                    message_id: b"m".to_vec(),
                    groups: Arc::clone(&no_groups),
                })
                .collect();
            batch.begin(requests);
            assert!(batch.next().is_some());
            batch.cancel_remaining();
            let claimed_at_cancel = started.load(Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(100));
            assert!(started.load(Ordering::SeqCst) <= claimed_at_cancel + 2);
            assert!(batch.next().is_none());
        }

        // 9. ArticleBatchFetcherInlineModeFetchesOnCallerThread
        {
            let calls = Arc::new(AtomicI32::new(0));
            let calls_cb = Arc::clone(&calls);
            let batch = ArticleBatchFetcher::new(
                move |_, req| {
                    calls_cb.fetch_add(1, Ordering::SeqCst);
                    let idx: u8 = std::str::from_utf8(&req.message_id)
                        .unwrap()
                        .parse()
                        .unwrap();
                    Ok(make_result(b'a' + idx, 4))
                },
                None,
            );
            batch.set_worker_count(0);
            let requests = (0..3)
                .map(|i| BatchRequest {
                    message_id: format!("{i}").into_bytes(),
                    groups: Arc::clone(&no_groups),
                })
                .collect();
            batch.begin(requests);
            let f0 = batch.next().unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(f0.data[0], b'a');
            let f1 = batch.next().unwrap();
            assert_eq!(f1.data[0], b'b');
            assert!(batch.next().is_some());
            assert!(batch.next().is_none());
            assert_eq!(calls.load(Ordering::SeqCst), 3);
        }

        // 10. ArticleBatchFetcherThrowingFetchDeliversFailedResult
        {
            let batch = ArticleBatchFetcher::new(
                |_, req| {
                    let idx: u8 = std::str::from_utf8(&req.message_id)
                        .unwrap()
                        .parse()
                        .unwrap();
                    if idx == 1 {
                        return Err("simulated fetch failure".to_string());
                    }
                    Ok(make_result(b'a' + idx, 4))
                },
                None,
            );
            batch.set_worker_count(2);
            let requests = (0..4)
                .map(|i| BatchRequest {
                    message_id: format!("{i}").into_bytes(),
                    groups: Arc::clone(&no_groups),
                })
                .collect();
            batch.begin(requests);
            for i in 0..4u8 {
                let fetched = batch.next().unwrap();
                if i == 1 {
                    assert!(!fetched.success);
                } else {
                    assert!(fetched.success);
                    assert_eq!(fetched.data[0], b'a' + i);
                }
            }
            assert!(batch.next().is_none());
        }

        // 11. ArticleBatchFetcherStopUnblocksConsumer
        {
            let release = Arc::new(AtomicBool::new(false));
            let release_cb = Arc::clone(&release);
            let stop_hook_called = Arc::new(AtomicBool::new(false));
            let stop_hook_cb = Arc::clone(&stop_hook_called);
            let batch = Arc::new(ArticleBatchFetcher::new(
                move |_, _| {
                    while !release_cb.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Ok(make_result(b's', 4))
                },
                Some(Arc::new(move || {
                    stop_hook_cb.store(true, Ordering::SeqCst);
                })),
            ));
            batch.set_worker_count(1);
            let requests = (0..2)
                .map(|_| BatchRequest {
                    message_id: b"m".to_vec(),
                    groups: Arc::clone(&no_groups),
                })
                .collect();
            batch.begin(requests);
            let batch_for_stop = Arc::clone(&batch);
            let stopper = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                batch_for_stop.stop();
            });
            let delivered = batch.next();
            // next() can return as soon as stop() sets the flag, before stop()
            // has run the hook, so wait for stop() to finish. Release the worker
            // before asserting: a failed assert drops the batch, and its Drop
            // joins the worker, which would otherwise spin on `release` forever.
            stopper.join().unwrap();
            release.store(true, Ordering::SeqCst);
            assert!(delivered.is_none());
            assert!(stop_hook_called.load(Ordering::SeqCst));
        }

        // 12. ArticleBatchFetcherRequestsOwnTheirGroups
        {
            let release = Arc::new(AtomicBool::new(false));
            let release_cb = Arc::clone(&release);
            let groups_intact = Arc::new(AtomicBool::new(false));
            let groups_cb = Arc::clone(&groups_intact);
            let batch = ArticleBatchFetcher::new(
                move |_, req| {
                    while !release_cb.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    let ok = req.groups.len() == 1
                        && req.groups[0].as_slice() == b"alt.binaries.ownership";
                    groups_cb.store(ok, Ordering::SeqCst);
                    Ok(make_result(b'g', 4))
                },
                None,
            );
            batch.set_worker_count(1);
            let source_groups = Arc::new(vec![b"alt.binaries.ownership".to_vec()]);
            let requests = vec![BatchRequest {
                message_id: b"m".to_vec(),
                groups: Arc::clone(&source_groups),
            }];
            batch.begin(requests);
            drop(source_groups);
            release.store(true, Ordering::SeqCst);
            assert!(batch.next().is_some());
            assert!(groups_intact.load(Ordering::SeqCst));
            assert!(batch.next().is_none());
        }
    }

    #[test]
    fn server_pool_full_suite_port_from_cpp() {
        let test_block_servers = |group: i32| {
            let mut pool = ServerPoolModel::new();
            pool.add_server(1, true, 0, false, group, 2);
            pool.add_server(2, true, 0, false, group, 2);
            pool.add_server(3, true, 1, false, 0, 2);
            pool.init_connections();
            pool.retry_interval = 60;

            pool.block_server(1, 1000);

            let con1 = pool.get_connection(0, None, &[], 1000);
            let con2 = pool.get_connection(0, None, &[], 1000);
            let con3 = pool.get_connection(0, None, &[], 1000);
            let con4 = pool.get_connection(0, None, &[], 1000);
            assert!(con1.is_some());
            assert!(con2.is_some());
            assert!(con3.is_none());
            assert!(con4.is_none());
            let sid2 = pool.connections[con2.unwrap()].server_id;
            assert_eq!(pool.get_server_by_id(sid2).unwrap().level, 0);

            if let Some(c1) = con1 {
                pool.free_connection(c1, false, 1000);
            }
            if let Some(c2) = con2 {
                pool.free_connection(c2, false, 1000);
            }
        };

        let test_optional_block_servers = |group: i32| {
            let mut pool = ServerPoolModel::new();
            pool.add_server(1, true, 0, true, group, 2);
            pool.add_server(2, true, 0, true, group, 2);
            pool.add_server(3, true, 1, false, 0, 2);
            pool.init_connections();
            pool.retry_interval = 60;

            pool.block_server(1, 1000);
            pool.block_server(2, 1000);

            let con1 = pool.get_connection(0, None, &[], 1000);
            let con2 = pool.get_connection(0, None, &[], 1000);
            let con3 = pool.get_connection(0, None, &[], 1000);
            let con4 = pool.get_connection(0, None, &[], 1000);

            assert!(con1.is_some());
            assert!(con2.is_some());
            assert!(con3.is_none());
            assert!(con4.is_none());
            let sid1 = pool.connections[con1.unwrap()].server_id;
            let sid2 = pool.connections[con2.unwrap()].server_id;
            assert_eq!(pool.get_server_by_id(sid1).unwrap().level, 1);
            assert_eq!(pool.get_server_by_id(sid2).unwrap().level, 1);
        };

        let test_block_optional_and_non_optional_servers = |group: i32| {
            let mut pool = ServerPoolModel::new();
            pool.add_server(1, true, 0, true, group, 2);
            pool.add_server(2, true, 0, false, group, 2);
            pool.add_server(3, true, 1, false, 0, 2);
            pool.init_connections();
            pool.retry_interval = 60;

            pool.block_server(1, 1000);
            pool.block_server(2, 1000);

            let con1 = pool.get_connection(0, None, &[], 1000);
            let con2 = pool.get_connection(0, None, &[], 1000);
            let con3 = pool.get_connection(0, None, &[], 1000);
            let con4 = pool.get_connection(0, None, &[], 1000);

            assert!(con1.is_none());
            assert!(con2.is_none());
            assert!(con3.is_none());
            assert!(con4.is_none());
        };

        // 1. SimpleLevelsTest
        {
            let mut pool = ServerPoolModel::new();
            pool.add_server(1, true, 2, false, 0, 2);
            pool.init_connections();
            assert_eq!(pool.max_norm_level, 0);

            pool.add_server(2, true, 10, false, 0, 3);
            pool.init_connections();
            assert_eq!(pool.max_norm_level, 1);

            let con1 = pool.get_connection(0, None, &[], 1000);
            let con2 = pool.get_connection(0, None, &[], 1000);
            let con3 = pool.get_connection(0, None, &[], 1000);
            assert!(con1.is_some());
            assert!(con2.is_some());
            assert!(con3.is_none());

            pool.free_connection(con1.unwrap(), false, 1000);
            let con3 = pool.get_connection(0, None, &[], 1000);
            assert!(con3.is_some());

            let con1 = pool.get_connection(1, None, &[], 1000);
            let con2 = pool.get_connection(1, None, &[], 1000);
            let con3 = pool.get_connection(1, None, &[], 1000);
            let con4 = pool.get_connection(1, None, &[], 1000);
            assert!(con1.is_some());
            assert!(con2.is_some());
            assert!(con3.is_some());
            assert!(con4.is_none());
        }

        // 2. WantServerTest
        {
            let mut pool = ServerPoolModel::new();
            pool.add_server(1, true, 0, false, 0, 2);
            pool.add_server(2, true, 0, false, 0, 1);
            pool.add_server(3, true, 1, false, 0, 3);
            pool.init_connections();

            let con1 = pool.get_connection(0, Some(1), &[], 1000).unwrap();
            let con2 = pool.get_connection(0, Some(1), &[], 1000).unwrap();
            assert_eq!(pool.connections[con1].server_id, 1);
            assert_eq!(pool.connections[con2].server_id, 1);

            assert!(pool.get_connection(0, Some(1), &[], 1000).is_none());

            let con3 = pool.get_connection(0, None, &[], 1000).unwrap();
            assert_ne!(pool.connections[con3].server_id, 1);
        }

        // 3. ActiveOnOffTest
        {
            let mut pool = ServerPoolModel::new();
            pool.add_server(1, true, 0, false, 0, 2);
            pool.add_server(2, true, 0, false, 0, 1);
            pool.init_connections();

            let con1 = pool.get_connection(0, None, &[], 1000);
            let con2 = pool.get_connection(0, None, &[], 1000);
            let con3 = pool.get_connection(0, None, &[], 1000);
            let con4 = pool.get_connection(0, None, &[], 1000);
            assert!(con1.is_some());
            assert!(con2.is_some());
            assert!(con3.is_some());
            assert!(con4.is_none());

            pool.free_connection(con1.unwrap(), false, 1000);
            pool.free_connection(con2.unwrap(), false, 1000);
            pool.free_connection(con3.unwrap(), false, 1000);

            assert_eq!(pool.generation, 1);

            pool.get_server_by_id_mut(1).unwrap().active = false;
            pool.changed(1000);
            assert_eq!(pool.generation, 2);

            let con1 = pool.get_connection(0, None, &[], 1000);
            let con2 = pool.get_connection(0, None, &[], 1000);
            let con3 = pool.get_connection(0, None, &[], 1000);
            assert!(con1.is_some());
            assert!(con2.is_none());
            assert!(con3.is_none());
        }

        // 4. IgnoreServersTest
        {
            let mut pool = ServerPoolModel::new();
            pool.add_server(1, true, 0, false, 0, 2);
            pool.add_server(2, true, 0, false, 0, 2);
            pool.init_connections();

            let con1 = pool.get_connection(0, None, &[1], 1000);
            let con2 = pool.get_connection(0, None, &[1], 1000);
            let con3 = pool.get_connection(0, None, &[1], 1000);
            let con4 = pool.get_connection(0, None, &[1], 1000);
            assert!(con1.is_some());
            assert!(con2.is_some());
            assert!(con3.is_none());
            assert!(con4.is_none());
        }

        // 5. IgnoreServersGroupedTest: a failed server rules out only itself (a
        // login error or "too many connections" says nothing about its backbone);
        // a 430 rules out its backbone
        {
            let mut pool = ServerPoolModel::new();
            pool.add_server(1, true, 0, false, 1, 2);
            pool.add_server(2, true, 0, false, 1, 2);
            pool.init_connections();

            let con1 = pool.get_connection(0, None, &[1], 1000);
            let con2 = pool.get_connection(0, None, &[1], 1000);
            let con3 = pool.get_connection(0, None, &[1], 1000);
            assert!(con1.is_some());
            assert!(con2.is_some());
            assert!(con3.is_none());

            let mut pool = ServerPoolModel::new();
            pool.add_server(1, true, 0, false, 1, 2);
            pool.add_server(2, true, 0, false, 1, 2);
            pool.add_server(3, true, 0, false, 2, 2);
            pool.init_connections();
            let miss = [BackboneMiss { group: 1, retention: 0 }];
            let con1 = pool.get_connection_missing(0, None, &[1], &miss, 1000);
            let con2 = pool.get_connection_missing(0, None, &[1], &miss, 1000);
            let con3 = pool.get_connection_missing(0, None, &[1], &miss, 1000);
            assert!(con1.is_some());
            assert!(con2.is_some());
            assert!(con3.is_none());
            assert!(pool.connections[con1.unwrap()].server_id == 3);
        }

        // 6-11. BlockServers variants
        test_block_servers(0);
        test_block_servers(1);
        test_optional_block_servers(0);
        test_optional_block_servers(1);
        test_block_optional_and_non_optional_servers(0);
        test_block_optional_and_non_optional_servers(1);

        // 12. SpeedTestServerSelection
        {
            let mut pool = ServerPoolModel::new();
            pool.add_server(1, true, 1, false, 0, 1);
            pool.add_server(2, true, 2, false, 0, 1);
            pool.init_connections();

            let con1 = pool.get_connection(0, Some(1), &[], 1000).unwrap();
            assert_eq!(pool.connections[con1].server_id, 1);
            assert!(pool.get_connection(0, Some(1), &[], 1000).is_none());
        }

        // 13. VerifyNormLevelCorrectness
        {
            let mut pool = ServerPoolModel::new();
            pool.add_server(1, true, 5, false, 0, 1);
            pool.init_connections();

            let norm_level = pool.get_server_by_id(1).unwrap().norm_level;
            assert_eq!(norm_level, 0);

            let con = pool.get_connection(norm_level, Some(1), &[], 1000);
            assert!(con.is_some());
            if let Some(c) = con {
                pool.free_connection(c, false, 1000);
            }
        }
    }

    #[test]
    fn transient_retries_wait_longer_then_give_up() {
        let server = 1usize;
        let mut retries = TransientRetries::new(3, 10);
        let now = 1000i64;

        assert!(retries.is_empty());
        assert!(retries.waiting_servers(now).is_empty());

        // 503, then success: the only trace is the wait
        assert!(retries.register(server, now));
        assert!(!retries.is_exhausted());
        assert_eq!(retries.waiting_servers(now + 9), vec![server]);
        assert!(retries.waiting_servers(now + 10).is_empty());

        // a second one waits twice as long
        assert!(retries.register(server, now + 10));
        assert_eq!(retries.waiting_servers(now + 29), vec![server]);
        assert!(retries.waiting_servers(now + 30).is_empty());

        // repeated 503 uses up the attempts: the server is given up on, and not waited for
        assert!(!retries.register(server, now + 30));
        assert!(retries.is_exhausted());
        assert!(retries.waiting_servers(now + 30).is_empty());
    }

    #[test]
    fn transient_retries_count_each_server_and_cap_the_wait() {
        let first = 1usize;
        let second = 2usize;
        let mut retries = TransientRetries::new(99, 50);
        let now = 1000i64;

        assert!(retries.register(first, now));
        assert!(retries.register(first, now));
        assert_eq!(
            retries.waiting_servers(now + TransientRetries::MAX_INTERVAL_SEC as i64 - 1),
            vec![first]
        );
        assert!(retries
            .waiting_servers(now + TransientRetries::MAX_INTERVAL_SEC as i64)
            .is_empty());

        // the other server isn't waited for yet
        assert_eq!(retries.waiting_servers(now), vec![first]);

        // one attempt only: no retry at all (ArticleRetries=0 or 1)
        let mut once = TransientRetries::new(1, 10);
        assert!(!once.register(second, now));
        assert!(once.is_exhausted());
    }

    #[test]
    fn transient_retries_preserve_separate_ordinary_retry_budgets_per_server() {
        let server_a = 10usize;
        let server_b = 20usize;
        let mut retries = TransientRetries::new(3, 10);

        // Server A starts with 3 ordinary retries and consumes 2 of them
        assert_eq!(retries.remained_retries(server_a), 3);
        retries.set_remained_retries(server_a, 2);
        retries.set_remained_retries(server_a, 1);
        assert_eq!(retries.remained_retries(server_a), 1);

        // Server A then encounters a transient 503 and enters backoff
        assert!(retries.register(server_a, 1000));

        // Switching to Server B gives Server B its own full ordinary retry budget (3),
        // while preserving Server A's remaining budget (1)
        assert_eq!(retries.remained_retries(server_b), 3);
        retries.set_remained_retries(server_b, 2);
        assert_eq!(retries.remained_retries(server_b), 2);
        assert_eq!(retries.remained_retries(server_a), 1);
    }
}
