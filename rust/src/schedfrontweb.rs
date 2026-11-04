//This file is part of nzbget. See <https://nzbget.com>.
//
//Program delivered under the terms of the GNU General Public License Version 2.

//! Rust hot-path kernels for `Scheduler.cpp`, `Frontend.cpp`, `ColoredFrontend.cpp`,
//! `NCursesFrontend.cpp`, `RemoteClient.cpp`, and `WebServer.cpp`.

use std::ffi::{CStr, c_char, c_int};

extern "C" {
    fn strcasecmp(a: *const c_char, b: *const c_char) -> c_int;
}

/// `Scheduler::FirstCheck`:
/// Comparator ordering scheduler tasks by `(hours, minutes)`.
/// Mirrors:
/// `(task1->m_hours < task2->m_hours) || ((task1->m_hours == task2->m_hours) && (task1->m_minutes < task2->m_minutes))`
#[inline]
pub fn scheduler_task_less(hours1: i32, minutes1: i32, hours2: i32, minutes2: i32) -> bool {
    (hours1 < hours2) || ((hours1 == hours2) && (minutes1 < minutes2))
}

/// `Scheduler::ExecuteTask`:
/// Checks whether process/extension/feed commands should be executed.
/// Mirrors:
/// `m_executeProcess || task->m_hours == Task::STARTUP_TASK`
#[inline]
pub fn scheduler_should_execute_process(
    execute_process: bool,
    task_hours: i32,
    startup_task: i32,
) -> bool {
    execute_process || task_hours == startup_task
}

/// `Scheduler::ExecuteTask`:
/// Converts KB/s speed limit to bytes/s.
/// Mirrors:
/// `atoi(task->m_param.c_str()) * 1024`
#[inline]
pub fn scheduler_calc_speed_limit_bytes(kb_per_sec: i32) -> i32 {
    kb_per_sec.wrapping_mul(1024)
}

/// `Scheduler::EditServer`:
/// Checks whether a server matches `server_ref` by numeric ID or case-insensitive name.
/// Uses C `strcasecmp` so active `LC_CTYPE` locale semantics are preserved.
///
/// # Safety
/// `server_name` and `server_ref` must be valid null-terminated C strings (or null).
#[inline]
pub unsafe fn scheduler_matches_server_ref(
    id: i32,
    server_id: i32,
    server_name: *const c_char,
    server_ref: *const c_char,
) -> bool {
    if id > 0 && server_id == id {
        return true;
    }
    if server_name.is_null() || server_ref.is_null() {
        return false;
    }
    strcasecmp(server_name, server_ref) == 0
}

/// `Scheduler::FetchFeed`:
/// Checks whether a feed matches `feed_ref` by numeric ID, case-insensitive name, or `"0"`,
/// and resolves the target feed ID to fetch (`0` for `"0"`, else `feed_id`).
/// Uses C `strcasecmp` so active `LC_CTYPE` locale semantics are preserved.
///
/// # Safety
/// `feed_name` and `feed_ref` must be valid null-terminated C strings (or null).
#[inline]
pub unsafe fn scheduler_matches_feed_ref(
    id: i32,
    feed_id: i32,
    feed_name: *const c_char,
    feed_ref: *const c_char,
) -> (bool, i32) {
    if feed_ref.is_null() {
        return (feed_id == id, feed_id);
    }
    let is_zero = strcasecmp(c"0".as_ptr(), feed_ref) == 0;
    let matched = feed_id == id
        || (!feed_name.is_null() && strcasecmp(feed_name, feed_ref) == 0)
        || is_zero;
    let target_id = if is_zero { 0 } else { feed_id };
    (matched, target_id)
}

/// `Scheduler::CheckScheduledResume`:
/// Checks whether scheduled auto-resume is due.
/// Mirrors:
/// `resumeTime > 0 && currentTime >= resumeTime`
#[inline]
pub fn scheduler_should_autoresume(resume_time: i64, current_time: i64) -> bool {
    resume_time > 0 && current_time >= resume_time
}

/// `Frontend::RequestMessages`:
/// Resolves `LogRequest.m_idFrom` in host byte order.
/// Mirrors:
/// `if (m_neededLogEntries == 0) { m_idFrom = m_neededLogFirstId > 0 ? m_neededLogFirstId : 1; } else { m_idFrom = 0; }`
#[inline]
pub fn frontend_resolve_log_id_from(needed_log_entries: u32, needed_log_first_id: u32) -> u32 {
    if needed_log_entries == 0 {
        if needed_log_first_id > 0 {
            needed_log_first_id
        } else {
            1
        }
    } else {
        0
    }
}

