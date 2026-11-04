// This file is part of nzbget. See <https://nzbget.com>.
//
// Copyright (C) 2024-2026 Denis <denis@nzbget.com>
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::ffi::{c_char, c_int, CStr};

/// Returns 1 when an XML-RPC multicall sub-response is neither a `<fault>` nor an `<array>`
/// and therefore needs `<array><data>...</data></array>` wrapping.
pub unsafe fn roc_xmlrpc_multicall_needs_array_wrap(resp: *const c_char) -> c_int {
    if resp.is_null() {
        return 1;
    }
    let bytes = unsafe { CStr::from_ptr(resp).to_bytes() };
    let fault = bytes.starts_with(b"<fault>");
    let array = !fault && bytes.starts_with(b"<array>");
    if !fault && !array {
        1
    } else {
        0
    }
}

/// Returns 1 when a TLS cipher optimizer RPC call has control access over JSON-RPC POST.
pub fn roc_xmlrpc_tls_cipher_access_allowed(
    user_access: c_int,
    ua_control: c_int,
    protocol: c_int,
    rp_json_rpc: c_int,
    http_method: c_int,
    hm_post: c_int,
) -> c_int {
    if user_access == ua_control && protocol == rp_json_rpc && http_method == hm_post {
        1
    } else {
        0
    }
}

/// Validates TLS cipher optimizer job ID parameter arity, string type, length (<= 128),
/// non-emptiness, and absence of embedded NUL bytes.
pub unsafe fn roc_xmlrpc_tls_cipher_validate_job_id(
    operation: c_int,
    param_count: usize,
    first_is_string: c_int,
    id_ptr: *const c_char,
    id_len: usize,
) -> c_int {
    if (operation == 0 && param_count != 0)
        || (operation == 1 && param_count > 1)
        || (operation == 2 && param_count != 1)
    {
        return 0;
    }
    if param_count > 0 {
        if first_is_string == 0 || id_ptr.is_null() || id_len == 0 || id_len > 128 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(id_ptr as *const u8, id_len) };
        if slice.contains(&0) {
            return 0;
        }
    }
    1
}

/// Computes megabyte fields for `StatusXmlCommand::Execute`.
pub unsafe fn roc_xmlrpc_status_calc_mbytes(
    remaining_size: i64,
    forced_size: i64,
    article_cache: i64,
    all_bytes: i64,
    month_bytes: i64,
    day_bytes: i64,
    out_remaining_mb: *mut c_int,
    out_forced_mb: *mut c_int,
    out_cache_mb: *mut c_int,
    out_downloaded_mb: *mut c_int,
    out_month_mb: *mut c_int,
    out_day_mb: *mut c_int,
) {
    if !out_remaining_mb.is_null() {
        unsafe { *out_remaining_mb = (remaining_size / 1024 / 1024) as c_int };
    }
    if !out_forced_mb.is_null() {
        unsafe { *out_forced_mb = (forced_size / 1024 / 1024) as c_int };
    }
    if !out_cache_mb.is_null() {
        unsafe { *out_cache_mb = (article_cache / 1024 / 1024) as c_int };
    }
    if !out_downloaded_mb.is_null() {
        unsafe { *out_downloaded_mb = (all_bytes / 1024 / 1024) as c_int };
    }
    if !out_month_mb.is_null() {
        unsafe { *out_month_mb = (month_bytes / 1024 / 1024) as c_int };
    }
    if !out_day_mb.is_null() {
        unsafe { *out_day_mb = (day_bytes / 1024 / 1024) as c_int };
    }
}

