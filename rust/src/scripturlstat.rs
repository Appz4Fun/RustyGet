//! Hot-path Rust implementations for `StatMeter`, `WebProcessor`, `UrlCoordinator`,
//! `DirectRenamer`, `NzbFile`, `QueueScriptController`/`QueueScriptCoordinator`,
//! `PostScriptController`, `ScanScriptController`, `ScriptConfig`, and `ScriptController`.

use std::collections::BTreeMap;

// ============================================================================
// 1. StatMeter (`daemon/nntp/StatMeter.cpp`)
// ============================================================================

pub const SPEEDMETER_SLOTS: usize = 30;
pub const SPEEDMETER_SLOTSIZE: i32 = 1;

/// Evaluates `StatMeter::IntervalCheck` time-step and timezone refresh decisions.
/// Returns `(adjust_clocks, clock_delta, adjust_time_offset)`.
pub fn eval_interval_check(
    cur_time: i64,
    last_check: i64,
    last_time_offset: i64,
    start_download: i64,
    stand_by: bool,
) -> (bool, i64, bool, bool) {
    let diff = cur_time - last_check;
    if diff > 60 || diff < 0 {
        let delta = diff + 1;
        let adjust_download = start_download != 0 && !stand_by;
        (true, delta, adjust_download, true)
    } else if last_time_offset > cur_time
        || cur_time - last_time_offset > 60 * 60 * 3
        || (cur_time - last_time_offset > 60 && !stand_by)
    {
        (false, 0, false, true)
    } else {
        (false, 0, false, false)
    }
}

/// Computes `StatMeter::CalcTotalStat` (`up_time_sec`, `dn_time_sec`).
pub fn calc_total_stat(
    cur_time: i64,
    start_server: i64,
    start_download: i64,
    paused_from: i64,
    stand_by: bool,
) -> (i32, i32) {
    let up_time_sec = if start_server > 0 {
        (cur_time - start_server) as i32
    } else {
        0
    };
    let dn_time_sec = if stand_by {
        (paused_from - start_download) as i32
    } else {
        (cur_time - start_download) as i32
    };
    (up_time_sec, dn_time_sec)
}

/// Computes `StatMeter::CalcCurrentDownloadSpeed`.
pub fn calc_current_download_speed(
    stand_by: bool,
    cur_time: i64,
    speed_start_time: i32,
    speed_total_bytes: i64,
) -> i64 {
    if stand_by {
        return 0;
    }
    let time_diff = cur_time - (speed_start_time as i64) * (SPEEDMETER_SLOTSIZE as i64);
    if time_diff == 0 {
        return 0;
    }
    speed_total_bytes / time_diff
}

/// Computes `StatMeter::CalcMomentaryDownloadSpeed`.
pub fn calc_momentary_download_speed(cur_time: i64, cur_sec_time: i64, cur_sec_bytes: i64) -> i64 {
    if cur_time == cur_sec_time {
        cur_sec_bytes
    } else {
        0
    }
}

/// Performs `StatMeter::AddSpeedReading` ring-buffer slot advancement and byte accumulation.
#[allow(clippy::too_many_arguments)]
pub fn add_speed_reading(
    bytes: i32,
    cur_time: i64,
    cur_sec_time: &mut i64,
    cur_sec_bytes: &mut i64,
    speed_bytes: &mut [i32; SPEEDMETER_SLOTS],
    speed_time: &mut [i32; SPEEDMETER_SLOTS],
    speed_bytes_index: &mut i32,
    speed_total_bytes: &mut i64,
    speed_start_time: &mut i32,
    speed_correction: &mut i64,
    all_bytes: &mut i64,
) {
    let now_slot = (cur_time as i32) / SPEEDMETER_SLOTSIZE;

    if cur_time != *cur_sec_time {
        *cur_sec_time = cur_time;
        *cur_sec_bytes = 0;
    }
    *cur_sec_bytes += bytes as i64;

    let mut idx = (*speed_bytes_index).clamp(0, (SPEEDMETER_SLOTS - 1) as i32) as usize;
    while now_slot > speed_time[idx] {
        idx += 1;
        if idx >= SPEEDMETER_SLOTS {
            idx = 0;
        }
        *speed_total_bytes -= speed_bytes[idx] as i64;
        *speed_start_time = speed_time[idx];
        speed_bytes[idx] = 0;
        speed_time[idx] = now_slot;
    }
    *speed_bytes_index = idx as i32;

    if cur_time > *speed_correction {
        let mut total: i64 = 0;
        for &b in speed_bytes.iter() {
            total += b as i64;
        }
        *speed_total_bytes = total;
        *speed_correction = cur_time;
    }

    if *speed_total_bytes == 0 {
        *speed_start_time = now_slot;
    }
    speed_bytes[idx] = speed_bytes[idx].wrapping_add(bytes);
    *speed_total_bytes += bytes as i64;
    *all_bytes += bytes as i64;
}

/// Computes `StatMeter::CalcQuotaUsage` (`month_bytes`, `day_bytes`).
pub fn calc_quota_usage(
    loc_time: i64,
    first_day: i32,
    bytes_per_days: &[i64],
    elapsed_slots: i32,
) -> (i64, i64) {
    let day_slot = (loc_time / 86400) as i32 - first_day;
    let day_bytes = if day_slot >= 0 && (day_slot as usize) < bytes_per_days.len() {
        bytes_per_days[day_slot as usize]
    } else {
        0
    };

    let mut month_bytes: i64 = 0;
    let end_slot = (day_slot - elapsed_slots).max(-1);
    let mut slot = day_slot;
    while slot >= 0 && slot > end_slot {
        if (slot as usize) < bytes_per_days.len() {
            month_bytes += bytes_per_days[slot as usize];
        }
        slot -= 1;
    }
    (month_bytes, day_bytes)
}

