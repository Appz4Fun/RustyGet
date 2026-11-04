//! XmlRpc, Options, Log, and NString hot-path decision kernels.
//!
//! Covers:
//! - `XmlCommand::JsonStep` & `XmlCommand::BoolToStr`
//! - `SetDownloadRateXmlCommand::Execute` download-rate bounds & KiB-to-B scaling
//! - `StatusXmlCommand::Execute` average download rate calculation
//! - `NzbInfoXmlCommand::AppendNzbInfoFields` message count fallback & post total time
//! - `NzbInfoXmlCommand::AppendPostInfoFields` cached message slice bounds
//! - `SaveConfigXmlCommand::Execute` JSON key-pair validation & `_TlsCipherBaseline` validation
//! - `ViewFeedXmlCommand::Execute` match filter & MiB conversion
//! - `TestServerXmlCommand::Execute` effective timeout & ARTICLE response status check
//! - `Options::CheckDir` relative-path & parent-dir separator join mode
//! - `Options::InitOptions` dupe/memory-pressure option clamping & normalization
//! - `Options::InitServers`, `Options::InitCategories`, and `Options::InitFeeds` definition completeness
//! - `Options::CheckOptions` write-flag reconciliation
//! - `Log::IntervalCheck` idle close check & `Log::RotateLog` old log deletion threshold
//! - `BString::Set`, `BString::Append`, `CString::Replace`, and `CString::TrimRight` bounds

use std::os::raw::{c_char, c_int};

extern "C" {
    fn strcasecmp(s1: *const c_char, s2: *const c_char) -> c_int;
}

/// `XmlCommand::JsonStep`: returns `0` when the byte following a JSON value
/// is `']'` or `'\0'`, otherwise `1`.
pub fn xmlrpc_json_step(next_char: u8) -> i32 {
    if next_char == b']' || next_char == 0 {
        0
    } else {
        1
    }
}

/// `XmlCommand::BoolToStr`: returns `"true\0"` / `"false\0"` in JSON mode,
/// or `"1\0"` / `"0\0"` in XML-RPC mode.
pub fn xmlrpc_bool_to_str(is_json: bool, value: bool) -> &'static [u8] {
    if is_json {
        if value {
            b"true\0"
        } else {
            b"false\0"
        }
    } else if value {
        b"1\0"
    } else {
        b"0\0"
    }
}

/// `SetDownloadRateXmlCommand::Execute`: validates that `rate` (in KiB/s) is
/// in `0..=i32::MAX / 1024` and computes `rate * 1024` (in B/s).
pub fn xmlrpc_validate_download_rate(rate: i32) -> (bool, i32) {
    if rate < 0 || rate > i32::MAX / 1024 {
        (false, 0)
    } else {
        (true, rate * 1024)
    }
}

/// `StatusXmlCommand::Execute`: computes `averageDownloadRate` (`allBytes / downloadTimeSec`
/// when `downloadTimeSec > 0`, otherwise `0`).
pub fn xmlrpc_calc_average_download_rate(download_time_sec: i32, all_bytes: i64) -> i64 {
    if download_time_sec > 0 {
        all_bytes / i64::from(download_time_sec)
    } else {
        0
    }
}

/// `NzbInfoXmlCommand::AppendNzbInfoFields`: resolves effective `messageCount`
/// (`message_count > 0 ? message_count : cached_message_count`) and
/// `postTotalTimeSec` (`post_total_sec + (has_post_start_time ? cur_time - post_start_time : 0)`).
pub fn xmlrpc_nzb_message_and_post_time(
    message_count: i32,
    cached_message_count: i32,
    post_total_sec: i32,
    has_post_start_time: bool,
    cur_time: i64,
    post_start_time: i64,
) -> (i32, i32) {
    let eff_msg_count = if message_count > 0 {
        message_count
    } else {
        cached_message_count
    };
    let active_post_sec = if has_post_start_time {
        cur_time.wrapping_sub(post_start_time)
    } else {
        0
    };
    let post_total_time_sec = i64::from(post_total_sec).wrapping_add(active_post_sec) as i32;
    (eff_msg_count, post_total_time_sec)
}

