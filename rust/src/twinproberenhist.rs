//! TwinCheck, DupeProbe, DirectRenamer & HistoryCoordinator Hot-Path Kernels
//!
//! Pure, zero-allocation state predicates and arithmetic kernels extracted from:
//! - `daemon/queue/TwinCheck.cpp`
//! - `daemon/queue/DupeProbe.cpp`
//! - `daemon/queue/DirectRenamer.cpp`
//! - `daemon/queue/HistoryCoordinator.cpp`

/// Evaluates whether an NZB entry should replace the current minimum-size PAR2 index candidate.
///
/// Mirrors `TwinCheckJob::IndexData` in `TwinCheck.cpp`:
/// `entry.par2 && (!index || entry.size < index->size)`
#[inline]
pub fn twin_check_job_should_replace_min_index(
    is_par2: i32,
    has_index: i32,
    entry_size: i64,
    min_size: i64,
) -> i32 {
    if is_par2 != 0 && (has_index == 0 || entry_size < min_size) {
        1
    } else {
        0
    }
}

/// Evaluates whether a found PAR2 index entry is within the maximum allowed index size.
///
/// Mirrors `TwinCheckJob::IndexData` in `TwinCheck.cpp`:
/// `index && index->size <= maxIndexSize`
#[inline]
pub fn twin_check_job_is_index_size_within_max(
    has_index: i32,
    min_size: i64,
    max_index_size: i64,
) -> i32 {
    if has_index != 0 && min_size <= max_index_size {
        1
    } else {
        0
    }
}

/// Resolves the fallback fingerprint classification when `TwinCheckJob::Run` completes.
///
/// Returns:
/// - `0`: keep computed `fingerprint` (`known != 0`)
/// - `1`: `"lost"` (`known == 0 && fetched != 0`)
/// - `2`: `"none"` (`known == 0 && fetched == 0`)
#[inline]
pub fn twin_check_job_fallback_fingerprint(known: i32, fetched: i32) -> i32 {
    if known != 0 {
        0
    } else if fetched != 0 {
        1
    } else {
        2
    }
}

/// Updates the `compared` and `differ` counters for a sampled article pair in `TwinCheckJob::Sample`.
///
/// Mirrors `TwinCheckJob::Sample` in `TwinCheck.cpp`:
/// `if (partA.Success && partB.Success) { compared++; if (partA.Offset != partB.Offset || partA.Data != partB.Data) differ++; }`
#[inline]
pub fn twin_check_job_compare_sampled_pair(
    part_a_success: i32,
    part_b_success: i32,
    part_a_offset: i64,
    part_b_offset: i64,
    data_equal: i32,
    in_compared: i32,
    in_differ: i32,
) -> (i32, i32) {
    if part_a_success != 0 && part_b_success != 0 {
        let compared = in_compared.wrapping_add(1);
        let differ = if part_a_offset != part_b_offset || data_equal == 0 {
            in_differ.wrapping_add(1)
        } else {
            in_differ
        };
        (compared, differ)
    } else {
        (in_compared, in_differ)
    }
}

/// Evaluates whether the suffix after a tab in a `PutPar2` header line indicates a complete set (`"1"`).
///
/// Mirrors `TwinCheck::PutPar2` in `TwinCheck.cpp`:
/// `tab != std::string::npos && line.substr(tab + 1) == "1"`
#[inline]
pub fn twin_check_put_par2_is_complete_flag(after_tab: &[u8]) -> i32 {
    if after_tab == b"1" {
        1
    } else {
        0
    }
}