/// Evaluates `StatMeter::CheckQuota` transition action and new quota-reached state.
/// Action codes:
/// - `0`: no log message
/// - `1`: warn `"Monthly quota reached at %s"`
/// - `2`: warn `"Daily quota reached at %s"`
/// - `3`: info `"Quota lifted"`
pub fn eval_quota_status(
    monthly_quota_mb: i32,
    daily_quota_mb: i32,
    month_bytes: i64,
    day_bytes: i64,
    was_quota_reached: bool,
) -> (bool, i32) {
    let monthly_reached =
        monthly_quota_mb > 0 && month_bytes >= (monthly_quota_mb as i64) * 1024 * 1024;
    let daily_reached = daily_quota_mb > 0 && day_bytes >= (daily_quota_mb as i64) * 1024 * 1024;
    let action = if monthly_reached && !was_quota_reached {
        1
    } else if daily_reached && !was_quota_reached {
        2
    } else if !monthly_reached && !daily_reached && was_quota_reached {
        3
    } else {
        0
    };
    (monthly_reached || daily_reached, action)
}

// ============================================================================
// 2. WebProcessor (`daemon/remote/WebServer.cpp`)
// ============================================================================

/// Validates URL characters in `WebProcessor::Dispatch`:
/// allows only `0..9 A..Z a..z . - + _ / ?` and rejects `..`.
pub fn web_is_safe_url_chars(url: &[u8]) -> bool {
    for i in 0..url.len() {
        let c = url[i];
        let allowed = c.is_ascii_alphanumeric()
            || c == b'.'
            || c == b'-'
            || c == b'+'
            || c == b'?'
            || c == b'_'
            || c == b'/';
        if !allowed || (c == b'.' && i + 1 < url.len() && url[i + 1] == b'.') {
            return false;
        }
    }
    true
}

/// Classifies static GET route in `WebProcessor::Dispatch`:
/// - `0`: invalid URL characters (404 Not Found with warning)
/// - `1`: `/extensions/` route with invalid subpath (404 Not Found without warning)
/// - `2`: `/extensions/` route with valid subpath (`ext_offset = 12`)
/// - `3`: `/combined.` with `?` (multi-file response)
/// - `4`: single-file response
pub fn web_classify_get_dispatch(url: &[u8]) -> i32 {
    if !web_is_safe_url_chars(url) {
        return 0;
    }
    if url.starts_with(b"/extensions/") {
        let ext_url = &url[12..];
        match ext_url.iter().position(|&b| b == b'/') {
            None | Some(0) => 1,
            Some(_) => 2,
        }
    } else if url.starts_with(b"/combined.") && url.contains(&b'?') {
        3
    } else {
        4
    }
}

/// Checks whether a missing static file 404 is ignorable in `WebProcessor::SendSingleFileResponse`.
pub fn web_is_ignorable_404(base_name: &[u8]) -> bool {
    base_name == b"package-info.json"
        || base_name == b"favicon.ico"
        || base_name.starts_with(b"apple-touch-icon")
}

/// Maps filename extension to MIME content-type in `WebProcessor::DetectContentType`.
/// Returns `None` if unknown or no extension.
pub fn web_detect_content_type(filename: &[u8]) -> Option<&'static [u8]> {
    let dot = filename.iter().rposition(|&b| b == b'.')?;
    let ext = &filename[dot..];
    if ext.eq_ignore_ascii_case(b".css") {
        Some(b"text/css\0")
    } else if ext.eq_ignore_ascii_case(b".html") {
        Some(b"text/html\0")
    } else if ext.eq_ignore_ascii_case(b".js") {
        Some(b"application/javascript\0")
    } else if ext.eq_ignore_ascii_case(b".png") {
        Some(b"image/png\0")
    } else if ext.eq_ignore_ascii_case(b".jpeg") {
        Some(b"image/jpeg\0")
    } else if ext.eq_ignore_ascii_case(b".gif") {
        Some(b"image/gif\0")
    } else if ext.eq_ignore_ascii_case(b".svg") {
        Some(b"image/svg+xml\0")
    } else if ext.eq_ignore_ascii_case(b".json") {
        Some(b"application/json\0")
    } else {
        None
    }
}

// ============================================================================
// 3. UrlCoordinator (`daemon/queue/UrlCoordinator.cpp`)
// ============================================================================

/// Selects the next URL index to download in `UrlCoordinator::GetNextUrl`.
/// Returns `-1` if no eligible URL entry exists.
pub fn select_next_url(
    kinds: &[i32],
    url_statuses: &[i32],
    delete_statuses: &[i32],
    priorities: &[i32],
    nk_url: i32,
    ls_none: i32,
    ds_none: i32,
) -> i32 {
    let count = kinds
        .len()
        .min(url_statuses.len())
        .min(delete_statuses.len())
        .min(priorities.len());
    let mut best_idx: i32 = -1;
    let mut best_prio: i32 = i32::MIN;
    for i in 0..count {
        if kinds[i] == nk_url
            && url_statuses[i] == ls_none
            && delete_statuses[i] == ds_none
            && (best_idx < 0 || priorities[i] > best_prio)
        {
            best_idx = i as i32;
            best_prio = priorities[i];
        }
    }
    best_idx
}

