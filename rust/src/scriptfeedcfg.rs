//! QueueScript, PostScript, ScriptConfig, ScanScript, CommandScript, SchedulerScript,
//! FeedFile & FeedInfo hot-path kernels.
//!
//! Each function mirrors the corresponding C++ logic byte-for-byte, including
//! C `strcasecmp` locale-aware case-folding and wrapping integer arithmetic.

use std::ffi::{c_char, c_int, CStr};

extern "C" {
    fn strcasecmp(a: *const c_char, b: *const c_char) -> c_int;
}

/// `QueueScriptController::StartScript` and `QueueScriptCoordinator::EnqueueScript`:
/// Checks whether queue script execution is enabled and permitted for `nzbInfo`.
/// Mirrors:
/// `has_queue_scripts && has_nzb && !skip_script_processing`
#[inline]
pub fn queue_script_can_start_or_enqueue(
    has_queue_scripts: bool,
    has_nzb: bool,
    skip_script_processing: bool,
) -> bool {
    has_queue_scripts && has_nzb && !skip_script_processing
}

/// `QueueScriptController::ExecuteScript` / `PostScriptController::ExecuteScript` /
/// `ScanScriptController::ExecuteScript`:
/// Computes `m_prefixLen = strlen(script->GetDisplayName()) + 2`.
#[inline]
pub fn script_ctrl_calc_prefix_len(display_name_len: usize) -> i32 {
    display_name_len.wrapping_add(2) as i32
}

/// `QueueScriptCoordinator::CheckQueue`:
/// Checks whether the candidate `queueItem` should replace `curNzbInfo`.
/// Mirrors:
/// `!curNzbInfo || queueItem->GetEvent() > maxEvent`
#[inline]
pub fn queue_script_is_better_queued_item(
    has_cur_nzb: bool,
    item_event: i32,
    max_event: i32,
) -> bool {
    !has_cur_nzb || item_event > max_event
}

/// `QueueScriptCoordinator::GetQueueSize`:
/// Computes total queue size including `m_curItem` if active.
#[inline]
pub fn queue_script_calc_queue_size(queue_len: usize, has_cur_item: bool) -> i32 {
    (queue_len as i32).wrapping_add(i32::from(has_cur_item))
}

/// `PostScriptController::ExecuteScript`:
/// Determines whether script execution should be skipped.
/// Mirrors:
/// `!script->GetPostScript() || m_postInfo->GetRequestParCheck()`
#[inline]
pub fn post_script_should_skip_execute(is_post_script: bool, request_par_check: bool) -> bool {
    !is_post_script || request_par_check
}

/// `PostScriptController::PrepareParams`:
/// Resolves compatibility `parStatus` (`psFailure` when deleted or marked bad).
/// Mirrors:
/// ```cpp
/// if (deleteStatus != NzbInfo::dsNone || markStatus == NzbInfo::ksBad)
///     parStatus = NzbInfo::psFailure;
/// ```
#[inline]
pub fn post_script_resolve_compat_par_status(
    par_status: i32,
    ps_failure: i32,
    delete_status: i32,
    ds_none: i32,
    mark_status: i32,
    ks_bad: i32,
) -> i32 {
    if delete_status != ds_none || mark_status == ks_bad {
        ps_failure
    } else {
        par_status
    }
}

/// `PostScriptController::AddMessage`:
/// Checks whether post-processing is paused for a non-force-priority job.
/// Mirrors:
/// `g_WorkState->GetPausePostProcess() && !m_postInfo->GetNzbInfo()->GetForcePriority() && !IsStopped()`
#[inline]
pub fn post_script_is_paused_wait_active(
    pause_post_process: bool,
    force_priority: bool,
    is_stopped: bool,
) -> bool {
    pause_post_process && !force_priority && !is_stopped
}

/// `PostScriptController::AddMessage`:
/// Computes updated `(stage_time, start_time)` while waiting for post-process unpause.
#[inline]
pub fn post_script_calc_paused_timestamps(
    cur_time: i64,
    wait_time: i64,
    stage_time: i64,
    start_time: i64,
) -> (i64, i64) {
    let delta = cur_time.wrapping_sub(wait_time);
    let new_stage = if stage_time > 0 {
        stage_time.wrapping_add(delta)
    } else {
        0
    };
    let new_start = if start_time > 0 {
        start_time.wrapping_add(delta)
    } else {
        0
    };
    (new_stage, new_start)
}