/// `Frontend::RequestMessages` / `Frontend::RequestFileList` / `RemoteClient`:
/// Validates a binary RPC response header after converting fields to host byte order.
/// Mirrors:
/// `read && (int)ntohl(resp.m_signature) == (int)NZBMESSAGE_SIGNATURE && ntohl(resp.m_structSize) == sizeof(resp)`
#[inline]
pub fn rpc_is_response_header_valid(
    read_ok: bool,
    resp_sig_host: u32,
    expected_sig: u32,
    resp_struct_size_host: u32,
    expected_struct_size: u32,
) -> bool {
    read_ok
        && (resp_sig_host as i32) == (expected_sig as i32)
        && resp_struct_size_host == expected_struct_size
}

/// `ColoredFrontend::PrintStatus` / `NCursesFrontend::PrintStatus`:
/// Selects the pause status suffix string.
/// Mirrors:
/// `m_pauseDownload ? (m_standBy ? ", Paused" : ", Pausing") : ""`
#[inline]
pub fn frontend_select_pause_status_suffix(pause_download: bool, stand_by: bool) -> &'static CStr {
    if pause_download {
        if stand_by {
            c", Paused"
        } else {
            c", Pausing"
        }
    } else {
        c""
    }
}

/// `ColoredFrontend::PrintStatus` / `NCursesFrontend::PrintStatus`:
/// Computes the speed limit in KB/s when `download_limit > 0`, or `0` otherwise.
#[inline]
pub fn frontend_calc_limit_kb(download_limit: i32) -> i32 {
    if download_limit > 0 {
        download_limit / 1024
    } else {
        0
    }
}

/// `NCursesFrontend::Run`:
/// Decides `(update_now, new_update_next_time)` for a loop iteration.
#[inline]
pub fn ncurses_decide_loop_update(
    key_pressed: bool,
    update_next_time: bool,
    data_update_pos: i32,
) -> (bool, bool) {
    if key_pressed {
        (true, true)
    } else if update_next_time {
        (true, false)
    } else if data_update_pos <= 0 {
        (true, false)
    } else {
        (false, update_next_time)
    }
}

/// `NCursesFrontend::Run`:
/// Selects the sleep interval in milliseconds (`100` in normal mode, `10` otherwise).
#[inline]
pub fn ncurses_select_sleep_interval(is_normal_mode: bool) -> i32 {
    if is_normal_mode {
        100
    } else {
        10
    }
}

/// `NCursesFrontend::PrintKeyInputBar`:
/// Checks whether the temporary hint banner is still within its 5-second window.
/// Mirrors:
/// `time - m_startHint < 5`
#[inline]
pub fn ncurses_is_hint_active(time: i64, start_hint: i64) -> bool {
    time.wrapping_sub(start_hint) < 5
}

/// `NCursesFrontend::PrintKeyInputBar`:
/// Selects the normal-mode keybar text.
#[inline]
pub fn ncurses_select_normal_keybar(group_files: bool) -> &'static CStr {
    if group_files {
        c"(Q)uit | (E)dit | (P)ause | (R)ate | (W)indow | (G)roup | (T)ime"
    } else {
        c"(Q)uit | (E)dit | (P)ause | (R)ate | (W)indow | (G)roup | (T)ime | n(Z)b"
    }
}

/// `NCursesFrontend::PrintFilename` / `NCursesFrontend::PrintGroupname`:
/// Resolves `(color, brace1, brace2)` for a queue row.
#[inline]
pub fn ncurses_select_row_highlight(
    is_edit_queue: bool,
    selected: bool,
    use_color: bool,
    highl_color: i32,
    text_color: i32,
) -> (i32, u8, u8) {
    if is_edit_queue && selected {
        if use_color {
            (highl_color, b'[', b']')
        } else {
            (highl_color, b'<', b'>')
        }
    } else {
        (text_color, b'[', b']')
    }
}

