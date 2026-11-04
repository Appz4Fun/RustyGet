//! Hot-path helpers for `NntpConnection`, `TlsSocket`, `WebDownloader`,
//! `ServerPool`, `RemoteServer`, `WebProcessor`, and `XmlRpcProcessor`.

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

/// Returns whether `TlsSocket::Start` should verify the peer certificate.
pub fn tls_should_verify_peer(has_store: bool, cert_verif_level: i32, cv_none: i32) -> bool {
    has_store && cert_verif_level > cv_none
}

/// Returns whether an OpenSSL error code represents an explicit TLS handshake rejection.
#[allow(clippy::too_many_arguments)]
pub fn tls_is_explicit_handshake_rejection(
    err_lib: i32,
    err_lib_ssl: i32,
    reason: i32,
    r_handshake_fail: i32,
    r_proto_ver: i32,
    r_no_shared_cipher: i32,
    r_unsupported_proto: i32,
) -> bool {
    err_lib == err_lib_ssl
        && (reason == r_handshake_fail
            || reason == r_proto_ver
            || reason == r_no_shared_cipher
            || reason == r_unsupported_proto)
}

/// Returns whether `TlsSocket::ValidateCert` should perform hostname verification.
pub fn tls_should_check_host(cert_verif_level: i32, cv_minimal: i32, host_empty: bool) -> bool {
    cert_verif_level > cv_minimal && !host_empty
}

/// Classifies an `SSL_get_error` code for `SSL_ERROR_WANT_READ` / `SSL_ERROR_WANT_WRITE`.
/// Returns `(retryable, want_read, want_write)`.
pub fn tls_classify_want_io(
    ssl_err: i32,
    want_read: i32,
    want_write: i32,
) -> (bool, bool, bool) {
    let r = ssl_err == want_read;
    let w = ssl_err == want_write;
    (r || w, r, w)
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

/// Selects `SecurePort` when `tls` is enabled, otherwise `ControlPort`.
pub fn remote_server_select_port(tls: bool, secure_port: i32, control_port: i32) -> i32 {
    if tls {
        secure_port
    } else {
        control_port
    }
}

/// Evaluates whether `RemoteServer::Run` has reached the active request processor cap
/// and whether the rate-limited warning should be logged (`now - last_warn >= 60`).
pub fn remote_server_should_warn_cap(
    active_count: i32,
    max_processors: i32,
    now: i64,
    last_warn: i64,
) -> (bool, bool) {
    let at_cap = active_count >= max_processors;
    let should_warn = at_cap && now.saturating_sub(last_warn) >= 60;
    (at_cap, should_warn)
}

/// Returns the maximum allowed HTTP POST `Content-Length` for `WebProcessor::Execute`.
pub fn web_proc_max_post_content_len(authorized: bool) -> i32 {
    if authorized {
        768 * 1024 * 1024
    } else {
        64 * 1024
    }
}

/// Validates whether an HTTP POST `Content-Length` is within bounds for `authorized`.
pub fn web_proc_is_content_len_valid(content_len: i32, authorized: bool) -> bool {
    content_len >= 0 && content_len <= web_proc_max_post_content_len(authorized)
}

/// Returns whether `WebProcessor::SendBodyResponse` should attempt gzip compression.
pub fn web_proc_should_use_gzip(
    gzip_enabled: bool,
    body_len: i32,
    max_uncompressed_size: i32,
) -> bool {
    gzip_enabled && body_len > max_uncompressed_size
}

/// Returns `"index.html\0"` when `url` ends with `'/'`, else `"\0"`.
pub fn web_proc_default_resource(url: &[u8]) -> &'static [u8] {
    if url.last() == Some(&b'/') {
        b"index.html\0"
    } else {
        b"\0"
    }
}

/// Computes `MiB/s` from `bytes` and `ms` in `XmlRpc.cpp`.
pub fn xmlrpc_mib_per_sec(bytes: u64, ms: f64) -> f64 {
    if ms > 0.0 {
        (bytes as f64 / 1024.0 / 1024.0) / (ms / 1000.0)
    } else {
        0.0
    }
}

/// Checks whether `XmlRpcProcessor::CreateCommand` should deny `method_name` for `user_access`.
/// Returns `1` if `uaAdd` is denied, `2` if `uaRestricted` is denied, or `0` if allowed.
pub fn xmlrpc_check_user_access_denied(
    user_access: i32,
    ua_add: i32,
    ua_restricted: i32,
    method_name: &[u8],
) -> i32 {
    if user_access == ua_add {
        let allowed = method_name.eq_ignore_ascii_case(b"append")
            || method_name.eq_ignore_ascii_case(b"appendurl")
            || method_name.eq_ignore_ascii_case(b"appendfleet")
            || method_name.eq_ignore_ascii_case(b"version");
        if !allowed {
            return 1;
        }
    } else if user_access == ua_restricted {
        let forbidden = method_name.eq_ignore_ascii_case(b"saveconfig")
            || method_name.eq_ignore_ascii_case(b"tlscipherstart")
            || method_name.eq_ignore_ascii_case(b"tlscipherstatus")
            || method_name.eq_ignore_ascii_case(b"tlsciphercancel");
        if forbidden {
            return 2;
        }
    }
    0
}