/// Checks if a URL download is hanging in `UrlCoordinator::ResetHangingDownloads`.
pub fn is_hanging_url_download(
    now: i64,
    last_update_time: i64,
    url_timeout: i32,
    status: i32,
    ad_running: i32,
) -> bool {
    url_timeout > 0
        && now - last_update_time > (url_timeout as i64) + 10
        && status == ad_running
}

// ============================================================================
// 4. DirectRenamer (`daemon/queue/DirectRenamer.cpp`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsDirectRenameParDelta {
    pub apply_hash16k: i32,
    pub par_flag_changed: i32,
    pub par_size: i64,
    pub par_current_success_size: i64,
    pub par_current_failed_size: i64,
    pub par_failed_size: i64,
    pub remaining_par_count: i32,
    pub unpause_non_par: i32,
}

/// Evaluates `DirectRenamer::ArticleDownloaded` hash16k eligibility and par-flag transition deltas.
#[allow(clippy::too_many_arguments)]
pub fn eval_direct_rename_article(
    article_size: i32,
    article_count: usize,
    old_par_file: bool,
    new_par_file: bool,
    file_paused: bool,
    file_size: i64,
    file_success_size: i64,
    file_failed_size: i64,
    file_missed_size: i64,
    par_size: i64,
    par_current_success_size: i64,
    par_current_failed_size: i64,
    par_failed_size: i64,
    remaining_par_count: i32,
) -> RsDirectRenameParDelta {
    let apply_hash16k = if article_size >= 16 * 1024 || article_count == 1 {
        1
    } else {
        0
    };
    if old_par_file == new_par_file {
        return RsDirectRenameParDelta {
            apply_hash16k,
            par_flag_changed: 0,
            par_size,
            par_current_success_size,
            par_current_failed_size,
            par_failed_size,
            remaining_par_count,
            unpause_non_par: 0,
        };
    }

    let delta: i64 = if new_par_file { 1 } else { -1 };
    let count_delta: i32 = if new_par_file { 1 } else { -1 };
    let unpause_non_par = if !new_par_file && file_paused { 1 } else { 0 };

    RsDirectRenameParDelta {
        apply_hash16k,
        par_flag_changed: 1,
        par_size: par_size + file_size * delta,
        par_current_success_size: par_current_success_size + file_success_size * delta,
        par_current_failed_size: par_current_failed_size
            + file_failed_size * delta
            + file_missed_size * delta,
        par_failed_size: par_failed_size + file_missed_size * delta,
        remaining_par_count: remaining_par_count + count_delta,
        unpause_non_par,
    }
}

/// Selects the smallest uncompleted par-file ID per par-set in `DirectRenamer::UnpausePars`.
/// Returns the number of selected file IDs written to `out_file_ids` (in exact `DirectRenamer::UnpausePars` order).
pub fn select_pars_to_unpause(
    ids: &[i32],
    set_ids: &[&[u8]],
    completed: &[u8],
    in_file_list: &[u8],
    sizes: &[i64],
    out_file_ids: &mut [i32],
) -> usize {
    let count = ids
        .len()
        .min(set_ids.len())
        .min(completed.len())
        .min(in_file_list.len())
        .min(sizes.len());
    if count == 0 || out_file_ids.is_empty() {
        return 0;
    }

    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| {
        let f1 = in_file_list[a] != 0;
        let f2 = in_file_list[b] != 0;
        if (!f1 && f2) || (f1 && f2 && sizes[a] < sizes[b]) {
            std::cmp::Ordering::Less
        } else if (f1 && !f2) || (f1 && f2 && sizes[b] < sizes[a]) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });

    let mut parsets: Vec<&[u8]> = Vec::new();
    for &idx in &order {
        if completed[idx] != 0 {
            parsets.push(set_ids[idx]);
        }
    }

    let mut written = 0;
    for &idx in &order {
        if !parsets.iter().any(|&s| s == set_ids[idx]) {
            parsets.push(set_ids[idx]);
            if in_file_list[idx] != 0 && written < out_file_ids.len() {
                out_file_ids[written] = ids[idx];
                written += 1;
            }
        }
    }
    written
}

// ============================================================================
// 5. NzbFile (`daemon/queue/NzbFile.cpp`)
// ============================================================================

/// Extracts `{{password}}` from `filename` in `NzbFile::ReadPasswordFromFilename`.
pub fn extract_password_from_filename(filename: &[u8]) -> Option<&[u8]> {
    let start_pos = filename.windows(2).position(|w| w == b"{{")?;
    let content_start = start_pos + 2;
    let rel_end = filename[content_start..]
        .windows(2)
        .position(|w| w == b"}}")?;
    if rel_end > 0 {
        Some(&filename[content_start..content_start + rel_end])
    } else {
        None
    }
}

/// Formats and sanitizes a segment Message-ID `<tag_content>` in `NzbFile::Parse_EndElement`,
/// replacing bytes `<= b' '` or `== 0x7f` with `b'_'` and truncating the formatted `<%s>`
/// string to 1023 bytes (`BString<1024>`) exactly like `vsnprintf(buf, 1024, "<%s>", ...)`.
pub fn sanitize_segment_message_id(tag_content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(tag_content.len().min(1022) + 2);
    out.push(b'<');
    for &b in tag_content {
        if out.len() >= 1023 {
            break;
        }
        if b <= b' ' || b == 0x7f {
            out.push(b'_');
        } else {
            out.push(b);
        }
    }
    if out.len() < 1023 {
        out.push(b'>');
    }
    out
}

