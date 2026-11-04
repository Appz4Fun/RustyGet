// Copyright (C) 2026 Denis <denis@nzbget.com>
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

//! Hot-path Rust port for `XmlRpc`, `RemoteServer`, `Connection`,
//! `WebDownloader`, `Options`, `Cleanup`, `Rename`, `ArchiveProcessor`,
//! `Log`, and `NString`.

use std::ffi::CStr;

// ============================================================================
// XmlRpc hot paths
// ============================================================================

/// Checks whether a request URL targets one of the XML-RPC / JSON-RPC / JSONP-RPC endpoints.
/// Matches `XmlRpcProcessor::IsRpcRequest`.
pub fn rpc_is_rpc_request(url: &[u8]) -> bool {
    url == b"/xmlrpc"
        || url.starts_with(b"/xmlrpc/")
        || url == b"/jsonrpc"
        || url.starts_with(b"/jsonrpc/")
        || url == b"/jsonprpc"
        || url.starts_with(b"/jsonprpc/")
}

/// Computes `(start_index, effective_nr_entries)` for `LogXmlCommand::Execute`.
pub fn rpc_log_slice_range(
    messages_len: i32,
    front_id: i32,
    id_from: i32,
    nr_entries: i32,
) -> (i32, i32) {
    let mut start = messages_len;
    let mut eff_entries = nr_entries;
    if eff_entries > 0 {
        if eff_entries > messages_len {
            eff_entries = messages_len;
        }
        start = messages_len - eff_entries;
    }
    if id_from > 0 && messages_len > 0 {
        eff_entries = messages_len;
        start = id_from - front_id;
        if start < 0 {
            start = 0;
        }
    }
    (start, eff_entries)
}

/// Evaluates whether a `FileInfo` matches the `ListFilesXmlCommand` filter and, if so,
/// computes its permille progress (`0..=1000`).
pub fn rpc_list_files_match_and_progress(
    nzb_id: i32,
    file_nzb_id: i32,
    file_id: i32,
    id_start: i32,
    id_end: i32,
    size: i64,
    remaining_size: i64,
    missed_size: i64,
    failed_size: i64,
    success_size: i64,
) -> Option<i32> {
    let matches = (nzb_id > 0 && nzb_id == file_nzb_id)
        || (nzb_id == 0 && (id_start == 0 || (id_start <= file_id && file_id <= id_end)));
    if !matches {
        return None;
    }
    let progress = if failed_size == 0 && success_size == 0 {
        0
    } else {
        let denom = size - missed_size;
        if denom <= 0 {
            0
        } else {
            (1000 - (remaining_size * 1000) / denom) as i32
        }
    };
    Some(progress)
}

const POST_STAGE_NAMES: [&CStr; 15] = [
    c"PP_QUEUED",
    c"LOADING_PARS",
    c"VERIFYING_SOURCES",
    c"REPAIRING",
    c"VERIFYING_REPAIRED",
    c"RENAMING",
    c"RENAMING",
    c"UNPACKING",
    c"MOVING",
    c"MOVING",
    c"POST_UNPACK_RENAMING",
    c"EXECUTING_SCRIPT",
    c"PP_FINISHED",
    c"POST_DOWNLOAD_RENAMING",
    c"STREAM_REPAIRING",
];

/// Maps `NzbInfo` state to the RPC group status string for `ListGroupsXmlCommand::DetectStatus`.
#[allow(clippy::too_many_arguments)]
pub fn rpc_detect_group_status(
    has_post_info: bool,
    post_stage: i32,
    has_queue_script_job: bool,
    queue_script_active: bool,
    direct_unpack_status: i32,
    active_downloads: i32,
    kind: i32,
    paused_size: i64,
    remaining_size: i64,
) -> &'static CStr {
    // PostInfo::ptQueued == 0, NzbInfo::nsRunning == 1, NzbInfo::nkUrl == 1
    if has_post_info {
        if post_stage == 0 && has_queue_script_job {
            if queue_script_active {
                c"QS_EXECUTING"
            } else {
                c"QS_QUEUED"
            }
        } else if direct_unpack_status == 1 {
            c"UNPACKING"
        } else if (0..POST_STAGE_NAMES.len() as i32).contains(&post_stage) {
            POST_STAGE_NAMES[post_stage as usize]
        } else {
            c"PP_QUEUED"
        }
    } else if active_downloads > 0 {
        if kind == 1 {
            c"FETCHING"
        } else {
            c"DOWNLOADING"
        }
    } else if paused_size > 0 && remaining_size == paused_size {
        c"PAUSED"
    } else {
        c"QUEUED"
    }
}