/// `NCursesFrontend::PrintTopHeader`:
/// Selects which right-aligned uptime/download-time caption fits in `chars_left`:
/// - `2`: `longCap + timeStr` (`chars_left - time_len - long_cap_len >= 0`)
/// - `1`: `shortCap + timeStr` (`chars_left - time_len - short_cap_len >= 0`)
/// - `0`: `timeStr` only (`chars_left - time_len >= 0`)
/// - `-1`: none fits
#[inline]
pub fn ncurses_select_top_header_caption(
    chars_left: i32,
    time_len: i32,
    long_cap_len: i32,
    short_cap_len: i32,
) -> i32 {
    if chars_left - time_len - long_cap_len >= 0 {
        2
    } else if chars_left - time_len - short_cap_len >= 0 {
        1
    } else if chars_left - time_len >= 0 {
        0
    } else {
        -1
    }
}

/// `NCursesFrontend::PrintGroupname`:
/// Computes `(h, m, s)` for a group's remaining download time, capping to `(99, 99, 99)`
/// when `h >= 100`. Returns `None` when `current_download_speed <= 0 || pause_download`.
#[inline]
pub fn ncurses_calc_group_remain_hms_capped(
    unpaused_remaining_size: i64,
    current_download_speed: i64,
    pause_download: bool,
) -> Option<(i32, i32, i32)> {
    if current_download_speed <= 0 || pause_download {
        return None;
    }
    let remain_sec = unpaused_remaining_size / current_download_speed;
    let h = (remain_sec / 3600) as i32;
    let m = ((remain_sec % 3600) / 60) as i32;
    let s = (remain_sec % 60) as i32;
    if h < 100 {
        Some((h, m, s))
    } else {
        Some((99, 99, 99))
    }
}

/// `RemoteClient::RequestServerList`:
/// Checks whether the remaining download time line should be printed.
/// Mirrors:
/// `downloadRate > 0 && !downloadPaused && !download2Paused && !downloadStandBy`
#[inline]
pub fn remote_client_should_print_remaining_time(
    download_rate: u32,
    download_paused: u32,
    download2_paused: u32,
    download_standby: u32,
) -> bool {
    download_rate > 0
        && download_paused == 0
        && download2_paused == 0
        && download_standby == 0
}

/// `WebProcessor::Execute`:
/// Checks whether an unauthorized HTTP request must immediately receive a 401 response.
/// Mirrors:
/// `(!g_Options->GetFormAuth() || m_rpcRequest) && !m_authorized`
#[inline]
pub fn web_proc_should_send_auth_response(
    form_auth: bool,
    rpc_request: bool,
    authorized: bool,
) -> bool {
    (!form_auth || rpc_request) && !authorized
}