/// `NzbInfoXmlCommand::AppendPostInfoFields`: computes `(should_iter, start, clamped_log_entries)`
/// for cached post-processing messages when `log_entries > 0` and `messages_size > 0`.
pub fn xmlrpc_post_info_log_slice(log_entries: i32, messages_size: i32) -> (bool, i32, i32) {
    if log_entries > 0 && messages_size > 0 {
        let clamped = log_entries.min(messages_size);
        let start = messages_size - clamped;
        (true, start, clamped)
    } else {
        (false, 0, log_entries)
    }
}

/// `SaveConfigXmlCommand::Execute`: validates JSON option entry keys `(key1, key2)`
/// using C `strcasecmp` for active `LC_CTYPE` locale fidelity.
/// Returns:
/// - `1` when `(key1, key2)` matches `("Name", "Value")` (no swap needed)
/// - `2` when `(key1, key2)` matches `("Value", "Name")` (swap `name` and `value`)
/// - `0` when malformed
///
/// # Safety
/// `key1` and `key2` must be null or valid NUL-terminated C strings.
pub unsafe fn xmlrpc_save_config_check_json_keys(
    key1: *const c_char,
    key2: *const c_char,
) -> i32 {
    if key1.is_null() || key2.is_null() {
        return 0;
    }
    let name_lit = b"Name\0".as_ptr().cast::<c_char>();
    let value_lit = b"Value\0".as_ptr().cast::<c_char>();
    if strcasecmp(key1, value_lit) == 0 && strcasecmp(key2, name_lit) == 0 {
        2
    } else if strcasecmp(key1, name_lit) == 0 && strcasecmp(key2, value_lit) == 0 {
        1
    } else {
        0
    }
}

/// `SaveConfigXmlCommand::Execute`: checks that the raw (undecoded) `_TlsCipherBaseline`
/// value does not contain `"\\u0000"` or `"&#"`.
pub fn xmlrpc_save_config_raw_baseline_valid(raw_value: &[u8]) -> bool {
    !raw_value.windows(6).any(|w| w == b"\\u0000")
        && !raw_value.windows(2).any(|w| w == b"&#")
}

/// `SaveConfigXmlCommand::Execute`: checks that the decoded `_TlsCipherBaseline`
/// value is the first baseline entry (`!has_cipher_baseline`), is at most `1 MiB`,
/// and begins with `"v1\n"`.
pub fn xmlrpc_save_config_decoded_baseline_valid(
    has_cipher_baseline: bool,
    decoded_value: &[u8],
) -> bool {
    !has_cipher_baseline
        && decoded_value.len() <= 1024 * 1024
        && decoded_value.starts_with(b"v1\n")
}

/// `ViewFeedXmlCommand::Execute`: checks whether a feed item should be included
/// (`include_non_matching || match_status == ms_accepted`) and computes `sizeMB`.
pub fn xmlrpc_view_feed_should_include(
    include_non_matching: bool,
    match_status: i32,
    ms_accepted: i32,
    size_bytes: i64,
) -> (bool, i32) {
    let include = include_non_matching || match_status == ms_accepted;
    let size_mb = if include {
        (size_bytes / 1024 / 1024) as i32
    } else {
        0
    };
    (include, size_mb)
}

/// `TestServerXmlCommand::Execute`: resolves effective connection timeout
/// (`param_timeout == 0 ? article_timeout : param_timeout`).
pub fn xmlrpc_test_server_effective_timeout(param_timeout: i32, article_timeout: i32) -> i32 {
    if param_timeout == 0 {
        article_timeout
    } else {
        param_timeout
    }
}

/// `TestServerXmlCommand::Execute`: checks whether the NNTP `ARTICLE` response
/// starts with `'4'` or `'2'`.
pub fn xmlrpc_test_server_response_ok(has_response: bool, first_char: u8) -> bool {
    has_response && (first_char == b'4' || first_char == b'2')
}