/// Returns `"RECIPIENT\0"`, `"DONOR\0"`, or `"NONE\0"` for `ExParStatus` in `NzbInfoXmlCommand`.
pub fn xmlrpc_ex_par_status(extra_par_blocks: i32) -> &'static [u8] {
    if extra_par_blocks > 0 {
        b"RECIPIENT\0"
    } else if extra_par_blocks < 0 {
        b"DONOR\0"
    } else {
        b"NONE\0"
    }
}

/// Parses `"score"`, `"all"`, or `"force"` (case-insensitive) into `EDupeMode`, or `-1` on invalid.
pub fn xmlrpc_parse_dupe_mode(
    dupe_mode_str: &[u8],
    dm_score: i32,
    dm_all: i32,
    dm_force: i32,
) -> i32 {
    if dupe_mode_str.eq_ignore_ascii_case(b"score") {
        dm_score
    } else if dupe_mode_str.eq_ignore_ascii_case(b"all") {
        dm_all
    } else if dupe_mode_str.eq_ignore_ascii_case(b"force") {
        dm_force
    } else {
        -1
    }
}

/// Returns whether `content` begins with `"http://"` or `"https://"` (case-insensitive).
pub fn xmlrpc_is_http_url(content: &[u8]) -> bool {
    (content.len() >= 7 && content[..7].eq_ignore_ascii_case(b"http://"))
        || (content.len() >= 8 && content[..8].eq_ignore_ascii_case(b"https://"))
}

/// Parses `CategorySource` (`"FeedFile"`, `"Auto"`, or default `NZBFile`) in `ViewFeedXmlCommand`.
pub fn xmlrpc_parse_category_source(
    cat_src_str: &[u8],
    cs_nzb_file: i32,
    cs_feed_file: i32,
    cs_auto: i32,
) -> i32 {
    if cat_src_str.eq_ignore_ascii_case(b"FeedFile") {
        cs_feed_file
    } else if cat_src_str.eq_ignore_ascii_case(b"Auto") {
        cs_auto
    } else {
        cs_nzb_file
    }
}

