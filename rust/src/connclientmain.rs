//! Hot-path calculations for `Connection`, `RemoteClient`, `NZBGet`, `TwinCheck`,
//! `QueueEditor`, and `NCursesFrontend`.
//!
//! Each helper matches the corresponding C++ routine byte-for-byte.

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

/// Checks whether `control_ip` is the wildcard `"0.0.0.0"` in `RemoteClient::InitConnection`.
pub fn remote_client_is_wildcard_control_ip(control_ip: &[u8]) -> bool {
    control_ip == b"0.0.0.0"
}

/// Checks whether `nzb_content` starts with `"http://"` (6 bytes) or `"https://"` (7 bytes),
/// matching `!strncasecmp(nzbContent, "http://", 6) || !strncasecmp(nzbContent, "https://", 7)`
/// in `RemoteClient::RequestServerDownload`.
pub fn remote_client_is_http_url(nzb_content: &[u8]) -> bool {
    let prefix6 = nzb_content
        .get(..6)
        .map(|s| s.eq_ignore_ascii_case(b"http:/"))
        .unwrap_or(false);
    let prefix7 = nzb_content
        .get(..7)
        .map(|s| s.eq_ignore_ascii_case(b"https:/"))
        .unwrap_or(false);
    prefix6 || prefix7
}

/// Calculates the file completion percentage in `RemoteClient::RequestServerList` and
/// `NCursesFrontend::PrintFilename`:
/// `(int)(100 - remaining_size * 100 / total_size)` when `remaining_size < total_size`,
/// or `-1` otherwise.
pub fn remote_client_calc_file_completed_pct(remaining_size: i64, total_size: i64) -> i32 {
    if remaining_size < total_size && total_size != 0 {
        (100i64.wrapping_sub(remaining_size.wrapping_mul(100) / total_size)) as i32
    } else {
        -1
    }
}

/// Splits `sec` into `(hours, minutes, seconds)` for `RemoteClient::RequestServerList`
/// and `NCursesFrontend`:
/// `h = (int)(sec / 3600); m = (int)((sec % 3600) / 60); s = (int)(sec % 60);`.
pub fn remote_client_calc_hms(sec: i64) -> (i32, i32, i32) {
    let h = (sec / 3600) as i32;
    let m = ((sec % 3600) / 60) as i32;
    let s = (sec % 60) as i32;
    (h, m, s)
}

/// Calculates the session average download speed in `RemoteClient::RequestServerList`:
/// `(int)(download_time_sec > 0 ? all_bytes / download_time_sec : 0)`.
pub fn remote_client_calc_avg_speed(all_bytes: i64, download_time_sec: i64) -> i32 {
    if download_time_sec > 0 {
        (all_bytes / download_time_sec) as i32
    } else {
        0
    }
}

/// Formats `serverState` in `RemoteClient::RequestServerList` (lines 700-726).
pub fn remote_client_format_server_state(
    download_paused: bool,
    download2_paused: bool,
    download_standby: bool,
    post_job_count: u32,
    post_paused: bool,
) -> Vec<u8> {
    let mut state = Vec::with_capacity(48);
    if download_paused || download2_paused {
        state.extend_from_slice(if download_standby {
            b"Paused"
        } else {
            b"Pausing"
        });
        if download_paused && download2_paused {
            state.extend_from_slice(b" (+2)");
        } else if download2_paused {
            state.extend_from_slice(b" (2)");
        }
    } else if !download_standby {
        state.extend_from_slice(b"Downloading");
    }

    if post_job_count > 0 || post_paused {
        if !state.is_empty() {
            state.extend_from_slice(b", Post-Processing");
        } else {
            state.extend_from_slice(b"Post-Processing");
        }
        if post_paused {
            state.extend_from_slice(b" paused");
        }
    }

    if state.is_empty() {
        state.extend_from_slice(b"Stand-By");
    }
    state
}