/// Computes `NzbFile::CalcHashes` (`full_content_hash`, `filtered_content_hash`)
/// from files already ordered in `NzbFile::CalcHashes` order.
pub fn calc_nzb_hashes(
    skip_flags: &[u8],
    article_counts: &[usize],
    message_ids: &[&[u8]],
) -> (u32, u32) {
    let file_count = skip_flags.len().min(article_counts.len());
    if file_count == 0 {
        return (0, 0);
    }

    let mut full_hash: u32 = 0;
    let mut filtered_hash: u32 = 0;
    let mut use_for_filtered_count: i32 = 0;
    let mut cur_offset = 0usize;

    for f_idx in 0..file_count {
        let skip = skip_flags[f_idx] != 0;
        let cnt = article_counts[f_idx];
        let end = cur_offset.saturating_add(cnt).min(message_ids.len());
        for &msg_id in &message_ids[cur_offset..end] {
            full_hash = crate::util::hash_bj96(msg_id, full_hash);
            if !skip {
                filtered_hash = crate::util::hash_bj96(msg_id, filtered_hash);
                use_for_filtered_count += 1;
            }
        }
        cur_offset = end;
    }

    if use_for_filtered_count < (file_count as i32) / 2 {
        filtered_hash = 0;
    }

    (full_hash, filtered_hash)
}

// ============================================================================
// 6. QueueScript (`daemon/extension/QueueScript.cpp`)
// ============================================================================

pub const QUEUE_EVENT_NAMES: [&[u8]; 7] = [
    b"FILE_DOWNLOADED",
    b"URL_COMPLETED",
    b"NZB_MARKED",
    b"NZB_ADDED",
    b"NZB_NAMED",
    b"NZB_DOWNLOADED",
    b"NZB_DELETED",
];

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsQueueScriptCmd {
    /// - `0`: not an `[NZB] ` command (log normally)
    /// - `1`: `[NZB] NZBPR_<key>=<val>`
    /// - `2`: `[NZB] DIRECTORY=<dir>` (only when `event == qe_nzb_downloaded`)
    /// - `3`: `[NZB] MARK=BAD`
    /// - `-1`: invalid `[NZB] ` command
    pub kind: i32,
    pub key_offset: usize,
    pub key_len: usize,
    pub val_offset: usize,
    pub val_len: usize,
}

/// Parses a `[NZB] ...` line in `QueueScriptController::AddMessage`.
pub fn parse_queue_script_cmd(
    msg_text: &[u8],
    event: i32,
    qe_nzb_downloaded: i32,
) -> RsQueueScriptCmd {
    if !msg_text.starts_with(b"[NZB] ") {
        return RsQueueScriptCmd::default();
    }
    let body = &msg_text[6..];
    if body.starts_with(b"NZBPR_") {
        let kv = &body[6..];
        if let Some(eq) = kv.iter().position(|&b| b == b'=') {
            RsQueueScriptCmd {
                kind: 1,
                key_offset: 12,
                key_len: eq,
                val_offset: 12 + eq + 1,
                val_len: kv.len() - eq - 1,
            }
        } else {
            RsQueueScriptCmd {
                kind: -1,
                ..Default::default()
            }
        }
    } else if body.starts_with(b"DIRECTORY=") && event == qe_nzb_downloaded {
        RsQueueScriptCmd {
            kind: 2,
            key_offset: 6,
            key_len: 9,
            val_offset: 16,
            val_len: body.len() - 10,
        }
    } else if body.starts_with(b"MARK=BAD") {
        RsQueueScriptCmd {
            kind: 3,
            ..Default::default()
        }
    } else {
        RsQueueScriptCmd {
            kind: -1,
            ..Default::default()
        }
    }
}

/// Evaluates the `EventInterval` suppression check in `QueueScriptCoordinator::EnqueueScript`.
pub fn should_skip_queue_event_interval(
    event: i32,
    qe_file_downloaded: i32,
    event_interval: i32,
    cur_time: i64,
    queue_script_time: i64,
) -> bool {
    event == qe_file_downloaded
        && (event_interval == -1
            || (event_interval > 0
                && cur_time - queue_script_time > 0
                && ((cur_time - queue_script_time) as i32) < event_interval))
}

/// Checks if a parameter value is truthy (`"yes"`, `"on"`, `"1"`, case-insensitive).
fn is_param_enabled_value(val: &[u8]) -> bool {
    val.eq_ignore_ascii_case(b"yes") || val.eq_ignore_ascii_case(b"on") || val == b"1"
}

