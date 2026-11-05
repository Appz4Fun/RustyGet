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

/// Deduplicates an ID list in-order while preserving the first occurrence of each ID
/// (`QueueEditor::InternEditList`).
pub fn tpr_queue_editor_dedup_id_list(ids: &[i32]) -> Vec<i32> {
    let mut unique = Vec::with_capacity(ids.len());
    for &id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    unique
}

/// Maps a `DownloadQueue::EEditAction` group edit action to its corresponding file edit action
/// (`QueueEditor::EditGroup`).
pub fn tpr_queue_editor_map_group_to_file_action(action: i32) -> i32 {
    match action {
        1 | 11 => 1,                  // eaFileMoveOffset / eaGroupMoveOffset -> eaFileMoveOffset
        2 | 12 => 2,                  // eaFileMoveTop / eaGroupMoveTop -> eaFileMoveTop
        3 | 13 => 3,                  // eaFileMoveBottom / eaGroupMoveBottom -> eaFileMoveBottom
        4 | 16 => 4,                  // eaFilePause / eaGroupPause -> eaFilePause
        5 | 17 => 5,                  // eaFileResume / eaGroupResume -> eaFileResume
        6 | 18 | 19 | 20 | 21 => 6,   // eaFileDelete / eaGroup*Delete -> eaFileDelete
        7 | 22 => 7,                  // eaFilePauseAllPars / eaGroupPauseAllPars -> eaFilePauseAllPars
        8 | 23 => 8,                  // eaFilePauseExtraPars / eaGroupPauseExtraPars -> eaFilePauseExtraPars
        9 => 9,                       // eaFileReorder
        10 => 10,                     // eaFileSplit
        _ => 0,
    }
}

/// Resolves `(target_id, before, offset)` when moving groups relative to `target_id`
/// (`QueueEditor::MoveGroupsTo`).
pub fn tpr_queue_editor_resolve_move_groups_target(
    target_id: i32,
    queue_ids: &[i32],
    moved_ids: &[i32],
    before: bool,
    max_id: i32,
) -> (i32, bool, i32) {
    if !moved_ids.contains(&target_id) {
        return (target_id, before, 0);
    }
    let mut found = false;
    for &qid in queue_ids {
        if found {
            if !moved_ids.contains(&qid) {
                return (qid, true, 0);
            }
        } else if qid == target_id {
            found = true;
        }
    }
    (target_id, before, max_id)
}

/// Returns the byte length of `path` up to (and excluding) the last `sep` byte, or `path.len()`
/// if `sep` does not appear (`ParChecker::AddDupeFiles`).
pub fn tpr_parchecker_strip_last_path_sep_len(path: &[u8], sep: u8) -> usize {
    path.iter().rposition(|&b| b == sep).unwrap_or(path.len())
}

/// Finds the next contiguous `[block_start, block_end]` inclusive run of non-zero entries
/// in `valid_blocks` starting at `start_idx` (`ParChecker::VerifyPartialDataFile`).
pub fn tpr_parchecker_next_valid_block_run(
    valid_blocks: &[u8],
    start_idx: usize,
) -> Option<(usize, usize)> {
    if start_idx >= valid_blocks.len() {
        return None;
    }
    let rel_start = valid_blocks[start_idx..].iter().position(|&v| v != 0)?;
    let block_start = start_idx + rel_start;
    let rel_len = valid_blocks[block_start..]
        .iter()
        .position(|&v| v == 0)
        .unwrap_or(valid_blocks.len() - block_start);
    Some((block_start, block_start + rel_len - 1))
}

/// Evaluates whether a file is eligible for renamed RAR signature inspection
/// (`UnpackController::CheckArchiveFiles`).
pub fn tpr_unpack_ctrl_should_check_renamed_rar_sig(
    has_renamed_archive_files: bool,
    archive_kind: i32,
    is_ignored_ext: bool,
) -> bool {
    !has_renamed_archive_files && (archive_kind == 0 || archive_kind == 6) && !is_ignored_ext
}