const EDIT_COMMAND_MAP: [(&[u8], i32); 51] = [
    (b"FileMoveOffset", 1),
    (b"FileMoveTop", 2),
    (b"FileMoveBottom", 3),
    (b"FilePause", 4),
    (b"FileResume", 5),
    (b"FileDelete", 6),
    (b"FilePauseAllPars", 7),
    (b"FilePauseExtraPars", 8),
    (b"FileReorder", 9),
    (b"FileSplit", 10),
    (b"GroupMoveOffset", 11),
    (b"GroupMoveTop", 12),
    (b"GroupMoveBottom", 13),
    (b"GroupMoveBefore", 14),
    (b"GroupMoveAfter", 15),
    (b"GroupPause", 16),
    (b"GroupResume", 17),
    (b"GroupDelete", 18),
    (b"GroupParkDelete", 19),
    (b"GroupDupeDelete", 20),
    (b"GroupFinalDelete", 21),
    (b"GroupPauseAllPars", 22),
    (b"GroupPauseExtraPars", 23),
    (b"GroupSetPriority", 24),
    (b"GroupSetCategory", 25),
    (b"GroupApplyCategory", 26),
    (b"GroupMerge", 27),
    (b"GroupSetParameter", 28),
    (b"GroupSetName", 29),
    (b"GroupSetDupeKey", 30),
    (b"GroupSetDupeScore", 31),
    (b"GroupSetDupeMode", 32),
    (b"GroupSort", 33),
    (b"GroupSortFiles", 34),
    (b"PostDelete", 35),
    (b"HistoryDelete", 36),
    (b"HistoryFinalDelete", 37),
    (b"HistoryReturn", 38),
    (b"HistoryProcess", 39),
    (b"HistoryRedownload", 40),
    (b"HistoryRetryFailed", 41),
    (b"HistorySetParameter", 42),
    (b"HistorySetDupeKey", 43),
    (b"HistorySetDupeScore", 44),
    (b"HistorySetDupeMode", 45),
    (b"HistorySetDupeBackup", 46),
    (b"HistoryMarkBad", 47),
    (b"HistoryMarkGood", 48),
    (b"HistoryMarkSuccess", 49),
    (b"HistorySetCategory", 50),
    (b"HistorySetName", 51),
];

/// Looks up `DownloadQueue::EEditAction` (`1..=51`) by case-insensitive command name,
/// returning `-1` if unknown.
pub fn rpc_lookup_edit_command(edit_command: &[u8]) -> i32 {
    for &(name, action_id) in &EDIT_COMMAND_MAP {
        if edit_command.eq_ignore_ascii_case(name) {
            return action_id;
        }
    }
    -1
}

/// Validates `EditQueueXmlCommand::Execute` arguments for score/priority, parameter, and dupe mode actions.
pub fn rpc_validate_edit_queue_args(action: i32, args: &[u8], parsed_int_ok: bool) -> bool {
    // eaGroupSetPriority = 24, eaGroupSetDupeScore = 31, eaHistorySetDupeScore = 44
    if (action == 24 || action == 31 || action == 44) && !parsed_int_ok {
        return false;
    }
    // eaGroupSetParameter = 28, eaHistorySetParameter = 42
    if (action == 28 || action == 42)
        && (args.is_empty() || args[0] == b'=' || !args.contains(&b'='))
    {
        return false;
    }
    // eaGroupSetDupeMode = 32, eaHistorySetDupeMode = 45
    if action == 32 || action == 45 {
        let valid_mode = args.eq_ignore_ascii_case(b"SCORE")
            || args.eq_ignore_ascii_case(b"ALL")
            || args.eq_ignore_ascii_case(b"FORCE");
        if !valid_mode {
            return false;
        }
    }
    true
}

/// Parses the log message kind string in `WriteLogXmlCommand::Execute`, returning:
/// `0` (`INFO`), `1` (`WARNING`), `2` (`ERROR`), `3` (`DEBUG`), `4` (`DETAIL`), or `-1` (invalid).
pub fn rpc_parse_write_log_kind(kind: &[u8]) -> i32 {
    match kind {
        b"INFO" => 0,
        b"WARNING" => 1,
        b"ERROR" => 2,
        b"DEBUG" => 3,
        b"DETAIL" => 4,
        _ => -1,
    }
}

const HISTORY_DUP_STATUS_NAMES: [&CStr; 7] = [
    c"FAILURE/INTERNAL_ERROR",
    c"SUCCESS/HIDDEN",
    c"FAILURE/HIDDEN",
    c"DELETED/MANUAL",
    c"DELETED/DUPE",
    c"FAILURE/BAD",
    c"SUCCESS/GOOD",
];

/// Maps `DupInfo::EStatus` (`0..=6`) to its RPC status string for `HistoryXmlCommand::DetectStatus`.
pub fn rpc_detect_history_dup_status(dup_status: i32) -> &'static CStr {
    if (0..HISTORY_DUP_STATUS_NAMES.len() as i32).contains(&dup_status) {
        HISTORY_DUP_STATUS_NAMES[dup_status as usize]
    } else {
        c"FAILURE/INTERNAL_ERROR"
    }
}

