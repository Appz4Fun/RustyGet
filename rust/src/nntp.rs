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

/// `ArticleFetcher::FetchFromConnection`:
/// Validates a completed yEnc decode in `ArticleFetcher` and computes
/// `(valid, offset, file_size, success)`.
pub fn article_fetcher_eval_decoded_yenc(
    check_status: i32,
    ds_finished: i32,
    format: i32,
    ef_yenc: i32,
    begin_pos: i64,
    end_pos: i64,
    size: i64,
    data_len: usize,
) -> (bool, i64, i64, bool) {
    if check_status != ds_finished
        || format != ef_yenc
        || begin_pos <= 0
        || end_pos <= 0
        || data_len == 0
    {
        return (false, 0, 0, false);
    }
    let offset = begin_pos - 1;
    let success = (data_len as i64) == end_pos.saturating_sub(begin_pos).saturating_add(1);
    (true, offset, size, success)
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

/// Classifies an `ArticleFetcher::FetchedArticle` result in `NntpHealthServer::Exchange`
/// (`(throw_exchange_error, answer_val)`).
pub fn nntp_health_classify_body_fetch_result(
    success: bool,
    transient: bool,
    ans_present: i32,
    ans_body_bad: i32,
) -> (bool, i32) {
    if success {
        (false, ans_present)
    } else if transient {
        (true, ans_body_bad)
    } else {
        (false, ans_body_bad)
    }
}

/// Evaluates whether an article fetch server on `level` is eligible to try (`ArticleFetcher::Fetch`).
pub fn article_fetcher_is_server_eligible_on_level(
    server_active: bool,
    server_norm_level: i32,
    target_level: i32,
    server_id: i32,
    server_group: i32,
    failed_ids: &[i32],
    failed_groups: &[i32],
    failed_levels: &[i32],
) -> bool {
    if !server_active || server_norm_level != target_level {
        return false;
    }
    let count = failed_ids
        .len()
        .min(failed_groups.len())
        .min(failed_levels.len());
    for i in 0..count {
        if failed_ids[i] == server_id
            || (failed_groups[i] > 0
                && failed_groups[i] == server_group
                && failed_levels[i] == server_norm_level)
        {
            return false;
        }
    }
    true
}

/// Evaluates whether a transient failure on a reused connection should retry immediately
/// without consuming a transient attempt (`ArticleFetcher::Fetch`).
pub fn article_fetcher_is_reused_stale_within_max(
    reused: bool,
    stale_count_after_inc: i32,
    max_connections: i32,
) -> bool {
    reused && stale_count_after_inc <= max_connections
}

/// Evaluates whether `ArticleFetcher::FetchFromConnection` has a known valid declared yEnc range.
pub fn article_fetcher_is_declared_range_known(
    begin_pos: i64,
    end_pos: i64,
    size: i64,
) -> bool {
    begin_pos > 0 && end_pos >= begin_pos && size > 0
}

/// Evaluates whether `ArticleFetcher::FetchFromConnection` should throttle bandwidth.
pub fn article_fetcher_should_throttle_bandwidth(
    stopped: bool,
    quota_reached: bool,
    speed_limit: i64,
    cur_speed: i64,
    momentary_speed: i64,
) -> bool {
    !stopped
        && !quota_reached
        && speed_limit > 0
        && (cur_speed > speed_limit || momentary_speed > speed_limit)
}

/// Computes the read buffer capacity (`read_buf_size + 1 + decode_slack`) for `Connection::Connection`.
pub fn conn_calc_read_buffer_cap(read_buf_size: i32, decode_slack: i32) -> i32 {
    read_buf_size.saturating_add(1).saturating_add(decode_slack)
}

/// Formats `port` as a decimal byte string for `Connection::Bind` and `Connection::DoConnect`.
pub fn conn_format_port_str(port: i32) -> Vec<u8> {
    format!("{port}").into_bytes()
}

/// Clamps `buf_avail` to `>= 0` for `Connection::ReadLine`.
pub fn conn_clamp_buf_avail(buf_avail: i32) -> i32 {
    if buf_avail > 0 {
        buf_avail
    } else {
        0
    }
}

/// Formats `"<prefix>: Error <err_code> - <err_msg>"` for `Connection::ReportError`.
pub fn conn_format_report_err_with_code(
    prefix: &[u8],
    err_code: i32,
    err_msg: &[u8],
) -> Vec<u8> {
    let mid = format!(": Error {err_code} - ");
    let mut out = Vec::with_capacity(prefix.len() + mid.len() + err_msg.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(mid.as_bytes());
    out.extend_from_slice(err_msg);
    out
}

/// Formats `"BODY <message_id>\r\n"` for `ArticleFetcher::FetchOnConnection`.
pub fn article_fetcher_format_body_cmd(message_id: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + message_id.len() + 2);
    out.extend_from_slice(b"BODY ");
    out.extend_from_slice(message_id);
    out.extend_from_slice(b"\r\n");
    out
}

/// Returns `true` when `!active_group.is_empty() && active_group == grp` for `NntpConnection::JoinGroup`.
pub fn nntp_conn_is_already_in_group(active_group: &[u8], grp: &[u8]) -> bool {
    !active_group.is_empty() && active_group == grp
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

/// Formats a pipelined batch of `"STAT <id>\r\n"` commands for `NntpHealthServer::Exchange`.
pub fn nntp_health_format_stat_batch(ids: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ids.len() * 48);
    for id in ids {
        out.extend_from_slice(b"STAT ");
        if id.first() == Some(&b'<') && id.last() == Some(&b'>') {
            out.extend_from_slice(id);
        } else {
            out.push(b'<');
            out.extend_from_slice(id);
            out.push(b'>');
        }
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Returns true when `ArticleFetcher::Fetch` should clear the unreachable flag for the server.
pub fn article_fetcher_should_clear_unreachable(success: bool, transient: bool) -> bool {
    success || !transient
}

/// Returns true when `ArticleFetcher::Fetch` should evaluate transient retry on an answering/reachable server.
pub fn article_fetcher_can_attempt_transient_retry(
    transient: bool,
    server_unreachable: bool,
) -> bool {
    transient && !server_unreachable
}

/// Returns true when `ArticleFetcher::Fetch` has remaining transient retry attempts (`attempt < max_transient_attempts`).
pub fn article_fetcher_has_more_transient_attempts(
    attempt: i32,
    max_transient_attempts: i32,
) -> bool {
    attempt < max_transient_attempts
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

/// Returns true when `ServerPool::CloseUnusedConnections` should disconnect a pooled connection on an idle level (`conn_norm_level == level && conn_status == cs_connected`).
pub fn server_pool_should_disconnect_idle_level_conn(
    conn_norm_level: i32,
    level: i32,
    conn_status: i32,
    cs_connected: i32,
) -> bool {
    conn_norm_level == level && conn_status == cs_connected
}

/// Classifies `NntpConnection::Connect` handshake result:
/// `0` = connected (`result == 0`),
/// `1` = disconnect & fail (`result == 1`),
/// `-1` = fail without disconnect (`result != 0 && result != 1`).
pub fn nntp_conn_classify_handshake_result(result: i32) -> i32 {
    if result == 1 {
        1
    } else if result == 0 {
        0
    } else {
        -1
    }
}

/// Returns true when `ArticleFetcher::WaitBeforeRetry` should keep sleeping (`waited < seconds * 10 && !is_stopped`).
pub fn article_fetcher_should_keep_waiting_retry(
    waited: i32,
    seconds: i32,
    is_stopped: bool,
) -> bool {
    waited < seconds.saturating_mul(10) && !is_stopped
}

/// Returns true when `ArticleFetcher::ReleaseConnection` should disconnect the connection (`!keep_connected || conn_status == cs_cancelled`).
pub fn article_fetcher_should_disconnect_on_release(
    keep_connected: bool,
    conn_status: i32,
    cs_cancelled: i32,
) -> bool {
    !keep_connected || conn_status == cs_cancelled
}

/// Returns true when `ArticleFetcher::FetchFromConnection` should treat a failed group join as transient and disconnect (`!has_response`).
pub fn article_fetcher_is_join_group_failure_transient(has_response: bool) -> bool {
    !has_response
}

/// Returns true when `ArticleBatchFetcher::Begin` should call `EnsureWorkers()` (`worker_count > 0 && !is_stopped`).
pub fn article_batch_should_ensure_workers(worker_count: i32, is_stopped: bool) -> bool {
    worker_count > 0 && !is_stopped
}

/// Returns true when a Unix-domain socket path length overflows `sun_path` (`host_len >= sun_path_cap`).
pub fn conn_is_unix_path_too_long(host_len: usize, sun_path_cap: usize) -> bool {
    host_len >= sun_path_cap
}

/// Resolves the `getaddrinfo` error code passed to `Connection::ReportError` (`if res != eai_system { res } else { 0 }`).
pub fn conn_resolve_gai_report_err_code(res: i32, eai_system: i32) -> i32 {
    if res != eai_system {
        res
    } else {
        0
    }
}

/// Returns true when `Connection::DoConnect` should log `"Socket creation failed"` (`is_invalid_socket && has_addr_list`).
pub fn conn_should_report_socket_create_failed(
    is_invalid_socket: bool,
    has_addr_list: bool,
) -> bool {
    is_invalid_socket && has_addr_list
}

/// Returns true when `Connection::DoConnect` should report connection failure and close a created socket (`!connected && !is_invalid_socket`).
pub fn conn_should_cleanup_unconnected_socket(
    connected: bool,
    is_invalid_socket: bool,
) -> bool {
    !connected && !is_invalid_socket
}

/// Returns true when `Connection::DoDisconnect` should `unlink` a listening Unix-domain socket (`is_unix_path && getsockopt_res == 0 && is_listening != 0`).
pub fn conn_should_unlink_listening_unix_socket(
    is_unix_path: bool,
    getsockopt_res: i32,
    is_listening: i32,
) -> bool {
    is_unix_path && getsockopt_res == 0 && is_listening != 0
}

/// Resolves the error code in `Connection::ReportError` (`if err_code == 0 { last_net_err } else { err_code }`).
pub fn conn_resolve_report_err_code(err_code: i32, last_net_err: i32) -> i32 {
    if err_code == 0 {
        last_net_err
    } else {
        err_code
    }
}

/// Returns true when `Network::SpeedTest::RunTest` should count an iteration as a failure (`elapsed_sec == 0.0 || downloaded < target_size`).
pub fn speed_test_is_iteration_failed(
    elapsed_sec: f64,
    downloaded: u32,
    target_size: u32,
) -> bool {
    elapsed_sec == 0.0 || downloaded < target_size
}

/// Returns true when `Network::SpeedTest::ExecuteTest` should break from its receive loop (`received <= 0 || elapsed_sec >= timeout_sec`).
pub fn speed_test_should_stop_recv_loop(
    received: i32,
    elapsed_sec: f64,
    timeout_sec: f64,
) -> bool {
    received <= 0 || elapsed_sec >= timeout_sec
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

// ============================================================================
// NntpHealthServer, NntpConnection, ServerPool, and Connection Helpers
// ============================================================================

/// Port of `NntpHealthServer::CapFor`.
pub fn nntp_health_cap_for(connections: i32, max_cap: i32) -> i32 {
    1.max(max_cap.min(connections / 2))
}

/// Port of STAT reply classification in `NntpHealthServer::Exchange`.
/// Returns `0` (`Answer::Present`), `1` (`Answer::Missing`), or `2` (`Answer::Error`).
pub fn nntp_health_classify_stat_reply(line: &[u8]) -> i32 {
    if line.starts_with(b"223") {
        0
    } else if line.starts_with(b"430") {
        1
    } else {
        2
    }
}

/// Maps NewsServer IP version (`4` / `6` / other) to `Connection::EIPVersion`.
pub fn nntp_map_ip_version(ip_version: i32, ip_v4: i32, ip_v6: i32, ip_auto: i32) -> i32 {
    if ip_version == 4 {
        ip_v4
    } else if ip_version == 6 {
        ip_v6
    } else {
        ip_auto
    }
}

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

/// Returns whether a candidate server matches `wantServer` (or its group).
pub fn server_pool_match_want_server(
    has_want: bool,
    cand_id: i32,
    want_id: i32,
    want_group: i32,
    cand_group: i32,
) -> bool {
    !has_want || cand_id == want_id || (want_group > 0 && want_group == cand_group)
}

/// Returns whether a candidate server matches an ignored server (or its same-level group).
pub fn server_pool_match_ignore_server(
    cand_id: i32,
    ign_id: i32,
    ign_group: i32,
    cand_group: i32,
    ign_norm_level: i32,
    cand_norm_level: i32,
) -> bool {
    cand_id == ign_id
        || (ign_group > 0 && ign_group == cand_group && ign_norm_level == cand_norm_level)
}

/// Computes remaining block seconds before `ServerPool::LogDebugInfo` unblocks a server.
pub fn server_pool_calc_remaining_block_sec(
    block_time: i64,
    retry_interval: i32,
    cur_time: i64,
) -> i32 {
    if block_time == 0 {
        return 0;
    }
    let end = block_time.saturating_add(i64::from(retry_interval));
    if end > cur_time {
        (end - cur_time).clamp(0, i64::from(i32::MAX)) as i32
    } else {
        0
    }
}

/// Selects `addrinfo.ai_family` for `Connection::Bind` and `Connection::DoConnect`:
/// `m_ipVersion == ipV4 ? AF_INET : m_ipVersion == ipV6 ? AF_INET6 : AF_UNSPEC`.
pub fn conn_select_ai_family(
    ip_version: i32,
    ip_v4: i32,
    ip_v6: i32,
    af_inet: i32,
    af_inet6: i32,
    af_unspec: i32,
) -> i32 {
    if ip_version == ip_v4 {
        af_inet
    } else if ip_version == ip_v6 {
        af_inet6
    } else {
        af_unspec
    }
}

/// Checks whether `host` is a Unix domain socket path (`!m_host.empty() && m_host.front() == '/'`).
pub fn conn_is_unix_domain_path(host: &[u8]) -> bool {
    !host.is_empty() && host[0] == b'/'
}

/// Calculates the buffered chunk length in `Connection::Recv`:
/// `size > m_bufAvail ? m_bufAvail : size`.
pub fn conn_recv_buffered_chunk_len(size: i32, buf_avail: i32) -> i32 {
    if size > buf_avail {
        buf_avail
    } else {
        size
    }
}

/// Calculates the `poll()` timeout in milliseconds for `Connection::ConnectWithTimeout`:
/// `m_timeout ? m_timeout * 1000 : -1`.
pub fn conn_poll_timeout_ms(timeout_sec: i32) -> i32 {
    if timeout_sec != 0 {
        timeout_sec.wrapping_mul(1000)
    } else {
        -1
    }
}

/// Checks whether `pollfd.revents` has any ready/error bits set (`revents & mask != 0`).
pub fn conn_poll_revents_ready(revents: i32, mask: i32) -> bool {
    (revents & mask) != 0
}

/// Computes the chunk length and newline presence for a single `Connection::ReadLine` buffer step.
/// Returns `(copy_len, found_newline)`.
pub fn conn_read_line_chunk(buf: &[u8], max_out: usize) -> (usize, bool) {
    let found_pos = buf.iter().position(|&b| b == b'\n');
    let raw_len = match found_pos {
        Some(idx) => idx + 1,
        None => buf.len(),
    };
    let copy_len = raw_len.min(max_out);
    (copy_len, found_pos.is_some())
}

/// Classifies `errno` after a failed `accept()` in `Connection::Accept`:
/// - `1`: transient connection-aborted/network errno -> retry `accept()` immediately
/// - `2`: descriptor/buffer exhaustion (`EMFILE`, `ENFILE`, `ENOBUFS`, `ENOMEM`) -> set `m_acceptBusy = true` and break
/// - `0`: other error -> set `m_acceptBusy = false` and break
#[allow(clippy::too_many_arguments)]
pub fn conn_classify_accept_errno(
    err: i32,
    eintr: i32,
    econnaborted: i32,
    eproto: i32,
    enetdown: i32,
    enoprotoopt: i32,
    ehostdown: i32,
    ehostunreach: i32,
    eopnotsupp: i32,
    enetunreach: i32,
    enonet: i32,
    emfile: i32,
    enfile: i32,
    enobufs: i32,
    enomem: i32,
) -> i32 {
    if err == eintr
        || err == econnaborted
        || err == eproto
        || err == enetdown
        || err == enoprotoopt
        || err == ehostdown
        || err == ehostunreach
        || err == eopnotsupp
        || err == enetunreach
        || (enonet != 0 && err == enonet)
    {
        1
    } else if err == emfile || err == enfile || err == enobufs || err == enomem {
        2
    } else {
        0
    }
}