/// Resolves `InterDir` disk space fallback when `InterDir` is empty and computes MB values.
pub unsafe fn roc_xmlrpc_status_calc_disk_space_mb(
    free_disk: i64,
    total_disk: i64,
    inter_dir_empty: c_int,
    free_inter: i64,
    total_inter: i64,
    out_free_inter: *mut i64,
    out_total_inter: *mut i64,
    out_free_disk_mb: *mut c_int,
    out_total_disk_mb: *mut c_int,
    out_free_inter_mb: *mut c_int,
    out_total_inter_mb: *mut c_int,
) {
    let eff_free_inter = if inter_dir_empty != 0 {
        free_disk
    } else {
        free_inter
    };
    let eff_total_inter = if inter_dir_empty != 0 {
        total_disk
    } else {
        total_inter
    };
    if !out_free_inter.is_null() {
        unsafe { *out_free_inter = eff_free_inter };
    }
    if !out_total_inter.is_null() {
        unsafe { *out_total_inter = eff_total_inter };
    }
    if !out_free_disk_mb.is_null() {
        unsafe { *out_free_disk_mb = (free_disk / 1024 / 1024) as c_int };
    }
    if !out_total_disk_mb.is_null() {
        unsafe { *out_total_disk_mb = (total_disk / 1024 / 1024) as c_int };
    }
    if !out_free_inter_mb.is_null() {
        unsafe { *out_free_inter_mb = (eff_free_inter / 1024 / 1024) as c_int };
    }
    if !out_total_inter_mb.is_null() {
        unsafe { *out_total_inter_mb = (eff_total_inter / 1024 / 1024) as c_int };
    }
}

/// Validates `LogXmlCommand` parameters (`!(nr_entries > 0 && id_from > 0)`).
pub fn roc_xmlrpc_log_params_valid(id_from: c_int, nr_entries: c_int) -> c_int {
    if nr_entries > 0 && id_from > 0 {
        0
    } else {
        1
    }
}

/// Validates `ListFilesXmlCommand` range and `nzbId` parameters.
pub fn roc_xmlrpc_list_files_params_valid(
    has_id_start: c_int,
    has_id_end: c_int,
    id_start: c_int,
    id_end: c_int,
    nzb_id: c_int,
) -> c_int {
    if has_id_start != 0 && (has_id_end == 0 || id_end < id_start) {
        return 0;
    }
    if nzb_id > 0 && (id_start != 0 || id_end != 0) {
        return 0;
    }
    1
}

/// Computes `StageTimeSec` and `TotalTimeSec` for `NzbInfoXmlCommand::AppendPostInfoFields`.
pub unsafe fn roc_xmlrpc_post_info_elapsed_sec(
    cur_time: i64,
    stage_time: i64,
    start_time: i64,
    out_stage_sec: *mut c_int,
    out_total_sec: *mut c_int,
) {
    let stage_sec = if stage_time != 0 {
        (cur_time - stage_time) as c_int
    } else {
        0
    };
    let total_sec = if start_time != 0 {
        (cur_time - start_time) as c_int
    } else {
        0
    };
    if !out_stage_sec.is_null() {
        unsafe { *out_stage_sec = stage_sec };
    }
    if !out_total_sec.is_null() {
        unsafe { *out_total_sec = total_sec };
    }
}

/// Computes `RemainingSizeMB` and `PausedSizeMB` for `ListGroupsXmlCommand::Execute`.
pub unsafe fn roc_xmlrpc_list_groups_calc_mb(
    remaining_size: i64,
    paused_size: i64,
    out_remaining_mb: *mut c_int,
    out_paused_mb: *mut c_int,
) {
    if !out_remaining_mb.is_null() {
        unsafe { *out_remaining_mb = (remaining_size / 1024 / 1024) as c_int };
    }
    if !out_paused_mb.is_null() {
        unsafe { *out_paused_mb = (paused_size / 1024 / 1024) as c_int };
    }
}

/// Returns 1 when a `HistoryInfo` entry of kind `hkDup` should be skipped because `dup` is false.
pub fn roc_xmlrpc_history_should_skip_dup(
    kind: c_int,
    hk_dup: c_int,
    include_dup: c_int,
) -> c_int {
    if kind == hk_dup && include_dup == 0 {
        1
    } else {
        0
    }
}

/// Returns 1 when an option value should be masked as `"***"` for restricted RPC users.
pub fn roc_xmlrpc_config_should_mask_value(
    user_access: c_int,
    ua_restricted: c_int,
    opt_restricted: c_int,
) -> c_int {
    if user_access == ua_restricted && opt_restricted != 0 {
        1
    } else {
        0
    }
}

/// Computes `totalSizeMB` and `customSizeMB` for `ServerVolumesXmlCommand::Execute`.
pub unsafe fn roc_xmlrpc_server_volumes_calc_mb(
    total_bytes: i64,
    custom_bytes: i64,
    out_total_mb: *mut u32,
    out_custom_mb: *mut u32,
) {
    if !out_total_mb.is_null() {
        unsafe { *out_total_mb = ((total_bytes / 1024 / 1024) as i32) as u32 };
    }
    if !out_custom_mb.is_null() {
        unsafe { *out_custom_mb = ((custom_bytes / 1024 / 1024) as i32) as u32 };
    }
}

