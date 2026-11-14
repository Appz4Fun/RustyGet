//! WebDownloader's HTTP decisions (WebDownloader.cpp): the status line
//! (CheckResponse), the response headers it acts on (ProcessHeader) and the
//! URL a redirect leads to (ParseRedirect).

use std::ffi::CStr;

use crate::url::{parse_url, Part};

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
        return Response {
            result: 2,
            warn: 1,
            ..Default::default()
        };
    };
    let line = response.to_bytes();
    let space = line.iter().position(|&b| b == b' ');
    let Some(space) = space.filter(|_| line.starts_with(b"HTTP")) else {
        return Response {
            result: 4,
            warn: 2,
            ..Default::default()
        };
    };
    let at = space + 1;
    let status = &line[at..];
    let http_status = crate::feed::c_atoi_bytes(status);
    let is = |code: &[u8]| status.starts_with(code);
    let (result, warn) = if is(b"400") || is(b"499") {
        (2, 3)
    } else if is(b"404") {
        (3, 3)
    } else if [&b"301"[..], b"302", b"303", b"307", b"308"]
        .iter()
        .any(|c| is(c))
    {
        (1, 0)
    } else if is(b"200") {
        (0, 0)
    } else {
        (4, 2)
    };
    Response {
        result,
        set_status: true,
        http_status,
        warn,
        status_offset: at,
    }
}

