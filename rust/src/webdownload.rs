//! WebDownloader's HTTP decisions (WebDownloader.cpp): the status line
//! (CheckResponse), the response headers it acts on (ProcessHeader) and the
//! URL a redirect leads to (ParseRedirect), as the C++ did them. Logging,
//! the connection and the file stay C++.

use std::ffi::{c_char, c_int, CStr};

use crate::url::{parse_url, Part};

extern "C" {
    fn atoi(s: *const c_char) -> c_int;
    fn strncasecmp(a: *const c_char, b: *const c_char, n: usize) -> c_int;
}

/// What CheckResponse decided.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Response {
    /// 0 running, 1 running a redirect, 2 connect error, 3 not found, 4 failed
    pub result: i32,
    /// whether the status line set the HTTP status
    pub set_status: bool,
    pub http_status: i32,
    /// 0 none, 1 "connection closed" (unless stopped), 2 failed with the
    /// whole line, 3 failed with the text from `status_offset`
    pub warn: i32,
    pub status_offset: usize,
}

/// CheckResponse for the first line (None: the connection closed).
pub fn check_response(response: Option<&CStr>) -> Response {
    let Some(response) = response else {
        return Response { result: 2, warn: 1, ..Default::default() };
    };
    let line = response.to_bytes();
    let space = line.iter().position(|&b| b == b' ');
    let Some(space) = space.filter(|_| line.starts_with(b"HTTP")) else {
        return Response { result: 4, warn: 2, ..Default::default() };
    };
    let at = space + 1;
    let status = &line[at..];
    let http_status = unsafe { atoi(response.as_ptr().add(at)) };
    let is = |code: &[u8]| status.starts_with(code);
    let (result, warn) = if is(b"400") || is(b"499") {
        (2, 3)
    } else if is(b"404") {
        (3, 3)
    } else if [&b"301"[..], b"302", b"303", b"307", b"308"].iter().any(|c| is(c)) {
        (1, 0)
    } else if is(b"200") {
        (0, 0)
    } else {
        (4, 2)
    };
    Response { result, set_status: true, http_status, warn, status_offset: at }
}

/// ProcessHeader: 0 nothing, 1 Content-Length (its atoi in the second
/// value), 2 gzip, 3 Content-Disposition, 4 Location while redirecting (the
/// address from the offset in the second value).
pub fn process_header(line: &CStr, redirecting: bool) -> (i32, i32) {
    let starts = |prefix: &CStr, n: usize| unsafe { strncasecmp(line.as_ptr(), prefix.as_ptr(), n) == 0 };
    if starts(c"Content-Length: ", 16) {
        // the line is at least 16 bytes: its prefix matched
        (1, unsafe { atoi(line.as_ptr().add(16)) })
    } else if starts(c"Content-Encoding: gzip", 22) {
        (2, 0)
    } else if starts(c"Content-Disposition: ", 21) {
        (3, 0)
    } else if redirecting && starts(c"Location: ", 10) {
        (4, 10)
    } else {
        (0, 0)
    }
}

/// A URL part as the C++ URL class holds it (None: a null CString).
fn part(addr: &[u8], valid: bool, p: Part, default: Option<&[u8]>) -> Option<Vec<u8>> {
    if !valid {
        return None;
    }
    match p {
        Some((start, len)) => Some(addr[start..start + len].to_vec()),
        None => default.map(<[u8]>::to_vec),
    }
}

/// printf's "%s": glibc prints a null pointer as "(null)".
fn s(v: &Option<Vec<u8>>) -> &[u8] {
    v.as_deref().unwrap_or(b"(null)")
}