/// Evaluates `QueueScriptCoordinator::UsableScript`.
pub fn is_usable_queue_script(
    is_queue_script: bool,
    queue_events: &[u8],
    script_name: &[u8],
    event: i32,
    qe_url_completed: i32,
    param_names: &[&[u8]],
    param_values: &[&[u8]],
    url_extensions: &[u8],
) -> bool {
    if !is_queue_script {
        return false;
    }
    let event_name = if event >= 0 && (event as usize) < QUEUE_EVENT_NAMES.len() {
        QUEUE_EVENT_NAMES[event as usize]
    } else {
        b""
    };
    if !queue_events.is_empty()
        && !event_name.is_empty()
        && !queue_events
            .windows(event_name.len())
            .any(|w| w == event_name)
    {
        return false;
    }

    let param_count = param_names.len().min(param_values.len());
    for i in 0..param_count {
        let varname = param_names[i];
        if !varname.is_empty()
            && varname[0] != b'*'
            && varname[varname.len() - 1] == b':'
            && is_param_enabled_value(param_values[i])
        {
            let stripped = &varname[..varname.len() - 1];
            let truncated = if stripped.len() > 1023 {
                &stripped[..1023]
            } else {
                stripped
            };
            if truncated == script_name {
                return true;
            }
        }
    }

    if event == qe_url_completed && !url_extensions.is_empty() {
        for tok in url_extensions.split(|&b| b == b',' || b == b';') {
            let trimmed = trim_ascii_spaces(tok);
            if !trimmed.is_empty() && trimmed == script_name {
                return true;
            }
        }
    }

    false
}