/// Evaluates whether an active unpack process should auto-terminate due to an error message
/// (`UnpackController::AddMessage` and `DirectUnpack::AddMessage`).
pub fn tpr_unpack_should_auto_stop_on_error(
    is_stopped: bool,
    decrypt_error: bool,
    password_error: bool,
    crc_or_volume_error: bool,
) -> bool {
    !is_stopped && (decrypt_error || password_error || crc_or_volume_error)
}

/// Returns the NUL-terminated `(wrong_type_verb, missing_dir_verb, start_msg_verb)` labels
/// for `HistoryCoordinator::HistoryRetry`.
pub fn tpr_history_coord_retry_action_labels(
    reset_failed: bool,
    reprocess: bool,
) -> (&'static [u8], &'static [u8], &'static [u8]) {
    let wrong_type = if reset_failed {
        b"retry failed articles\0".as_slice()
    } else {
        b"download remaining files\0".as_slice()
    };
    let (missing_dir, start_msg) = if reset_failed {
        (
            b"retry failed articles for\0".as_slice(),
            b"Retrying failed articles for\0".as_slice(),
        )
    } else if reprocess {
        (
            b"post-process again\0".as_slice(),
            b"Post-processing again\0".as_slice(),
        )
    } else {
        (
            b"download remaining files for\0".as_slice(),
            b"Downloading remaining files for\0".as_slice(),
        )
    };
    (wrong_type, missing_dir, start_msg)
}

/// Evaluates whether a feed history entry is older than `border_date` (`FeedCoordinator::CleanupHistory`),
/// returning `Some(url_len_without_query)` when it should be deleted, or `None` to keep it.
pub fn tpr_feed_coord_should_delete_history_entry(
    last_seen: i64,
    border_date: i64,
    url: &[u8],
) -> Option<usize> {
    if last_seen < border_date {
        Some(url.iter().position(|&b| b == b'?').unwrap_or(url.len()))
    } else {
        None
    }
}

/// Computes the feed item age (`cur_time - item_time`) with wrapping semantics (`FeedFilter::Field` case 5).
pub fn tpr_feed_filter_calc_age(cur_time: i64, item_time: i64) -> i64 {
    cur_time.wrapping_sub(item_time)
}

/// Applies wrapping signed addition for `addPriority` and `addDupeScore` in `FeedFilter::Apply`.
pub fn tpr_feed_filter_wrapping_add_i32(cur: i32, delta: i32) -> i32 {
    cur.wrapping_add(delta)
}

/// Maps the filter match status code (`1 -> msAccepted`, `2 -> msRejected`, `_ -> msIgnored`)
/// in `FeedFilter::SetMatch`.
pub fn tpr_feed_filter_map_match_status(
    status: i32,
    ms_accepted: i32,
    ms_rejected: i32,
    ms_ignored: i32,
) -> i32 {
    match status {
        1 => ms_accepted,
        2 => ms_rejected,
        _ => ms_ignored,
    }
}

/// Formats the default feed name (`"Feed<id>"`) when `name` is empty (`FeedInfo::FeedInfo`).
pub fn tpr_feed_info_default_name(id: i32) -> Vec<u8> {
    format!("Feed{id}").into_bytes()
}

fn c_atoi_i32(bytes: &[u8]) -> i32 {
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    let mut neg = false;
    if idx < bytes.len() {
        if bytes[idx] == b'-' {
            neg = true;
            idx += 1;
        } else if bytes[idx] == b'+' {
            idx += 1;
        }
    }
    let mut acc: i64 = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        acc = acc
            .wrapping_mul(10)
            .wrapping_add(i64::from(bytes[idx] - b'0'));
        idx += 1;
    }
    if neg {
        acc.wrapping_neg() as i32
    } else {
        acc as i32
    }
}