/// Evaluates whether `TwinCheck::ServiceWork` should prune stored file lists on this pass.
///
/// Mirrors `TwinCheck::ServiceWork` in `TwinCheck.cpp`:
/// `curTime - lastPrune >= 60 * 60 && g_jobCount == 0`
#[inline]
pub fn twin_check_should_prune_store(
    cur_time: i64,
    last_prune: i64,
    prune_interval_sec: i64,
    job_count: i32,
) -> i32 {
    if cur_time.wrapping_sub(last_prune) >= prune_interval_sec && job_count == 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether an NZB item carries duplicate-check twin hints (`DupeSameSize` or `DupeSameInner`).
///
/// Mirrors `TwinCheck::ServiceWork` in `TwinCheck.cpp`:
/// `bool hinted = nzbInfo->GetParameters()->Find("DupeSameSize") || nzbInfo->GetParameters()->Find("DupeSameInner");`
#[inline]
pub fn twin_check_is_job_hinted(has_dupe_same_size: i32, has_dupe_same_inner: i32) -> i32 {
    if has_dupe_same_size != 0 || has_dupe_same_inner != 0 {
        1
    } else {
        0
    }
}

/// Comparator for sorting `TwinCheck` jobs by rank ascending, then hinted descending.
///
/// Mirrors `TwinCheck::ServiceWork` in `TwinCheck.cpp`:
/// `a.rank != b.rank ? a.rank < b.rank : a.hinted > b.hinted`
#[inline]
pub fn twin_check_job_less(a_rank: i32, a_hinted: i32, b_rank: i32, b_hinted: i32) -> i32 {
    let a_h = a_hinted != 0;
    let b_h = b_hinted != 0;
    if a_rank != b_rank {
        if a_rank < b_rank {
            1
        } else {
            0
        }
    } else if a_h && !b_h {
        1
    } else {
        0
    }
}

/// Evaluates whether `TwinCheck::ServiceWork` should stop launching additional background jobs.
///
/// Mirrors `TwinCheck::ServiceWork` in `TwinCheck.cpp`:
/// `g_stopping || (int)g_jobs.size() >= MaxRunning`
#[inline]
pub fn twin_check_should_stop_launching_jobs(
    stopping: i32,
    active_jobs: i32,
    max_running: i32,
) -> i32 {
    if stopping != 0 || active_jobs >= max_running {
        1
    } else {
        0
    }
}

/// Computes the elapsed seconds a primary probe has been running for `nzbId`, or `-1` if this probe does not match.
///
/// Mirrors `DupeProbe::ProbingFor` in `DupeProbe.cpp`:
/// `for (const ProbeJob* job : g_probes) if (job->nzbId == nzbId && !job->countAll) return (int)(Util::CurrentTime() - job->started);`
#[inline]
pub fn dupe_probe_probing_for_elapsed_sec(
    probe_nzb_id: i32,
    query_nzb_id: i32,
    count_all: i32,
    cur_time: i64,
    started: i64,
) -> i32 {
    if probe_nzb_id == query_nzb_id && count_all == 0 {
        (cur_time.wrapping_sub(started) as i32).max(0)
    } else {
        -1
    }
}

/// Maps a global sample position against a file's `[offset, offset + count)` article range.
///
/// Mirrors `DupeProbe::SamplesOf` in `DupeProbe.cpp`:
/// - Returns `-1` when `position >= offset + count` (caller should `break` to next file).
/// - Returns `(position - offset) as isize` when `position >= offset`.
#[inline]
pub fn dupe_probe_samples_of_file_index(position: usize, offset: usize, count: usize) -> isize {
    if position >= offset.saturating_add(count) {
        -1
    } else if position >= offset {
        (position - offset) as isize
    } else {
        -2
    }
}

/// Evaluates whether a server is currently blocked or was blocked within `recent_block_sec`.
///
/// Mirrors `DupeProbe::ProbeServer` in `DupeProbe.cpp`:
/// `server->IsBlocked() || (blockTime > 0 && curTime - blockTime < RecentBlockSec)`
#[inline]
pub fn dupe_probe_is_server_recently_blocked(
    is_blocked: i32,
    block_time: i64,
    cur_time: i64,
    recent_block_sec: i64,
) -> i32 {
    if is_blocked != 0 || (block_time > 0 && cur_time.wrapping_sub(block_time) < recent_block_sec) {
        1
    } else {
        0
    }
}

/// Computes updated `(unknown, unreachable)` counters when a server connection fails during `DupeProbe::ProbeServer`.
///
/// Mirrors `DupeProbe::ProbeServer` in `DupeProbe.cpp`:
/// `result.unknown += remaining + 1; result.unreachable = result.exists + result.missing == 0;`
#[inline]
pub fn dupe_probe_apply_connect_failure(
    in_unknown: i32,
    remaining: i32,
    exists: i32,
    missing: i32,
) -> (i32, i32) {
    let out_unknown = in_unknown.wrapping_add(remaining).wrapping_add(1);
    let out_unreachable = if exists.wrapping_add(missing) == 0 {
        1
    } else {
        0
    };
    (out_unknown, out_unreachable)
}

/// Evaluates whether an NNTP `GROUP` join reply indicates success (starts with `'2'`).
///
/// Mirrors `DupeProbe::ProbeServer` in `DupeProbe.cpp`:
/// `joined && !strncmp(joined, "2", 1)`
#[inline]
pub fn dupe_probe_is_join_group_ok(joined: &[u8]) -> i32 {
    if joined.first() == Some(&b'2') {
        1
    } else {
        0
    }
}

/// Evaluates whether a server probe finished without unknown samples and checked at least one article.
///
/// Mirrors `DupeProbe::Measure` in `DupeProbe.cpp`:
/// `result.unknown == 0 && result.exists + result.missing > 0`
#[inline]
pub fn dupe_probe_is_server_measured_complete(unknown: i32, exists: i32, missing: i32) -> i32 {
    if unknown == 0 && exists.wrapping_add(missing) > 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether a dead-flagged history item recovered enough sampled articles (`>= 50%`) to retry.
///
/// Mirrors `DupeProbe::Recheck` in `DupeProbe.cpp`:
/// `result.existing * 2 >= result.sampled`
#[inline]
pub fn dupe_probe_should_retry_recheck(existing: i32, sampled: i32) -> i32 {
    if existing.saturating_mul(2) >= sampled {
        1
    } else {
        0
    }
}

/// Classifies the final action of `DupeProbe::Run` after measuring primary servers.
///
/// Returns:
/// - `1`: dead posting (`dead != 0`)
/// - `2`: alive posting (`existing >= min_alive_samples`)
/// - `3`: finished with no reachable primary servers (`finished != 0 && reached_servers == 0`)
/// - `4`: finished with inconclusive sample count (`finished != 0`)
/// - `0`: aborted / unfinished before completion
#[inline]
pub fn dupe_probe_classify_run_outcome(
    dead: i32,
    existing: i32,
    min_alive_samples: i32,
    finished: i32,
    reached_servers: i32,
) -> i32 {
    if dead != 0 {
        1
    } else if existing >= min_alive_samples {
        2
    } else if finished != 0 && reached_servers == 0 {
        3
    } else if finished != 0 {
        4
    } else {
        0
    }
}

/// Evaluates whether `DirectRenamer` should retry with ignore-extension matching when creating hardlinks.
///
/// Mirrors `DirectRenamer::RenameFilesInProgress` and `DirectRenamer::RenameCompletedFiles` in `DirectRenamer.cpp`:
/// `if (hardLinking && !matchIgnoreExt)`
#[inline]
pub fn direct_renamer_needs_ignore_ext_pass(hard_linking: i32, match_ignore_ext: i32) -> i32 {
    if hard_linking != 0 && match_ignore_ext == 0 {
        1
    } else {
        0
    }
}

/// Computes the reverse history index during reverse-order history deletion.
///
/// Mirrors `HistoryCoordinator::HistoryDelete` in `HistoryCoordinator.cpp`:
/// `int index = downloadQueue->GetHistory()->size() - 1 - (itHistory - downloadQueue->GetHistory()->begin());`
#[inline]
pub fn history_coord_reverse_delete_index(history_size: i32, forward_offset: i32) -> i32 {
    history_size
        .wrapping_sub(1)
        .wrapping_sub(forward_offset)
}

/// Evaluates whether `HistoryCoordinator::HistoryRedownload` should restore the paused state on files.
///
/// Mirrors `HistoryCoordinator::HistoryRedownload` in `HistoryCoordinator.cpp`:
/// `bool paused = restorePauseState && nzbInfo->GetDeletePaused();`
#[inline]
pub fn history_coord_should_restore_paused_on_redownload(
    restore_pause_state: i32,
    delete_paused: i32,
) -> i32 {
    if restore_pause_state != 0 && delete_paused != 0 {
        1
    } else {
        0
    }
}

/// Builds the `_twin.` filename in `RepairController::PostParChecker::FetchTwinFile`:
/// `"_twin." + (entry_filename.is_empty() ? format!("{twin_files_count}.par2") : entry_filename)`.
pub fn repair_ctrl_build_twin_filename(entry_filename: &[u8], twin_files_count: usize) -> Vec<u8> {
    let mut out = b"_twin.".to_vec();
    if entry_filename.is_empty() {
        out.extend_from_slice(format!("{twin_files_count}.par2").as_bytes());
    } else {
        out.extend_from_slice(entry_filename);
    }
    out
}

/// Formats a live store prune entry in `TwinCheck::ServiceWork`:
/// returns `format!("{nzb_id}\t") + identity + b"\n"` when `!identity.is_empty()`, else empty.
pub fn twin_check_format_live_identity_line(nzb_id: i32, identity: &[u8]) -> Vec<u8> {
    if identity.is_empty() {
        return Vec::new();
    }
    let mut out = format!("{nzb_id}\t").into_bytes();
    out.extend_from_slice(identity);
    out.push(b'\n');
    out
}

/// Selects and sorts active servers for `DupeProbe::Measure`:
/// filters to `active[i] != 0`, deduplicates positive `groups[i]` in iteration order,
/// sorts by `(norm_levels[i], ids[i])`, and returns the sorted server `ids`.
pub fn dupe_probe_select_server_order(
    active: &[u8],
    groups: &[i32],
    norm_levels: &[i32],
    ids: &[i32],
) -> Vec<i32> {
    let n = active
        .len()
        .min(groups.len())
        .min(norm_levels.len())
        .min(ids.len());
    let mut seen_groups: Vec<i32> = Vec::new();
    let mut order: Vec<(i32, i32)> = Vec::new();
    for i in 0..n {
        if active[i] == 0 {
            continue;
        }
        let g = groups[i];
        if g > 0 {
            if seen_groups.contains(&g) {
                continue;
            }
            seen_groups.push(g);
        }
        order.push((norm_levels[i], ids[i]));
    }
    order.sort_unstable();
    order.into_iter().map(|(_, id)| id).collect()
}

/// Executes one character step of `DirectUnpack::ReadLine`:
/// updates `buf` and `backspace`, and returns `(final_i_if_break, should_break)`.
pub fn direct_unpack_readline_step(
    buf: &mut [u8],
    i: usize,
    ch: i32,
    backspace: &mut bool,
) -> (usize, bool) {
    if i + 1 >= buf.len() {
        return (i, true);
    }
    if ch == i32::from(b'\x08') {
        *backspace = true;
        buf[i] = 0;
    }
    if !*backspace {
        buf[i] = ch as u8;
        buf[i + 1] = 0;
    }
    if ch == -1 {
        return (i, true);
    }
    if ch == i32::from(b'\n') {
        return (i + 1, true);
    }
    if i > 35 && ch == i32::from(b' ') {
        let nul_end = buf[..=i + 1]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(i + 1);
        if crate::dlunpackrepair::direct_unpack_is_insert_disk_prompt(&buf[..nul_end], i, ch) {
            return (i + 1, true);
        }
    }
    (i, false)
}

/// Compares two NZB entries by `(filename.is_empty() ? subject : filename)` (`TwinCheckJob::Sample`).
pub fn twin_check_compare_entry_names(
    a_filename: &[u8],
    a_subject: &[u8],
    b_filename: &[u8],
    b_subject: &[u8],
) -> bool {
    let ka = if a_filename.is_empty() {
        a_subject
    } else {
        a_filename
    };
    let kb = if b_filename.is_empty() {
        b_subject
    } else {
        b_filename
    };
    ka < kb
}

/// Splits the `"fingerprint\tcomplete"` output of `nzbget_rs_twin_put_par2` (`TwinCheck::PutPar2`).
pub fn twin_check_split_put_par2_result(result: &[u8]) -> (&[u8], bool) {
    match result.iter().position(|&b| b == b'\t') {
        Some(tab) => (
            &result[..tab],
            twin_check_put_par2_is_complete_flag(&result[tab + 1..]) != 0,
        ),
        None => (result, false),
    }
}

/// Returns whether `queued_filename` represents a merged download containing `|` (`TwinCheck::ServiceWork`).
pub fn twin_check_is_merged_queued_filename(queued_filename: &[u8]) -> bool {
    queued_filename.contains(&b'|')
}

/// Checks whether a queue observer event should nudge `TwinCheck` (`TwinCheck::DownloadQueueObserver::Update`).
pub fn twin_check_should_nudge_on_queue_event(
    action: i32,
    ea_nzb_added: i32,
    has_nzb_info: bool,
    dupe_key: &[u8],
) -> bool {
    action == ea_nzb_added && has_nzb_info && !dupe_key.is_empty()
}

/// Formats `"{server_name} ({message_id})"` for `DupeProbe::ProbeServer`.
pub fn dupe_probe_format_found_on(server_name: &[u8], message_id: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(server_name.len() + message_id.len() + 3);
    out.extend_from_slice(server_name);
    out.extend_from_slice(b" (");
    out.extend_from_slice(message_id);
    out.push(b')');
    out
}

/// Counts how many entries in `found` equal `1` (`DupeProbe::ProbeServer`, `DupeProbe::Measure`).
pub fn dupe_probe_count_found_samples(found: &[u8]) -> i32 {
    found.iter().filter(|&&v| v == 1).count() as i32
}

/// Formats `", {unreachable_servers} unreachable left out"` when `unreachable_servers > 0`, else empty (`DupeProbe::Run`).
pub fn dupe_probe_format_unreachable_suffix(unreachable_servers: i32) -> Vec<u8> {
    if unreachable_servers > 0 {
        format!(", {unreachable_servers} unreachable left out").into_bytes()
    } else {
        Vec::new()
    }
}

/// Splits a `"name=value"` parameter assignment at the first `'='`.
/// Returns `Some((name, value))` if `'='` is present, else `None`
/// (`HistoryCoordinator::HistorySetParameter`, `QueueEditor::SetNzbParameter`).
pub fn tpr_split_param_assignment(text: &[u8]) -> Option<(&[u8], &[u8])> {
    text.iter()
        .position(|&b| b == b'=')
        .map(|eq| (&text[..eq], &text[eq + 1..]))
}

/// Evaluates whether an article fetch server on `level` is eligible to try (`ArticleFetcher::Fetch`).
pub fn tpr_article_fetcher_is_server_eligible_on_level(
    server_active: bool,
    server_norm_level: i32,
    target_level: i32,
    server_id: i32,
    server_group: i32,
    failed_ids: &[i32],
    failed_groups: &[i32],
    failed_levels: &[i32],
) -> bool {
    if !server_active || server_norm_level != target_level {
        return false;
    }
    let count = failed_ids
        .len()
        .min(failed_groups.len())
        .min(failed_levels.len());
    for i in 0..count {
        if failed_ids[i] == server_id
            || (failed_groups[i] > 0
                && failed_groups[i] == server_group
                && failed_levels[i] == server_norm_level)
        {
            return false;
        }
    }
    true
}

/// Evaluates whether a transient failure on a reused connection should retry immediately
/// without consuming a transient attempt (`ArticleFetcher::Fetch`).
pub fn tpr_article_fetcher_is_reused_stale_within_max(
    reused: bool,
    stale_count_after_inc: i32,
    max_connections: i32,
) -> bool {
    reused && stale_count_after_inc <= max_connections
}

/// Evaluates whether `ArticleFetcher::FetchFromConnection` has a known valid declared yEnc range.
pub fn tpr_article_fetcher_is_declared_range_known(
    begin_pos: i64,
    end_pos: i64,
    size: i64,
) -> bool {
    begin_pos > 0 && end_pos >= begin_pos && size > 0
}

/// Evaluates whether `ArticleFetcher::FetchFromConnection` should throttle bandwidth.
pub fn tpr_article_fetcher_should_throttle_bandwidth(
    stopped: bool,
    quota_reached: bool,
    speed_limit: i64,
    cur_speed: i64,
    momentary_speed: i64,
) -> bool {
    !stopped
        && !quota_reached
        && speed_limit > 0
        && (cur_speed > speed_limit || momentary_speed > speed_limit)
}

/// Validates a BinRPC response header (`RemoteClient::ReceiveBoolResponse`, `RequestServerList`,
/// `RequestServerLog`, `RequestPostQueue`, `RequestHistory`).
pub fn tpr_binrpc_is_valid_response_header(
    read_ok: bool,
    host_signature: i32,
    expected_signature: i32,
    host_struct_size: u32,
    expected_struct_size: u32,
) -> bool {
    read_ok && host_signature == expected_signature && host_struct_size == expected_struct_size
}

/// Computes elapsed seconds from `start_time` to `cur_time` when `start_time != 0`
/// (`PostQueueBinCommand::Execute`).
pub fn tpr_binrpc_calc_elapsed_sec(cur_time: i64, start_time: i64) -> u32 {
    if start_time != 0 {
        (cur_time.wrapping_sub(start_time) as i32) as u32
    } else {
        0
    }
}

/// Evaluates whether a history item should be included in `HistoryBinCommand::Execute`.
pub fn tpr_binrpc_should_include_history_entry(
    kind: i32,
    hk_dup: i32,
    show_hidden: bool,
) -> bool {
    kind != hk_dup || show_hidden
}

/// Maps a file-level edit action to its corresponding group-level edit action
/// (`NCursesFrontend::EditQueue`).
pub fn tpr_ncurses_map_file_to_group_action(action: i32) -> i32 {
    match action {
        1 => 11, // eaFileMoveOffset -> eaGroupMoveOffset
        2 => 12, // eaFileMoveTop -> eaGroupMoveTop
        3 => 13, // eaFileMoveBottom -> eaGroupMoveBottom
        4 => 16, // eaFilePause -> eaGroupPause
        5 => 17, // eaFileResume -> eaGroupResume
        6 => 18, // eaFileDelete -> eaGroupDelete
        7 => 22, // eaFilePauseAllPars -> eaGroupPauseAllPars
        8 => 23, // eaFilePauseExtraPars -> eaGroupPauseExtraPars
        _ => 0,
    }
}

/// Resolves whether a file pause action should pause or resume the selected file
/// (`NCursesFrontend::EditQueue`).
pub fn tpr_ncurses_resolve_file_pause_action(
    action: i32,
    ea_file_pause: i32,
    ea_file_resume: i32,
    file_paused: bool,
) -> i32 {
    if action == ea_file_pause {
        if !file_paused {
            ea_file_pause
        } else {
            ea_file_resume
        }
    } else {
        action
    }
}

/// Computes the new `queue_scroll_offset` and target `entry` for `KEY_PPAGE` / `KEY_NPAGE`
/// in `NCursesFrontend::UpdateInput`.
pub fn tpr_ncurses_calc_page_nav(
    is_page_down: bool,
    selected: i32,
    queue_size: i32,
    scroll_offset: i32,
    win_height: i32,
) -> Option<(i32, i32)> {
    if !is_page_down {
        if selected > 0 {
            if selected == scroll_offset {
                Some((
                    scroll_offset.wrapping_sub(win_height),
                    selected.wrapping_sub(win_height),
                ))
            } else {
                Some((scroll_offset, scroll_offset))
            }
        } else {
            None
        }
    } else if selected < queue_size.wrapping_sub(1) {
        let bottom = scroll_offset.wrapping_add(win_height).wrapping_sub(1);
        if selected == bottom {
            Some((
                scroll_offset.wrapping_add(win_height),
                selected.wrapping_add(win_height),
            ))
        } else {
            Some((scroll_offset, bottom))
        }
    } else {
        None
    }
}

/// Resolves the fallback `param` source (`0=param, 1=process, 2=downloadRate`) and
/// `(definition, completed)` flags for `Options::InitScheduler`.
pub fn tpr_options_resolve_scheduler_task_state(
    has_time: bool,
    has_weekdays: bool,
    has_command: bool,
    has_download_rate: bool,
    has_param_ptr: bool,
    param_empty: bool,
    process_empty: bool,
    download_rate_empty: bool,
) -> (i32, bool, bool) {
    let param_source = if param_empty && !process_empty {
        1
    } else if param_empty && !download_rate_empty {
        2
    } else {
        0
    };
    let has_effective_param = (param_source != 0) || has_param_ptr;
    let definition =
        has_time || has_weekdays || has_command || has_download_rate || has_effective_param;
    let completed = has_time && has_command;
    (param_source, definition, completed)
}

/// Splits an `option=value` string at the first `=` where the option name is non-empty
/// (`Options::SplitOptionString`).
pub fn tpr_options_split_option_string(option: &[u8]) -> Option<(&[u8], &[u8])> {
    option
        .iter()
        .position(|&b| b == b'=')
        .filter(|&eq| eq > 0)
        .map(|eq| (&option[..eq], &option[eq + 1..]))
}

/// Resolves whether a single-argument script should use `found_executor` as `m_cmdLine`
/// and whether to prepend `found_executor` to `m_cmdArgs` (`ScriptController::PrepareArgs`).
pub fn tpr_script_resolve_single_arg_executor(
    has_found: bool,
    found_executor: &[u8],
    arg0: &[u8],
) -> (bool, bool) {
    (has_found, has_found && found_executor != arg0)
}

/// Computes the trimmed length of an XML/SAX error message after stripping trailing
/// `\n`, `\r`, and `' '` (`NzbFile::SAX_error`).
pub fn tpr_nzbfile_trim_sax_error_trailing(buf: &[u8]) -> usize {
    let mut len = buf.len();
    while len > 0 && matches!(buf[len - 1], b'\n' | b'\r' | b' ') {
        len -= 1;
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_twin_check_kernels() {
        assert_eq!(twin_check_job_should_replace_min_index(1, 0, 500, 0), 1);
        assert_eq!(twin_check_job_should_replace_min_index(1, 1, 300, 500), 1);
        assert_eq!(twin_check_job_should_replace_min_index(1, 1, 600, 500), 0);
        assert_eq!(twin_check_job_should_replace_min_index(0, 0, 100, 500), 0);

        assert_eq!(twin_check_job_is_index_size_within_max(1, 512, 1024), 1);
        assert_eq!(twin_check_job_is_index_size_within_max(1, 2048, 1024), 0);
        assert_eq!(twin_check_job_is_index_size_within_max(0, 512, 1024), 0);

        assert_eq!(twin_check_job_fallback_fingerprint(1, 1), 0);
        assert_eq!(twin_check_job_fallback_fingerprint(0, 1), 1);
        assert_eq!(twin_check_job_fallback_fingerprint(0, 0), 2);

        assert_eq!(
            twin_check_job_compare_sampled_pair(1, 1, 100, 100, 1, 3, 1),
            (4, 1)
        );
        assert_eq!(
            twin_check_job_compare_sampled_pair(1, 1, 100, 200, 1, 3, 1),
            (4, 2)
        );
        assert_eq!(
            twin_check_job_compare_sampled_pair(1, 0, 100, 100, 1, 3, 1),
            (3, 1)
        );

        assert_eq!(twin_check_put_par2_is_complete_flag(b"1"), 1);
        assert_eq!(twin_check_put_par2_is_complete_flag(b"0"), 0);

        assert_eq!(twin_check_should_prune_store(4000, 100, 3600, 0), 1);
        assert_eq!(twin_check_should_prune_store(4000, 100, 3600, 1), 0);
        assert_eq!(twin_check_should_prune_store(1000, 100, 3600, 0), 0);

        assert_eq!(twin_check_is_job_hinted(1, 0), 1);
        assert_eq!(twin_check_is_job_hinted(0, 1), 1);
        assert_eq!(twin_check_is_job_hinted(0, 0), 0);

        assert_eq!(twin_check_job_less(1, 0, 2, 1), 1);
        assert_eq!(twin_check_job_less(2, 1, 2, 0), 1);
        assert_eq!(twin_check_job_less(2, 0, 2, 1), 0);

        assert_eq!(twin_check_should_stop_launching_jobs(0, 2, 2), 1);
        assert_eq!(twin_check_should_stop_launching_jobs(1, 0, 2), 1);
        assert_eq!(twin_check_should_stop_launching_jobs(0, 1, 2), 0);
    }

    #[test]
    fn test_dupe_probe_renamer_and_history_kernels() {
        assert_eq!(dupe_probe_probing_for_elapsed_sec(42, 42, 0, 1050, 1000), 50);
        assert_eq!(dupe_probe_probing_for_elapsed_sec(42, 42, 0, 1000, 1000), 0);
        assert_eq!(dupe_probe_probing_for_elapsed_sec(42, 42, 1, 1050, 1000), -1);
        assert_eq!(dupe_probe_probing_for_elapsed_sec(42, 99, 0, 1050, 1000), -1);

        assert_eq!(dupe_probe_samples_of_file_index(15, 10, 5), -1);
        assert_eq!(dupe_probe_samples_of_file_index(12, 10, 5), 2);

        assert_eq!(dupe_probe_is_server_recently_blocked(1, 0, 1000, 30), 1);
        assert_eq!(dupe_probe_is_server_recently_blocked(0, 980, 1000, 30), 1);
        assert_eq!(dupe_probe_is_server_recently_blocked(0, 900, 1000, 30), 0);

        assert_eq!(dupe_probe_apply_connect_failure(1, 4, 0, 0), (6, 1));
        assert_eq!(dupe_probe_apply_connect_failure(1, 4, 2, 0), (6, 0));

        assert_eq!(dupe_probe_is_join_group_ok(b"211 123 1 123 alt.binaries.test"), 1);
        assert_eq!(dupe_probe_is_join_group_ok(b"411 no such group"), 0);
        assert_eq!(dupe_probe_is_join_group_ok(b""), 0);

        assert_eq!(dupe_probe_is_server_measured_complete(0, 3, 5), 1);
        assert_eq!(dupe_probe_is_server_measured_complete(1, 3, 5), 0);
        assert_eq!(dupe_probe_is_server_measured_complete(0, 0, 0), 0);

        assert_eq!(dupe_probe_should_retry_recheck(5, 10), 1);
        assert_eq!(dupe_probe_should_retry_recheck(4, 10), 0);

        assert_eq!(dupe_probe_classify_run_outcome(1, 0, 2, 1, 1), 1);
        assert_eq!(dupe_probe_classify_run_outcome(0, 2, 2, 0, 1), 2);
        assert_eq!(dupe_probe_classify_run_outcome(0, 0, 2, 1, 0), 3);
        assert_eq!(dupe_probe_classify_run_outcome(0, 1, 2, 1, 1), 4);
        assert_eq!(dupe_probe_classify_run_outcome(0, 1, 2, 0, 1), 0);

        assert_eq!(direct_renamer_needs_ignore_ext_pass(1, 0), 1);
        assert_eq!(direct_renamer_needs_ignore_ext_pass(1, 1), 0);
        assert_eq!(direct_renamer_needs_ignore_ext_pass(0, 0), 0);

        assert_eq!(history_coord_reverse_delete_index(10, 2), 7);
        assert_eq!(history_coord_should_restore_paused_on_redownload(1, 1), 1);
        assert_eq!(history_coord_should_restore_paused_on_redownload(1, 0), 0);

        assert_eq!(
            repair_ctrl_build_twin_filename(b"movie.vol00+01.par2", 3),
            b"_twin.movie.vol00+01.par2"
        );
        assert_eq!(repair_ctrl_build_twin_filename(b"", 3), b"_twin.3.par2");

        assert_eq!(
            twin_check_format_live_identity_line(42, b"fp\tpost"),
            b"42\tfp\tpost\n"
        );
        assert!(twin_check_format_live_identity_line(42, b"").is_empty());

        assert_eq!(
            dupe_probe_select_server_order(
                &[1, 0, 1, 1, 1],
                &[1, 0, 1, 0, 2],
                &[2, 0, 0, 1, 0],
                &[10, 20, 30, 40, 50],
            ),
            vec![50, 40, 10]
        );

        let mut buf = [0u8; 64];
        let mut bs = false;
        assert_eq!(
            direct_unpack_readline_step(&mut buf, 0, i32::from(b'A'), &mut bs),
            (0, false)
        );
        assert_eq!(buf[0], b'A');
        assert_eq!(
            direct_unpack_readline_step(&mut buf, 1, i32::from(b'\n'), &mut bs),
            (2, true)
        );

        assert!(twin_check_compare_entry_names(
            b"a.rar", b"subjB", b"b.rar", b"subjA"
        ));
        assert!(twin_check_compare_entry_names(
            b"", b"subjA", b"", b"subjB"
        ));
        assert_eq!(
            twin_check_split_put_par2_result(b"abc123\t1"),
            (&b"abc123"[..], true)
        );
        assert_eq!(
            twin_check_split_put_par2_result(b"abc123\t0"),
            (&b"abc123"[..], false)
        );
        assert!(twin_check_is_merged_queued_filename(b"a.nzb|b.nzb"));
        assert!(!twin_check_is_merged_queued_filename(b"a.nzb"));
        assert!(twin_check_should_nudge_on_queue_event(1, 1, true, b"key"));
        assert!(!twin_check_should_nudge_on_queue_event(1, 1, true, b""));

        assert_eq!(
            dupe_probe_format_found_on(b"News1", b"<msg@id>"),
            b"News1 (<msg@id>)"
        );
        assert_eq!(dupe_probe_count_found_samples(&[0, 1, 1, 0, 1]), 3);
        assert_eq!(
            dupe_probe_format_unreachable_suffix(2),
            b", 2 unreachable left out"
        );
        assert!(dupe_probe_format_unreachable_suffix(0).is_empty());

        assert_eq!(
            tpr_split_param_assignment(b"*Unpack:Password=secret=1"),
            Some((&b"*Unpack:Password"[..], &b"secret=1"[..]))
        );
        assert_eq!(tpr_split_param_assignment(b"invalid"), None);
        assert!(tpr_article_fetcher_is_server_eligible_on_level(
            true,
            0,
            0,
            2,
            1,
            &[1],
            &[2],
            &[0]
        ));
        assert!(!tpr_article_fetcher_is_server_eligible_on_level(
            true,
            0,
            0,
            2,
            1,
            &[1],
            &[1],
            &[0]
        ));
        assert!(tpr_article_fetcher_is_reused_stale_within_max(true, 4, 4));
        assert!(!tpr_article_fetcher_is_reused_stale_within_max(true, 5, 4));
        assert!(tpr_article_fetcher_is_declared_range_known(1, 100, 1000));
        assert!(!tpr_article_fetcher_is_declared_range_known(0, 100, 1000));
        assert!(tpr_article_fetcher_should_throttle_bandwidth(
            false, false, 1000, 1200, 800
        ));
        assert!(!tpr_article_fetcher_should_throttle_bandwidth(
            false, true, 1000, 1200, 800
        ));

        assert!(tpr_binrpc_is_valid_response_header(true, 123, 123, 64, 64));
        assert!(!tpr_binrpc_is_valid_response_header(false, 123, 123, 64, 64));
        assert!(!tpr_binrpc_is_valid_response_header(true, 124, 123, 64, 64));
        assert_eq!(tpr_binrpc_calc_elapsed_sec(150, 100), 50);
        assert_eq!(tpr_binrpc_calc_elapsed_sec(150, 0), 0);
        assert!(tpr_binrpc_should_include_history_entry(1, 3, false));
        assert!(!tpr_binrpc_should_include_history_entry(3, 3, false));
        assert!(tpr_binrpc_should_include_history_entry(3, 3, true));
        assert_eq!(tpr_ncurses_map_file_to_group_action(1), 11);
        assert_eq!(tpr_ncurses_map_file_to_group_action(4), 16);
        assert_eq!(tpr_ncurses_map_file_to_group_action(8), 23);
        assert_eq!(tpr_ncurses_map_file_to_group_action(99), 0);
        assert_eq!(tpr_ncurses_resolve_file_pause_action(4, 4, 5, false), 4);
        assert_eq!(tpr_ncurses_resolve_file_pause_action(4, 4, 5, true), 5);
        assert_eq!(
            tpr_ncurses_calc_page_nav(false, 5, 20, 5, 10),
            Some((-5, -5))
        );
        assert_eq!(
            tpr_ncurses_calc_page_nav(false, 8, 20, 5, 10),
            Some((5, 5))
        );
        assert_eq!(
            tpr_ncurses_calc_page_nav(true, 14, 20, 5, 10),
            Some((15, 24))
        );
        assert_eq!(
            tpr_ncurses_calc_page_nav(true, 8, 20, 5, 10),
            Some((5, 14))
        );
        assert_eq!(
            tpr_options_resolve_scheduler_task_state(
                true, false, true, false, false, true, false, true
            ),
            (1, true, true)
        );
        assert_eq!(
            tpr_options_split_option_string(b"Server1.Host=news.example.com"),
            Some((&b"Server1.Host"[..], &b"news.example.com"[..]))
        );
        assert_eq!(tpr_options_split_option_string(b"=value"), None);
        assert_eq!(
            tpr_script_resolve_single_arg_executor(true, b"/bin/sh", b"script.sh"),
            (true, true)
        );
        assert_eq!(
            tpr_script_resolve_single_arg_executor(true, b"script.sh", b"script.sh"),
            (true, false)
        );
        assert_eq!(tpr_nzbfile_trim_sax_error_trailing(b"bad xml \r\n "), 7);
    }
}