/// ProcessHeader: 0 nothing, 1 Content-Length (its integer value in the second
/// value), 2 gzip, 3 Content-Disposition, 4 Location while redirecting (the
/// address from the offset in the second value).
pub fn process_header(line: &CStr, redirecting: bool) -> (i32, i32) {
    let bytes = line.to_bytes();
    let starts = |prefix: &[u8]| {
        bytes.len() >= prefix.len() && bytes[..prefix.len()].eq_ignore_ascii_case(prefix)
    };
    if starts(b"Content-Length: ") {
        (1, crate::feed::c_atoi_bytes(&bytes[16..]))
    } else if starts(b"Content-Encoding: gzip") {
        (2, 0)
    } else if starts(b"Content-Disposition: ") {
        (3, 0)
    } else if redirecting && starts(b"Location: ") {
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
    let mut headers = Vec::with_capacity(method.len() + host.len() + version.len() + 112);
    headers.extend_from_slice(method);
    headers.extend_from_slice(b" / HTTP/1.1\r\nHost: ");
    headers.extend_from_slice(host);
    headers.extend_from_slice(b"\r\nUser-Agent: RustyGet/");
    headers.extend_from_slice(version);
    headers.extend_from_slice(b"\r\n");
    if include_gzip {
        headers.extend_from_slice(b"Accept-Encoding: gzip\r\n");
    }
    headers.extend_from_slice(b"Accept: */*\r\nConnection: close\r\n\r\n");
    headers
}

/// Parses a raw HTTP response (`HttpClient::GET` in `daemon/connect/HttpClient.h`),
/// returning `Some((status_code, trimmed_body))` on a valid HTTP status line and header boundary.
pub fn http_client_parse_response(raw_resp: &[u8]) -> Option<(u32, Vec<u8>)> {
    let header_end = raw_resp.windows(4).position(|w| w == b"\r\n\r\n")?;
    let header_part = &raw_resp[..header_end + 2];
    let first_line_end = header_part
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(header_part.len());
    let first_line = &header_part[..first_line_end];
    let mut tokens = first_line
        .split(|&b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .filter(|t| !t.is_empty());
    let http_version = tokens.next()?;
    let status_tok = tokens.next()?;
    let status_str = std::str::from_utf8(status_tok).ok()?;
    let status_code: u32 = status_str.parse().ok()?;
    if !http_client_validate_version(http_version, true) {
        return None;
    }
    let mut body = &raw_resp[header_end + 4..];
    while let Some((&b, rest)) = body.split_first() {
        if b == 0 || b > b' ' {
            break;
        }
        body = rest;
    }
    while let Some((&b, rest)) = body.split_last() {
        if b == 0 || b > b' ' {
            break;
        }
        body = rest;
    }
    Some((status_code, body.to_vec()))
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
pub fn speed_test_make_request(path: &[u8], query: &[u8], filesize: u32, host: &[u8]) -> Vec<u8> {
    let size_str = filesize.to_string();
    let mut req = Vec::with_capacity(path.len() + query.len() + size_str.len() + host.len() + 56);
    req.extend_from_slice(b"GET /");
    req.extend_from_slice(path);
    req.extend_from_slice(query);
    req.extend_from_slice(size_str.as_bytes());
    req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
    req.extend_from_slice(host);
    req.extend_from_slice(b"\r\nConnection: close\r\n\r\n");
    req
}

/// Port of `Network::SpeedTest::RunTest` (`daemon/connect/NetworkSpeedTest.h`).
/// `on_log(0, msg)` emits `info`, `on_log(1, msg)` emits `error`.
pub fn run_network_speed_test<E, L>(mut execute_iter: E, mut on_log: L) -> Result<f64, &'static str>
where
    E: FnMut(&[u8], i64) -> (u32, f64),
    L: FnMut(i32, &str),
{
    const SPECS: &[(&str, u32, i32, i64)] = &[
        ("100 KiB", 1024 * 100, 10, 1),
        ("1 MiB", 1024 * 1024, 8, 2),
        ("10 MiB", 1024 * 1024 * 10, 6, 2),
        ("25 MiB", 1024 * 1024 * 25, 4, 2),
        ("100 MiB", 1024 * 1024 * 100, 3, 3),
        ("250 MiB", 1024 * 1024 * 250, 2, 4),
        ("1 GiB", 1024 * 1024 * 1024, 2, 5),
    ];

    on_log(0, "Download speed test starting...");

    // (median, p25, p90)
    let mut dataset: Vec<(f64, f64, f64)> = Vec::new();

    for &(name, size, mut iterations, timeout_sec) in SPECS {
        let mut measurements = Vec::new();
        let mut failures = 0i32;
        let mut spec_ok = true;
        let req = speed_test_make_request(b"__down", b"?bytes=", size, b"speed.cloudflare.com");

        while {
            iterations -= 1;
            iterations != 0
        } {
            let (downloaded, elapsed) = execute_iter(&req, timeout_sec);
            if crate::nntp::speed_test_is_iteration_failed(elapsed, downloaded, size) {
                failures += 1;
                if speed_test_exceeded_max_failures(failures, iterations) {
                    spec_ok = false;
                    break;
                }
                continue;
            }
            let speed = speed_test_calc_mbps(downloaded, elapsed);
            measurements.push(speed);
            on_log(0, &format!("Downloaded {name} at {speed:.2} Mbps"));
        }

        if !spec_ok {
            break;
        }

        let (_, _, _, median, p25, _, p90) = crate::util::data_analytics_compute(&mut measurements)
            .unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        dataset.push((median, p25, p90));

        let more = if dataset.len() < 2 {
            speed_test_is_required_more(dataset.len(), 0.0, 0.0)
        } else {
            speed_test_is_required_more(
                dataset.len(),
                dataset[dataset.len() - 1].0,
                dataset[dataset.len() - 2].1,
            )
        };
        if !more {
            break;
        }
    }

    if dataset.is_empty() {
        const ERR_MSG: &str = "No speed data was collected during the network speed test";
        on_log(1, ERR_MSG);
        return Err(ERR_MSG);
    }

    let first_median = dataset[0].0;
    let p90s: Vec<f64> = dataset.iter().map(|d| d.2).collect();
    let final_speed = speed_test_final_speed(first_median, &p90s);
    on_log(0, &format!("Download speed {final_speed:.2} Mbps"));
    Ok(final_speed)
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

/// `WebDownloader::EStatus` (`daemon/connect/WebDownloader.h`).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebDlStatus {
    AdUndefined = 0,
    AdRunning = 1,
    AdFinished = 2,
    AdFailed = 3,
    AdRetry = 4,
    AdNotFound = 5,
    AdRedirect = 6,
    AdConnectError = 7,
    AdFatalError = 8,
}

pub const WEBDL_LOG_DEBUG: i32 = 0;
pub const WEBDL_LOG_DETAIL: i32 = 1;
pub const WEBDL_LOG_WARN: i32 = 2;
pub const WEBDL_LOG_ERROR: i32 = 3;

/// Configuration snapshot passed into `WebDownloader` operations.
pub struct WebDlConfig<'a> {
    pub initial_url: &'a [u8],
    pub output_filename: &'a [u8],
    pub info_name: &'a [u8],
    pub version: &'a [u8],
    pub write_buffer_kb: i32,
    pub url_retries: i32,
    pub url_interval_sec: i32,
    pub retry: bool,
    pub force: bool,
    pub tls_disabled: bool,
    pub gzip_disabled: bool,
}

/// Host callbacks bridging `WebDownloader`'s `Connection`, `Thread`, `Subject`,
/// and subclass `ProcessHeader` hooks.
pub trait WebDlIo {
    fn connect(&mut self, host: &CStr, port: i32, tls: bool) -> bool;
    fn free_conn(&mut self);
    fn write_line(&mut self, line: &CStr);
    /// Reads one header/status line into `buf` (including NUL terminator).
    /// Returns `Some(len)` where `buf[..len]` is the line and `buf[len] == 0`,
    /// or `None` on EOF/error.
    fn read_line(&mut self, buf: &mut [u8]) -> Option<usize>;
    /// Drains any buffered bytes from `Connection::ReadBuffer` or calls `TryRecv`.
    fn read_body_chunk(&mut self, buf: &mut [u8]) -> i32;
    fn is_stopped(&mut self) -> bool;
    fn is_paused(&mut self) -> bool;
    fn touch_time(&mut self);
    fn sleep_100ms(&mut self);
    fn set_status(&mut self, status: WebDlStatus);
    fn set_http_status(&mut self, http_status: i32);
    fn set_original_filename(&mut self, filename: &[u8]);
    fn set_url(&mut self, url: &[u8]);
    fn on_header(&mut self, line: &CStr);
    fn on_log(&mut self, level: i32, msg: &[u8]);
}

#[cfg(unix)]
enum OutSink {
    Raw(std::fs::File),
    Buffered(std::io::BufWriter<std::fs::File>),
}

#[cfg(unix)]
impl OutSink {
    fn open(output_filename: &[u8], write_buffer_kb: i32) -> Option<Self> {
        use std::os::fd::FromRawFd;
        let nul = output_filename
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(output_filename.len());
        if nul == 0 {
            return None;
        }
        let mut cbuf = Vec::with_capacity(nul + 1);
        cbuf.extend_from_slice(&output_filename[..nul]);
        cbuf.push(0);
        let cstr = CStr::from_bytes_with_nul(&cbuf).ok()?;
        let fd = crate::fsops::open_cloexec_fd(cstr, 2, 0, 1, 2);
        if fd < 0 {
            return None;
        }
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        if write_buffer_kb > 0 {
            Some(Self::Buffered(std::io::BufWriter::with_capacity(
                (write_buffer_kb as usize).saturating_mul(1024),
                file,
            )))
        } else {
            Some(Self::Raw(file))
        }
    }

    fn write_bytes(&mut self, data: &[u8]) -> bool {
        use std::io::Write;
        if data.is_empty() {
            return true;
        }
        match self {
            Self::Raw(f) => f.write_all(data).is_ok(),
            Self::Buffered(bw) => bw.write_all(data).is_ok(),
        }
    }

    fn finish(self) {
        use std::io::Write;
        if let Self::Buffered(mut bw) = self {
            let _ = bw.flush();
        }
    }
}

#[cfg(unix)]
fn unlink_broken_output(output_filename: &[u8]) {
    let nul = output_filename
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(output_filename.len());
    if nul == 0 {
        return;
    }
    let mut cbuf = Vec::with_capacity(nul + 1);
    cbuf.extend_from_slice(&output_filename[..nul]);
    cbuf.push(0);
    if let Ok(cstr) = CStr::from_bytes_with_nul(&cbuf) {
        unsafe {
            libc::unlink(cstr.as_ptr());
        }
    }
}

fn cstring_lossless_truncate(bytes: &[u8]) -> std::ffi::CString {
    let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    unsafe { std::ffi::CString::from_vec_unchecked(bytes[..nul].to_vec()) }
}

fn url_encode_vec(raw: &[u8]) -> Vec<u8> {
    let nul = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let mut out = Vec::with_capacity(nul);
    crate::text::url_encode(&raw[..nul], &mut out);
    out
}

/// Executes a single HTTP/HTTPS fetch (`WebDownloader::Download`).
#[cfg(unix)]
pub fn webdl_download<I: WebDlIo>(
    current_url: &mut Vec<u8>,
    cfg: &WebDlConfig<'_>,
    io: &mut I,
) -> WebDlStatus {
    use crate::artwritercore::join_c_slices;

    let url_c = cstring_lossless_truncate(current_url);
    let u = parse_url(&url_c);
    let addr_bytes = url_c.to_bytes();

    if !u.valid {
        io.on_log(
            WEBDL_LOG_ERROR,
            &join_c_slices(&[b"URL is not valid: ", addr_bytes]),
        );
        return WebDlStatus::AdFatalError;
    }

    let protocol = u
        .protocol
        .map_or(&b""[..], |(st, len)| &addr_bytes[st..st + len]);
    let host = u
        .host
        .map_or(&b""[..], |(st, len)| &addr_bytes[st..st + len]);
    let resource = u
        .resource
        .map_or(&b"/"[..], |(st, len)| &addr_bytes[st..st + len]);
    let raw_port = u.port;

    let is_http = protocol.eq_ignore_ascii_case(b"http");
    let is_https = protocol.eq_ignore_ascii_case(b"https");
    if !is_http && !is_https {
        io.on_log(
            WEBDL_LOG_ERROR,
            &join_c_slices(&[b"Unsupported protocol in URL: ", addr_bytes]),
        );
        return WebDlStatus::AdFatalError;
    }

    let conn_port = if raw_port == 0 {
        if is_https {
            443
        } else {
            80
        }
    } else {
        raw_port
    };
    let tls = is_https;

    if tls && cfg.tls_disabled {
        io.on_log(
            WEBDL_LOG_ERROR,
            &join_c_slices(&[
                b"Program was compiled without TLS/SSL-support. Cannot download using https protocol. URL: ",
                addr_bytes,
            ]),
        );
        return WebDlStatus::AdFatalError;
    }

    let host_c = cstring_lossless_truncate(host);
    let connected = io.connect(&host_c, conn_port, tls);
    if !connected || io.is_stopped() {
        io.free_conn();
        return WebDlStatus::AdConnectError;
    }

    io.on_log(
        WEBDL_LOG_DETAIL,
        &join_c_slices(&[b"Downloading ", cfg.info_name]),
    );

    // SendHeaders: keep unlimited length so long signed URLs never lose HTTP/1.0\r\n.
    let req_line =
        cstring_lossless_truncate(&join_c_slices(&[b"GET ", resource, b" HTTP/1.0\r\n"]));
    io.write_line(&req_line);

    let ua_line = cstring_lossless_truncate(&join_c_slices(&[
        b"User-Agent: RustyGet/",
        cfg.version,
        b"\r\n",
    ]));
    io.write_line(&ua_line);

    let host_line = if raw_port == 0 || (raw_port == 80 && is_http) || (raw_port == 443 && is_https)
    {
        cstring_lossless_truncate(&join_c_slices(&[b"Host: ", host, b"\r\n"]))
    } else {
        let port_str = raw_port.to_string();
        cstring_lossless_truncate(&join_c_slices(&[
            b"Host: ",
            host,
            b":",
            port_str.as_bytes(),
            b"\r\n",
        ]))
    };
    io.write_line(&host_line);
    io.write_line(c"Accept: */*\r\n");
    if !cfg.gzip_disabled {
        io.write_line(c"Accept-Encoding: gzip\r\n");
    }
    io.write_line(c"Connection: close\r\n");
    io.write_line(c"\r\n");

    // DownloadHeaders
    let mut status = WebDlStatus::AdRunning;
    let mut confirmed_length = false;
    let mut content_len: i32 = -1;
    let mut first_line = true;
    let mut gzip = false;
    let mut redirecting = false;
    let mut redirected = false;
    let mut line_buf = vec![0u8; 1024 * 10];

    while !io.is_stopped() {
        io.touch_time();
        let read_opt = io.read_line(&mut line_buf);

        if first_line {
            let line_cstr = read_opt.and_then(|n| {
                line_buf[n] = 0;
                CStr::from_bytes_until_nul(&line_buf[..=n]).ok()
            });
            let r = check_response(line_cstr);
            if r.set_status {
                io.set_http_status(r.http_status);
            }
            let raw_resp = line_cstr.map_or(&b""[..], CStr::to_bytes);
            match r.warn {
                1 => {
                    if !io.is_stopped() {
                        io.on_log(
                            WEBDL_LOG_WARN,
                            &join_c_slices(&[
                                b"URL ",
                                cfg.info_name,
                                b": Connection closed by remote host",
                            ]),
                        );
                    }
                }
                2 => {
                    io.on_log(
                        WEBDL_LOG_WARN,
                        &join_c_slices(&[b"URL ", cfg.info_name, b" failed: ", raw_resp]),
                    );
                }
                3 => {
                    let off = r.status_offset.min(raw_resp.len());
                    io.on_log(
                        WEBDL_LOG_WARN,
                        &join_c_slices(&[b"URL ", cfg.info_name, b" failed: ", &raw_resp[off..]]),
                    );
                }
                _ => {}
            }
            status = match r.result {
                0 => WebDlStatus::AdRunning,
                1 => {
                    redirecting = true;
                    WebDlStatus::AdRunning
                }
                2 => WebDlStatus::AdConnectError,
                3 => WebDlStatus::AdNotFound,
                _ => WebDlStatus::AdFailed,
            };
            if status != WebDlStatus::AdRunning {
                break;
            }
            first_line = false;
        }

        let Some(read_n) = read_opt else {
            if !io.is_stopped() {
                io.on_log(
                    WEBDL_LOG_WARN,
                    &join_c_slices(&[b"URL ", cfg.info_name, b" failed: Unexpected end of file"]),
                );
            }
            status = WebDlStatus::AdFailed;
            break;
        };

        line_buf[read_n] = 0;
        let c_len = line_buf[..read_n]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(read_n);
        io.on_log(
            WEBDL_LOG_DEBUG,
            &join_c_slices(&[b"Header: ", &line_buf[..c_len]]),
        );

        if matches!(line_buf.first(), Some(b'\r' | b'\n')) && c_len > 0 {
            break;
        }

        let trimmed_end = crate::util::trim_right_line(&line_buf[..c_len]);
        line_buf[trimmed_end] = 0;
        let Ok(trimmed_cstr) = CStr::from_bytes_with_nul(&line_buf[..=trimmed_end]) else {
            continue;
        };

        let (hdr_kind, hdr_val) = process_header(trimmed_cstr, redirecting);
        match hdr_kind {
            1 => {
                content_len = hdr_val;
                confirmed_length = true;
            }
            2 => {
                if !cfg.gzip_disabled {
                    gzip = true;
                }
            }
            3 => {
                let lower = crate::wildmask::Lower::Fold(&crate::util::byte_tolower_locale);
                if let Some(fname) =
                    crate::webutil::content_disposition_filename(trimmed_cstr.to_bytes(), &lower)
                {
                    let fn_len = fname.iter().position(|&b| b == 0).unwrap_or(fname.len());
                    let fn_slice = &fname[..fn_len];
                    if !fn_slice.is_empty() {
                        let base_idx = crate::paths::base_file_name(fn_slice);
                        let orig = &fn_slice[base_idx..];
                        io.set_original_filename(orig);
                        io.on_log(
                            WEBDL_LOG_DEBUG,
                            &join_c_slices(&[b"OriginalFilename: ", orig]),
                        );
                    }
                }
            }
            4 => {
                let loc_off = (hdr_val as usize).min(trimmed_end);
                if let Ok(loc_cstr) = CStr::from_bytes_with_nul(&line_buf[loc_off..=trimmed_end]) {
                    let old_c = cstring_lossless_truncate(current_url);
                    let new_loc = redirect(&old_c, loc_cstr);
                    let old_bytes = old_c.to_bytes();
                    let old_no_q = &old_bytes[..old_bytes
                        .iter()
                        .position(|&b| b == b'?')
                        .unwrap_or(old_bytes.len())];
                    let new_no_q = &new_loc[..new_loc
                        .iter()
                        .position(|&b| b == b'?')
                        .unwrap_or(new_loc.len())];
                    io.on_log(
                        WEBDL_LOG_DETAIL,
                        &join_c_slices(&[b"URL ", old_no_q, b" redirected to ", new_no_q]),
                    );
                    *current_url = url_encode_vec(&new_loc);
                    io.set_url(current_url);
                    redirected = true;
                }
            }
            _ => {}
        }

        io.on_header(trimmed_cstr);

        if redirected {
            status = WebDlStatus::AdRedirect;
            break;
        }
    }

    // DownloadBody
    if status == WebDlStatus::AdRunning {
        let mut out_sink: Option<OutSink> = None;
        let mut end = false;
        let mut written_len: i32 = 0;
        let mut decomp = if gzip {
            Some(flate2::Decompress::new_gzip(15))
        } else {
            None
        };
        let mut decomp_buf = if gzip {
            vec![0u8; 1024 * 10]
        } else {
            Vec::new()
        };

        while !io.is_stopped() {
            io.touch_time();
            let len = io.read_body_chunk(&mut line_buf);
            if len <= 0 {
                if len == 0 && content_len == -1 && written_len > 0 {
                    end = true;
                    break;
                }
                if !io.is_stopped() {
                    io.on_log(
                        WEBDL_LOG_WARN,
                        &join_c_slices(&[
                            b"URL ",
                            cfg.info_name,
                            b" failed: Unexpected end of file",
                        ]),
                    );
                }
                status = WebDlStatus::AdFailed;
                break;
            }

            let chunk = &line_buf[..len as usize];
            if out_sink.is_none() {
                match OutSink::open(cfg.output_filename, cfg.write_buffer_kb) {
                    Some(s) => out_sink = Some(s),
                    None => {
                        io.on_log(
                            WEBDL_LOG_ERROR,
                            &join_c_slices(&[b"Could not create file ", cfg.output_filename]),
                        );
                        status = WebDlStatus::AdFatalError;
                        break;
                    }
                }
            }
            let sink = out_sink.as_mut().unwrap();

            let write_ok = if let Some(dc) = decomp.as_mut() {
                let mut rem = chunk;
                loop {
                    let before_in = dc.total_in();
                    let before_out = dc.total_out();
                    let res = dc.decompress(rem, &mut decomp_buf, flate2::FlushDecompress::None);
                    let consumed = (dc.total_in() - before_in) as usize;
                    let produced = (dc.total_out() - before_out) as usize;
                    rem = &rem[consumed.min(rem.len())..];
                    match res {
                        Err(_) => {
                            io.on_log(
                                WEBDL_LOG_ERROR,
                                &join_c_slices(&[b"URL ", cfg.info_name, b": GUnzip failed"]),
                            );
                            break false;
                        }
                        Ok(st) => {
                            if produced > 0 && !sink.write_bytes(&decomp_buf[..produced]) {
                                break false;
                            }
                            if matches!(st, flate2::Status::StreamEnd) {
                                confirmed_length = true;
                                break true;
                            }
                            if produced == 0 && consumed == 0 {
                                break true;
                            }
                        }
                    }
                }
            } else {
                sink.write_bytes(chunk)
            };

            if !write_ok {
                status = WebDlStatus::AdFatalError;
                break;
            }

            let state =
                webdl_check_body_chunk(len, content_len, written_len, gzip, confirmed_length);
            written_len = written_len.saturating_add(len);
            if state == 2 {
                end = true;
                break;
            }
        }

        if let Some(sink) = out_sink.take() {
            sink.finish();
        }

        let (resolved_raw, warn_incomplete) = resolve_body_end_status(
            end,
            status as i32,
            io.is_stopped(),
            WebDlStatus::AdRunning as i32,
            WebDlStatus::AdFailed as i32,
            WebDlStatus::AdFinished as i32,
        );
        if warn_incomplete {
            io.on_log(
                WEBDL_LOG_WARN,
                &join_c_slices(&[b"URL ", cfg.info_name, b" failed: file incomplete"]),
            );
        }
        status = match resolved_raw {
            2 => WebDlStatus::AdFinished,
            8 => WebDlStatus::AdFatalError,
            _ => WebDlStatus::AdFailed,
        };
    }

    if io.is_stopped() {
        status = WebDlStatus::AdFailed;
    }

    io.free_conn();

    if status != WebDlStatus::AdFinished {
        unlink_broken_output(cfg.output_filename);
    }

    status
}

/// Follows up to `max_redirects` HTTP redirects (`WebDownloader::DownloadWithRedirects`).
#[cfg(unix)]
pub fn webdl_download_with_redirects<I: WebDlIo>(
    max_redirects: i32,
    cfg: &WebDlConfig<'_>,
    io: &mut I,
) -> WebDlStatus {
    let mut current_url = url_encode_vec(cfg.initial_url);
    if current_url != cfg.initial_url {
        io.set_url(&current_url);
    }
    webdl_download_with_redirects_state(max_redirects, &mut current_url, cfg, io)
}

#[cfg(unix)]
fn webdl_download_with_redirects_state<I: WebDlIo>(
    mut max_redirects: i32,
    current_url: &mut Vec<u8>,
    cfg: &WebDlConfig<'_>,
    io: &mut I,
) -> WebDlStatus {
    use crate::artwritercore::join_c_slices;

    let mut status = WebDlStatus::AdRedirect;
    while status == WebDlStatus::AdRedirect && max_redirects >= 0 {
        max_redirects -= 1;
        status = webdl_download(current_url, cfg, io);
    }

    if status == WebDlStatus::AdRedirect {
        io.on_log(
            WEBDL_LOG_WARN,
            &join_c_slices(&[b"Too many redirects for ", cfg.info_name]),
        );
        status = WebDlStatus::AdFailed;
    }

    status
}

/// Runs the full retry/backoff loop (`WebDownloader::Run`).
#[cfg(unix)]
pub fn webdl_run_loop<I: WebDlIo>(cfg: &WebDlConfig<'_>, io: &mut I) -> WebDlStatus {
    use crate::artwritercore::join_c_slices;

    io.set_status(WebDlStatus::AdRunning);

    let mut rem_dl = if cfg.retry { cfg.url_retries.max(1) } else { 1 };
    let mut rem_conn = if cfg.retry { rem_dl.max(10) } else { 1 };

    let mut current_url = url_encode_vec(cfg.initial_url);
    if current_url != cfg.initial_url {
        io.set_url(&current_url);
    }

    let mut status = WebDlStatus::AdFailed;

    while !io.is_stopped() && rem_dl > 0 && rem_conn > 0 {
        io.touch_time();

        status = webdl_download_with_redirects_state(5, &mut current_url, cfg, io);

        if ((status == WebDlStatus::AdFailed && rem_dl > 1)
            || (status == WebDlStatus::AdConnectError && rem_conn > 1))
            && !io.is_stopped()
            && !(!cfg.force && io.is_paused())
        {
            let wait_msg = format!("Waiting {} sec to retry", cfg.url_interval_sec);
            io.on_log(WEBDL_LOG_DETAIL, wait_msg.as_bytes());
            let limit_ms =
                ((cfg.url_interval_sec as i64) * 1000).clamp(i32::MIN as i64, i32::MAX as i64);
            let mut msec: i32 = 0;
            while !io.is_stopped() && (msec as i64) < limit_ms && !(!cfg.force && io.is_paused()) {
                io.touch_time();
                io.sleep_100ms();
                msec = msec.wrapping_add(100);
            }
        }

        if io.is_stopped() || (!cfg.force && io.is_paused()) {
            status = WebDlStatus::AdRetry;
            break;
        }

        if matches!(
            status,
            WebDlStatus::AdFinished | WebDlStatus::AdFatalError | WebDlStatus::AdNotFound
        ) {
            break;
        }

        let (next_dl, next_conn) = step_retry_counters(
            status as i32,
            WebDlStatus::AdConnectError as i32,
            rem_dl,
            rem_conn,
        );
        rem_dl = next_dl;
        rem_conn = next_conn;
    }

    if status != WebDlStatus::AdFinished && status != WebDlStatus::AdRetry {
        status = WebDlStatus::AdFailed;
    }

    if status == WebDlStatus::AdFailed {
        if io.is_stopped() {
            io.on_log(
                WEBDL_LOG_DETAIL,
                &join_c_slices(&[b"Download ", cfg.info_name, b" cancelled"]),
            );
        } else {
            io.on_log(
                WEBDL_LOG_ERROR,
                &join_c_slices(&[b"Download ", cfg.info_name, b" failed"]),
            );
        }
    }

    if status == WebDlStatus::AdFinished {
        io.on_log(
            WEBDL_LOG_DETAIL,
            &join_c_slices(&[b"Download ", cfg.info_name, b" completed"]),
        );
    }

    io.set_status(status);
    status
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
        assert_eq!(
            redirect(old, c"https://other.org/z"),
            b"https://other.org/z"
        );
        assert_eq!(redirect(old, c"//cdn.org/f"), b"https://cdn.org/f");
        assert_eq!(
            redirect(old, c"/root?x"),
            b"https://example.com:8443/root?x"
        );
        assert_eq!(
            redirect(old, c"next?y"),
            b"https://example.com:8443/a/b/next?y"
        );
        assert_eq!(redirect(c"http://h", c"rel"), b"http://h/rel");
        assert_eq!(redirect(c"bad", c"rel"), b"(null)://(null)rel");
    }

    #[test]
    fn test_run_network_speed_test() {
        let mut logs = Vec::new();
        let res = run_network_speed_test(
            |req, _timeout_sec| {
                assert!(req.starts_with(b"GET /__down?bytes="));
                (100 * 1024, 0.01)
            },
            |kind, msg| {
                logs.push((kind, msg.to_string()));
            },
        );
        let mbps = res.expect("speed test should succeed");
        assert!((mbps - 81.92).abs() < 0.1, "mbps={mbps}");
        assert!(logs
            .iter()
            .any(|(k, m)| *k == 0 && m == "Download speed test starting..."));
        assert!(logs
            .iter()
            .any(|(k, m)| *k == 0 && m.starts_with("Download speed ")));

        let err = run_network_speed_test(|_, _| (0, 0.0), |_, _| {});
        assert_eq!(
            err.unwrap_err(),
            "No speed data was collected during the network speed test"
        );
    }

    #[test]
    fn test_http_client_parse_response() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n  203.0.113.42\r\n";
        let (status, body) = http_client_parse_response(raw).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"203.0.113.42");
        assert!(http_client_parse_response(b"INVALID\r\n\r\nbody").is_none());
        assert!(http_client_parse_response(b"HTTP/1.1 200 OK\r\nno-double-crlf").is_none());
    }

    #[cfg(unix)]
    struct MockWebIo {
        responses: std::collections::VecDeque<Vec<u8>>,
        cur_stream: Vec<u8>,
        cur_pos: usize,
        sent_lines: Vec<Vec<u8>>,
        logs: Vec<(i32, Vec<u8>)>,
        headers_seen: Vec<Vec<u8>>,
        http_status: i32,
        orig_filename: Vec<u8>,
        last_url: Vec<u8>,
        status: WebDlStatus,
        stopped: bool,
        paused: bool,
        sleeps: usize,
    }

    #[cfg(unix)]
    impl MockWebIo {
        fn new(responses: Vec<Vec<u8>>) -> Self {
            Self {
                responses: responses.into(),
                cur_stream: Vec::new(),
                cur_pos: 0,
                sent_lines: Vec::new(),
                logs: Vec::new(),
                headers_seen: Vec::new(),
                http_status: 0,
                orig_filename: Vec::new(),
                last_url: Vec::new(),
                status: WebDlStatus::AdUndefined,
                stopped: false,
                paused: false,
                sleeps: 0,
            }
        }
    }

    #[cfg(unix)]
    impl WebDlIo for MockWebIo {
        fn connect(&mut self, _host: &CStr, _port: i32, _tls: bool) -> bool {
            match self.responses.pop_front() {
                Some(resp) => {
                    self.cur_stream = resp;
                    self.cur_pos = 0;
                    true
                }
                None => false,
            }
        }
        fn free_conn(&mut self) {
            self.cur_stream.clear();
            self.cur_pos = 0;
        }
        fn write_line(&mut self, line: &CStr) {
            self.sent_lines.push(line.to_bytes().to_vec());
        }
        fn read_line(&mut self, buf: &mut [u8]) -> Option<usize> {
            if self.cur_pos >= self.cur_stream.len() || buf.is_empty() {
                return None;
            }
            let rem = &self.cur_stream[self.cur_pos..];
            let nl = rem.iter().position(|&b| b == b'\n');
            let take = nl.map_or(rem.len(), |i| i + 1).min(buf.len() - 1);
            buf[..take].copy_from_slice(&rem[..take]);
            buf[take] = 0;
            self.cur_pos += take;
            Some(take)
        }
        fn read_body_chunk(&mut self, buf: &mut [u8]) -> i32 {
            if self.cur_pos >= self.cur_stream.len() {
                return 0;
            }
            let take = (self.cur_stream.len() - self.cur_pos).min(buf.len());
            buf[..take].copy_from_slice(&self.cur_stream[self.cur_pos..self.cur_pos + take]);
            self.cur_pos += take;
            take as i32
        }
        fn is_stopped(&mut self) -> bool {
            self.stopped
        }
        fn is_paused(&mut self) -> bool {
            self.paused
        }
        fn touch_time(&mut self) {}
        fn sleep_100ms(&mut self) {
            self.sleeps += 1;
        }
        fn set_status(&mut self, status: WebDlStatus) {
            self.status = status;
        }
        fn set_http_status(&mut self, http_status: i32) {
            self.http_status = http_status;
        }
        fn set_original_filename(&mut self, filename: &[u8]) {
            self.orig_filename = filename.to_vec();
        }
        fn set_url(&mut self, url: &[u8]) {
            self.last_url = url.to_vec();
        }
        fn on_header(&mut self, line: &CStr) {
            self.headers_seen.push(line.to_bytes().to_vec());
        }
        fn on_log(&mut self, level: i32, msg: &[u8]) {
            self.logs.push((level, msg.to_vec()));
        }
    }

    #[test]
    #[cfg(unix)]
    fn webdl_redirect_gzip_and_dnzb_headers() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        let out_path = format!("/tmp/rg_webdl_{}_{}.nzb", std::process::id(), 219);
        let payload = b"<nzb><file subject=\"test\"/></nzb>";
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(payload).unwrap();
        let gz_bytes = enc.finish().unwrap();

        let resp1 =
            b"HTTP/1.1 302 Found\r\nLocation: /dl/item name.nzb?apikey=SECRET123\r\n\r\n".to_vec();
        let mut resp2 = format!(
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Disposition: attachment; filename=\"/sub/release.nzb\"\r\nX-DNZB-Category: Movies\r\nContent-Length: {}\r\n\r\n",
            gz_bytes.len()
        )
        .into_bytes();
        resp2.extend_from_slice(&gz_bytes);

        let mut io = MockWebIo::new(vec![resp1, resp2]);
        let cfg = WebDlConfig {
            initial_url: b"http://indexer.example:8080/get?id=1&apikey=OLDSECRET",
            output_filename: out_path.as_bytes(),
            info_name: b"TestFetch",
            version: b"24.0",
            write_buffer_kb: 0,
            url_retries: 2,
            url_interval_sec: 1,
            retry: true,
            force: false,
            tls_disabled: false,
            gzip_disabled: false,
        };

        let st = webdl_run_loop(&cfg, &mut io);
        assert_eq!(st, WebDlStatus::AdFinished);
        assert_eq!(io.status, WebDlStatus::AdFinished);
        assert_eq!(io.http_status, 200);
        assert_eq!(io.orig_filename, b"release.nzb");
        assert_eq!(
            io.last_url,
            b"http://indexer.example:8080/dl/item%20name.nzb?apikey=SECRET123"
        );
        assert!(io
            .headers_seen
            .iter()
            .any(|h| h == b"X-DNZB-Category: Movies"));
        // Verify redirect log stripped query secrets!
        assert!(io.logs.iter().any(|(lvl, m)| {
            *lvl == WEBDL_LOG_DETAIL
                && m == b"URL http://indexer.example:8080/get redirected to http://indexer.example:8080/dl/item name.nzb"
        }));
        let disk_bytes = std::fs::read(&out_path).unwrap();
        assert_eq!(disk_bytes, payload);
        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    #[cfg(unix)]
    fn webdl_corrupt_gzip_deletes_partial_file_and_retries() {
        let out_path = format!("/tmp/rg_webdl_badgz_{}_{}.nzb", std::process::id(), 219);
        let bad_resp =
            b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 8\r\n\r\nNOT_GZIP"
                .to_vec();
        let mut io = MockWebIo::new(vec![bad_resp]);
        let cfg = WebDlConfig {
            initial_url: b"http://127.0.0.1/bad.nzb",
            output_filename: out_path.as_bytes(),
            info_name: b"BadGz",
            version: b"24.0",
            write_buffer_kb: 4,
            url_retries: 1,
            url_interval_sec: 0,
            retry: false,
            force: true,
            tls_disabled: false,
            gzip_disabled: false,
        };
        let st = webdl_download_with_redirects(5, &cfg, &mut io);
        assert_eq!(st, WebDlStatus::AdFatalError);
        assert!(!std::path::Path::new(&out_path).exists());
    }
}
