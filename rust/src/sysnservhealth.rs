//! System (`SystemInfo`, `OS`, `CPU`), NServ (`YEncoder`, `NzbGenerator`,
//! `NServMain`), DupeSearch (`NntpHealthServer`, `HttpGet`, `XmlReader`),
//! Connect (`HttpClient`, `NetworkSpeedTest`), PostProcess (`PostDownloadRenamer`,
//! `PostUnpackRenamer`), and SystemHealth (`CheckAndRepairValidator`,
//! `LoggingValidator`, `FeedValidator`) hot-path routines ported from C++ to Rust.

use std::ffi::CStr;

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Port of `SystemInfo::GetPythonVersion` string extraction after initial `Util::Trim`.
/// Given `trimmed_line`, locates `"Python"`, skips `"Python"` + 1 byte (matching
/// `pos + sizeof("Python")`), trims ASCII whitespace/control bytes (`<= b' '`),
/// and returns `Some((start_offset, len))`.
pub fn sys_extract_python_version(trimmed_line: &[u8]) -> Option<(usize, usize)> {
    const MARKER: &[u8] = b"Python";
    const SKIP_LEN: usize = MARKER.len() + 1; // sizeof("Python") == 7
    let pos = find_subslice(trimmed_line, MARKER)?;
    if pos + SKIP_LEN >= trimmed_line.len() {
        return Some((trimmed_line.len(), 0));
    }
    let mut start = pos + SKIP_LEN;
    let mut end = trimmed_line.len();
    while start < end && trimmed_line[start] <= b' ' {
        start += 1;
    }
    while end > start && trimmed_line[end - 1] <= b' ' {
        end -= 1;
    }
    Some((start, end - start))
}