/// Parses the branch name in `StartUpdateXmlCommand::Execute`, returning:
/// `0` (`brStable`), `1` (`brTesting`), `2` (`brDevel`), or `-1` (invalid).
pub fn rpc_parse_update_branch(branch_name: &[u8]) -> i32 {
    if branch_name.eq_ignore_ascii_case(b"stable") {
        0
    } else if branch_name.eq_ignore_ascii_case(b"testing") {
        1
    } else if branch_name.eq_ignore_ascii_case(b"devel") {
        2
    } else {
        -1
    }
}

/// Parses a boolean option string in `SaveConfigXmlCommand::Execute`, falling back to `def_val`.
pub fn rpc_save_config_parse_bool(val: Option<&[u8]>, def_val: bool) -> bool {
    let Some(v) = val else {
        return def_val;
    };
    if v.eq_ignore_ascii_case(b"yes")
        || v.eq_ignore_ascii_case(b"true")
        || v == b"1"
        || v.eq_ignore_ascii_case(b"on")
        || v.eq_ignore_ascii_case(b"enable")
        || v.eq_ignore_ascii_case(b"enabled")
    {
        return true;
    }
    if v.eq_ignore_ascii_case(b"no")
        || v.eq_ignore_ascii_case(b"false")
        || v == b"0"
        || v.eq_ignore_ascii_case(b"off")
        || v.eq_ignore_ascii_case(b"disable")
        || v.eq_ignore_ascii_case(b"disabled")
    {
        return false;
    }
    def_val
}

/// Evaluates global configuration conflict rules R7.2, R7.3, and R7.4 in `SaveConfigXmlCommand::Execute`.
/// Returns:
/// - `0`: no conflict
/// - `2`: Rule 7.2 (`DupeSearch=yes` with `HealthCheck=park`)
/// - `3`: Rule 7.3 (`DirectWrite=yes` with `FlushQueue=no`)
/// - `4`: Rule 7.4 (`DirectUnpack=yes` with `UnpackPauseQueue=no`)
pub fn rpc_save_config_check_global_conflicts(
    dupe_search_val: Option<&[u8]>,
    health_check_val: Option<&[u8]>,
    flush_queue_val: Option<&[u8]>,
    direct_write_val: Option<&[u8]>,
    unpack_pause_queue_val: Option<&[u8]>,
    direct_unpack_val: Option<&[u8]>,
) -> i32 {
    if dupe_search_val.is_some() || health_check_val.is_some() {
        let dupe_search = rpc_save_config_parse_bool(dupe_search_val, false);
        let health_check_park = health_check_val.is_some_and(|v| {
            v.eq_ignore_ascii_case(b"park") || v.eq_ignore_ascii_case(b"hcpark")
        });
        if dupe_search && health_check_park {
            return 2;
        }
    }

    if flush_queue_val.is_some() || direct_write_val.is_some() {
        let flush_queue = rpc_save_config_parse_bool(flush_queue_val, true);
        let direct_write = rpc_save_config_parse_bool(direct_write_val, true);
        if direct_write && !flush_queue {
            return 3;
        }
    }

    if unpack_pause_queue_val.is_some() || direct_unpack_val.is_some() {
        let unpack_pause_queue = rpc_save_config_parse_bool(unpack_pause_queue_val, false);
        let direct_unpack = rpc_save_config_parse_bool(direct_unpack_val, false);
        if direct_unpack && !unpack_pause_queue {
            return 4;
        }
    }

    0
}

// ============================================================================
// RemoteServer hot paths
// ============================================================================

/// Classifies the 4-byte connection signature in `RequestProcessor::Execute`:
/// - `1`: binary NZBGet RPC (`ntohl(signature) == nzb_magic_be`)
/// - `2`: HTTP request (`"POST"`, `"GET "`, or `"OPTI"`)
/// - `0`: unrecognized protocol signature
pub fn remote_classify_signature(sig_bytes: &[u8; 4], nzb_magic_be: u32) -> i32 {
    if u32::from_be_bytes(*sig_bytes) == nzb_magic_be {
        1
    } else if sig_bytes == b"POST" || sig_bytes == b"GET " || sig_bytes == b"OPTI" {
        2
    } else {
        0
    }
}

/// Parses the HTTP method and URL slice from the remaining request line after the 4-byte
/// signature in `RequestProcessor::ServWebRequest`.
/// Returns `Some((http_method, url_offset, url_len))` where `http_method` is:
/// - `0`: `WebProcessor::hmPost`
/// - `1`: `WebProcessor::hmGet`
/// - `2`: `WebProcessor::hmOptions`
pub fn remote_parse_web_request_line(
    sig_bytes: &[u8; 4],
    line: &[u8],
) -> Option<(i32, usize, usize)> {
    let (http_method, start_offset) = if sig_bytes == b"POST" {
        if line.is_empty() {
            return None;
        }
        (0, 1usize)
    } else if sig_bytes == b"OPTI" && line.len() > 4 {
        (2, 4usize)
    } else if sig_bytes == b"GET " {
        (1, 0usize)
    } else {
        return None;
    };

    let rest = &line[start_offset..];
    let url_len = rest.iter().position(|&b| b == b' ').unwrap_or(rest.len());
    Some((http_method, start_offset, url_len))
}