/// Evaluates `anySeriesId` in `FeedItemInfo::BuildDupeKey`.
pub fn tpr_feed_item_has_any_series_id(
    series: &[u8],
    rage_id_str: &[u8],
    tvdb_id_str: &[u8],
    tvmaze_id_str: &[u8],
    item_rage_id: i32,
    item_tvdb_id: i32,
    item_tvmaze_id: i32,
) -> bool {
    let rage = if !rage_id_str.is_empty() {
        c_atoi_i32(rage_id_str)
    } else {
        item_rage_id
    };
    let tvdb = if !tvdb_id_str.is_empty() {
        c_atoi_i32(tvdb_id_str)
    } else {
        item_tvdb_id
    };
    let tvmaze = if !tvmaze_id_str.is_empty() {
        c_atoi_i32(tvmaze_id_str)
    } else {
        item_tvmaze_id
    };
    !series.is_empty() || rage != 0 || tvdb != 0 || tvmaze != 0
}

/// Formats `subCategory` for `Scanner::ProcessSubdirectory`.
pub fn tpr_scanner_format_subcategory(category: &[u8], sep: u8, filename: &[u8]) -> Vec<u8> {
    if category.is_empty() {
        filename.to_vec()
    } else {
        let mut out = Vec::with_capacity(category.len() + 1 + filename.len());
        out.extend_from_slice(category);
        out.push(sep);
        out.extend_from_slice(filename);
        out
    }
}

/// Computes `(present_articles, is_warning)` for missing-articles logging in `Scanner::AddFileToQueue`.
pub fn tpr_scanner_eval_missed_articles_log(
    total_articles: i32,
    missed_articles: i32,
    sev: i32,
) -> (i32, bool) {
    (total_articles.wrapping_sub(missed_articles), sev >= 2)
}

/// Formats a numbered external collision candidate path (`"%s%c%s_%i.%s"` or `"%s%c%s_%i"`)
/// in `Scanner::AddExternalFile`.
pub fn tpr_scanner_format_numbered_external_filename(
    nzb_dir: &[u8],
    sep: u8,
    base_part: &[u8],
    ext_part: &[u8],
    has_ext: bool,
    num: i32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(nzb_dir.len() + base_part.len() + ext_part.len() + 16);
    out.extend_from_slice(nzb_dir);
    out.push(sep);
    out.extend_from_slice(base_part);
    out.extend_from_slice(format!("_{num}").as_bytes());
    if has_ext {
        out.push(b'.');
        out.extend_from_slice(ext_part);
    }
    out
}

/// Formats `DownloadBinCommand::Execute` response message (`"Collection %s added to queue"` vs `"Download Request failed for %s"`).
pub fn tpr_binrpc_format_download_response_msg(ok: bool, base_filename: &[u8]) -> Vec<u8> {
    if ok {
        let mut out = Vec::with_capacity(base_filename.len() + 27);
        out.extend_from_slice(b"Collection ");
        out.extend_from_slice(base_filename);
        out.extend_from_slice(b" added to queue");
        out
    } else {
        let mut out = Vec::with_capacity(base_filename.len() + 28);
        out.extend_from_slice(b"Download Request failed for ");
        out.extend_from_slice(base_filename);
        out
    }
}

/// Evaluates `ListBinCommand::Execute` NZB group regex match flag (`matchGroup && (!regEx || regEx->Match(...))`).
pub fn tpr_binrpc_eval_nzb_group_match(
    match_group: bool,
    has_regex: bool,
    regex_matched: bool,
) -> u32 {
    u32::from(match_group && (!has_regex || regex_matched))
}

/// Formats `ListBinCommand::Execute` file regex match target (`"%s/%s"`, `nzb_name`, `base_filename`).
pub fn tpr_binrpc_format_file_match_name(nzb_name: &[u8], base_filename: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(nzb_name.len() + 1 + base_filename.len());
    out.extend_from_slice(nzb_name);
    out.push(b'/');
    out.extend_from_slice(base_filename);
    out
}