/// Port of `SystemInfo::ParseUnpackerVersion`.
/// Finds the first `[0-9]+\.[0-9]+` span in `line` and returns `Some((start, len))`.
pub fn sys_parse_unpacker_version(line: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    let n = line.len();
    while i < n {
        if line[i].is_ascii_digit() {
            let start = i;
            while i < n && line[i].is_ascii_digit() {
                i += 1;
            }
            if i < n && line[i] == b'.' && i + 1 < n && line[i + 1].is_ascii_digit() {
                i += 1;
                while i < n && line[i].is_ascii_digit() {
                    i += 1;
                }
                return Some((start, i - start));
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Port of `SystemInfo::ParseParVersion`.
/// Returns the index of the last `' '` byte in `line` (matching `line.find_last_of(" ")`).
pub fn sys_parse_par_version(line: &[u8]) -> Option<usize> {
    line.iter().rposition(|&b| b == b' ')
}

/// Port of `OS::TrimQuotes`.
/// Strips at most one leading `"` and at most one trailing `"` and returns `(start, len)`.
pub fn os_trim_quotes(s: &[u8]) -> (usize, usize) {
    let mut start = 0;
    let mut end = s.len();
    if start < end && s[start] == b'"' {
        start += 1;
    }
    if start < end && s[end - 1] == b'"' {
        end -= 1;
    }
    (start, end - start)
}

/// Port of `OS::Init` (`__APPLE__`) field extraction from `sw_vers` output.
/// Finds `prefix` in `output` and returns `(val_start, val_len)` up to the next `'\n'`.
pub fn os_parse_sw_vers_field(output: &[u8], prefix: &[u8]) -> Option<(usize, usize)> {
    let pos = find_subslice(output, prefix)?;
    let val_start = pos + prefix.len();
    let end_pos = output[pos..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|rel| pos + rel)
        .unwrap_or(output.len());
    let val_len = end_pos.saturating_sub(val_start);
    Some((val_start, val_len))
}

/// Port of `OS::Init` (`WIN32`) build-number to Windows version mapping.
pub fn os_map_windows_build(build_num: i64) -> Option<&'static CStr> {
    if build_num >= 22000 {
        Some(c"11")
    } else if build_num >= 10240 {
        Some(c"10")
    } else if build_num >= 9200 {
        Some(c"8")
    } else if build_num >= 7600 {
        Some(c"7")
    } else if build_num >= 2600 {
        Some(c"XP")
    } else {
        None
    }
}

/// Port of `CPU::GetCanonicalCPUArch`.
/// Returns `Some(canonical_cstr)` if `arch` maps to a canonical architecture, or `None`
/// if `arch` should be returned unchanged.
pub fn cpu_canonical_arch(arch: &[u8]) -> Option<&'static CStr> {
    if arch == b"i386" || arch == b"x86" {
        return Some(c"i686");
    }
    if arch == b"amd64" || arch == b"AMD64" {
        return Some(c"x86_64");
    }
    if contains_subslice(arch, b"armv5") || contains_subslice(arch, b"armv6") {
        return Some(c"armel");
    }
    if contains_subslice(arch, b"armv7") || contains_subslice(arch, b"armv8") {
        return Some(c"armhf");
    }
    None
}

/// Port of `CPU::GetCPUModelFromCPUInfo` line matching.
/// If `line` contains one of the recognized CPU model keys and a `':'`, returns
/// `Some(colon_pos + 1)`.
pub fn cpu_parse_cpuinfo_line(line: &[u8]) -> Option<usize> {
    let matched = contains_subslice(line, b"model name")
        || contains_subslice(line, b"Processor")
        || contains_subslice(line, b"cpu model")
        || contains_subslice(line, b"Hardware")
        || contains_subslice(line, b"system type");
    if !matched {
        return None;
    }
    let colon_pos = line.iter().position(|&b| b == b':')?;
    Some(colon_pos + 1)
}

/// Port of `NServOpts` verbosity target flag evaluation.
/// Returns `(mute_info_warn_err, mute_detail, enable_debug)`.
pub fn nserv_verbosity_flags(verbosity: i32) -> (bool, bool, bool) {
    (verbosity < 1, verbosity < 2, verbosity > 2)
}

/// Port of `NntpHealthServer::CapFor`.
pub fn nntp_health_cap_for(connections: i32, max_cap: i32) -> i32 {
    1.max(max_cap.min(connections / 2))
}

/// Port of `Bracketed` check in `NntpHealthServer.cpp`.
pub fn nntp_health_is_bracketed(msg_id: &[u8]) -> bool {
    !msg_id.is_empty() && msg_id.first() == Some(&b'<') && msg_id.last() == Some(&b'>')
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

/// Port of watchdog wait & stop calculation in `HttpGet::Fetch`.
/// Returns `(should_stop, wait_ms)`.
pub fn http_get_calc_watchdog_wait(
    deadline_ms: i64,
    now_ms: i64,
    file_size: i64,
    max_bytes: usize,
) -> (bool, i64) {
    let left = if deadline_ms > 0 {
        deadline_ms.saturating_sub(now_ms)
    } else {
        250
    };
    if left <= 0 || file_size > (max_bytes as i64) {
        (true, 0)
    } else {
        (false, left.min(250))
    }
}

/// Port of `LocalName` in `XmlReader.cpp`.
/// Returns the byte offset of the local name (after the last `':'`, or `0`).
pub fn xml_reader_local_name_offset(name: &[u8]) -> usize {
    match name.iter().rposition(|&b| b == b':') {
        Some(pos) => pos + 1,
        None => 0,
    }
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

/// Port of pre-loop plan classification in `PostDownloadRenamer::Controller::Run`
/// and `PostUnpackRenamer::Controller::Run`.
/// Returns:
/// - `0`: proceed with rename actions
/// - `1`: disc structure detected
/// - `2`: ambiguous multi-file collection detected
/// - `3`: no qualifying media file (`!canRename`)
/// - `4`: actions empty and target name is obfuscated
/// - `5`: actions empty and target name is clean
pub fn post_renamer_classify_plan(
    is_disc: bool,
    is_ambiguous: bool,
    can_rename: bool,
    actions_empty: bool,
    target_obfuscated: bool,
) -> i32 {
    if is_disc {
        1
    } else if is_ambiguous {
        2
    } else if !can_rename {
        3
    } else if actions_empty {
        if target_obfuscated {
            4
        } else {
            5
        }
    } else {
        0
    }
}

/// Port of post-loop final status decision in `PostDownloadRenamer::Controller::Run`
/// and `PostUnpackRenamer::Controller::Run`.
/// Returns:
/// - `1`: stopped / cancelled
/// - `2`: finished with errors (`Failure`)
/// - `3`: successful (`Success`)
/// - `4`: no files needed renaming (`Skipped`)
pub fn post_renamer_decide_final_status(
    is_stopped: bool,
    any_failed: bool,
    any_renamed: bool,
) -> i32 {
    if is_stopped {
        1
    } else if any_failed {
        2
    } else if any_renamed {
        3
    } else {
        4
    }
}

/// Port of `SystemHealth::CheckAndRepair::ParBufferValidator::Validate`.
/// Returns `0` (`Ok`), `1` (`< 0` error), or `2` (`< 250` MB info).
pub fn health_validate_par_buffer(par_repair: bool, buffer_mb: i32) -> i32 {
    if !par_repair {
        return 0;
    }
    if buffer_mb < 0 {
        return 1;
    }
    if buffer_mb < 250 {
        return 2;
    }
    0
}

/// Port of `SystemHealth::CheckAndRepair::ParThreadsValidator::Validate`.
/// Returns `0` (`Ok`), `1` (`< 0` error), or `2` (`threads > hw_threads` warning).
pub fn health_validate_par_threads(par_repair: bool, threads: i32, hw_threads: usize) -> i32 {
    if !par_repair {
        return 0;
    }
    if threads < 0 {
        return 1;
    }
    if threads == 0 {
        return 0;
    }
    if hw_threads > 0 && (threads as usize) > hw_threads {
        return 2;
    }
    0
}

/// Port of `SystemHealth::CheckAndRepair::ParTimeLimitValidator::Validate`.
/// Returns `0` (`Ok`), `1` (`< 0` error), or `2` (`1..=4` minutes info).
pub fn health_validate_par_time_limit(par_repair: bool, limit: i32) -> i32 {
    if !par_repair {
        return 0;
    }
    if limit < 0 {
        return 1;
    }
    if limit == 0 {
        return 0;
    }
    if limit < 5 {
        return 2;
    }
    0
}

/// Port of `SystemHealth::Logging::RotateLogValidator::Validate`.
/// Returns `0` (`Ok`), `1` (`< 0` error), or `2` (`> 365` warning).
pub fn health_validate_rotate_log(is_rotate: bool, rotate_log: i32) -> i32 {
    if !is_rotate {
        return 0;
    }
    if rotate_log < 0 {
        return 1;
    }
    if rotate_log > 365 {
        return 2;
    }
    0
}

/// Port of `SystemHealth::Logging::TimeCorrectionValidator::Validate`.
/// Returns `true` if `abs(val) > 1440 && abs(val) < 1000000`.
pub fn health_validate_time_correction(val: i32) -> bool {
    let abs_val = (val as i64).abs();
    abs_val > 1440 && abs_val < 1_000_000
}

/// Port of `SystemHealth::Feeds::UrlValidator::Validate`.
/// Returns `0` (`Ok`), `1` (empty warning), or `2` (missing `http://`/`https://` warning).
pub fn health_validate_feed_url(url: &[u8]) -> i32 {
    if url.is_empty() {
        return 1;
    }
    if !contains_subslice(url, b"http://") && !contains_subslice(url, b"https://") {
        return 2;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_info_and_os_cpu_helpers() {
        assert_eq!(
            sys_extract_python_version(b"Python 3.12.3"),
            Some((7, 6))
        );
        assert_eq!(sys_extract_python_version(b"Python"), Some((6, 0)));
        assert_eq!(sys_extract_python_version(b"NoPy"), None);

        let line = b"7-Zip (a) 19.00 (x64) : Copyright (c) 1999-2018";
        let (s, l) = sys_parse_unpacker_version(line).unwrap();
        assert_eq!(&line[s..s + l], b"19.00");
        assert_eq!(sys_parse_unpacker_version(b"no version here 12."), None);

        assert_eq!(
            sys_parse_par_version(b"par2cmdline-turbo version 1.1.1"),
            Some(25)
        );
        assert_eq!(sys_parse_par_version(b"nospace"), None);

        assert_eq!(os_trim_quotes(b"\"Debian GNU/Linux\""), (1, 16));
        assert_eq!(os_trim_quotes(b"\""), (1, 0));
        assert_eq!(os_trim_quotes(b"rolling"), (0, 7));

        let sw = b"ProductName:\tmacOS\nProductVersion:\t15.1\n";
        let (ns, nl) = os_parse_sw_vers_field(sw, b"ProductName:").unwrap();
        assert_eq!(&sw[ns..ns + nl], b"\tmacOS");
        let (vs, vl) = os_parse_sw_vers_field(sw, b"ProductVersion:").unwrap();
        assert_eq!(&sw[vs..vs + vl], b"\t15.1");

        assert_eq!(os_map_windows_build(22631), Some(c"11"));
        assert_eq!(os_map_windows_build(19045), Some(c"10"));
        assert_eq!(os_map_windows_build(9200), Some(c"8"));
        assert_eq!(os_map_windows_build(7601), Some(c"7"));
        assert_eq!(os_map_windows_build(2600), Some(c"XP"));
        assert_eq!(os_map_windows_build(1000), None);

        assert_eq!(cpu_canonical_arch(b"i386"), Some(c"i686"));
        assert_eq!(cpu_canonical_arch(b"AMD64"), Some(c"x86_64"));
        assert_eq!(cpu_canonical_arch(b"armv6l"), Some(c"armel"));
        assert_eq!(cpu_canonical_arch(b"armv8l"), Some(c"armhf"));
        assert_eq!(cpu_canonical_arch(b"aarch64"), None);

        assert_eq!(
            cpu_parse_cpuinfo_line(b"model name\t: AMD Ryzen 9"),
            Some(12)
        );
        assert_eq!(cpu_parse_cpuinfo_line(b"bogomips\t: 5000.0"), None);
    }

    #[test]
    fn test_nserv_dupesearch_connect_and_post_renamer() {
        assert_eq!(nserv_verbosity_flags(0), (true, true, false));
        assert_eq!(nserv_verbosity_flags(2), (false, false, false));
        assert_eq!(nserv_verbosity_flags(3), (false, false, true));

        assert_eq!(nntp_health_cap_for(10, 4), 4);
        assert_eq!(nntp_health_cap_for(1, 4), 1);
        assert!(nntp_health_is_bracketed(b"<abc@news>"));
        assert!(!nntp_health_is_bracketed(b"abc@news"));
        assert_eq!(nntp_health_classify_stat_reply(b"223 0 <id>"), 0);
        assert_eq!(nntp_health_classify_stat_reply(b"430 no such article"), 1);
        assert_eq!(nntp_health_classify_stat_reply(b"500 err"), 2);

        assert_eq!(http_get_calc_watchdog_wait(1000, 900, 50, 100), (false, 100));
        assert_eq!(http_get_calc_watchdog_wait(1000, 1000, 50, 100), (true, 0));
        assert_eq!(http_get_calc_watchdog_wait(0, 1000, 101, 100), (true, 0));

        assert_eq!(xml_reader_local_name_offset(b"newznab:attr"), 8);
        assert_eq!(xml_reader_local_name_offset(b"item"), 0);
        assert_eq!(http_client_find_header_colon(b"Content-Type: text/plain"), Some(12));
        assert!(http_client_validate_version(b"HTTP/1.1", true));
        assert!(!http_client_validate_version(b"HTTP/1.1", false));
        assert!(!http_client_validate_version(b"FTP/1.0", true));
        assert_eq!(
            http_client_build_headers(b"GET", b"example.com", b"24.0", true),
            b"GET / HTTP/1.1\r\nHost: example.com\r\nUser-Agent: nzbget/24.0\r\nAccept-Encoding: gzip\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );

        assert!((speed_test_calc_mbps(1_000_000, 1.0) - 8.0).abs() < 1e-9);
        assert!(speed_test_is_required_more(1, 10.0, 20.0));
        assert!(!speed_test_is_required_more(2, 10.0, 20.0));
        assert!(speed_test_exceeded_max_failures(3, 4));
        assert!(!speed_test_exceeded_max_failures(2, 4));
        assert!((speed_test_final_speed(50.0, &[40.0, 85.5, 70.0]) - 85.5).abs() < 1e-9);
        assert_eq!(
            speed_test_make_request(b"__down", b"?bytes=", 1000, b"speed.cloudflare.com"),
            b"GET /__down?bytes=1000 HTTP/1.1\r\nHost: speed.cloudflare.com\r\nConnection: close\r\n\r\n"
        );

        assert_eq!(post_renamer_classify_plan(true, false, true, false, false), 1);
        assert_eq!(post_renamer_classify_plan(false, false, true, true, true), 4);
        assert_eq!(post_renamer_classify_plan(false, false, true, false, false), 0);
        assert_eq!(post_renamer_decide_final_status(false, false, true), 3);
    }

    #[test]
    fn test_system_health_validators() {
        assert_eq!(health_validate_par_buffer(false, 100), 0);
        assert_eq!(health_validate_par_buffer(true, -1), 1);
        assert_eq!(health_validate_par_buffer(true, 100), 2);
        assert_eq!(health_validate_par_buffer(true, 250), 0);

        assert_eq!(health_validate_par_threads(true, -1, 8), 1);
        assert_eq!(health_validate_par_threads(true, 0, 8), 0);
        assert_eq!(health_validate_par_threads(true, 16, 8), 2);
        assert_eq!(health_validate_par_threads(true, 8, 8), 0);

        assert_eq!(health_validate_par_time_limit(true, -1), 1);
        assert_eq!(health_validate_par_time_limit(true, 0), 0);
        assert_eq!(health_validate_par_time_limit(true, 3), 2);
        assert_eq!(health_validate_par_time_limit(true, 5), 0);

        assert_eq!(health_validate_rotate_log(false, 500), 0);
        assert_eq!(health_validate_rotate_log(true, -1), 1);
        assert_eq!(health_validate_rotate_log(true, 400), 2);
        assert_eq!(health_validate_rotate_log(true, 30), 0);

        assert!(health_validate_time_correction(2000));
        assert!(!health_validate_time_correction(60));

        assert_eq!(health_validate_feed_url(b""), 1);
        assert_eq!(health_validate_feed_url(b"ftp://example.com"), 2);
        assert_eq!(health_validate_feed_url(b"https://example.com/rss"), 0);
    }
}