/// Aligns a length up to a 4-byte boundary in `RemoteClient::RequestServerEditQueue`:
/// `len + (len % 4 > 0 ? 4 - len % 4 : 0)`.
pub fn remote_client_align4(len: i32) -> i32 {
    let rem = len % 4;
    if rem > 0 {
        len.wrapping_add(4 - rem)
    } else {
        len
    }
}

/// Checks whether `umask_val` is a valid octal mask (`uMask < 01000`) in `NZBGet::Phase1_Init`.
pub fn nzbget_is_valid_umask(umask_val: u32) -> bool {
    umask_val < 0o1000
}

/// Determines whether `NZBGet::StartRemoteServer` should start the secure control server:
/// `secure_control && !(is_unix && !control_ip.is_empty() && control_ip[0] == b'/')`.
pub fn nzbget_should_start_secure_server(
    secure_control: bool,
    is_unix: bool,
    control_ip: &[u8],
) -> bool {
    if !secure_control {
        return false;
    }
    if is_unix && !control_ip.is_empty() && control_ip[0] == b'/' {
        return false;
    }
    true
}

/// Determines whether `NZBGet::DoMainLoop` should stop standalone coordinators:
/// `!server_mode && !queue_more && !url_more && !post_more`.
pub fn nzbget_should_stop_standalone(
    server_mode: bool,
    queue_more: bool,
    url_more: bool,
    post_more: bool,
) -> bool {
    !server_mode && !queue_more && !url_more && !post_more
}

/// Computes the exit code for `NZBGet::ProcessWebGet` (`status == ad_finished ? 0 : 1`).
pub fn nzbget_webget_exit_code(status: i32, ad_finished: i32) -> i32 {
    if status == ad_finished {
        0
    } else {
        1
    }
}

/// Computes the exit code for `NZBGet::ProcessSigVerify` (`ok ? 93 : 1`).
pub fn nzbget_sigverify_exit_code(ok: bool) -> i32 {
    if ok {
        93
    } else {
        1
    }
}

/// Parses the file count from a `TwinCheck` fingerprint (`"<hex>-<files>"`),
/// matching `known ? atoi(fingerprint.c_str() + fingerprint.rfind('-') + 1) : 0`.
pub fn twin_check_parse_fingerprint_files(fingerprint: &[u8]) -> i32 {
    if fingerprint.is_empty() {
        return 0;
    }
    let start = match fingerprint.iter().rposition(|&b| b == b'-') {
        Some(pos) => pos + 1,
        None => 0,
    };
    c_atoi(&fingerprint[start..])
}

fn c_atoi(bytes: &[u8]) -> i32 {
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut neg = false;
    if i < bytes.len() {
        if bytes[i] == b'-' {
            neg = true;
            i += 1;
        } else if bytes[i] == b'+' {
            i += 1;
        }
    }
    let mut acc: i64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        acc = acc
            .wrapping_mul(10)
            .wrapping_add((bytes[i] - b'0') as i64);
        i += 1;
    }
    if neg {
        (-acc) as i32
    } else {
        acc as i32
    }
}