// ============================================================================
// Connection & WebDownloader hot paths
// ============================================================================

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

/// Validates URL protocol and resolves default ports for `WebDownloader::CreateConnection`.
/// Returns `(result_code, resolved_port, is_tls)`:
/// - `result_code == 0`: valid (`adRunning`)
/// - `result_code == -1`: URL is not valid
/// - `result_code == -2`: unsupported protocol
/// - `result_code == -3`: `https` requested when compiled with `DISABLE_TLS`
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

/// Evaluates the body read/write completion state in `WebDownloader::DownloadBody`:
/// - `1`: EOF reached cleanly before write (`chunk_len == 0 && content_len == -1 && written_len > 0`)
/// - `-1`: unexpected EOF or read error (`chunk_len <= 0`)
/// - `2`: complete body written after adding `chunk_len`
/// - `0`: continue reading body chunks
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

// ============================================================================
// Options hot paths
// ============================================================================

/// Finds the first `${Var}` reference in `s` for `Options::SetOption`.
/// Returns `Some((dollar_pos, var_len))` where `s[dollar_pos + 2 .. dollar_pos + 2 + var_len]`
/// is the variable name and the whole token has length `var_len + 3`.
pub fn options_find_var_reference(s: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i + 1 < s.len() {
        if s[i] == b'$' && s[i + 1] == b'{' {
            let rest = &s[i + 2..];
            let end_offset = rest.iter().position(|&b| b == b'}')?;
            return Some((i, end_offset));
        }
        i += 1;
    }
    None
}

/// Strips a single trailing `\n` and then a single trailing `\r` from `line[..len]`
/// (matching `Options::LoadConfigFile` lines 1572–1585) and checks whether the line
/// should be skipped (empty, starts with `'#'`, or consists solely of spaces `' '`).
pub fn options_trim_and_check_config_line(line: &[u8]) -> (usize, bool) {
    let mut len = line.len();
    if len > 0 && line[len - 1] == b'\n' {
        len -= 1;
    }
    if len > 0 && line[len - 1] == b'\r' {
        len -= 1;
    }
    let trimmed = &line[..len];
    let skip =
        trimmed.is_empty() || trimmed[0] == b'#' || trimmed.iter().all(|&b| b == b' ');
    (len, skip)
}

const SCHEDULER_COMMAND_MAP: [(&[u8], i32); 27] = [
    (b"pausedownload", 0),
    (b"pause", 0),
    (b"unpausedownload", 1),
    (b"resumedownload", 1),
    (b"unpause", 1),
    (b"resume", 1),
    (b"pausepostprocess", 2),
    (b"unpausepostprocess", 3),
    (b"resumepostprocess", 3),
    (b"pausepost", 2),
    (b"unpausepost", 3),
    (b"resumepost", 3),
    (b"downloadrate", 4),
    (b"setdownloadrate", 4),
    (b"rate", 4),
    (b"speed", 4),
    (b"script", 5),
    (b"process", 6),
    (b"pausescan", 7),
    (b"unpausescan", 8),
    (b"resumescan", 8),
    (b"activateserver", 9),
    (b"activateservers", 9),
    (b"deactivateserver", 10),
    (b"deactivateservers", 10),
    (b"fetchfeed", 11),
    (b"fetchfeeds", 11),
];

/// Case-insensitively maps a scheduler command string to `Options::ESchedulerCommand` (`0..=11`),
/// or `-1` if unknown.
pub fn options_lookup_scheduler_command(cmd: &[u8]) -> i32 {
    for &(name, val) in &SCHEDULER_COMMAND_MAP {
        if cmd.eq_ignore_ascii_case(name) {
            return val;
        }
    }
    -1
}

/// Validates `TaskX.Param` for `Options::InitScheduler`, returning:
/// - `0`: valid
/// - `1`: invalid `scProcess` parameter (`param` non-empty and `SplitCommandLine` empty)
/// - `2`: invalid `scDownloadRate` integer parameter (`< 0` or non-integer)
/// - `3`: missing required `TaskX.Param`
pub fn options_validate_scheduler_param(
    task_command: i32,
    param: Option<&[u8]>,
    split_cmd_empty: bool,
) -> i32 {
    // scProcess = 6, scDownloadRate = 4, scScript = 5, scActivateServer = 9,
    // scDeactivateServer = 10, scFetchFeed = 11
    if let Some(p) = param {
        if !p.is_empty() && task_command == 6 && split_cmd_empty {
            return 1;
        }
    }

    if task_command == 4 {
        let Some(p) = param else {
            return 3;
        };
        let mut idx = 0;
        while idx < p.len() && matches!(p[idx], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            idx += 1;
        }
        let rest = &p[idx..];
        if !rest.is_empty() {
            let (neg, digits) = if rest[0] == b'-' {
                (true, &rest[1..])
            } else if rest[0] == b'+' {
                (false, &rest[1..])
            } else {
                (false, rest)
            };
            if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
                return 2;
            }
            let mut val: std::os::raw::c_long = 0;
            for &d in digits {
                let digit = (d - b'0') as std::os::raw::c_long;
                val = val.saturating_mul(10);
                val = if neg {
                    val.saturating_sub(digit)
                } else {
                    val.saturating_add(digit)
                };
            }
            if (val as i32) < 0 {
                return 2;
            }
        }
    }

    if matches!(task_command, 5 | 6 | 9 | 10 | 11) {
        let is_empty = match param {
            None => true,
            Some(p) => p.is_empty(),
        };
        if is_empty {
            return 3;
        }
    }

    0
}