/// `ScriptConfig::LoadConfigUnlocked`:
/// Checks whether a trimmed configuration line is empty, a `#` comment, or all spaces.
/// Mirrors:
/// `buf[0] == 0 || buf[0] == '#' || strspn(buf, " ") == strlen(buf)`
#[inline]
pub fn script_config_is_comment_or_blank_line(line: &CStr) -> bool {
    let bytes = line.to_bytes();
    if bytes.is_empty() || bytes[0] == b'#' {
        return true;
    }
    bytes.iter().all(|&b| b == b' ')
}

/// `ScriptConfig::SaveConfig`:
/// Checks whether a configuration line contains `'='` and does not start with `'#'`.
/// Mirrors:
/// `const char* eq = strchr(buf, '='); eq && buf[0] != '#'`
#[inline]
pub fn script_config_is_editable_option_line(line: &CStr) -> bool {
    let bytes = line.to_bytes();
    !bytes.is_empty() && bytes[0] != b'#' && bytes.contains(&b'=')
}

/// `ScriptConfig::SaveConfig`:
/// Checks whether a line of length `len` ends with `'\n'`.
#[inline]
pub fn script_config_line_has_trailing_lf(line: &CStr) -> bool {
    let bytes = line.to_bytes();
    !bytes.is_empty() && bytes[bytes.len() - 1] == b'\n'
}

/// `ScanScriptController::PrepareParams`:
/// Returns the length of `dir` after stripping a single trailing `sep` if present.
/// Mirrors:
/// ```cpp
/// int len = strlen(dir);
/// if (dir[len-1] == PATH_SEPARATOR) dir[len-1] = '\0';
/// ```
#[inline]
pub fn scan_script_trim_trailing_sep_len(dir: &CStr, sep: u8) -> usize {
    let bytes = dir.to_bytes();
    if !bytes.is_empty() && bytes[bytes.len() - 1] == sep {
        bytes.len() - 1
    } else {
        bytes.len()
    }
}

/// `CommandScriptLog::AddMessage`:
/// Checks whether `script_id` matches the active `id_script_gen`.
#[inline]
pub fn cmd_script_should_append_log_message(script_id: i32, id_script_gen: i32) -> bool {
    script_id == id_script_gen
}

/// `SchedulerScriptController::ExecuteScript`:
/// Checks whether `m_taskId` is non-zero (so `" for Task%i"` is formatted).
#[inline]
pub fn sched_script_should_format_task_suffix(task_id: i32) -> bool {
    task_id != 0
}

/// `FeedFile::Parse_StartElement`:
/// Classifies an `<enclosure>` attribute name:
/// - `1`: `"url"`
/// - `2`: `"length"`
/// - `0`: other
#[inline]
pub fn feed_file_classify_enclosure_attr(attr_name: &CStr) -> i32 {
    match attr_name.to_bytes() {
        b"url" => 1,
        b"length" => 2,
        _ => 0,
    }
}

/// `FeedFile::Parse_StartElement`:
/// Checks whether `elem_name` and attributes represent a valid `newznab:attr` or `nZEDb:attr`
/// element with `atts[0] == "name"` and `atts[2] == "value"`.
///
/// # Safety
/// All non-null pointers must point to valid null-terminated C strings.
#[inline]
pub unsafe fn feed_file_is_newznab_attr_element(
    elem_name: *const c_char,
    att0: *const c_char,
    att1: *const c_char,
    att2: *const c_char,
    att3: *const c_char,
) -> bool {
    if elem_name.is_null()
        || att0.is_null()
        || att1.is_null()
        || att2.is_null()
        || att3.is_null()
    {
        return false;
    }
    let name_bytes = CStr::from_ptr(elem_name).to_bytes();
    if name_bytes != b"newznab:attr" && name_bytes != b"nZEDb:attr" {
        return false;
    }
    CStr::from_ptr(att0).to_bytes() == b"name" && CStr::from_ptr(att2).to_bytes() == b"value"
}

/// `FeedFile::Parse_StartElement`:
/// Classifies a `newznab:attr` / `nZEDb:attr` attribute key (`atts[1]`):
/// - `1`: `"size"` when `cur_size == 0`
/// - `2`: `"imdb"`
/// - `3`: `"rageid"`
/// - `4`: `"tvdbid"`
/// - `5`: `"tvmazeid"`
/// - `6`: `"episode"`
/// - `7`: `"season"`
/// - `0`: other (or `"size"` when `cur_size != 0`)
#[inline]
pub fn feed_file_classify_newznab_attr_key(key: &CStr, cur_size: i64) -> i32 {
    match key.to_bytes() {
        b"size" if cur_size == 0 => 1,
        b"size" => 0,
        b"imdb" => 2,
        b"rageid" => 3,
        b"tvdbid" => 4,
        b"tvmazeid" => 5,
        b"episode" => 6,
        b"season" => 7,
        _ => 0,
    }
}