/// ParseRedirect: the address `location` (a Location header) leads to from `old_url`.
pub fn redirect(old_url: &CStr, location: &CStr) -> Vec<u8> {
    let loc = location.to_bytes();
    if parse_url(location).valid {
        return loc.to_vec();
    }
    let old = old_url.to_bytes();
    let u = parse_url(old_url);
    let protocol = part(old, u.valid, u.protocol, None);
    let mut out = Vec::new();
    if loc.starts_with(b"//") {
        // protocol-relative: another host, same protocol
        out.extend_from_slice(s(&protocol));
        out.push(b':');
        out.extend_from_slice(loc);
        return out;
    }
    // within the host
    let resource = if loc.first() == Some(&b'/') {
        loc.to_vec()
    } else {
        // the old resource up to its query and last '/' (a null one as empty)
        let mut r = part(old, u.valid, u.resource, Some(b"/")).unwrap_or_default();
        if let Some(q) = r.iter().position(|&b| b == b'?') {
            r.truncate(q);
        }
        if let Some(slash) = r.iter().rposition(|&b| b == b'/') {
            r.truncate(slash + 1);
        }
        r.extend_from_slice(loc);
        r
    };
    let host = part(old, u.valid, u.host, None);
    out.extend_from_slice(s(&protocol));
    out.extend_from_slice(b"://");
    out.extend_from_slice(s(&host));
    if u.port > 0 {
        out.push(b':');
        out.extend_from_slice(u.port.to_string().as_bytes());
    }
    out.extend_from_slice(&resource);
    out
}

/// `WebDownloader::Run`:
/// Updates `(remainedDownloadRetries, remainedConnectRetries)` after an unsuccessful attempt.
pub fn step_retry_counters(
    status: i32,
    ad_connect_error: i32,
    rem_dl: i32,
    rem_conn: i32,
) -> (i32, i32) {
    if status != ad_connect_error {
        (rem_dl.wrapping_sub(1), rem_conn)
    } else {
        (rem_dl, rem_conn.wrapping_sub(1))
    }
}

/// `WebDownloader::DownloadBody`:
/// Resolves final body status from `(end, status, is_stopped)`:
/// returns `(new_status, warn_incomplete)`.
pub fn resolve_body_end_status(
    end: bool,
    status: i32,
    is_stopped: bool,
    ad_running: i32,
    ad_failed: i32,
    ad_finished: i32,
) -> (i32, bool) {
    let mut cur = status;
    let mut warn_incomplete = false;
    if !end && cur == ad_running && !is_stopped {
        warn_incomplete = true;
        cur = ad_failed;
    }
    if end {
        cur = ad_finished;
    }
    (cur, warn_incomplete)
}

/// Port of header line colon search in `HttpClient::ReadHeaders`.
pub fn http_client_find_header_colon(line: &[u8]) -> Option<usize> {
    line.iter().position(|&b| b == b':')
}

/// Port of HTTP status-line version validation in `HttpClient::ReadStatusCode`.
pub fn http_client_validate_version(http_version: &[u8], stream_ok: bool) -> bool {
    stream_ok && http_version.windows(5).any(|w| w == b"HTTP/")
}

/// Port of `HttpClient::GetHeaders`.
pub fn http_client_build_headers(
    method: &[u8],
    host: &[u8],
    version: &[u8],
    include_gzip: bool,
) -> Vec<u8> {
    let mut headers = Vec::with_capacity(
        method.len() + host.len() + version.len() + 112,
    );
    headers.extend_from_slice(method);
    headers.extend_from_slice(b" / HTTP/1.1\r\nHost: ");
    headers.extend_from_slice(host);
    headers.extend_from_slice(b"\r\nUser-Agent: nzbget/");
    headers.extend_from_slice(version);
    headers.extend_from_slice(b"\r\n");
    if include_gzip {
        headers.extend_from_slice(b"Accept-Encoding: gzip\r\n");
    }
    headers.extend_from_slice(b"Accept: */*\r\nConnection: close\r\n\r\n");
    headers
}

/// Port of `SpeedTest::CalculateSpeedInMbps`.
pub fn speed_test_calc_mbps(bytes: u32, time_sec: f64) -> f64 {
    let bits = (bytes as f64) * 8.0;
    let speed = bits / time_sec;
    speed / 1_000_000.0
}