/// `Options::CheckDir`: determines whether `dir` is relative to a non-empty
/// `parent_dir` and how they should be joined:
/// - `0`: `dir` is already absolute (starts with `path_sep`, `alt_path_sep`, or `<drive>:`)
///   or `parent_dir` is empty
/// - `1`: `parent_dir` already ends with `path_sep` or `alt_path_sep` (format `"%s%s"`)
/// - `2`: `parent_dir` needs a `path_sep` inserted between `parent_dir` and `dir` (format `"%s%c%s"`)
pub fn options_check_dir_join_mode(
    dir: &[u8],
    parent_dir: &[u8],
    path_sep: u8,
    alt_path_sep: u8,
) -> i32 {
    if parent_dir.is_empty() {
        return 0;
    }
    let d0 = dir.first().copied().unwrap_or(0);
    let d1 = dir.get(1).copied().unwrap_or(0);
    let is_abs = d0 == path_sep || d0 == alt_path_sep || (d0 != 0 && d1 == b':');
    if is_abs {
        return 0;
    }
    let last_parent = parent_dir[parent_dir.len() - 1];
    if last_parent == path_sep || last_parent == alt_path_sep {
        1
    } else {
        2
    }
}

/// `Options::InitOptions`: normalizes and clamps `MemoryPressureInterval` and
/// the `Dupe*` integer options.
#[allow(clippy::too_many_arguments)]
pub fn options_normalize_dupe_and_mem_options(
    mem_pressure_interval: i32,
    dupe_stream_timeout: i32,
    dupe_search_delay: i32,
    dupe_search_max_donors: i32,
    dupe_health_percent: i32,
    dupe_health_min: i32,
    dupe_health_max: i32,
    dupe_body_checks: i32,
    dupe_health_budget: i32,
    dupe_min_alive: i32,
    dupe_fast_donors: i32,
) -> (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32) {
    let norm_mem = if mem_pressure_interval > 0 {
        mem_pressure_interval
    } else {
        500
    };
    let norm_stream_timeout = dupe_stream_timeout.max(0);
    let norm_search_delay = dupe_search_delay.max(0);
    let norm_max_donors = dupe_search_max_donors.max(0);
    let norm_health_pct = dupe_health_percent.max(1).min(100);
    let norm_health_min = dupe_health_min.max(1);
    let norm_health_max = dupe_health_max.max(norm_health_min);
    let norm_body_checks = dupe_body_checks.max(0);
    let norm_health_budget = dupe_health_budget.max(5);
    let norm_min_alive = dupe_min_alive.max(0).min(100);
    let norm_fast_donors = dupe_fast_donors.max(0);
    (
        norm_mem,
        norm_stream_timeout,
        norm_search_delay,
        norm_max_donors,
        norm_health_pct,
        norm_health_min,
        norm_health_max,
        norm_body_checks,
        norm_health_budget,
        norm_min_alive,
        norm_fast_donors,
    )
}

/// `Options::InitServers`: classifies server option definition completeness:
/// - `0`: no field defined (stop iterating)
/// - `1`: complete (`has_host && has_port && has_connections`)
/// - `2`: incomplete definition (emit `ConfigError`)
pub fn options_check_server_def(
    has_any_field: bool,
    has_host: bool,
    has_port: bool,
    has_connections: bool,
) -> i32 {
    if !has_any_field {
        0
    } else if has_host && has_port && has_connections {
        1
    } else {
        2
    }
}

/// `Options::InitCategories` & `Options::InitFeeds`: classifies category or feed
/// definition completeness:
/// - `0`: no field defined (stop iterating)
/// - `1`: complete (`has_required`)
/// - `2`: incomplete definition (emit `ConfigError`)
pub fn options_check_category_or_feed_def(has_any_field: bool, has_required: bool) -> i32 {
    if !has_any_field {
        0
    } else if has_required {
        1
    } else {
        2
    }
}

/// `Options::CheckOptions`: reconciles `DirectWrite` (disabled when `RawArticle`
/// is enabled) and `DirectRename` (disabled when `SkipWrite` is enabled).
pub fn options_reconcile_write_flags(
    raw_article: bool,
    skip_write: bool,
    direct_write: bool,
    direct_rename: bool,
) -> (bool, bool) {
    let out_direct_write = if raw_article { false } else { direct_write };
    let out_direct_rename = if skip_write { false } else { direct_rename };
    (out_direct_write, out_direct_rename)
}

/// `Log::IntervalCheck` & `Log::RotateLog`:
/// - `log_should_close_idle`: returns `true` when `|cur_time - last_written| > 1`
/// - `log_should_delete_old_rotated`: returns `true` when `file_day <= cur_day - rotate_log`
pub fn log_should_close_idle(cur_time: i64, last_written: i64) -> bool {
    cur_time.wrapping_sub(last_written).saturating_abs() > 1
}