/// `FeedFile::Parse_EndElement`:
/// Classifies a closing XML element tag when `m_feedItemInfo` is active:
/// - `1`: `"title"`
/// - `2`: `"link"` (when `url_empty` is true)
/// - `3`: `"category"`
/// - `4`: `"description"`
/// - `5`: `"pubDate"`
/// - `0`: other / no action
#[inline]
pub fn feed_file_classify_end_element(elem_name: &CStr, has_item: bool, url_empty: bool) -> i32 {
    if !has_item {
        return 0;
    }
    match elem_name.to_bytes() {
        b"title" => 1,
        b"link" if url_empty => 2,
        b"category" => 3,
        b"description" => 4,
        b"pubDate" => 5,
        _ => 0,
    }
}

/// `FeedFile::SAX_error`:
/// Computes the length of `err_msg` after stripping trailing `'\n'`, `'\r'`, and `' '` bytes.
#[inline]
pub fn feed_file_trim_trailing_crlf_space_len(err_msg: &CStr) -> usize {
    let bytes = err_msg.to_bytes();
    let mut end = bytes.len();
    while end > 0 && matches!(bytes[end - 1], b'\n' | b'\r' | b' ') {
        end -= 1;
    }
    end
}

/// `FeedItemInfo::Attributes::Find`:
/// Compares an attribute name against `target_name` case-insensitively using C `strcasecmp`
/// so the active `LC_CTYPE` locale is respected.
///
/// # Safety
/// `attr_name` and `target_name` must be valid null-terminated C strings (or null).
#[inline]
pub unsafe fn feed_info_attr_name_eq_ignore_case(
    attr_name: *const c_char,
    target_name: *const c_char,
) -> bool {
    if attr_name.is_null() || target_name.is_null() {
        return false;
    }
    strcasecmp(attr_name, target_name) == 0
}

/// `FeedItemInfo::AppendDupeKey`:
/// Checks whether `extra_dupe_key` should be appended to `m_dupeKey`.
/// Mirrors:
/// `!m_dupeKey.empty() && !Util::EmptyStr(extraDupeKey)`
///
/// # Safety
/// `extra_dupe_key` must be either null or a valid null-terminated C string.
#[inline]
pub unsafe fn feed_info_should_append_dupe_key(
    dupe_key_empty: bool,
    extra_dupe_key: *const c_char,
) -> bool {
    !dupe_key_empty && !extra_dupe_key.is_null() && *extra_dupe_key != 0
}

/// `FeedItemInfo::GetSeasonNum` / `FeedItemInfo::GetEpisodeNum`:
/// Checks whether `ParseSeasonEpisode()` should be invoked lazily.
/// Mirrors:
/// `m_season.empty() && !m_seasonEpisodeParsed`
#[inline]
pub fn feed_info_should_parse_season_episode(field_empty: bool, parsed: bool) -> bool {
    field_empty && !parsed
}