/// Validates target server availability for `TestServerSpeedXmlCommand::Execute`:
/// returns 0 if ready, 1 if not found, 2 if inactive, 3 if maxConnections == 0.
pub fn roc_xmlrpc_test_server_speed_check_server(
    has_server: c_int,
    is_active: c_int,
    max_connections: c_int,
) -> c_int {
    if has_server == 0 {
        1
    } else if is_active == 0 {
        2
    } else if max_connections == 0 {
        3
    } else {
        0
    }
}

/// Validates `TestDiskSpeedXmlCommand::Execute` parameters and computes byte sizes.
/// Returns 0 if valid, 1 if `write_buffer_kib < 0`, 2 if `max_file_size_gib < 1`,
/// 3 if `timeout_sec < 1`.
pub unsafe fn roc_xmlrpc_test_disk_speed_validate_params(
    write_buffer_kib: c_int,
    max_file_size_gib: c_int,
    timeout_sec: c_int,
    out_buffer_bytes: *mut usize,
    out_max_file_bytes: *mut u64,
) -> c_int {
    if write_buffer_kib < 0 {
        return 1;
    }
    if max_file_size_gib < 1 {
        return 2;
    }
    if timeout_sec < 1 {
        return 3;
    }
    if !out_buffer_bytes.is_null() {
        unsafe { *out_buffer_bytes = (write_buffer_kib as usize) * 1024usize };
    }
    if !out_max_file_bytes.is_null() {
        unsafe {
            *out_max_file_bytes = (max_file_size_gib as u64) * 1024u64 * 1024u64 * 1024u64;
        }
    }
    0
}

/// Rule 7.1 in `Options::CheckOptions`: returns 1 when `ServerX.Encryption` is disabled
/// while `ServerX.Cipher` is non-empty.
pub unsafe fn roc_options_server_cipher_without_tls(
    tls_enabled: c_int,
    cipher: *const c_char,
) -> c_int {
    if tls_enabled == 0 && !cipher.is_null() && unsafe { *cipher != 0 } {
        1
    } else {
        0
    }
}

/// Evaluates Rules 7.2, 7.3, and 7.4 in `Options::CheckOptions`.
pub unsafe fn roc_options_reconcile_conflict_rules(
    dupe_search: c_int,
    health_check: c_int,
    hc_park: c_int,
    flush_queue: c_int,
    direct_write: c_int,
    unpack_pause_queue: c_int,
    direct_unpack: c_int,
    out_disable_dupe_search: *mut c_int,
    out_enable_flush_queue: *mut c_int,
    out_enforce_unpack_mutex: *mut c_int,
) {
    let disable_dupe = if dupe_search != 0 && health_check == hc_park {
        1
    } else {
        0
    };
    let enable_flush = if flush_queue == 0 && direct_write != 0 {
        1
    } else {
        0
    };
    let enforce_mutex = if unpack_pause_queue == 0 && direct_unpack != 0 {
        1
    } else {
        0
    };
    if !out_disable_dupe_search.is_null() {
        unsafe { *out_disable_dupe_search = disable_dupe };
    }
    if !out_enable_flush_queue.is_null() {
        unsafe { *out_enable_flush_queue = enable_flush };
    }
    if !out_enforce_unpack_mutex.is_null() {
        unsafe { *out_enforce_unpack_mutex = enforce_mutex };
    }
}

/// Evaluates `DirectClientMode` in `NZBGet::BootConfig`.
pub fn roc_nzbget_boot_is_direct_client_mode(
    test_backtrace: c_int,
    web_get: c_int,
    sig_verify: c_int,
    client_op: c_int,
    op_no_operation: c_int,
    print_options: c_int,
) -> c_int {
    if test_backtrace != 0
        || web_get != 0
        || sig_verify != 0
        || client_op != op_no_operation
        || print_options != 0
    {
        1
    } else {
        0
    }
}

/// Returns 1 when `NZBGet::BootConfig` should pause all activities due to configuration errors.
pub fn roc_nzbget_boot_should_pause_on_config_errors(
    config_errors: c_int,
    client_op: c_int,
    op_no_operation: c_int,
) -> c_int {
    if config_errors != 0 && client_op == op_no_operation {
        1
    } else {
        0
    }
}