/// Formats `TwinCheck::Identity`: returns `""` when `files_param` is empty, `"none"`, or `"lost"`,
/// otherwise `"<files_param>#<posting>"`.
pub fn twin_check_format_identity(files_param: &[u8], posting: &[u8]) -> Vec<u8> {
    if files_param.is_empty() || files_param == b"none" || files_param == b"lost" {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(files_param.len() + 1 + posting.len());
    out.extend_from_slice(files_param);
    out.push(b'#');
    out.extend_from_slice(posting);
    out
}

/// Evaluates whether an item is a duplicate in `TwinCheck::ServiceWork`:
/// `queued ? (remaining_size > 0 && paused_size >= remaining_size) : (delete_status == ds_dupe || delete_status == ds_copy)`.
pub fn twin_check_is_dupe_item(
    queued: bool,
    remaining_size: i64,
    paused_size: i64,
    delete_status: i32,
    ds_dupe: i32,
    ds_copy: i32,
) -> bool {
    if queued {
        remaining_size > 0 && paused_size >= remaining_size
    } else {
        delete_status == ds_dupe || delete_status == ds_copy
    }
}

/// Returns the service polling interval in seconds for `TwinCheck::ServiceInterval`:
/// `nudged ? 1 : 15`.
pub fn twin_check_service_interval(nudged: bool) -> i32 {
    if nudged {
        1
    } else {
        15
    }
}

/// Clamps `entry + offset` to `[0, size - 1]` in `QueueEditor::MoveEntry` and `QueueEditor::MoveGroup`.
/// Returns `Some(new_entry)` if `new_entry >= 0 && new_entry <= size - 1`, or `None` when `size <= 0`.
pub fn queue_editor_clamp_move_target(entry: i32, offset: i32, size: i32) -> Option<i32> {
    let mut new_entry = entry.wrapping_add(offset);
    if new_entry < 0 {
        new_entry = 0;
    }
    if new_entry > size.wrapping_sub(1) {
        new_entry = size.wrapping_sub(1);
    }
    if new_entry >= 0 && new_entry <= size.wrapping_sub(1) {
        Some(new_entry)
    } else {
        None
    }
}

/// Evaluates the group deletion flags in `QueueEditor::EditGroup`:
/// returns `(parking, avoid_history, set_dupe_status, cleanup_disk)`.
#[allow(clippy::too_many_arguments)]
pub fn queue_editor_eval_group_delete_flags(
    action: i32,
    ea_park_delete: i32,
    ea_dupe_delete: i32,
    ea_final_delete: i32,
    keep_history: i32,
    unpack_cleaned_up_disk: bool,
    current_success_articles: i32,
) -> (bool, bool, bool, bool) {
    let parking = action == ea_park_delete
        && keep_history > 0
        && !unpack_cleaned_up_disk
        && current_success_articles > 0;
    let avoid_history = action == ea_final_delete;
    let set_dupe_status = action == ea_dupe_delete;
    let cleanup_disk = action != ea_park_delete || !parking;
    (parking, avoid_history, set_dupe_status, cleanup_disk)
}

/// Computes the offset between `move_id` and `target_id` in `QueueEditor::MoveGroupsTo`
/// (lines 1256-1280).
pub fn queue_editor_calc_move_groups_to_offset(
    queue_ids: &[i32],
    target_id: i32,
    move_id: i32,
    before: bool,
    item_list_size: usize,
) -> i32 {
    let mut offset: i32 = 0;
    let mut progress = false;
    let mut step: i32 = 0;
    for &id in queue_ids {
        if id == target_id || id == move_id {
            if !progress {
                step = if id == target_id { -1 } else { 1 };
                let base: i32 = if before { 0 } else { 1 };
                let sub: i32 = if step > 0 { item_list_size as i32 } else { 0 };
                offset = base.wrapping_sub(sub);
                progress = true;
            } else {
                break;
            }
        }
        if progress {
            offset = offset.wrapping_add(step);
        }
    }
    offset
}

/// Calculates the wrapped message line count and window width in `NCursesFrontend::PrintMessage`:
/// `win_width = screen_width - 8; msg_lines = len / win_width + (len % win_width > 0 ? 1 : 0)`.
pub fn ncurses_calc_msg_lines(text_len: i32, screen_width: i32) -> (i32, i32) {
    let win_width = screen_width.wrapping_sub(8);
    if win_width <= 0 {
        return (win_width, 0);
    }
    let mut msg_lines = text_len / win_width;
    if text_len % win_width > 0 {
        msg_lines += 1;
    }
    (win_width, msg_lines)
}

/// Sanitizes control characters (`\n`, `\r`, `\b`) to spaces in `NCursesFrontend::PrintMessage`.
pub fn ncurses_sanitize_message_chars(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        if *b == b'\n' || *b == b'\r' || *b == b'\x08' {
            *b = b' ';
        }
    }
}