pub fn log_should_delete_old_rotated(file_day: i32, cur_day: i32, rotate_log: i32) -> bool {
    file_day <= cur_day.wrapping_sub(rotate_log)
}

/// `BString<size>::Set` & `BString<size>::Append`: computes bounded copy length.
pub fn nstring_bstring_set_len(size: i32, len: i32) -> i32 {
    let cap = size.saturating_sub(1).max(0);
    if len > 0 {
        cap.min(len)
    } else {
        cap
    }
}

pub fn nstring_bstring_append_len(size: i32, cur_len: i32, str_len: i32) -> i32 {
    let avail = size.wrapping_sub(cur_len).wrapping_sub(1);
    avail.min(str_len)
}

/// `CString::Replace(int pos, int len, const char* str, int strLen)`:
/// computes `(valid, add_len, del_len, new_len)` where `valid` is `pos <= cur_len`.
pub fn nstring_cstring_replace_bounds(
    pos: i32,
    len: i32,
    cur_len: i32,
    raw_str_len: i32,
    str_len_param: i32,
) -> (bool, i32, i32, i32) {
    if pos > cur_len {
        return (false, 0, 0, 0);
    }
    let add_len = if str_len_param > 0 {
        raw_str_len.min(str_len_param)
    } else {
        raw_str_len
    };
    let del_len = if pos.wrapping_add(len) <= cur_len {
        len
    } else {
        cur_len.wrapping_sub(pos)
    };
    let new_len = cur_len.wrapping_sub(del_len).wrapping_add(add_len);
    (true, add_len, del_len, new_len)
}

/// `CString::TrimRight`: returns the length of `s` after stripping trailing
/// `'\n'`, `'\r'`, `' '`, and `'\t'` bytes.
pub fn nstring_cstring_trim_right_len(s: &[u8]) -> usize {
    let mut end = s.len();
    while end > 0 {
        let ch = s[end - 1];
        if ch == b'\n' || ch == b'\r' || ch == b' ' || ch == b'\t' {
            end -= 1;
        } else {
            break;
        }
    }
    end
}

/// Finds the byte offset of `"</struct>"` in `req` for `XmlRpcProcessor::MutliCall`.
pub fn xmlrpc_multicall_find_struct_end(req: &[u8]) -> Option<usize> {
    req.windows(9).position(|w| w == b"</struct>")
}

/// Scans a NUL-terminated C string linearly until `"</struct>"` is matched or NUL is reached,
/// avoiding a full-buffer `strlen` on each multicall iteration.
///
/// # Safety
/// `request_ptr` must be null or point to a valid NUL-terminated C string.
pub unsafe fn xmlrpc_multicall_find_struct_end_cstr(
    request_ptr: *const c_char,
) -> Option<usize> {
    if request_ptr.is_null() {
        return None;
    }
    const NEEDLE: &[u8; 9] = b"</struct>";
    let base = request_ptr.cast::<u8>();
    let mut i = 0usize;
    loop {
        let mut matched = 0usize;
        while matched < NEEDLE.len() {
            let b = *base.add(i + matched);
            if b == 0 {
                return None;
            }
            if b != NEEDLE[matched] {
                break;
            }
            matched += 1;
        }
        if matched == NEEDLE.len() {
            return Some(i);
        }
        if *base.add(i) == 0 {
            return None;
        }
        i += 1;
    }
}