/// Formats `WebProcessor::SendBodyResponse` quoted hex ETag (`"\"%zx\""`).
pub fn tpr_web_format_quoted_hex_etag(hash: usize) -> Vec<u8> {
    format!("\"{hash:x}\"").into_bytes()
}

/// Evaluates whether gzipped output is smaller than the original body in `WebProcessor::SendBodyResponse`.
pub fn tpr_web_is_gzip_smaller(gzipped_len: i32, body_len: i32) -> bool {
    gzipped_len > 0 && gzipped_len < body_len
}

/// Formats `WebProcessor::SendSingleFileResponse` file path (`"%s%s%s"`, `web_dir`, `url`, `def_res`).
pub fn tpr_web_format_single_file_path(web_dir: &[u8], url: &[u8], def_res: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(web_dir.len() + url.len() + def_res.len());
    out.extend_from_slice(web_dir);
    out.extend_from_slice(url);
    out.extend_from_slice(def_res);
    out
}

/// Splits `XmlRpcProcessor::SetUrl` into `(path_set_len, query_offset)`.
/// When `'?'` is absent, `path_set_len` is `0` (matching `CString::Set(url, 0)` which copies the full string).
pub fn tpr_xmlrpc_split_url_path_and_query(url: &[u8]) -> (usize, Option<usize>) {
    match url.iter().position(|&b| b == b'?') {
        Some(pos) => (pos, Some(pos)),
        None => (0, None),
    }
}

/// Evaluates whether `EditQueueXmlCommand::Execute` should replace `args` with the formatted `offset`.
pub fn tpr_xmlrpc_should_use_edit_offset(
    has_offset: bool,
    action: i32,
    ea_file_move_offset: i32,
    ea_group_move_offset: i32,
) -> bool {
    has_offset && (action == ea_file_move_offset || action == ea_group_move_offset)
}

