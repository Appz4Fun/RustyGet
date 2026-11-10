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
        return Response { result: 2, warn: 1, ..Default::default() };
    };
    let line = response.to_bytes();
    let space = line.iter().position(|&b| b == b' ');
    let Some(space) = space.filter(|_| line.starts_with(b"HTTP")) else {
        return Response { result: 4, warn: 2, ..Default::default() };
    };
    let at = space + 1;
    let status = &line[at..];
    let http_status = crate::feed::c_atoi_bytes(status);
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

/// ProcessHeader: 0 nothing, 1 Content-Length (its integer value in the second
/// value), 2 gzip, 3 Content-Disposition, 4 Location while redirecting (the
/// address from the offset in the second value).
pub fn process_header(line: &CStr, redirecting: bool) -> (i32, i32) {
    let bytes = line.to_bytes();
    let starts = |prefix: &[u8]| bytes.len() >= prefix.len() && bytes[..prefix.len()].eq_ignore_ascii_case(prefix);
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

/// Port of `Network::SpeedTest::RunTest` (`daemon/connect/NetworkSpeedTest.h`).
/// `on_log(0, msg)` emits `info`, `on_log(1, msg)` emits `error`.
pub fn run_network_speed_test<E, L>(
    mut execute_iter: E,
    mut on_log: L,
) -> Result<f64, &'static str>
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
        assert!(logs.iter().any(|(k, m)| *k == 0 && m == "Download speed test starting..."));
        assert!(logs.iter().any(|(k, m)| *k == 0 && m.starts_with("Download speed ")));

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
}