/// Computes the clamped debounce offset in milliseconds from `g_reloadFirstRequestTime`
/// during burst reload requests in `Reload()`.
pub fn roc_nzbget_reload_clamp_debounce_ms(
    elapsed_since_first_ms: i64,
    debounce_delay_ms: i64,
    max_burst_delay_ms: i64,
) -> i64 {
    let candidate = elapsed_since_first_ms.saturating_add(debounce_delay_ms);
    if candidate < max_burst_delay_ms {
        candidate
    } else {
        max_burst_delay_ms
    }
}

/// Computes unpaused remaining group bytes and active download thread flags for
/// `RemoteClient::RequestServerList`.
pub unsafe fn roc_remote_client_calc_group_unpaused_remaining(
    remaining_size: i64,
    paused_size: i64,
    active_downloads: c_int,
    out_has_threads: *mut c_int,
    out_threads_plural: *mut c_int,
) -> i64 {
    if !out_has_threads.is_null() {
        unsafe { *out_has_threads = if active_downloads > 0 { 1 } else { 0 } };
    }
    if !out_threads_plural.is_null() {
        unsafe { *out_threads_plural = if active_downloads > 1 { 1 } else { 0 } };
    }
    remaining_size - paused_size
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_xmlrpc_helpers() {
        let fault = CString::new("<fault><value>err</value></fault>").unwrap();
        let array = CString::new("<array><data></data></array>").unwrap();
        let scalar = CString::new("<i4>42</i4>").unwrap();
        unsafe {
            assert_eq!(roc_xmlrpc_multicall_needs_array_wrap(fault.as_ptr()), 0);
            assert_eq!(roc_xmlrpc_multicall_needs_array_wrap(array.as_ptr()), 0);
            assert_eq!(roc_xmlrpc_multicall_needs_array_wrap(scalar.as_ptr()), 1);
        }

        assert_eq!(roc_xmlrpc_tls_cipher_access_allowed(0, 0, 2, 2, 1, 1), 1);
        assert_eq!(roc_xmlrpc_tls_cipher_access_allowed(1, 0, 2, 2, 1, 1), 0);

        let job = CString::new("job-123").unwrap();
        unsafe {
            assert_eq!(
                roc_xmlrpc_tls_cipher_validate_job_id(0, 0, 0, std::ptr::null(), 0),
                1
            );
            assert_eq!(
                roc_xmlrpc_tls_cipher_validate_job_id(2, 1, 1, job.as_ptr(), 7),
                1
            );
            assert_eq!(
                roc_xmlrpc_tls_cipher_validate_job_id(2, 0, 0, std::ptr::null(), 0),
                0
            );
        }

        let mut rem_mb = 0;
        let mut forced_mb = 0;
        let mut cache_mb = 0;
        let mut dl_mb = 0;
        let mut mo_mb = 0;
        let mut day_mb = 0;
        unsafe {
            roc_xmlrpc_status_calc_mbytes(
                10 * 1024 * 1024,
                2 * 1024 * 1024,
                64 * 1024 * 1024,
                100 * 1024 * 1024,
                50 * 1024 * 1024,
                5 * 1024 * 1024,
                &mut rem_mb,
                &mut forced_mb,
                &mut cache_mb,
                &mut dl_mb,
                &mut mo_mb,
                &mut day_mb,
            );
        }
        assert_eq!((rem_mb, forced_mb, cache_mb, dl_mb, mo_mb, day_mb), (10, 2, 64, 100, 50, 5));

        let mut fi = 0;
        let mut ti = 0;
        let mut fd_mb = 0;
        let mut td_mb = 0;
        let mut fi_mb = 0;
        let mut ti_mb = 0;
        unsafe {
            roc_xmlrpc_status_calc_disk_space_mb(
                20 * 1024 * 1024,
                100 * 1024 * 1024,
                1,
                0,
                0,
                &mut fi,
                &mut ti,
                &mut fd_mb,
                &mut td_mb,
                &mut fi_mb,
                &mut ti_mb,
            );
        }
        assert_eq!((fd_mb, td_mb, fi_mb, ti_mb), (20, 100, 20, 100));

        assert_eq!(roc_xmlrpc_log_params_valid(0, 100), 1);
        assert_eq!(roc_xmlrpc_log_params_valid(5, 10), 0);
        assert_eq!(roc_xmlrpc_list_files_params_valid(1, 1, 1, 10, 0), 1);
        assert_eq!(roc_xmlrpc_list_files_params_valid(1, 1, 10, 5, 0), 0);
        assert_eq!(roc_xmlrpc_list_files_params_valid(1, 1, 1, 10, 3), 0);

        let mut st_sec = 0;
        let mut tot_sec = 0;
        unsafe {
            roc_xmlrpc_post_info_elapsed_sec(1000, 950, 900, &mut st_sec, &mut tot_sec);
        }
        assert_eq!((st_sec, tot_sec), (50, 100));

        let mut lg_rem = 0;
        let mut lg_pau = 0;
        unsafe {
            roc_xmlrpc_list_groups_calc_mb(8 * 1024 * 1024, 3 * 1024 * 1024, &mut lg_rem, &mut lg_pau);
        }
        assert_eq!((lg_rem, lg_pau), (8, 3));

        assert_eq!(roc_xmlrpc_history_should_skip_dup(2, 2, 0), 1);
        assert_eq!(roc_xmlrpc_history_should_skip_dup(2, 2, 1), 0);
        assert_eq!(roc_xmlrpc_config_should_mask_value(1, 1, 1), 1);
        assert_eq!(roc_xmlrpc_config_should_mask_value(0, 1, 1), 0);

        let mut sv_tot = 0;
        let mut sv_cus = 0;
        unsafe {
            roc_xmlrpc_server_volumes_calc_mb(12 * 1024 * 1024, 4 * 1024 * 1024, &mut sv_tot, &mut sv_cus);
        }
        assert_eq!((sv_tot, sv_cus), (12, 4));

        assert_eq!(roc_xmlrpc_test_server_speed_check_server(1, 1, 8), 0);
        assert_eq!(roc_xmlrpc_test_server_speed_check_server(0, 1, 8), 1);
        assert_eq!(roc_xmlrpc_test_server_speed_check_server(1, 0, 8), 2);
        assert_eq!(roc_xmlrpc_test_server_speed_check_server(1, 1, 0), 3);

        let mut buf_bytes = 0;
        let mut max_bytes = 0;
        unsafe {
            assert_eq!(
                roc_xmlrpc_test_disk_speed_validate_params(64, 2, 10, &mut buf_bytes, &mut max_bytes),
                0
            );
        }
        assert_eq!(buf_bytes, 64 * 1024);
        assert_eq!(max_bytes, 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_options_nzbget_and_remote_client_helpers() {
        let cipher = CString::new("TLS_AES_128_GCM_SHA256").unwrap();
        unsafe {
            assert_eq!(roc_options_server_cipher_without_tls(0, cipher.as_ptr()), 1);
            assert_eq!(roc_options_server_cipher_without_tls(1, cipher.as_ptr()), 0);
        }

        let mut dis_dupe = 0;
        let mut en_flush = 0;
        let mut enf_mutex = 0;
        unsafe {
            roc_options_reconcile_conflict_rules(
                1,
                1,
                1,
                0,
                1,
                0,
                1,
                &mut dis_dupe,
                &mut en_flush,
                &mut enf_mutex,
            );
        }
        assert_eq!((dis_dupe, en_flush, enf_mutex), (1, 1, 1));

        assert_eq!(roc_nzbget_boot_is_direct_client_mode(0, 0, 0, 0, 0, 0), 0);
        assert_eq!(roc_nzbget_boot_is_direct_client_mode(0, 0, 0, 2, 0, 0), 1);
        assert_eq!(roc_nzbget_boot_should_pause_on_config_errors(1, 0, 0), 1);
        assert_eq!(roc_nzbget_boot_should_pause_on_config_errors(1, 2, 0), 0);
        assert_eq!(roc_nzbget_reload_clamp_debounce_ms(200, 100, 1500), 300);
        assert_eq!(roc_nzbget_reload_clamp_debounce_ms(1450, 100, 1500), 1500);

        let mut has_th = 0;
        let mut th_pl = 0;
        let unpaused = unsafe {
            roc_remote_client_calc_group_unpaused_remaining(1000, 300, 4, &mut has_th, &mut th_pl)
        };
        assert_eq!((unpaused, has_th, th_pl), (700, 1, 1));
    }
}