/// Formats `ReadUrlXmlCommand::Execute` temporary filename (`"%s%creadurl-%i.tmp"`, `temp_dir`, `sep`, `num`).
pub fn tpr_xmlrpc_format_readurl_temp_filename(temp_dir: &[u8], sep: u8, num: i32) -> Vec<u8> {
    let suffix = format!("readurl-{num}.tmp");
    let mut out = Vec::with_capacity(temp_dir.len() + 1 + suffix.len());
    out.extend_from_slice(temp_dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `TestServerXmlCommand::Execute` synthetic `ARTICLE <%s@nzbget.com>\r\n` command.
pub fn tpr_xmlrpc_format_testserver_article_cmd(id: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(id.len() + 24);
    out.extend_from_slice(b"ARTICLE <");
    out.extend_from_slice(id);
    out.extend_from_slice(b"@nzbget.com>\r\n");
    out
}

/// Format `"<prefix> for <name>"` (e.g. `"unpack for MyNzb"`, `"Unpack for MyNzb"`)
/// for post-processing controllers (`DirectUnpack.cpp`, `UnpackController.cpp`,
/// `Cleanup.cpp`, `Rename.cpp`).
pub fn tpr_post_format_info_name(prefix: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + 5 + name.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(b" for ");
    out.extend_from_slice(name);
    out
}

/// Format the `-p<password>` or `-p-` CLI flag for `unrar` / `7z` in
/// `DirectUnpack::ExecuteUnrar`, `UnpackController::ExecuteUnrar`, and
/// `UnpackController::ExecuteSevenZip`.
pub fn tpr_unpack_format_password_arg(password: &[u8]) -> Vec<u8> {
    if !password.is_empty() {
        let mut out = Vec::with_capacity(2 + password.len());
        out.extend_from_slice(b"-p");
        out.extend_from_slice(password);
        out
    } else {
        b"-p-".to_vec()
    }
}

/// Format the unpack destination directory (`"<dest_dir><sep>_unpack"` when
/// `use_temp_unpack_dir` is true, or `"<dest_dir>"` otherwise) for
/// `DirectUnpack::CreateUnpackDir` and `UnpackController::CreateUnpackDir`.
pub fn tpr_unpack_format_unpack_dir(
    dest_dir: &[u8],
    sep: u8,
    use_temp_unpack_dir: bool,
) -> Vec<u8> {
    if use_temp_unpack_dir {
        let mut out = Vec::with_capacity(dest_dir.len() + 8);
        out.extend_from_slice(dest_dir);
        out.push(sep);
        out.extend_from_slice(b"_unpack");
        out
    } else {
        dest_dir.to_vec()
    }
}

/// Select the NUL-terminated format string for the second-unpack-attempt skipped
/// error in `UnpackController::Run` (`UnpackController.cpp`).
pub fn tpr_unpack_ctrl_second_attempt_skip_errmsg(
    last_unpack_status: i32,
    us_password: i32,
) -> &'static [u8] {
    if last_unpack_status == us_password {
        b"%s failed: checksum error in the encrypted file. Corrupt file or wrong password.\0"
    } else {
        b"%s failed.\0"
    }
}

/// Format `"<dest_base_name>.<index:03>"` (matching `"%s.%.3i"`) for
/// `UnpackController::JoinFile` in `UnpackController.cpp`.
pub fn tpr_unpack_ctrl_format_split_piece_name(dest_base_name: &[u8], index: i32) -> Vec<u8> {
    let suffix = if index < 0 {
        format!("-{:03}", -(index as i64))
    } else {
        format!("{index:03}")
    };
    let mut out = Vec::with_capacity(dest_base_name.len() + 1 + suffix.len());
    out.extend_from_slice(dest_base_name);
    out.push(b'.');
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Format `"<dest_dir><sep><dest_base_name>.<index:03>"` (matching `"%s%c%s.%.3i"`)
/// for `UnpackController::JoinFile` in `UnpackController.cpp`.
pub fn tpr_unpack_ctrl_format_split_piece_path(
    dest_dir: &[u8],
    sep: u8,
    dest_base_name: &[u8],
    index: i32,
) -> Vec<u8> {
    let piece_name = tpr_unpack_ctrl_format_split_piece_name(dest_base_name, index);
    let mut out = Vec::with_capacity(dest_dir.len() + 1 + piece_name.len());
    out.extend_from_slice(dest_dir);
    out.push(sep);
    out.extend_from_slice(&piece_name);
    out
}

/// Return the NUL-terminated PAR-check request reason (`"par-check with forced repair"`
/// vs `"par-check/repair"`) for `UnpackController::RequestParCheck` (`UnpackController.cpp`).
pub fn tpr_unpack_ctrl_request_par_reason(force_repair: bool) -> &'static [u8] {
    if force_repair {
        b"par-check with forced repair\0"
    } else {
        b"par-check/repair\0"
    }
}

/// Return the NUL-terminated `"archive "` or `""` kind prefix for
/// `RenameController::Run` log messages in `Rename.cpp`.
pub fn tpr_rename_ctrl_archive_kind_prefix(kind: i32, jk_rar: i32) -> &'static [u8] {
    if kind == jk_rar {
        b"archive \0"
    } else {
        b"\0"
    }
}

/// Return the NUL-terminated rejection reason (`"describes other files"` vs
/// `"names the files otherwise"`) for a twin PAR2 set in
/// `RepairController::PostParChecker::RequestTwinIndex` (`Repair.cpp`).
pub fn tpr_repair_ctrl_twin_par2_reject_reason(cmp: i32) -> &'static [u8] {
    if cmp == 1 {
        b"describes other files\0"
    } else {
        b"names the files otherwise\0"
    }
}

/// Convert estimated repair time in seconds to whole minutes (`seconds / 60`)
/// for `RepairController::UpdateParCheckProgress` (`Repair.cpp`).
pub fn tpr_repair_ctrl_seconds_to_minutes(seconds: i32) -> i32 {
    seconds / 60
}