// ============================================================================
// Cleanup, Rename, ArchiveProcessor, Log & NString hot paths
// ============================================================================

/// Determines the post-move action and `NzbInfo::EMoveStatus` in `MoveController::Run`:
/// - `(1, 2)`: move succeeded -> set `DestDir`, clear `FinalDir`, set `msSuccess` (`2`)
/// - `(2, 0)`: interrupted by stop -> log resume-on-restart message, keep `msNone` (`0`)
/// - `(3, 1)`: move failed -> log error and set `msFailure` (`1`)
pub fn move_controller_decide_outcome(ok: bool, is_stopped: bool) -> (i32, i32) {
    if ok {
        (1, 2)
    } else if is_stopped {
        (2, 0)
    } else {
        (3, 1)
    }
}

/// Determines the message kind and `NzbInfo::ECleanupStatus` in `CleanupController::Run`:
/// - `(1, 2)`: cleanup succeeded and deleted files -> `csSuccess` (`2`)
/// - `(2, 2)`: cleanup succeeded with nothing to delete -> `csSuccess` (`2`)
/// - `(3, 1)`: cleanup failed -> `csFailure` (`1`)
pub fn cleanup_controller_decide_outcome(ok: bool, deleted: bool) -> (i32, i32) {
    if ok && deleted {
        (1, 2)
    } else if ok {
        (2, 2)
    } else {
        (3, 1)
    }
}

/// Computes `(initial_rename_status, request_extra_pars, request_par_check)` for
/// `RenameController::RenameCompleted`.
pub fn rename_controller_complete_action(
    kind: i32,
    renamed_count: i32,
    has_damaged_pars: bool,
    remaining_par_count: i32,
    has_missed_files: bool,
    par_status: i32,
) -> (i32, bool, bool) {
    // NzbInfo::rsNothing = 2, NzbInfo::rsSuccess = 3, NzbInfo::psSkipped = 1
    let initial_status = if renamed_count > 0 { 3 } else { 2 };
    let request_extra_pars =
        kind == 0 && renamed_count == 0 && has_damaged_pars && remaining_par_count > 0;
    let request_par_check = has_missed_files && par_status <= 1;
    (initial_status, request_extra_pars, request_par_check)
}

/// Case-insensitively checks if `ext` is `".nzb"` for `ArchiveProcessor::IsNzbFile`.
pub fn archive_is_nzb_extension(ext: &[u8]) -> bool {
    ext.eq_ignore_ascii_case(b".nzb")
}

/// Determines the disposal action for `ArchiveProcessor::DisposeArchive`:
/// - `1`: `Delete` configured, but `non_nzb_file_count > 0` -> move to processed dir to prevent data loss
/// - `2`: `Delete` configured and `non_nzb_file_count == 0` -> delete archive
/// - `3`: `Move` configured -> move archive to processed dir
pub fn archive_decide_dispose_action(config_action: i32, non_nzb_file_count: i32) -> i32 {
    // Options::ENzbDirArchiveAction::Delete == 0
    if config_action == 0 {
        if non_nzb_file_count > 0 {
            1
        } else {
            2
        }
    } else {
        3
    }
}

/// Evaluates whether `Log::Filelog` should rotate the log file on a day boundary.
pub fn log_should_rotate(rawtime: i64, last_written: i64, write_log_mode: i32) -> bool {
    // Options::wlRotate == 3, day == 86400
    const DAY: i32 = 86400;
    (rawtime as i32) / DAY != (last_written as i32) / DAY && write_log_mode == 3
}

/// Determines `(write_to_file, keep_in_screen_log)` for a message target in `Log::InitOptions`:
/// - `mtNone` (`0`): `(false, false)`
/// - `mtScreen` (`1`): `(false, true)`
/// - `mtLog` (`2`): `(true, false)`
/// - `mtBoth` (`3`): `(true, true)`
pub fn log_init_message_disposition(target: i32) -> (bool, bool) {
    let write_to_file = target == 2 || target == 3;
    let keep_in_screen = target == 1 || target == 3;
    (write_to_file, keep_in_screen)
}