/// Classifies `counter` in `ResetServerVolumeXmlCommand`:
/// `1` for `""` (reset all), `2` for `"CUSTOM"` (reset custom), `0` for invalid.
pub fn xmlrpc_validate_reset_counter(counter: &[u8]) -> i32 {
    if counter.is_empty() {
        1
    } else if counter == b"CUSTOM" {
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nntp_and_tls_helpers() {
        assert_eq!(nntp_map_ip_version(4, 1, 2, 0), 1);
        assert_eq!(nntp_map_ip_version(6, 1, 2, 0), 2);
        assert_eq!(nntp_map_ip_version(0, 1, 2, 0), 0);

        assert!(tls_should_verify_peer(true, 1, 0));
        assert!(!tls_should_verify_peer(false, 1, 0));
        assert!(!tls_should_verify_peer(true, 0, 0));

        assert!(tls_is_explicit_handshake_rejection(20, 20, 102, 100, 101, 102, 103));
        assert!(!tls_is_explicit_handshake_rejection(10, 20, 102, 100, 101, 102, 103));
        assert!(!tls_is_explicit_handshake_rejection(20, 20, 999, 100, 101, 102, 103));

        assert!(tls_should_check_host(2, 1, false));
        assert!(!tls_should_check_host(1, 1, false));
        assert!(!tls_should_check_host(2, 1, true));

        assert_eq!(tls_classify_want_io(2, 2, 3), (true, true, false));
        assert_eq!(tls_classify_want_io(3, 2, 3), (true, false, true));
        assert_eq!(tls_classify_want_io(5, 2, 3), (false, false, false));
    }

    #[test]
    fn webdl_and_server_pool_helpers() {
        assert_eq!(webdl_calc_initial_retries(5, false), (1, 1));
        assert_eq!(webdl_calc_initial_retries(5, true), (5, 10));
        assert_eq!(webdl_calc_initial_retries(15, true), (15, 15));
        assert_eq!(webdl_calc_initial_retries(0, true), (1, 10));

        assert!(webdl_should_wait_before_retry(2, 2, 6, 2, 0, false, false, false));
        assert!(webdl_should_wait_before_retry(6, 2, 6, 0, 2, false, true, true));
        assert!(!webdl_should_wait_before_retry(2, 2, 6, 1, 0, false, false, false));
        assert!(!webdl_should_wait_before_retry(2, 2, 6, 2, 0, false, false, true));
        assert!(!webdl_should_wait_before_retry(2, 2, 6, 2, 0, true, true, false));

        assert!(webdl_is_default_http_port(b"HTTP", 80));
        assert!(webdl_is_default_http_port(b"https", 443));
        assert!(webdl_is_default_http_port(b"https", 0));
        assert!(!webdl_is_default_http_port(b"http", 8080));

        assert!(webdl_is_header_end_line(b'\r'));
        assert!(webdl_is_header_end_line(b'\n'));
        assert!(!webdl_is_header_end_line(b'C'));

        assert_eq!(webdl_strip_query_len(b"https://a/b?token=secret"), 11);
        assert_eq!(webdl_strip_query_len(b"https://a/b"), 11);

        assert!(!server_pool_can_advance_level(0, 0, true, false, false));
        assert!(server_pool_can_advance_level(0, 0, true, true, true));
        assert!(server_pool_can_advance_level(1, 0, true, false, false));

        assert!(server_pool_match_want_server(false, 2, 1, 0, 0));
        assert!(server_pool_match_want_server(true, 1, 1, 0, 0));
        assert!(server_pool_match_want_server(true, 2, 1, 3, 3));
        assert!(!server_pool_match_want_server(true, 2, 1, 0, 0));

        assert!(server_pool_match_ignore_server(1, 1, 0, 0, 0, 1));
        assert!(server_pool_match_ignore_server(2, 1, 4, 4, 2, 2));
        assert!(!server_pool_match_ignore_server(2, 1, 4, 4, 2, 3));

        assert_eq!(server_pool_calc_remaining_block_sec(0, 60, 100), 0);
        assert_eq!(server_pool_calc_remaining_block_sec(100, 60, 120), 40);
        assert_eq!(server_pool_calc_remaining_block_sec(100, 60, 170), 0);
    }

    #[test]
    fn remote_web_and_xmlrpc_helpers() {
        assert_eq!(remote_server_select_port(true, 6791, 6789), 6791);
        assert_eq!(remote_server_select_port(false, 6791, 6789), 6789);

        assert_eq!(remote_server_should_warn_cap(64, 64, 200, 100), (true, true));
        assert_eq!(remote_server_should_warn_cap(64, 64, 150, 100), (true, false));
        assert_eq!(remote_server_should_warn_cap(10, 64, 200, 100), (false, false));

        assert!(web_proc_is_content_len_valid(1024, false));
        assert!(!web_proc_is_content_len_valid(100 * 1024, false));
        assert!(web_proc_is_content_len_valid(100 * 1024, true));
        assert!(!web_proc_is_content_len_valid(-1, true));

        assert!(web_proc_should_use_gzip(true, 501, 500));
        assert!(!web_proc_should_use_gzip(true, 500, 500));

        assert_eq!(web_proc_default_resource(b"/"), b"index.html\0");
        assert_eq!(web_proc_default_resource(b"/app.js"), b"\0");

        assert!((xmlrpc_mib_per_sec(10 * 1024 * 1024, 500.0) - 20.0).abs() < 1e-9);
        assert_eq!(xmlrpc_mib_per_sec(10 * 1024 * 1024, 0.0), 0.0);

        assert_eq!(xmlrpc_check_user_access_denied(2, 2, 1, b"append"), 0);
        assert_eq!(xmlrpc_check_user_access_denied(2, 2, 1, b"status"), 1);
        assert_eq!(xmlrpc_check_user_access_denied(1, 2, 1, b"saveconfig"), 2);
        assert_eq!(xmlrpc_check_user_access_denied(1, 2, 1, b"status"), 0);

        assert_eq!(xmlrpc_ex_par_status(2), b"RECIPIENT\0");
        assert_eq!(xmlrpc_ex_par_status(-1), b"DONOR\0");
        assert_eq!(xmlrpc_ex_par_status(0), b"NONE\0");

        assert_eq!(xmlrpc_parse_dupe_mode(b"SCORE", 0, 1, 2), 0);
        assert_eq!(xmlrpc_parse_dupe_mode(b"all", 0, 1, 2), 1);
        assert_eq!(xmlrpc_parse_dupe_mode(b"Force", 0, 1, 2), 2);
        assert_eq!(xmlrpc_parse_dupe_mode(b"bad", 0, 1, 2), -1);

        assert!(xmlrpc_is_http_url(b"https://example.com/a.nzb"));
        assert!(!xmlrpc_is_http_url(b"ftp://example.com"));

        assert_eq!(xmlrpc_parse_category_source(b"FeedFile", 0, 1, 2), 1);
        assert_eq!(xmlrpc_parse_category_source(b"auto", 0, 1, 2), 2);
        assert_eq!(xmlrpc_parse_category_source(b"", 0, 1, 2), 0);

        assert_eq!(xmlrpc_validate_reset_counter(b""), 1);
        assert_eq!(xmlrpc_validate_reset_counter(b"CUSTOM"), 2);
        assert_eq!(xmlrpc_validate_reset_counter(b"other"), 0);
    }
}