fn trim_ascii_spaces(mut s: &[u8]) -> &[u8] {
    while let Some((&last, rest)) = s.split_last() {
        if last == b'\n' || last == b'\r' || last == b' ' || last == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    while let Some((&first, rest)) = s.split_first() {
        if first == b'\n' || first == b'\r' || first == b' ' || first == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

/// Evaluates whether a queued script item should be ignored/erased in `QueueScriptCoordinator::CheckQueue`.
pub fn should_ignore_queued_script_event(
    has_nzb_info: bool,
    delete_status: i32,
    ds_none: i32,
    mark_status: i32,
    ks_bad: i32,
    event: i32,
    qe_nzb_deleted: i32,
    qe_nzb_marked: i32,
) -> bool {
    !has_nzb_info
        || (delete_status != ds_none && event != qe_nzb_deleted && event != qe_nzb_marked)
        || (mark_status == ks_bad && event != qe_nzb_marked)
}

// ============================================================================
// 7. PostScript (`daemon/extension/PostScript.cpp`)
// ============================================================================

/// Builds the comma-separated list of enabled post-script names in `PostScriptController::Run`.
pub fn build_enabled_script_comma_list(
    param_names: &[&[u8]],
    param_values: &[&[u8]],
) -> Vec<u8> {
    let count = param_names.len().min(param_values.len());
    let mut out = Vec::new();
    for i in 0..count {
        let varname = param_names[i];
        if !varname.is_empty()
            && varname[0] != b'*'
            && varname[varname.len() - 1] == b':'
            && is_param_enabled_value(param_values[i])
        {
            out.extend_from_slice(&varname[..varname.len() - 1]);
            out.push(b',');
        }
    }
    out
}

/// Evaluates `PostScriptController::AnalyseExitCode`.
/// Returns `(script_status, message_action, request_par_check)` where `message_action`:
/// - `1`: info `"%s successful"`
/// - `2`: error `"%s failed"`
/// - `3`: info `"%s skipped"`
/// - `4`: error `"%s requested par-check/repair, but the collection was already checked"`
/// - `5`: info `"%s requested par-check/repair"`
/// - `6`: error `"%s failed (terminated with unknown status)"`
pub fn analyse_post_script_exit_code(
    exit_code: i32,
    par_status: i32,
    ps_skipped: i32,
    parcheck_enabled: bool,
    sr_none: i32,
    sr_failure: i32,
    sr_success: i32,
) -> (i32, i32, bool) {
    match exit_code {
        93 => (sr_success, 1, false),
        94 | -1 => (sr_failure, 2, false),
        95 => (sr_none, 3, false),
        92 if parcheck_enabled => {
            if par_status > ps_skipped {
                (sr_failure, 4, false)
            } else {
                (sr_success, 5, true)
            }
        }
        _ => (sr_failure, 6, false),
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsPostScriptCmd {
    /// - `0`: not an `[NZB] ` command
    /// - `1`: `[NZB] FINALDIR=<dir>`
    /// - `2`: `[NZB] DIRECTORY=<dir>`
    /// - `3`: `[NZB] NZBPR_<key>=<val>`
    /// - `4`: `[NZB] MARK=BAD`
    /// - `-1`: invalid `[NZB] ` command
    pub kind: i32,
    pub key_offset: usize,
    pub key_len: usize,
    pub val_offset: usize,
    pub val_len: usize,
}

/// Parses a `[NZB] ...` line in `PostScriptController::AddMessage`.
pub fn parse_post_script_cmd(msg_text: &[u8]) -> RsPostScriptCmd {
    if !msg_text.starts_with(b"[NZB] ") {
        return RsPostScriptCmd::default();
    }
    let body = &msg_text[6..];
    if body.starts_with(b"FINALDIR=") {
        RsPostScriptCmd {
            kind: 1,
            key_offset: 6,
            key_len: 8,
            val_offset: 15,
            val_len: body.len() - 9,
        }
    } else if body.starts_with(b"DIRECTORY=") {
        RsPostScriptCmd {
            kind: 2,
            key_offset: 6,
            key_len: 9,
            val_offset: 16,
            val_len: body.len() - 10,
        }
    } else if body.starts_with(b"NZBPR_") {
        let kv = &body[6..];
        if let Some(eq) = kv.iter().position(|&b| b == b'=') {
            RsPostScriptCmd {
                kind: 3,
                key_offset: 12,
                key_len: eq,
                val_offset: 12 + eq + 1,
                val_len: kv.len() - eq - 1,
            }
        } else {
            RsPostScriptCmd {
                kind: -1,
                ..Default::default()
            }
        }
    } else if body.starts_with(b"MARK=BAD") {
        RsPostScriptCmd {
            kind: 4,
            ..Default::default()
        }
    } else {
        RsPostScriptCmd {
            kind: -1,
            ..Default::default()
        }
    }
}

// ============================================================================
// 8. ScanScript (`daemon/extension/ScanScript.cpp`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsScanScriptCmd {
    /// - `0`: not an `[NZB] ` command
    /// - `1`: `[NZB] NZBNAME=<val>`
    /// - `2`: `[NZB] CATEGORY=<val>`
    /// - `3`: `[NZB] NZBPR_<key>=<val>`
    /// - `4`: `[NZB] PRIORITY=<int>`
    /// - `5`: `[NZB] TOP=<bool>`
    /// - `6`: `[NZB] PAUSED=<bool>`
    /// - `7`: `[NZB] DUPEKEY=<val>`
    /// - `8`: `[NZB] DUPESCORE=<int>`
    /// - `9`: `[NZB] DUPEMODE=<mode>` (`int_val` = 0 score, 1 all, 2 force)
    /// - `-1`: invalid `[NZB] ` command
    /// - `-2`: invalid value for `DUPEMODE` (`val_offset`/`val_len` point to value)
    pub kind: i32,
    pub key_offset: usize,
    pub key_len: usize,
    pub val_offset: usize,
    pub val_len: usize,
    pub int_val: i32,
}

fn c_atoi(bytes: &[u8]) -> i32 {
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    let mut sign: i64 = 1;
    if idx < bytes.len() {
        if bytes[idx] == b'-' {
            sign = -1;
            idx += 1;
        } else if bytes[idx] == b'+' {
            idx += 1;
        }
    }
    let mut acc: i64 = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        acc = acc
            .wrapping_mul(10)
            .wrapping_add((bytes[idx] - b'0') as i64);
        idx += 1;
    }
    (acc * sign) as i32
}

/// Parses a `[NZB] ...` line in `ScanScriptController::AddMessage`.
pub fn parse_scan_script_cmd(msg_text: &[u8]) -> RsScanScriptCmd {
    if !msg_text.starts_with(b"[NZB] ") {
        return RsScanScriptCmd::default();
    }
    let body = &msg_text[6..];
    if body.starts_with(b"NZBNAME=") {
        RsScanScriptCmd {
            kind: 1,
            val_offset: 14,
            val_len: body.len() - 8,
            ..Default::default()
        }
    } else if body.starts_with(b"CATEGORY=") {
        RsScanScriptCmd {
            kind: 2,
            val_offset: 15,
            val_len: body.len() - 9,
            ..Default::default()
        }
    } else if body.starts_with(b"NZBPR_") {
        let kv = &body[6..];
        if let Some(eq) = kv.iter().position(|&b| b == b'=') {
            RsScanScriptCmd {
                kind: 3,
                key_offset: 12,
                key_len: eq,
                val_offset: 12 + eq + 1,
                val_len: kv.len() - eq - 1,
                int_val: 0,
            }
        } else {
            RsScanScriptCmd {
                kind: -1,
                ..Default::default()
            }
        }
    } else if body.starts_with(b"PRIORITY=") {
        RsScanScriptCmd {
            kind: 4,
            val_offset: 15,
            val_len: body.len() - 9,
            int_val: c_atoi(&body[9..]),
            ..Default::default()
        }
    } else if body.starts_with(b"TOP=") {
        RsScanScriptCmd {
            kind: 5,
            val_offset: 10,
            val_len: body.len() - 4,
            int_val: if c_atoi(&body[4..]) != 0 { 1 } else { 0 },
            ..Default::default()
        }
    } else if body.starts_with(b"PAUSED=") {
        RsScanScriptCmd {
            kind: 6,
            val_offset: 13,
            val_len: body.len() - 7,
            int_val: if c_atoi(&body[7..]) != 0 { 1 } else { 0 },
            ..Default::default()
        }
    } else if body.starts_with(b"DUPEKEY=") {
        RsScanScriptCmd {
            kind: 7,
            val_offset: 14,
            val_len: body.len() - 8,
            ..Default::default()
        }
    } else if body.starts_with(b"DUPESCORE=") {
        RsScanScriptCmd {
            kind: 8,
            val_offset: 16,
            val_len: body.len() - 10,
            int_val: c_atoi(&body[10..]),
            ..Default::default()
        }
    } else if body.starts_with(b"DUPEMODE=") {
        let val = &body[9..];
        let mode = if val.eq_ignore_ascii_case(b"score") {
            0
        } else if val.eq_ignore_ascii_case(b"all") {
            1
        } else if val.eq_ignore_ascii_case(b"force") {
            2
        } else {
            return RsScanScriptCmd {
                kind: -2,
                val_offset: 15,
                val_len: val.len(),
                ..Default::default()
            };
        };
        RsScanScriptCmd {
            kind: 9,
            val_offset: 15,
            val_len: val.len(),
            int_val: mode,
            ..Default::default()
        }
    } else {
        RsScanScriptCmd {
            kind: -1,
            ..Default::default()
        }
    }
}

// ============================================================================
// 9. ScriptConfig (`daemon/extension/ScriptConfig.cpp`)
// ============================================================================

/// Builds the canonical versioned `v1\n` cipher baseline token in `ScriptConfig::CipherBaseline`.
pub fn build_cipher_baseline(names: &[&[u8]], values: &[&[u8]]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let count = names.len().min(values.len());
    let mut fields: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

    for i in 0..count {
        let raw_name = names[i];
        let mut name: Vec<u8> = raw_name
            .iter()
            .map(|&b| {
                if b.is_ascii_uppercase() {
                    b + (b'a' - b'A')
                } else {
                    b
                }
            })
            .collect();
        if !name.starts_with(b"server") {
            continue;
        }
        let dot = match name[6..].iter().position(|&b| b == b'.') {
            Some(pos) => 6 + pos,
            None => continue,
        };
        if dot == 6
            || name[6] == b'0'
            || !name[6..dot].iter().all(|&c| c.is_ascii_digit())
        {
            continue;
        }
        let suffix = &name[dot + 1..];
        if suffix != b"host" && suffix != b"port" && suffix != b"cipher" {
            continue;
        }
        let raw_val = values[i];
        let mut hex_val = Vec::with_capacity(raw_val.len() * 2);
        for &c in raw_val {
            hex_val.push(HEX[(c >> 4) as usize]);
            hex_val.push(HEX[(c & 15) as usize]);
        }
        fields.insert(std::mem::take(&mut name), hex_val);
    }

    let mut token = Vec::from(&b"v1\n"[..]);
    for (name, val) in fields {
        token.extend_from_slice(&name);
        token.push(b'=');
        token.extend_from_slice(&val);
        token.push(b'\n');
    }
    token
}

// ============================================================================
// 10. ScriptController (`daemon/util/ScriptController.cpp`)
// ============================================================================

/// Checks whether an environment variable string `var` matches the `NZBXX_...` pattern
/// filtered out in `EnvironmentStrings::InitFromCurrentProcess`.
pub fn is_nzbget_internal_env_var(var: &[u8]) -> bool {
    var.len() > 5 && var.starts_with(b"NZB") && var[5] == b'_'
}

/// Normalizes an environment variable name in `ScriptController::SetEnvVarSpecial`:
/// replaces `.:*!"$%&/()=`+~#'{}[]@- ` with `_` and converts ASCII letters to uppercase.
pub fn normalize_special_env_var_name(varname: &[u8]) -> Vec<u8> {
    const SPECIAL: &[u8] = b".:*!\"$%&/()=`+~#'{}[]@- ";
    varname
        .iter()
        .map(|&b| {
            let replaced = if SPECIAL.contains(&b) { b'_' } else { b };
            replaced.to_ascii_uppercase()
        })
        .collect()
}

/// Strips trailing `\n`, `\r`, and `' '` in-place and classifies the log prefix
/// in `ScriptController::ProcessOutput`.
/// Returns `(is_empty, message_kind, text_offset)` where `message_kind`:
/// - `0`: `mkInfo`
/// - `1`: `mkWarning`
/// - `2`: `mkError`
/// - `3`: `mkDetail`
/// - `4`: `mkDebug`
pub fn classify_script_output_line(line: &[u8]) -> ( usize, i32, usize) {
    let mut trimmed_len = line.len();
    while trimmed_len > 0 {
        let last = line[trimmed_len - 1];
        if last == b'\n' || last == b'\r' || last == b' ' {
            trimmed_len -= 1;
        } else {
            break;
        }
    }
    if trimmed_len == 0 {
        return (0, 0, 0);
    }
    let trimmed = &line[..trimmed_len];
    if trimmed.starts_with(b"[INFO] ") {
        (trimmed_len, 0, 7)
    } else if trimmed.starts_with(b"[WARNING] ") {
        (trimmed_len, 1, 10)
    } else if trimmed.starts_with(b"[ERROR] ") {
        (trimmed_len, 2, 8)
    } else if trimmed.starts_with(b"[DETAIL] ") {
        (trimmed_len, 3, 9)
    } else if trimmed.starts_with(b"[DEBUG] ") {
        (trimmed_len, 4, 8)
    } else {
        (trimmed_len, 0, 0)
    }
}

/// Evaluates `ScriptController::CheckEnvSize`.
/// Returns `Some(exceeded_total)` if cumulative environment size exceeds `limit`, or `None`.
pub fn check_env_size_limit(env_lens: &[usize], ptr_size: usize, limit: usize) -> Option<usize> {
    if limit == 0 {
        return None;
    }
    let mut total = 0usize;
    for &len in env_lens {
        total = total.saturating_add(len).saturating_add(1).saturating_add(ptr_size);
        if total > limit {
            return Some(total);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statmeter_functions() {
        let (adj_clk, delta, adj_dl, adj_tz) = eval_interval_check(200, 100, 100, 50, false);
        assert!(adj_clk);
        assert_eq!(delta, 101);
        assert!(adj_dl);
        assert!(adj_tz);

        let (up, dn) = calc_total_stat(1000, 900, 950, 980, true);
        assert_eq!(up, 100);
        assert_eq!(dn, 30);

        assert_eq!(calc_current_download_speed(false, 110, 100, 5000), 500);
        assert_eq!(calc_current_or_momentary(), 1024);

        let mut cur_sec_time = 0i64;
        let mut cur_sec_bytes = 0i64;
        let mut speed_bytes = [0i32; SPEEDMETER_SLOTS];
        let mut speed_time = [100i32; SPEEDMETER_SLOTS];
        let mut idx = 0i32;
        let mut total = 0i64;
        let mut start = 100i32;
        let mut corr = 100i64;
        let mut all = 0i64;
        add_speed_reading(
            4096,
            105,
            &mut cur_sec_time,
            &mut cur_sec_bytes,
            &mut speed_bytes,
            &mut speed_time,
            &mut idx,
            &mut total,
            &mut start,
            &mut corr,
            &mut all,
        );
        assert_eq!(cur_sec_time, 105);
        assert_eq!(cur_sec_bytes, 4096);
        assert_eq!(total, 4096);
        assert_eq!(all, 4096);

        let days = [100i64, 200, 300, 400];
        let (month_b, day_b) = calc_quota_usage(86400 * 13, 10, &days, 3);
        assert_eq!(day_b, 400);
        assert_eq!(month_b, 400 + 300 + 200);

        let (reached, action) = eval_quota_status(1, 0, 2 * 1024 * 1024, 0, false);
        assert!(reached);
        assert_eq!(action, 1);
    }

    fn calc_current_or_momentary() -> i64 {
        calc_momentary_download_speed(50, 50, 1024)
    }

    #[test]
    fn test_web_functions() {
        assert!(web_is_safe_url_chars(b"/index.html?a+1_2-3"));
        assert!(!web_is_safe_url_chars(b"/../secret"));
        assert_eq!(web_classify_get_dispatch(b"/extensions/myext/script.js"), 2);
        assert_eq!(web_classify_get_dispatch(b"/extensions//bad"), 1);
        assert_eq!(web_classify_get_dispatch(b"/combined.js?a.js+b.js"), 3);
        assert!(web_is_ignorable_404(b"apple-touch-icon-120x120.png"));
        assert!(!web_is_ignorable_404(b"index.html"));
        assert_eq!(web_detect_content_type(b"app.CSS"), Some(&b"text/css\0"[..]));
    }

    #[test]
    fn test_url_and_direct_renamer() {
        let idx = select_next_url(&[0, 1, 1], &[0, 0, 0], &[0, 0, 0], &[100, 10, 50], 1, 0, 0);
        assert_eq!(idx, 2);
        assert!(is_hanging_url_download(100, 50, 30, 1, 1));

        let delta = eval_direct_rename_article(
            16384, 2, false, true, true, 1000, 200, 100, 50, 0, 0, 0, 0, 0,
        );
        assert_eq!(delta.apply_hash16k, 1);
        assert_eq!(delta.par_flag_changed, 1);
        assert_eq!(delta.par_size, 1000);
        assert_eq!(delta.remaining_par_count, 1);

        let mut out_ids = [0i32; 4];
        let n = select_pars_to_unpause(
            &[10, 11, 12],
            &[b"setA", b"setA", b"setB"],
            &[0, 0, 0],
            &[1, 1, 1],
            &[500, 100, 300],
            &mut out_ids,
        );
        assert_eq!(&out_ids[..n], &[11, 12]);
    }

    #[test]
    fn test_nzbfile_and_scripts() {
        assert_eq!(
            extract_password_from_filename(b"Movie.{{s3cr3t}}.nzb"),
            Some(&b"s3cr3t"[..])
        );
        assert_eq!(
            sanitize_segment_message_id(b"abc\r\n def\x7f@news"),
            b"<abc___def_@news>"
        );

        let long_id = sanitize_segment_message_id(&vec![b'a'; 1022]);
        assert_eq!(long_id.len(), 1023);
        assert_eq!(long_id[0], b'<');
        assert_eq!(long_id[1022], b'a');

        let (full, filt) = calc_nzb_hashes(
            &[0, 1],
            &[1, 1],
            &[b"<id1>", b"<id2>"],
        );
        assert_ne!(full, 0);
        assert_ne!(filt, 0);

        assert!(is_usable_queue_script(
            true,
            b"URL_COMPLETED",
            b"second.py",
            1,
            1,
            &[],
            &[],
            b"first.py,\tsecond.py\r\n",
        ));

        let qcmd = parse_queue_script_cmd(b"[NZB] NZBPR_myvar=hello", 0, 5);
        assert_eq!(qcmd.kind, 1);

        let pcmd = parse_post_script_cmd(b"[NZB] FINALDIR=/tmp/out");
        assert_eq!(pcmd.kind, 1);

        let scmd = parse_scan_script_cmd(b"[NZB] DUPEMODE=FORCE");
        assert_eq!(scmd.kind, 9);
        assert_eq!(scmd.int_val, 2);

        let baseline = build_cipher_baseline(
            &[b"Server1.Host", b"Server1.Cipher", b"Server0.Host"],
            &[b"news.example.com", b"TLS_AES_128_GCM_SHA256", b"ignored"],
        );
        assert!(baseline.starts_with(b"v1\nserver1.cipher="));

        assert!(is_nzbget_internal_env_var(b"NZBOP_TEMPDIR=/tmp"));
        assert!(!is_nzbget_internal_env_var(b"NZBGET_ROOT=/opt"));
        assert_eq!(
            normalize_special_env_var_name(b"NZBPO_my.opt-name"),
            b"NZBPO_MY_OPT_NAME"
        );
        assert_eq!(
            classify_script_output_line(b"[WARNING] Disk low \r\n"),
            (18, 1, 10)
        );
        assert_eq!(check_env_size_limit(&[10, 20], 8, 25), Some(48));
    }
}