/// Locates `"</name>"` and optional `"params</name>"` within a `<struct>` slice for
/// `XmlRpcProcessor::MutliCall`. Returns `(name_end_off, params_start_off_or_zero)`.
pub fn xmlrpc_multicall_locate_parts(call_slice: &[u8]) -> Option<(usize, usize)> {
    let name_off = call_slice.windows(7).position(|w| w == b"</name>")?;
    let params_off = call_slice[name_off..]
        .windows(13)
        .position(|w| w == b"params</name>")
        .map(|rel| name_off + rel)
        .unwrap_or(0);
    Some((name_off, params_off))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_xmlrpc_json_step_and_bool_to_str() {
        assert_eq!(xmlrpc_json_step(b']'), 0);
        assert_eq!(xmlrpc_json_step(0), 0);
        assert_eq!(xmlrpc_json_step(b','), 1);
        assert_eq!(xmlrpc_json_step(b' '), 1);

        assert_eq!(xmlrpc_bool_to_str(true, true), b"true\0");
        assert_eq!(xmlrpc_bool_to_str(true, false), b"false\0");
        assert_eq!(xmlrpc_bool_to_str(false, true), b"1\0");
        assert_eq!(xmlrpc_bool_to_str(false, false), b"0\0");
    }

    #[test]
    fn test_xmlrpc_download_rate_and_avg_rate() {
        assert_eq!(xmlrpc_validate_download_rate(-1), (false, 0));
        assert_eq!(xmlrpc_validate_download_rate(0), (true, 0));
        assert_eq!(xmlrpc_validate_download_rate(100), (true, 102_400));
        assert_eq!(
            xmlrpc_validate_download_rate(i32::MAX / 1024),
            (true, (i32::MAX / 1024) * 1024)
        );
        assert_eq!(
            xmlrpc_validate_download_rate(i32::MAX / 1024 + 1),
            (false, 0)
        );

        assert_eq!(xmlrpc_calc_average_download_rate(0, 10_000), 0);
        assert_eq!(xmlrpc_calc_average_download_rate(-5, 10_000), 0);
        assert_eq!(xmlrpc_calc_average_download_rate(10, 50_000), 5_000);
    }

    #[test]
    fn test_xmlrpc_nzb_and_post_info_helpers() {
        assert_eq!(
            xmlrpc_nzb_message_and_post_time(12, 5, 100, true, 1050, 1000),
            (12, 150)
        );
        assert_eq!(
            xmlrpc_nzb_message_and_post_time(0, 5, 100, false, 1050, 0),
            (5, 100)
        );

        assert_eq!(xmlrpc_post_info_log_slice(0, 10), (false, 0, 0));
        assert_eq!(xmlrpc_post_info_log_slice(5, 0), (false, 0, 5));
        assert_eq!(xmlrpc_post_info_log_slice(5, 20), (true, 15, 5));
        assert_eq!(xmlrpc_post_info_log_slice(25, 20), (true, 0, 20));
    }

    #[test]
    fn test_xmlrpc_save_config_helpers() {
        let name = CString::new("Name").unwrap();
        let value = CString::new("Value").unwrap();
        let name_lo = CString::new("name").unwrap();
        let value_up = CString::new("VALUE").unwrap();
        let other = CString::new("Other").unwrap();

        unsafe {
            assert_eq!(
                xmlrpc_save_config_check_json_keys(name.as_ptr(), value.as_ptr()),
                1
            );
            assert_eq!(
                xmlrpc_save_config_check_json_keys(name_lo.as_ptr(), value_up.as_ptr()),
                1
            );
            assert_eq!(
                xmlrpc_save_config_check_json_keys(value.as_ptr(), name.as_ptr()),
                2
            );
            assert_eq!(
                xmlrpc_save_config_check_json_keys(other.as_ptr(), value.as_ptr()),
                0
            );
            assert_eq!(
                xmlrpc_save_config_check_json_keys(std::ptr::null(), value.as_ptr()),
                0
            );
        }

        assert!(xmlrpc_save_config_raw_baseline_valid(b"v1\\nfoo"));
        assert!(!xmlrpc_save_config_raw_baseline_valid(b"v1\\u0000foo"));
        assert!(!xmlrpc_save_config_raw_baseline_valid(b"v1&#0;foo"));

        assert!(xmlrpc_save_config_decoded_baseline_valid(false, b"v1\nabc"));
        assert!(!xmlrpc_save_config_decoded_baseline_valid(true, b"v1\nabc"));
        assert!(!xmlrpc_save_config_decoded_baseline_valid(false, b"v2\nabc"));
    }

    #[test]
    fn test_xmlrpc_feed_and_test_server_helpers() {
        assert_eq!(
            xmlrpc_view_feed_should_include(false, 1, 1, 10 * 1024 * 1024),
            (true, 10)
        );
        assert_eq!(
            xmlrpc_view_feed_should_include(false, 0, 1, 10 * 1024 * 1024),
            (false, 0)
        );
        assert_eq!(
            xmlrpc_view_feed_should_include(true, 0, 1, 5 * 1024 * 1024),
            (true, 5)
        );

        assert_eq!(xmlrpc_test_server_effective_timeout(0, 60), 60);
        assert_eq!(xmlrpc_test_server_effective_timeout(15, 60), 15);
        assert!(xmlrpc_test_server_response_ok(true, b'4'));
        assert!(xmlrpc_test_server_response_ok(true, b'2'));
        assert!(!xmlrpc_test_server_response_ok(true, b'5'));
        assert!(!xmlrpc_test_server_response_ok(false, b'2'));
    }

    #[test]
    fn test_options_helpers() {
        assert_eq!(options_check_dir_join_mode(b"/abs", b"/main", b'/', b'\\'), 0);
        assert_eq!(options_check_dir_join_mode(b"C:\\abs", b"/main", b'/', b'\\'), 0);
        assert_eq!(options_check_dir_join_mode(b"rel", b"", b'/', b'\\'), 0);
        assert_eq!(options_check_dir_join_mode(b"rel", b"/main/", b'/', b'\\'), 1);
        assert_eq!(options_check_dir_join_mode(b"rel", b"/main", b'/', b'\\'), 2);

        let norm = options_normalize_dupe_and_mem_options(0, -1, -5, -2, 150, 0, 3, -1, 2, 120, -4);
        assert_eq!(norm, (500, 0, 0, 0, 100, 1, 3, 0, 5, 100, 0));

        let norm2 = options_normalize_dupe_and_mem_options(250, 10, 5, 3, 25, 10, 4, 2, 15, -5, 2);
        assert_eq!(norm2, (250, 10, 5, 3, 25, 10, 10, 2, 15, 0, 2));

        assert_eq!(options_check_server_def(false, false, false, false), 0);
        assert_eq!(options_check_server_def(true, true, true, true), 1);
        assert_eq!(options_check_server_def(true, true, false, true), 2);

        assert_eq!(options_check_category_or_feed_def(false, false), 0);
        assert_eq!(options_check_category_or_feed_def(true, true), 1);
        assert_eq!(options_check_category_or_feed_def(true, false), 2);

        assert_eq!(options_reconcile_write_flags(true, false, true, true), (false, true));
        assert_eq!(options_reconcile_write_flags(false, true, true, true), (true, false));
    }

    #[test]
    fn test_log_and_nstring_helpers() {
        assert!(!log_should_close_idle(100, 99));
        assert!(log_should_close_idle(100, 98));
        assert!(log_should_delete_old_rotated(10, 13, 3));
        assert!(!log_should_delete_old_rotated(11, 13, 3));

        assert_eq!(nstring_bstring_set_len(100, 0), 99);
        assert_eq!(nstring_bstring_set_len(100, 50), 50);
        assert_eq!(nstring_bstring_set_len(100, 150), 99);

        assert_eq!(nstring_bstring_append_len(100, 40, 20), 20);
        assert_eq!(nstring_bstring_append_len(100, 90, 20), 9);

        assert_eq!(
            nstring_cstring_replace_bounds(10, 2, 5, 3, 0),
            (false, 0, 0, 0)
        );
        assert_eq!(
            nstring_cstring_replace_bounds(2, 3, 10, 5, 0),
            (true, 5, 3, 12)
        );
        assert_eq!(
            nstring_cstring_replace_bounds(8, 5, 10, 4, 2),
            (true, 2, 2, 10)
        );

        assert_eq!(nstring_cstring_trim_right_len(b"hello \t\r\n"), 5);
        assert_eq!(nstring_cstring_trim_right_len(b"   \n"), 0);
        assert_eq!(nstring_cstring_trim_right_len(b"abc"), 3);

        let xml = b"<struct><member><name>methodName</name><value><string>version</string></value></member><member><name>params</name><value><array/></value></member></struct>";
        assert_eq!(xmlrpc_multicall_find_struct_end(xml), Some(146));
        let c_xml = CString::new(&xml[..]).unwrap();
        unsafe {
            assert_eq!(
                xmlrpc_multicall_find_struct_end_cstr(c_xml.as_ptr()),
                Some(146)
            );
            assert_eq!(
                xmlrpc_multicall_find_struct_end_cstr(std::ptr::null()),
                None
            );
        }
        assert_eq!(
            xmlrpc_multicall_locate_parts(&xml[..146]),
            Some((32, 101))
        );
        assert_eq!(xmlrpc_multicall_locate_parts(b"<struct></struct>"), None);
    }
}