/// Decodes a UTF-8 byte slice into UTF-16 code units following the exact state machine in
/// `WString::WString(const char* utfstr)` (`daemon/util/NString.cpp`), returning the number
/// of code units written (excluding the trailing NUL terminator).
pub fn wstring_decode_utf8_to_utf16(utf8: &[u8], out: &mut [u16]) -> usize {
    let mut written = 0usize;
    let mut codepoint: u32 = 0;
    let mut i = 0usize;
    while i < utf8.len() && utf8[i] != 0 {
        let ch = utf8[i];
        if ch <= 0x7f {
            codepoint = ch as u32;
        } else if ch <= 0xbf {
            codepoint = (codepoint << 6) | ((ch & 0x3f) as u32);
        } else if ch <= 0xdf {
            codepoint = (ch & 0x1f) as u32;
        } else if ch <= 0xef {
            codepoint = (ch & 0x0f) as u32;
        } else {
            codepoint = (ch & 0x07) as u32;
        }
        i += 1;
        let next_byte = if i < utf8.len() { utf8[i] } else { 0 };
        if (next_byte & 0xc0) != 0x80 && codepoint <= 0x10ffff {
            if codepoint > 0xffff {
                if written + 1 < out.len() {
                    out[written] = (0xd800 + (codepoint >> 10)) as u16;
                    out[written + 1] = (0xdc00 + (codepoint & 0x03ff)) as u16;
                    written += 2;
                }
            } else if !(0xd800..0xe000).contains(&codepoint) && written < out.len() {
                out[written] = codepoint as u16;
                written += 1;
            }
        }
    }
    written
}