/// Formats `Season` (`S%02d`) and `Episode` (`E` + `ep_raw` with `'-'` removed and
/// ASCII letters uppercased) for `FeedItemInfo::ParseSeasonEpisode`.
pub fn feed_info_format_season_episode(season_num: i32, ep_raw: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let season = format!("S{season_num:02}").into_bytes();
    let mut episode = Vec::with_capacity(1 + ep_raw.len());
    episode.push(b'E');
    for &b in ep_raw {
        if b != b'-' {
            episode.push(b.to_ascii_uppercase());
        }
    }
    (season, episode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_queue_and_post_script_helpers() {
        assert!(queue_script_can_start_or_enqueue(true, true, false));
        assert!(!queue_script_can_start_or_enqueue(false, true, false));
        assert!(!queue_script_can_start_or_enqueue(true, true, true));

        assert_eq!(script_ctrl_calc_prefix_len(12), 14);
        assert!(queue_script_is_better_queued_item(false, 0, 0));
        assert!(queue_script_is_better_queued_item(true, 4, 2));
        assert!(!queue_script_is_better_queued_item(true, 2, 4));
        assert_eq!(queue_script_calc_queue_size(3, true), 4);
        assert_eq!(queue_script_calc_queue_size(3, false), 3);

        assert!(post_script_should_skip_execute(false, false));
        assert!(post_script_should_skip_execute(true, true));
        assert!(!post_script_should_skip_execute(true, false));

        assert_eq!(
            post_script_resolve_compat_par_status(2, 1, 0, 0, 0, 1),
            2
        );
        assert_eq!(
            post_script_resolve_compat_par_status(2, 1, 3, 0, 0, 1),
            1
        );
        assert_eq!(
            post_script_resolve_compat_par_status(2, 1, 0, 0, 1, 1),
            1
        );

        assert!(post_script_is_paused_wait_active(true, false, false));
        assert!(!post_script_is_paused_wait_active(true, true, false));
        assert_eq!(
            post_script_calc_paused_timestamps(150, 100, 1000, 0),
            (1050, 0)
        );
    }

    #[test]
    fn test_script_config_and_scan_cmd_sched_helpers() {
        assert!(script_config_is_comment_or_blank_line(c""));
        assert!(script_config_is_comment_or_blank_line(c"# comment"));
        assert!(script_config_is_comment_or_blank_line(c"    "));
        assert!(!script_config_is_comment_or_blank_line(c"Server1.Host=news.example.com"));

        assert!(script_config_is_editable_option_line(c"Server1.Host=news.example.com"));
        assert!(!script_config_is_editable_option_line(c"#Server1.Host=news.example.com"));
        assert!(!script_config_is_editable_option_line(c"NoEquals"));

        assert!(script_config_line_has_trailing_lf(c"hello\n"));
        assert!(!script_config_line_has_trailing_lf(c"hello"));

        assert_eq!(scan_script_trim_trailing_sep_len(c"/downloads/tv/", b'/'), 13);
        assert_eq!(scan_script_trim_trailing_sep_len(c"/downloads/tv", b'/'), 13);

        assert!(cmd_script_should_append_log_message(5, 5));
        assert!(!cmd_script_should_append_log_message(4, 5));
        assert!(sched_script_should_format_task_suffix(1));
        assert!(!sched_script_should_format_task_suffix(0));
    }

    #[test]
    fn test_feed_file_and_info_helpers() {
        assert_eq!(feed_file_classify_enclosure_attr(c"url"), 1);
        assert_eq!(feed_file_classify_enclosure_attr(c"length"), 2);
        assert_eq!(feed_file_classify_enclosure_attr(c"type"), 0);

        let elem = CString::new("newznab:attr").unwrap();
        let k_name = CString::new("name").unwrap();
        let v_name = CString::new("imdb").unwrap();
        let k_val = CString::new("value").unwrap();
        let v_val = CString::new("1588173").unwrap();
        unsafe {
            assert!(feed_file_is_newznab_attr_element(
                elem.as_ptr(),
                k_name.as_ptr(),
                v_name.as_ptr(),
                k_val.as_ptr(),
                v_val.as_ptr()
            ));
        }

        assert_eq!(feed_file_classify_newznab_attr_key(c"size", 0), 1);
        assert_eq!(feed_file_classify_newznab_attr_key(c"size", 100), 0);
        assert_eq!(feed_file_classify_newznab_attr_key(c"imdb", 0), 2);
        assert_eq!(feed_file_classify_newznab_attr_key(c"rageid", 0), 3);
        assert_eq!(feed_file_classify_newznab_attr_key(c"tvdbid", 0), 4);
        assert_eq!(feed_file_classify_newznab_attr_key(c"tvmazeid", 0), 5);
        assert_eq!(feed_file_classify_newznab_attr_key(c"episode", 0), 6);
        assert_eq!(feed_file_classify_newznab_attr_key(c"season", 0), 7);

        assert_eq!(feed_file_classify_end_element(c"title", true, false), 1);
        assert_eq!(feed_file_classify_end_element(c"link", true, true), 2);
        assert_eq!(feed_file_classify_end_element(c"link", true, false), 0);
        assert_eq!(feed_file_classify_end_element(c"category", true, false), 3);
        assert_eq!(feed_file_classify_end_element(c"description", true, false), 4);
        assert_eq!(feed_file_classify_end_element(c"pubDate", true, false), 5);

        assert_eq!(
            feed_file_trim_trailing_crlf_space_len(c"xml parse error \r\n"),
            15
        );

        unsafe {
            assert!(feed_info_attr_name_eq_ignore_case(
                c"Category".as_ptr(),
                c"category".as_ptr()
            ));
            assert!(!feed_info_attr_name_eq_ignore_case(
                c"Category".as_ptr(),
                c"size".as_ptr()
            ));
            assert!(feed_info_should_append_dupe_key(false, c"extra".as_ptr()));
            assert!(!feed_info_should_append_dupe_key(true, c"extra".as_ptr()));
            assert!(!feed_info_should_append_dupe_key(false, c"".as_ptr()));
        }

        assert!(feed_info_should_parse_season_episode(true, false));
        assert!(!feed_info_should_parse_season_episode(false, false));
        assert!(!feed_info_should_parse_season_episode(true, true));

        let (season, episode) = feed_info_format_season_episode(3, b"02-e03");
        assert_eq!(season, b"S03");
        assert_eq!(episode, b"E02E03");
    }
}