/// Selects the edit-queue keybar status string in `NCursesFrontend::PrintKeyInputBar`.
pub fn ncurses_select_edit_queue_keybar(
    selected_entry: i32,
    queue_size: i32,
) -> &'static [u8] {
    if selected_entry > 0 && queue_size > 1 && selected_entry == queue_size - 1 {
        b"(Q)uit | (E)xit | (P)ause | (D)elete | (U)p/(T)op\0"
    } else if queue_size > 1 && selected_entry == 0 {
        b"(Q)uit | (E)xit | (P)ause | (D)elete | dow(N)/(B)ottom\0"
    } else if queue_size > 1 {
        b"(Q)uit | (E)xit | (P)ause | (D)elete | (U)p/dow(N)/(T)op/(B)ottom\0"
    } else {
        b"(Q)uit | (E)xit | (P)ause | (D)elete\0"
    }
}

/// Computes `nameLen` in `NCursesFrontend::PrintGroupname`:
/// `calc_col_width ? (screen_width - 1 - 9 - 11 - 11 - 9) : (screen_width - 1 - col_files - 2 - col_total - 2 - col_left - 2 - 9)`.
pub fn ncurses_calc_group_name_len(
    screen_width: i32,
    calc_col_width: bool,
    col_files: i32,
    col_total: i32,
    col_left: i32,
) -> i32 {
    if calc_col_width {
        screen_width - 1 - 9 - 11 - 11 - 9
    } else {
        screen_width - 1 - col_files - 2 - col_total - 2 - col_left - 2 - 9
    }
}