/// Port of `SpeedTest::IsRequiredMoreTests`.
pub fn speed_test_is_required_more(dataset_len: usize, curr_median: f64, prev_p25: f64) -> bool {
    if dataset_len < 2 {
        return true;
    }
    curr_median > prev_p25
}

/// Port of `SpeedTest::HasExceededMaxFailures`.
pub fn speed_test_exceeded_max_failures(failures: i32, iterations: i32) -> bool {
    failures > (iterations / 2).max(2)
}

/// Port of `SpeedTest::GetFinalSpeed`.
pub fn speed_test_final_speed(first_median: f64, p90s: &[f64]) -> f64 {
    let mut max_speed = first_median;
    for &speed in p90s {
        if max_speed < speed {
            max_speed = speed;
        }
    }
    max_speed
}

/// Port of `SpeedTest::MakeRequest`.
pub fn speed_test_make_request(
    path: &[u8],
    query: &[u8],
    filesize: u32,
    host: &[u8],
) -> Vec<u8> {
    let size_str = filesize.to_string();
    let mut req = Vec::with_capacity(
        path.len() + query.len() + size_str.len() + host.len() + 56,
    );
    req.extend_from_slice(b"GET /");
    req.extend_from_slice(path);
    req.extend_from_slice(query);
    req.extend_from_slice(size_str.as_bytes());
    req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
    req.extend_from_slice(host);
    req.extend_from_slice(b"\r\nConnection: close\r\n\r\n");
    req
}

/// Computes initial `(remained_download_retries, remained_connect_retries)` for `WebDownloader::Run`.
pub fn webdl_calc_initial_retries(url_retries: i32, retry_enabled: bool) -> (i32, i32) {
    let dl = if !retry_enabled {
        1
    } else if url_retries > 0 {
        url_retries
    } else {
        1
    };
    let conn = if !retry_enabled {
        1
    } else if dl > 10 {
        dl
    } else {
        10
    };
    (dl, conn)
}

/// Returns whether `WebDownloader::Run` should sleep `UrlInterval` seconds before retrying.
#[allow(clippy::too_many_arguments)]
pub fn webdl_should_wait_before_retry(
    status: i32,
    ad_failed: i32,
    ad_connect_error: i32,
    rem_dl: i32,
    rem_conn: i32,
    is_stopped: bool,
    force: bool,
    pause_download: bool,
) -> bool {
    ((status == ad_failed && rem_dl > 1) || (status == ad_connect_error && rem_conn > 1))
        && !is_stopped
        && !(!force && pause_download)
}

/// Returns whether `(protocol, port)` uses the standard HTTP/HTTPS port (`80`/`443` or `0`).
pub fn webdl_is_default_http_port(protocol: &[u8], port: i32) -> bool {
    (protocol.eq_ignore_ascii_case(b"http") && (port == 80 || port == 0))
        || (protocol.eq_ignore_ascii_case(b"https") && (port == 443 || port == 0))
}

/// Returns whether the first byte of an HTTP header line marks the end of headers (`\r` or `\n`).
pub fn webdl_is_header_end_line(first_ch: u8) -> bool {
    first_ch == b'\r' || first_ch == b'\n'
}

/// Returns the byte length of `url` before any `'?'` query string.
pub fn webdl_strip_query_len(url: &[u8]) -> usize {
    url.iter().position(|&b| b == b'?').unwrap_or(url.len())
}

/// Validates URL protocol and resolves default ports for `WebDownloader::CreateConnection`.
/// Returns `(result_code, resolved_port, is_tls)`.
pub fn webdl_resolve_protocol_port(
    is_valid: bool,
    protocol: &[u8],
    port: i32,
    tls_disabled: bool,
) -> (i32, i32, bool) {
    if !is_valid {
        return (-1, port, false);
    }
    let is_http = protocol.eq_ignore_ascii_case(b"http");
    let is_https = protocol.eq_ignore_ascii_case(b"https");
    if !is_http && !is_https {
        return (-2, port, false);
    }
    let mut resolved_port = port;
    if resolved_port == 0 && is_http {
        resolved_port = 80;
    }
    if resolved_port == 0 && is_https {
        resolved_port = 443;
    }
    if tls_disabled && is_https {
        return (-3, resolved_port, true);
    }
    (0, resolved_port, is_https)
}