/// Computes the target allocation capacity for `StringBuilder::Reserve(int capacity, bool exact)`.
/// Returns `0` when no reallocation is needed (`requested_capacity <= old_capacity && old_capacity != 0`),
/// or the positive `newCapacity` to allocate.
pub fn string_builder_calc_reserve_capacity(
    old_capacity: i32,
    requested_capacity: i32,
    exact: bool,
) -> i32 {
    if requested_capacity > old_capacity || old_capacity == 0 {
        let smart_capacity = if exact {
            0
        } else {
            ((old_capacity as f64) * 1.5) as i32
        };
        if smart_capacity > requested_capacity {
            smart_capacity
        } else {
            requested_capacity
        }
    } else {
        -1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_is_rpc_request() {
        assert!(rpc_is_rpc_request(b"/xmlrpc"));
        assert!(rpc_is_rpc_request(b"/xmlrpc/status"));
        assert!(rpc_is_rpc_request(b"/jsonrpc"));
        assert!(rpc_is_rpc_request(b"/jsonrpc/"));
        assert!(rpc_is_rpc_request(b"/jsonprpc"));
        assert!(rpc_is_rpc_request(b"/jsonprpc/version"));
        assert!(!rpc_is_rpc_request(b"/xmlrpc2"));
        assert!(!rpc_is_rpc_request(b"/index.html"));
    }

    #[test]
    fn test_rpc_log_slice_range() {
        assert_eq!(rpc_log_slice_range(100, 1, 0, 20), (80, 20));
        assert_eq!(rpc_log_slice_range(10, 1, 0, 50), (0, 10));
        assert_eq!(rpc_log_slice_range(50, 100, 110, 0), (10, 50));
        assert_eq!(rpc_log_slice_range(50, 100, 50, 0), (0, 50));
    }

    #[test]
    fn test_rpc_list_files_match_and_progress() {
        assert_eq!(
            rpc_list_files_match_and_progress(5, 5, 10, 0, 0, 1000, 250, 0, 0, 750),
            Some(750)
        );
        assert_eq!(
            rpc_list_files_match_and_progress(5, 6, 10, 0, 0, 1000, 250, 0, 0, 750),
            None
        );
        assert_eq!(
            rpc_list_files_match_and_progress(0, 6, 10, 5, 15, 1000, 1000, 0, 0, 0),
            Some(0)
        );
    }

    #[test]
    fn test_rpc_detect_group_status() {
        assert_eq!(
            rpc_detect_group_status(true, 0, true, true, 0, 0, 0, 0, 0),
            c"QS_EXECUTING"
        );
        assert_eq!(
            rpc_detect_group_status(true, 0, true, false, 0, 0, 0, 0, 0),
            c"QS_QUEUED"
        );
        assert_eq!(
            rpc_detect_group_status(true, 2, false, false, 1, 0, 0, 0, 0),
            c"UNPACKING"
        );
        assert_eq!(
            rpc_detect_group_status(true, 3, false, false, 0, 0, 0, 0, 0),
            c"REPAIRING"
        );
        assert_eq!(
            rpc_detect_group_status(false, 0, false, false, 0, 2, 1, 0, 100),
            c"FETCHING"
        );
        assert_eq!(
            rpc_detect_group_status(false, 0, false, false, 0, 2, 0, 0, 100),
            c"DOWNLOADING"
        );
        assert_eq!(
            rpc_detect_group_status(false, 0, false, false, 0, 0, 0, 100, 100),
            c"PAUSED"
        );
        assert_eq!(
            rpc_detect_group_status(false, 0, false, false, 0, 0, 0, 50, 100),
            c"QUEUED"
        );
    }

    #[test]
    fn test_rpc_lookup_and_validate_edit_queue() {
        assert_eq!(rpc_lookup_edit_command(b"FileMoveOffset"), 1);
        assert_eq!(rpc_lookup_edit_command(b"groupsetdupescore"), 31);
        assert_eq!(rpc_lookup_edit_command(b"HistorySetName"), 51);
        assert_eq!(rpc_lookup_edit_command(b"UnknownCommand"), -1);

        assert!(rpc_validate_edit_queue_args(31, b"100", true));
        assert!(!rpc_validate_edit_queue_args(31, b"999999999999", false));
        assert!(rpc_validate_edit_queue_args(28, b"*Unpack:Password=secret", true));
        assert!(!rpc_validate_edit_queue_args(28, b"=secret", true));
        assert!(!rpc_validate_edit_queue_args(28, b"noequals", true));
        assert!(rpc_validate_edit_queue_args(32, b"score", false));
        assert!(!rpc_validate_edit_queue_args(32, b"invalid", false));
    }

    #[test]
    fn test_rpc_write_log_history_update_and_conflicts() {
        assert_eq!(rpc_parse_write_log_kind(b"INFO"), 0);
        assert_eq!(rpc_parse_write_log_kind(b"DETAIL"), 4);
        assert_eq!(rpc_parse_write_log_kind(b"OTHER"), -1);

        assert_eq!(rpc_detect_history_dup_status(1), c"SUCCESS/HIDDEN");
        assert_eq!(rpc_detect_history_dup_status(6), c"SUCCESS/GOOD");
        assert_eq!(rpc_detect_history_dup_status(99), c"FAILURE/INTERNAL_ERROR");

        assert_eq!(rpc_parse_update_branch(b"Stable"), 0);
        assert_eq!(rpc_parse_update_branch(b"TESTING"), 1);
        assert_eq!(rpc_parse_update_branch(b"devel"), 2);
        assert_eq!(rpc_parse_update_branch(b"nightly"), -1);

        assert_eq!(
            rpc_save_config_check_global_conflicts(
                Some(b"yes"),
                Some(b"park"),
                None,
                None,
                None,
                None
            ),
            2
        );
        assert_eq!(
            rpc_save_config_check_global_conflicts(
                None,
                None,
                Some(b"no"),
                Some(b"yes"),
                None,
                None
            ),
            3
        );
        assert_eq!(
            rpc_save_config_check_global_conflicts(
                None,
                None,
                None,
                None,
                Some(b"no"),
                Some(b"yes")
            ),
            4
        );
        assert_eq!(
            rpc_save_config_check_global_conflicts(
                Some(b"yes"),
                Some(b"dupe"),
                Some(b"yes"),
                Some(b"yes"),
                Some(b"yes"),
                Some(b"yes")
            ),
            0
        );
    }

    #[test]
    fn test_remote_signature_and_request_line() {
        let magic = 0x6e7a622d_u32;
        assert_eq!(remote_classify_signature(&magic.to_be_bytes(), magic), 1);
        assert_eq!(remote_classify_signature(b"POST", magic), 2);
        assert_eq!(remote_classify_signature(b"GET ", magic), 2);
        assert_eq!(remote_classify_signature(b"OPTI", magic), 2);
        assert_eq!(remote_classify_signature(b"HEAD", magic), 0);

        assert_eq!(
            remote_parse_web_request_line(b"GET ", b"/jsonrpc HTTP/1.1\r\n"),
            Some((1, 0, 8))
        );
        assert_eq!(
            remote_parse_web_request_line(b"POST", b" /xmlrpc HTTP/1.1\r\n"),
            Some((0, 1, 7))
        );
        assert_eq!(
            remote_parse_web_request_line(b"OPTI", b"ONS /jsonrpc HTTP/1.1\r\n"),
            Some((2, 4, 8))
        );
    }

    #[test]
    fn test_connection_and_webdownloader_helpers() {
        assert_eq!(conn_read_line_chunk(b"hello\nworld", 100), (6, true));
        assert_eq!(conn_read_line_chunk(b"hello world", 5), (5, false));

        assert_eq!(
            conn_classify_accept_errno(4, 4, 53, 71, 50, 42, 64, 65, 45, 51, 0, 24, 23, 55, 12),
            1
        );
        assert_eq!(
            conn_classify_accept_errno(24, 4, 53, 71, 50, 42, 64, 65, 45, 51, 0, 24, 23, 55, 12),
            2
        );
        assert_eq!(
            conn_classify_accept_errno(22, 4, 53, 71, 50, 42, 64, 65, 45, 51, 0, 24, 23, 55, 12),
            0
        );

        assert_eq!(
            webdl_resolve_protocol_port(true, b"http", 0, false),
            (0, 80, false)
        );
        assert_eq!(
            webdl_resolve_protocol_port(true, b"HTTPS", 0, false),
            (0, 443, true)
        );
        assert_eq!(
            webdl_resolve_protocol_port(true, b"https", 8443, true),
            (-3, 8443, true)
        );
        assert_eq!(
            webdl_resolve_protocol_port(true, b"ftp", 21, false),
            (-2, 21, false)
        );

        assert_eq!(webdl_check_body_chunk(0, -1, 512, false, false), 1);
        assert_eq!(webdl_check_body_chunk(0, 1024, 512, false, false), -1);
        assert_eq!(webdl_check_body_chunk(512, 1024, 512, false, false), 2);
        assert_eq!(webdl_check_body_chunk(256, 1024, 512, false, false), 0);
        assert_eq!(webdl_check_body_chunk(128, -1, 512, true, false), 0);
        assert_eq!(webdl_check_body_chunk(128, -1, 512, true, true), 2);
    }

    #[test]
    fn test_options_helpers() {
        assert_eq!(
            options_find_var_reference(b"${MainDir}/dst"),
            Some((0, 7))
        );
        assert_eq!(
            options_find_var_reference(b"/path/${Category}/sub"),
            Some((6, 8))
        );
        assert_eq!(options_find_var_reference(b"/plain/path"), None);

        assert_eq!(
            options_trim_and_check_config_line(b"MainDir=/downloads\r\n"),
            (18, false)
        );
        assert_eq!(
            options_trim_and_check_config_line(b"# comment line\n"),
            (14, true)
        );
        assert_eq!(options_trim_and_check_config_line(b"   \r\n"), (3, true));

        assert_eq!(options_lookup_scheduler_command(b"PauseDownload"), 0);
        assert_eq!(options_lookup_scheduler_command(b"DownloadRate"), 4);
        assert_eq!(options_lookup_scheduler_command(b"FetchFeeds"), 11);
        assert_eq!(options_lookup_scheduler_command(b"InvalidCmd"), -1);

        assert_eq!(options_validate_scheduler_param(4, Some(b"1024"), false), 0);
        assert_eq!(options_validate_scheduler_param(4, Some(b"  1024"), false), 0);
        assert_eq!(options_validate_scheduler_param(4, Some(b"\xc2\xa01000"), false), 2);
        assert_eq!(options_validate_scheduler_param(4, Some(b"-5"), false), 2);
        assert_eq!(options_validate_scheduler_param(4, None, false), 3);
        assert_eq!(options_validate_scheduler_param(6, Some(b"\"unclosed"), true), 1);
        assert_eq!(options_validate_scheduler_param(5, Some(b""), false), 3);
    }

    #[test]
    fn test_cleanup_rename_archive_log_nstring() {
        assert_eq!(move_controller_decide_outcome(true, false), (1, 2));
        assert_eq!(move_controller_decide_outcome(false, true), (2, 0));
        assert_eq!(move_controller_decide_outcome(false, false), (3, 1));

        assert_eq!(cleanup_controller_decide_outcome(true, true), (1, 2));
        assert_eq!(cleanup_controller_decide_outcome(true, false), (2, 2));
        assert_eq!(cleanup_controller_decide_outcome(false, false), (3, 1));

        assert_eq!(
            rename_controller_complete_action(0, 0, true, 2, true, 0),
            (2, true, true)
        );
        assert_eq!(
            rename_controller_complete_action(1, 3, false, 0, false, 3),
            (3, false, false)
        );

        assert!(archive_is_nzb_extension(b".NZB"));
        assert!(!archive_is_nzb_extension(b".rar"));
        assert_eq!(archive_decide_dispose_action(0, 2), 1);
        assert_eq!(archive_decide_dispose_action(0, 0), 2);
        assert_eq!(archive_decide_dispose_action(1, 0), 3);

        assert!(log_should_rotate(86400 * 2, 86400, 3));
        assert!(!log_should_rotate(86400 + 10, 86400 + 5, 3));
        assert_eq!(log_init_message_disposition(0), (false, false));
        assert_eq!(log_init_message_disposition(1), (false, true));
        assert_eq!(log_init_message_disposition(2), (true, false));
        assert_eq!(log_init_message_disposition(3), (true, true));

        let mut utf16 = [0u16; 16];
        let n = wstring_decode_utf8_to_utf16("A\u{00E9}\u{1F600}".as_bytes(), &mut utf16);
        assert_eq!(n, 4);
        assert_eq!(utf16[0], b'A' as u16);
        assert_eq!(utf16[1], 0x00e9);

        assert_eq!(string_builder_calc_reserve_capacity(100, 120, false), 150);
        assert_eq!(string_builder_calc_reserve_capacity(100, 120, true), 120);
        assert_eq!(string_builder_calc_reserve_capacity(100, 80, false), -1);
    }
}