/// Format the twin-set PAR error message (`twin_err_action == 1` or `2`) for
/// `ParChecker::ProcessTwinSet` (`ParChecker.cpp`).
pub fn tpr_parchecker_format_twin_err_msg(
    twin_err_action: i32,
    own_err_msg: &[u8],
) -> Option<Vec<u8>> {
    match twin_err_action {
        1 => Some(b"not enough par-blocks, the twin's included".to_vec()),
        2 => {
            let suffix = b"; the twin's didn't do either";
            let mut out = Vec::with_capacity(own_err_msg.len() + suffix.len());
            out.extend_from_slice(own_err_msg);
            out.extend_from_slice(suffix);
            Some(out)
        }
        _ => None,
    }
}

/// Format `"Unrar: Extracting <suffix>"` after stripping the unpack directory
/// prefix in `DirectUnpack::AddMessage` and `UnpackController::AddMessage`.
pub fn tpr_unpack_format_extracting_msg(suffix: &[u8]) -> Vec<u8> {
    let prefix = b"Unrar: Extracting ";
    let mut out = Vec::with_capacity(prefix.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(suffix);
    out
}

/// Format `"Cancelling <info_name> due to errors"` for
/// `UnpackController::AddMessage` (`UnpackController.cpp`).
pub fn tpr_unpack_ctrl_format_cancel_msg(info_name: &[u8]) -> Vec<u8> {
    let prefix = b"Cancelling ";
    let suffix = b" due to errors";
    let mut out = Vec::with_capacity(prefix.len() + info_name.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(info_name);
    out.extend_from_slice(suffix);
    out
}

/// Format `"Checking renamed rar-files for <info_name>"` for
/// `RarRenamer::Execute` (`RarRenamer.cpp`).
pub fn tpr_rar_renamer_format_exec_progress(info_name: &[u8]) -> Vec<u8> {
    let prefix = b"Checking renamed rar-files for ";
    let mut out = Vec::with_capacity(prefix.len() + info_name.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(info_name);
    out
}

/// Format `"Checking file <filename>"` for `RarRenamer::CheckFiles`
/// (`RarRenamer.cpp`).
pub fn tpr_rar_renamer_format_file_progress(filename: &[u8]) -> Vec<u8> {
    let prefix = b"Checking file ";
    let mut out = Vec::with_capacity(prefix.len() + filename.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(filename);
    out
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

        assert_eq!(
            tpr_queue_editor_dedup_id_list(&[4, 2, 4, 1, 2, 3]),
            vec![4, 2, 1, 3]
        );
        assert_eq!(tpr_queue_editor_map_group_to_file_action(16), 4);
        assert_eq!(tpr_queue_editor_map_group_to_file_action(19), 6);
        assert_eq!(tpr_queue_editor_map_group_to_file_action(25), 0);
        assert_eq!(
            tpr_queue_editor_resolve_move_groups_target(2, &[1, 2, 3, 4], &[2, 3], false, 999),
            (4, true, 0)
        );
        assert_eq!(
            tpr_queue_editor_resolve_move_groups_target(3, &[1, 2, 3], &[2, 3], false, 999),
            (3, false, 999)
        );
        assert_eq!(
            tpr_parchecker_strip_last_path_sep_len(b"/tmp/dir/file.par2", b'/'),
            8
        );
        assert_eq!(tpr_parchecker_strip_last_path_sep_len(b"file.par2", b'/'), 9);
        assert_eq!(
            tpr_parchecker_next_valid_block_run(&[0, 1, 1, 0, 1], 0),
            Some((1, 2))
        );
        assert_eq!(
            tpr_parchecker_next_valid_block_run(&[0, 1, 1, 0, 1], 3),
            Some((4, 4))
        );
        assert_eq!(tpr_parchecker_next_valid_block_run(&[0, 0], 0), None);
        assert!(tpr_unpack_ctrl_should_check_renamed_rar_sig(false, 6, false));
        assert!(!tpr_unpack_ctrl_should_check_renamed_rar_sig(true, 6, false));
        assert!(tpr_unpack_should_auto_stop_on_error(
            false, false, true, false
        ));
        assert!(!tpr_unpack_should_auto_stop_on_error(
            true, false, true, false
        ));
        assert_eq!(
            tpr_history_coord_retry_action_labels(true, false),
            (
                b"retry failed articles\0".as_slice(),
                b"retry failed articles for\0".as_slice(),
                b"Retrying failed articles for\0".as_slice(),
            )
        );
        assert_eq!(
            tpr_history_coord_retry_action_labels(false, true),
            (
                b"download remaining files\0".as_slice(),
                b"post-process again\0".as_slice(),
                b"Post-processing again\0".as_slice(),
            )
        );
        assert_eq!(
            tpr_feed_coord_should_delete_history_entry(
                100,
                200,
                b"https://idx.example/get?apikey=secret"
            ),
            Some(23)
        );
        assert_eq!(
            tpr_feed_coord_should_delete_history_entry(
                200,
                200,
                b"https://idx.example/get?apikey=secret"
            ),
            None
        );
        assert_eq!(tpr_feed_filter_calc_age(1000, 940), 60);
        assert_eq!(tpr_feed_filter_wrapping_add_i32(10, -3), 7);
        assert_eq!(tpr_feed_filter_map_match_status(1, 10, 20, 30), 10);
        assert_eq!(tpr_feed_filter_map_match_status(2, 10, 20, 30), 20);
        assert_eq!(tpr_feed_filter_map_match_status(0, 10, 20, 30), 30);
        assert_eq!(tpr_feed_info_default_name(3), b"Feed3");
        assert!(tpr_feed_item_has_any_series_id(
            b"", b"123", b"", b"", 0, 0, 0
        ));
        assert!(!tpr_feed_item_has_any_series_id(
            b"", b"0", b"", b"", 99, 0, 0
        ));
        assert!(tpr_feed_item_has_any_series_id(
            b"", b"", b"", b"", 0, 42, 0
        ));
        assert_eq!(
            tpr_scanner_format_subcategory(b"Movies", b'/', b"HD"),
            b"Movies/HD"
        );
        assert_eq!(tpr_scanner_format_subcategory(b"", b'/', b"HD"), b"HD");
        assert_eq!(tpr_scanner_eval_missed_articles_log(100, 3, 2), (97, true));
        assert_eq!(tpr_scanner_eval_missed_articles_log(100, 1, 1), (99, false));
        assert_eq!(
            tpr_scanner_format_numbered_external_filename(
                b"/nzb", b'/', b"movie", b"nzb", true, 2
            ),
            b"/nzb/movie_2.nzb"
        );
        assert_eq!(
            tpr_scanner_format_numbered_external_filename(b"/nzb", b'/', b"movie", b"", false, 3),
            b"/nzb/movie_3"
        );
        assert_eq!(
            tpr_binrpc_format_download_response_msg(true, b"test.nzb"),
            b"Collection test.nzb added to queue"
        );
        assert_eq!(
            tpr_binrpc_format_download_response_msg(false, b"test.nzb"),
            b"Download Request failed for test.nzb"
        );
        assert_eq!(tpr_binrpc_eval_nzb_group_match(true, false, false), 1);
        assert_eq!(tpr_binrpc_eval_nzb_group_match(true, true, false), 0);
        assert_eq!(
            tpr_binrpc_format_file_match_name(b"MyNzb", b"part01.rar"),
            b"MyNzb/part01.rar"
        );
        assert_eq!(tpr_web_format_quoted_hex_etag(0x1a2b), b"\"1a2b\"");
        assert!(tpr_web_is_gzip_smaller(120, 300));
        assert!(!tpr_web_is_gzip_smaller(0, 300));
        assert!(!tpr_web_is_gzip_smaller(300, 300));
        assert_eq!(
            tpr_web_format_single_file_path(b"/web", b"/", b"index.html"),
            b"/web/index.html"
        );
        assert_eq!(
            tpr_xmlrpc_split_url_path_and_query(b"/jsonrpc/status?id=1"),
            (15, Some(15))
        );
        assert_eq!(
            tpr_xmlrpc_split_url_path_and_query(b"/jsonrpc/status"),
            (0, None)
        );
        assert!(tpr_xmlrpc_should_use_edit_offset(true, 1, 1, 13));
        assert!(!tpr_xmlrpc_should_use_edit_offset(false, 1, 1, 13));
        assert!(!tpr_xmlrpc_should_use_edit_offset(true, 2, 1, 13));
        assert_eq!(
            tpr_xmlrpc_format_readurl_temp_filename(b"/tmp", b'/', 2),
            b"/tmp/readurl-2.tmp"
        );
        assert_eq!(
            tpr_xmlrpc_format_testserver_article_cmd(b"12345"),
            b"ARTICLE <12345@nzbget.com>\r\n"
        );
        assert_eq!(
            tpr_post_format_info_name(b"unpack", b"MyNzb"),
            b"unpack for MyNzb"
        );
        assert_eq!(tpr_unpack_format_password_arg(b"secret"), b"-psecret");
        assert_eq!(tpr_unpack_format_password_arg(b""), b"-p-");
        assert_eq!(
            tpr_unpack_format_unpack_dir(b"/dst", b'/', true),
            b"/dst/_unpack"
        );
        assert_eq!(
            tpr_unpack_format_unpack_dir(b"/dst", b'/', false),
            b"/dst"
        );
        assert_eq!(
            tpr_unpack_ctrl_second_attempt_skip_errmsg(3, 3),
            b"%s failed: checksum error in the encrypted file. Corrupt file or wrong password.\0"
        );
        assert_eq!(
            tpr_unpack_ctrl_second_attempt_skip_errmsg(1, 3),
            b"%s failed.\0"
        );
        assert_eq!(
            tpr_unpack_ctrl_format_split_piece_name(b"archive.rar", 7),
            b"archive.rar.007"
        );
        assert_eq!(
            tpr_unpack_ctrl_format_split_piece_path(b"/dst", b'/', b"archive.rar", 7),
            b"/dst/archive.rar.007"
        );
        assert_eq!(
            tpr_unpack_ctrl_request_par_reason(true),
            b"par-check with forced repair\0"
        );
        assert_eq!(
            tpr_unpack_ctrl_request_par_reason(false),
            b"par-check/repair\0"
        );
        assert_eq!(tpr_rename_ctrl_archive_kind_prefix(1, 1), b"archive \0");
        assert_eq!(tpr_rename_ctrl_archive_kind_prefix(0, 1), b"\0");
        assert_eq!(
            tpr_repair_ctrl_twin_par2_reject_reason(1),
            b"describes other files\0"
        );
        assert_eq!(
            tpr_repair_ctrl_twin_par2_reject_reason(2),
            b"names the files otherwise\0"
        );
        assert_eq!(tpr_repair_ctrl_seconds_to_minutes(185), 3);
        assert_eq!(
            tpr_parchecker_format_twin_err_msg(1, b"err"),
            Some(b"not enough par-blocks, the twin's included".to_vec())
        );
        assert_eq!(
            tpr_parchecker_format_twin_err_msg(2, b"need 5 blocks"),
            Some(b"need 5 blocks; the twin's didn't do either".to_vec())
        );
        assert_eq!(tpr_parchecker_format_twin_err_msg(0, b"err"), None);
        assert_eq!(
            tpr_unpack_format_extracting_msg(b"video.mkv"),
            b"Unrar: Extracting video.mkv"
        );
        assert_eq!(
            tpr_unpack_ctrl_format_cancel_msg(b"unpack for MyNzb"),
            b"Cancelling unpack for MyNzb due to errors"
        );
        assert_eq!(
            tpr_rar_renamer_format_exec_progress(b"MyNzb"),
            b"Checking renamed rar-files for MyNzb"
        );
        assert_eq!(
            tpr_rar_renamer_format_file_progress(b"part01.rar"),
            b"Checking file part01.rar"
        );
    }
}