/// Cycles `m_queueWindowPercentage` on key `'w'` in `NCursesFrontend::UpdateInput`.
pub fn ncurses_next_queue_win_pct(cur_pct: i32, is_edit_queue: bool) -> i32 {
    if cur_pct == 50 {
        100
    } else if cur_pct == 100 && !is_edit_queue {
        0
    } else {
        50
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conn_helpers() {
        assert_eq!(conn_select_ai_family(4, 4, 6, 2, 30, 0), 2);
        assert_eq!(conn_select_ai_family(6, 4, 6, 2, 30, 0), 30);
        assert_eq!(conn_select_ai_family(0, 4, 6, 2, 30, 0), 0);

        assert!(conn_is_unix_domain_path(b"/tmp/nzbget.sock"));
        assert!(!conn_is_unix_domain_path(b"127.0.0.1"));
        assert!(!conn_is_unix_domain_path(b""));

        assert_eq!(conn_recv_buffered_chunk_len(512, 128), 128);
        assert_eq!(conn_recv_buffered_chunk_len(64, 128), 64);

        assert_eq!(conn_poll_timeout_ms(60), 60000);
        assert_eq!(conn_poll_timeout_ms(0), -1);

        assert!(conn_poll_revents_ready(0x04, 0x1D));
        assert!(!conn_poll_revents_ready(0x20, 0x1D));
    }

    #[test]
    fn test_remote_client_helpers() {
        assert!(remote_client_is_wildcard_control_ip(b"0.0.0.0"));
        assert!(!remote_client_is_wildcard_control_ip(b"127.0.0.1"));

        assert!(remote_client_is_http_url(b"http://example.com/a.nzb"));
        assert!(remote_client_is_http_url(b"HTTPS://example.com/a.nzb"));
        assert!(!remote_client_is_http_url(b"/tmp/file.nzb"));

        assert_eq!(remote_client_calc_file_completed_pct(250, 1000), 75);
        assert_eq!(remote_client_calc_file_completed_pct(1000, 1000), -1);
        assert_eq!(remote_client_calc_file_completed_pct(0, 0), -1);

        assert_eq!(remote_client_calc_hms(3661), (1, 1, 1));
        assert_eq!(remote_client_calc_avg_speed(10240, 10), 1024);
        assert_eq!(remote_client_calc_avg_speed(10240, 0), 0);

        assert_eq!(
            remote_client_format_server_state(false, false, true, 0, false),
            b"Stand-By"
        );
        assert_eq!(
            remote_client_format_server_state(false, false, false, 2, true),
            b"Downloading, Post-Processing paused"
        );
        assert_eq!(
            remote_client_format_server_state(true, true, true, 0, false),
            b"Paused (+2)"
        );
        assert_eq!(
            remote_client_format_server_state(false, true, false, 1, false),
            b"Pausing (2), Post-Processing"
        );

        assert_eq!(remote_client_align4(0), 0);
        assert_eq!(remote_client_align4(1), 4);
        assert_eq!(remote_client_align4(4), 4);
        assert_eq!(remote_client_align4(5), 8);
    }

    #[test]
    fn test_nzbget_and_twin_check_helpers() {
        assert!(nzbget_is_valid_umask(0o022));
        assert!(!nzbget_is_valid_umask(0o1000));

        assert!(nzbget_should_start_secure_server(true, true, b"0.0.0.0"));
        assert!(!nzbget_should_start_secure_server(true, true, b"/tmp/sock"));
        assert!(!nzbget_should_start_secure_server(false, true, b"0.0.0.0"));

        assert!(nzbget_should_stop_standalone(false, false, false, false));
        assert!(!nzbget_should_stop_standalone(true, false, false, false));
        assert!(!nzbget_should_stop_standalone(false, true, false, false));

        assert_eq!(nzbget_webget_exit_code(3, 3), 0);
        assert_eq!(nzbget_webget_exit_code(2, 3), 1);
        assert_eq!(nzbget_sigverify_exit_code(true), 93);
        assert_eq!(nzbget_sigverify_exit_code(false), 1);

        assert_eq!(twin_check_parse_fingerprint_files(b"0123456789abcdef-42"), 42);
        assert_eq!(twin_check_parse_fingerprint_files(b""), 0);

        assert!(twin_check_format_identity(b"", b"123").is_empty());
        assert!(twin_check_format_identity(b"none", b"123").is_empty());
        assert!(twin_check_format_identity(b"lost", b"123").is_empty());
        assert_eq!(
            twin_check_format_identity(b"deadbeef-18", b"999"),
            b"deadbeef-18#999"
        );

        assert!(twin_check_is_dupe_item(true, 100, 100, 0, 1, 2));
        assert!(!twin_check_is_dupe_item(true, 100, 50, 0, 1, 2));
        assert!(twin_check_is_dupe_item(false, 0, 0, 1, 1, 2));
        assert_eq!(twin_check_service_interval(true), 1);
        assert_eq!(twin_check_service_interval(false), 15);
    }

    #[test]
    fn test_queue_editor_and_ncurses_helpers() {
        assert_eq!(queue_editor_clamp_move_target(2, -5, 10), Some(0));
        assert_eq!(queue_editor_clamp_move_target(2, 20, 10), Some(9));
        assert_eq!(queue_editor_clamp_move_target(0, 0, 0), None);

        let (park, avoid, dupe, clean) =
            queue_editor_eval_group_delete_flags(19, 19, 20, 21, 7, false, 5);
        assert!(park);
        assert!(!avoid);
        assert!(!dupe);
        assert!(!clean);

        let offset = queue_editor_calc_move_groups_to_offset(&[10, 20, 30, 40], 40, 10, false, 2);
        assert_eq!(offset, 2);

        assert_eq!(ncurses_calc_msg_lines(150, 80), (72, 3));
        let mut msg = *b"hello\nworld\r\x08!";
        ncurses_sanitize_message_chars(&mut msg);
        assert_eq!(&msg, b"hello world  !");

        assert_eq!(ncurses_calc_group_name_len(80, true, 0, 0, 0), 39);
        assert_eq!(ncurses_calc_group_name_len(80, false, 5, 8, 8), 43);
        assert_eq!(ncurses_next_queue_win_pct(50, false), 100);
        assert_eq!(ncurses_next_queue_win_pct(100, false), 0);
        assert_eq!(ncurses_next_queue_win_pct(100, true), 50);
    }
}