/// Evaluates the body read/write completion state in `WebDownloader::DownloadBody`.
pub fn webdl_check_body_chunk(
    chunk_len: i32,
    content_len: i32,
    written_len: i32,
    gzip: bool,
    confirmed_length: bool,
) -> i32 {
    if chunk_len <= 0 {
        if chunk_len == 0 && content_len == -1 && written_len > 0 {
            return 1;
        }
        return -1;
    }
    let new_written = written_len.saturating_add(chunk_len);
    if new_written == content_len || (content_len == -1 && gzip && confirmed_length) {
        2
    } else {
        0
    }
}

/// Formats `"GET <resource> HTTP/1.0\r\n"` for `WebDownloader::SendHeaders`.
pub fn webdl_format_get_request_line(resource: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(resource.len() + 15);
    out.extend_from_slice(b"GET ");
    out.extend_from_slice(resource);
    out.extend_from_slice(b" HTTP/1.0\r\n");
    out
}

/// Formats `"User-Agent: nzbget/<version_rev>\r\n"` for `WebDownloader::SendHeaders`.
pub fn webdl_format_user_agent_header(version_rev: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(version_rev.len() + 21);
    out.extend_from_slice(b"User-Agent: nzbget/");
    out.extend_from_slice(version_rev);
    out.extend_from_slice(b"\r\n");
    out
}

/// Formats `"Host: <host>\r\n"` or `"Host: <host>:<port>\r\n"` for `WebDownloader::SendHeaders`.
pub fn webdl_format_host_header(
    is_default_port: bool,
    host: &[u8],
    port: i32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(host.len() + 20);
    out.extend_from_slice(b"Host: ");
    out.extend_from_slice(host);
    if !is_default_port {
        out.extend_from_slice(format!(":{port}").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out
}

/// Converts kilobytes to bytes (`kb * 1024`) for `WebDownloader::PrepareFile`
/// and `NCursesFrontend::UpdateInput`.
pub fn webdl_kb_to_bytes(kb: i32) -> i32 {
    kb.wrapping_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses() {
        let r = check_response(Some(c"HTTP/1.1 200 OK"));
        assert_eq!((r.result, r.http_status, r.warn), (0, 200, 0));
        assert_eq!(check_response(Some(c"HTTP/1.1 302 Found")).result, 1);
        assert_eq!(check_response(Some(c"HTTP/1.1 404 Not Found")).result, 3);
        assert_eq!(check_response(Some(c"HTTP/1.1 500 Oops")).result, 4);
        assert_eq!(check_response(Some(c"garbage")).warn, 2);
        assert_eq!(check_response(None).result, 2);
    }

    #[test]
    fn headers() {
        assert_eq!(process_header(c"content-length: 1234", false), (1, 1234));
        assert_eq!(process_header(c"Content-Encoding: gzip", false), (2, 0));
        assert_eq!(process_header(c"Location: /x", false), (0, 0));
        assert_eq!(process_header(c"Location: /x", true), (4, 10));
    }

    #[test]
    fn redirects() {
        let old = c"https://example.com:8443/a/b/get?apikey=1";
        assert_eq!(redirect(old, c"https://other.org/z"), b"https://other.org/z");
        assert_eq!(redirect(old, c"//cdn.org/f"), b"https://cdn.org/f");
        assert_eq!(redirect(old, c"/root?x"), b"https://example.com:8443/root?x");
        assert_eq!(redirect(old, c"next?y"), b"https://example.com:8443/a/b/next?y");
        assert_eq!(redirect(c"http://h", c"rel"), b"http://h/rel");
        assert_eq!(redirect(c"bad", c"rel"), b"(null)://(null)rel");
    }
}