/// `WebProcessor::SendBodyResponse`:
/// Checks whether `old_etag` is non-null and equal to `new_etag`.
/// Mirrors:
/// `m_oldETag && !strcmp(newETag, m_oldETag)`
///
/// # Safety
/// `old_etag` and `new_etag` must be either null or valid null-terminated C strings.
#[inline]
pub unsafe fn web_proc_is_etag_unchanged(
    old_etag: *const c_char,
    new_etag: *const c_char,
) -> bool {
    if old_etag.is_null() || new_etag.is_null() {
        return false;
    }
    CStr::from_ptr(old_etag).to_bytes() == CStr::from_ptr(new_etag).to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_helpers() {
        assert!(scheduler_task_less(8, 30, 9, 0));
        assert!(scheduler_task_less(8, 15, 8, 30));
        assert!(!scheduler_task_less(9, 0, 8, 30));

        assert!(scheduler_should_execute_process(false, -1, -1));
        assert!(scheduler_should_execute_process(true, 10, -1));
        assert!(!scheduler_should_execute_process(false, 10, -1));

        assert_eq!(scheduler_calc_speed_limit_bytes(50), 51200);

        unsafe {
            assert!(scheduler_matches_server_ref(
                2,
                2,
                c"Primary".as_ptr(),
                c"2".as_ptr()
            ));
            assert!(scheduler_matches_server_ref(
                0,
                2,
                c"Primary".as_ptr(),
                c"primary".as_ptr()
            ));
            assert!(!scheduler_matches_server_ref(
                0,
                2,
                c"Primary".as_ptr(),
                c"backup".as_ptr()
            ));

            assert_eq!(
                scheduler_matches_feed_ref(0, 3, c"Movies".as_ptr(), c"0".as_ptr()),
                (true, 0)
            );
            assert_eq!(
                scheduler_matches_feed_ref(0, 3, c"Movies".as_ptr(), c"movies".as_ptr()),
                (true, 3)
            );
            assert_eq!(
                scheduler_matches_feed_ref(0, 3, c"Movies".as_ptr(), c"tv".as_ptr()),
                (false, 3)
            );
        }

        assert!(scheduler_should_autoresume(100, 100));
        assert!(!scheduler_should_autoresume(0, 100));
        assert!(!scheduler_should_autoresume(100, 99));
    }

    #[test]
    fn test_frontend_and_ncurses_helpers() {
        assert_eq!(frontend_resolve_log_id_from(0, 0), 1);
        assert_eq!(frontend_resolve_log_id_from(0, 42), 42);
        assert_eq!(frontend_resolve_log_id_from(10, 42), 0);

        assert!(rpc_is_response_header_valid(true, 0x6e7a622d, 0x6e7a622d, 24, 24));
        assert!(!rpc_is_response_header_valid(false, 0x6e7a622d, 0x6e7a622d, 24, 24));
        assert!(!rpc_is_response_header_valid(true, 0x6e7a622d, 0x6e7a622d, 20, 24));

        assert_eq!(
            frontend_select_pause_status_suffix(true, true).to_bytes(),
            b", Paused"
        );
        assert_eq!(
            frontend_select_pause_status_suffix(true, false).to_bytes(),
            b", Pausing"
        );
        assert_eq!(
            frontend_select_pause_status_suffix(false, true).to_bytes(),
            b""
        );

        assert_eq!(frontend_calc_limit_kb(2048), 2);
        assert_eq!(frontend_calc_limit_kb(0), 0);

        assert_eq!(ncurses_decide_loop_update(true, false, 50), (true, true));
        assert_eq!(ncurses_decide_loop_update(false, true, 50), (true, false));
        assert_eq!(ncurses_decide_loop_update(false, false, 0), (true, false));
        assert_eq!(ncurses_decide_loop_update(false, false, 50), (false, false));

        assert_eq!(ncurses_select_sleep_interval(true), 100);
        assert_eq!(ncurses_select_sleep_interval(false), 10);

        assert!(ncurses_is_hint_active(104, 100));
        assert!(!ncurses_is_hint_active(105, 100));

        assert_eq!(
            ncurses_select_row_highlight(true, true, false, 10, 1),
            (10, b'<', b'>')
        );
        assert_eq!(
            ncurses_select_row_highlight(true, true, true, 10, 1),
            (10, b'[', b']')
        );
        assert_eq!(
            ncurses_select_row_highlight(false, true, false, 10, 1),
            (1, b'[', b']')
        );

        assert_eq!(ncurses_select_top_header_caption(30, 8, 15, 4), 2);
        assert_eq!(ncurses_select_top_header_caption(15, 8, 15, 4), 1);
        assert_eq!(ncurses_select_top_header_caption(9, 8, 15, 4), 0);
        assert_eq!(ncurses_select_top_header_caption(5, 8, 15, 4), -1);

        assert_eq!(
            ncurses_calc_group_remain_hms_capped(36610, 10, false),
            Some((1, 1, 1))
        );
        assert_eq!(
            ncurses_calc_group_remain_hms_capped(3600000, 1, false),
            Some((99, 99, 99))
        );
        assert_eq!(ncurses_calc_group_remain_hms_capped(36610, 10, true), None);
    }

    #[test]
    fn test_remote_client_and_web_helpers() {
        assert!(remote_client_should_print_remaining_time(100, 0, 0, 0));
        assert!(!remote_client_should_print_remaining_time(0, 0, 0, 0));
        assert!(!remote_client_should_print_remaining_time(100, 1, 0, 0));

        assert!(web_proc_should_send_auth_response(false, false, false));
        assert!(web_proc_should_send_auth_response(true, true, false));
        assert!(!web_proc_should_send_auth_response(true, false, false));
        assert!(!web_proc_should_send_auth_response(false, true, true));

        unsafe {
            assert!(web_proc_is_etag_unchanged(
                c"\"1a2b\"".as_ptr(),
                c"\"1a2b\"".as_ptr()
            ));
            assert!(!web_proc_is_etag_unchanged(
                c"\"1a2b\"".as_ptr(),
                c"\"3c4d\"".as_ptr()
            ));
            assert!(!web_proc_is_etag_unchanged(
                std::ptr::null(),
                c"\"1a2b\"".as_ptr()
            ));
        }
    }
}
