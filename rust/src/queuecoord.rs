//! Rust implementation of `QueueCoordinator` (`daemon/queue/QueueCoordinator.cpp`).

use std::ffi::{c_char, CStr};

#[inline]
pub fn strcasecmp_bytes(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[inline]
pub fn find_subslice_ci(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|w| strcasecmp_bytes(w, needle))
}

// ============================================================================
// Status Constants Shared with HistoryCoordinator
// ============================================================================

pub const MARK_NONE: i32 = 0;
pub const MARK_BAD: i32 = 1;
pub const MARK_GOOD: i32 = 2;
pub const MARK_SUCCESS: i32 = 3;

pub const DELETE_NONE: i32 = 0;
pub const DELETE_MANUAL: i32 = 1;
pub const DELETE_HEALTH: i32 = 2;
pub const DELETE_DUPE: i32 = 3;
pub const DELETE_BAD: i32 = 4;
pub const DELETE_GOOD: i32 = 5;
pub const DELETE_COPY: i32 = 6;
pub const DELETE_SCAN: i32 = 7;

pub const PAR_NONE: i32 = 0;
pub const PAR_SKIPPED: i32 = 1;
pub const PAR_FAILURE: i32 = 2;
pub const PAR_SUCCESS: i32 = 3;
pub const PAR_REPAIR_POSSIBLE: i32 = 4;
pub const PAR_MANUAL: i32 = 5;

pub const UNPACK_NONE: i32 = 0;
pub const UNPACK_SKIPPED: i32 = 1;
pub const UNPACK_FAILURE: i32 = 2;
pub const UNPACK_SUCCESS: i32 = 3;
pub const UNPACK_SPACE: i32 = 4;
pub const UNPACK_PASSWORD: i32 = 5;

pub const DUP_STATUS_UNDEFINED: i32 = 0;
pub const DUP_STATUS_SUCCESS: i32 = 1;
pub const DUP_STATUS_FAILED: i32 = 2;
pub const DUP_STATUS_DELETED: i32 = 3;
pub const DUP_STATUS_DUPE: i32 = 4;
pub const DUP_STATUS_BAD: i32 = 5;
pub const DUP_STATUS_GOOD: i32 = 6;

pub const ARTICLE_UNDEFINED: i32 = 0;
pub const ARTICLE_RUNNING: i32 = 1;
pub const ARTICLE_FINISHED: i32 = 2;
pub const ARTICLE_FAILED: i32 = 3;

pub const PARTIAL_STATE_NONE: i32 = 0;
pub const PARTIAL_STATE_PARTIAL: i32 = 1;
pub const PARTIAL_STATE_COMPLETED: i32 = 2;

pub const COMPLETED_FILE_NONE: i32 = 0;
pub const COMPLETED_FILE_SUCCESS: i32 = 1;
pub const COMPLETED_FILE_PARTIAL: i32 = 2;
pub const COMPLETED_FILE_FAILURE: i32 = 3;

/// `QueueCoordinator::DeleteFileInfo` (`QueueCoordinator.cpp`):
/// Classifies `CompletedFile::EStatus` from article counts.
pub fn classify_completed_file_status(
    total_articles: i32,
    success_articles: i32,
    missed_articles: i32,
    failed_articles: i32,
) -> i32 {
    if total_articles == success_articles {
        COMPLETED_FILE_SUCCESS
    } else if total_articles == missed_articles + failed_articles {
        COMPLETED_FILE_FAILURE
    } else if success_articles > 0 || failed_articles > 0 {
        COMPLETED_FILE_PARTIAL
    } else {
        COMPLETED_FILE_NONE
    }
}

/// Base filename after last `/` or `\`.
pub fn base_file_name(path: &[u8]) -> &[u8] {
    if let Some(pos) = path.iter().rposition(|&b| b == b'/' || b == b'\\') {
        &path[pos + 1..]
    } else {
        path
    }
}

/// `QueueCoordinator::DeleteFileInfo` (`QueueCoordinator.cpp`):
/// Resolves the filename recorded in `CompletedFile`, preserving relative subfolder
/// paths under `dest_dir` and falling back to `file_filename` when `cfFailure` never
/// created `output_filename` on disk.
pub fn resolve_completed_filename(
    completed: bool,
    file_status: i32,
    output_filename: &[u8],
    file_filename: &[u8],
    dest_dir: &[u8],
    path_sep: u8,
    output_file_exists: bool,
) -> Vec<u8> {
    let mut filename: Vec<u8> = if completed && !output_filename.is_empty() {
        base_file_name(output_filename).to_vec()
    } else {
        file_filename.to_vec()
    };

    let mut dest_prefix = Vec::with_capacity(dest_dir.len() + 1);
    dest_prefix.extend_from_slice(dest_dir);
    dest_prefix.push(path_sep);

    if completed
        && !output_filename.is_empty()
        && output_filename.len() > dest_prefix.len()
        && output_filename.starts_with(&dest_prefix)
        && output_filename[dest_prefix.len()..].contains(&path_sep)
    {
        filename = output_filename[dest_prefix.len()..].to_vec();
    }

    if completed
        && file_status == COMPLETED_FILE_FAILURE
        && !file_filename.is_empty()
        && !output_file_exists
    {
        filename = file_filename.to_vec();
    }

    filename
}

// ============================================================================
// QueueCoordinator (daemon/queue/QueueCoordinator.cpp)
// ============================================================================

/// `QueueCoordinator::AdjustDownloadsLimit` (`QueueCoordinator.cpp`):
/// Base 2 threads + maxConnections for each active level-0 or level-1 server.
pub fn calc_downloads_limit(norm_levels: &[i32], actives: &[u8], max_conns: &[i32]) -> i32 {
    let mut limit = 2i32;
    let n = norm_levels.len().min(actives.len()).min(max_conns.len());
    for i in 0..n {
        if (norm_levels[i] == 0 || norm_levels[i] == 1) && actives[i] != 0 {
            limit += max_conns[i];
        }
    }
    limit
}

/// `QueueCoordinator::IsPropagationWait` (`QueueCoordinator.cpp`).
#[inline]
pub fn is_propagation_wait(post_time: i64, propagation_delay: i32, cur_date: i64) -> bool {
    propagation_delay > 0 && (post_time as i32).wrapping_add(propagation_delay) >= (cur_date as i32)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RsCandidateFile {
    pub nzb_priority: i32,
    pub extra_priority: i32,
    pub paused: i32,
    pub deleted: i32,
    pub already_checked: i32,
    pub post_time: i64,
}

/// `QueueCoordinator::GetNextArticle` (`QueueCoordinator.cpp`) inner candidate selection:
/// Selects the best runnable file index (`out_best_idx`, or `-1`) and the earliest
/// propagation-held file index (`out_prop_wait_idx`, or `-1`).
pub fn select_next_file(
    files: &[RsCandidateFile],
    propagation_delay: i32,
    cur_date: i64,
) -> (i32, i32) {
    let mut best_idx: Option<usize> = None;
    let mut prop_wait_idx: Option<usize> = None;

    for (idx, f) in files.iter().enumerate() {
        if f.already_checked != 0 || f.paused != 0 || f.deleted != 0 {
            continue;
        }
        let prop_wait = is_propagation_wait(f.post_time, propagation_delay, cur_date);
        if !prop_wait {
            let higher = match best_idx {
                None => true,
                Some(prev_i) => {
                    let prev = &files[prev_i];
                    let cur_ep = f.extra_priority != 0;
                    let prev_ep = prev.extra_priority != 0;
                    (cur_ep == prev_ep && f.nzb_priority > prev.nzb_priority)
                        || (cur_ep && !prev_ep)
                }
            };
            if higher {
                best_idx = Some(idx);
            }
        } else {
            let earlier = match prop_wait_idx {
                None => true,
                Some(prev_i) => f.post_time < files[prev_i].post_time,
            };
            if earlier {
                prop_wait_idx = Some(idx);
            }
        }
    }

    (
        best_idx.map(|i| i as i32).unwrap_or(-1),
        prop_wait_idx.map(|i| i as i32).unwrap_or(-1),
    )
}

/// `QueueCoordinator::CertainlyParless` `dataName` check (`QueueCoordinator.cpp`).
pub fn is_data_filename(filename: &[u8]) -> bool {
    let Some(dot_pos) = filename.iter().rposition(|&b| b == b'.') else {
        return false;
    };
    let ext = &filename[dot_pos..];
    if ext.len() <= 1 {
        return false;
    }
    const EXTENSIONS: &[&[u8]] = &[
        b".mkv", b".mp4", b".avi", b".m4v", b".ts", b".m2ts", b".mov", b".wmv", b".mpg", b".mpeg",
        b".iso", b".rar", b".7z", b".zip", b".nfo", b".sfv", b".srr", b".srt", b".sub", b".idx",
        b".jpg", b".png", b".txt", b".flac", b".mp3", b".m4a",
    ];
    for &known in EXTENSIONS {
        if strcasecmp_bytes(ext, known) {
            return true;
        }
    }
    let digits = if ext[1] == b'r' || ext[1] == b'R' {
        &ext[2..]
    } else {
        &ext[1..]
    };
    digits.len() >= 2 && digits.len() <= 3 && digits.iter().all(|b| b.is_ascii_digit())
}

/// `QueueCoordinator::HasSizeTwin` `sizeNear` check (`QueueCoordinator.cpp`):
/// `llabs(size1 - size2) * 1000 <= size1 * 3`
#[inline]
pub fn size_near(size1: i64, size2: i64) -> bool {
    (size1 - size2).wrapping_abs().saturating_mul(1000) <= size1.saturating_mul(3)
}

pub const DUPE_HOPELESS_TRIED_PERCENT: i64 = 10;
pub const DUPE_HOPELESS_ALIVE_PERCENT: i64 = 10;
pub const DEAD_DOWNLOAD_FAILURES: i32 = 64;
pub const DEAD_RUN_FAILURES: i32 = 40;
pub const PROJECTED_FAILURE_SAMPLE: i32 = 200;
pub const SLOW_PROBE_SEC: i32 = 10;
pub const PARK_BELOW_CRITICAL_SAMPLE: i32 = 1000;

/// `QueueCoordinator::DownloadHopeless` (`QueueCoordinator.cpp`).
pub fn download_hopeless(total: i32, current_success: i32, current_failed: i32) -> bool {
    let tried = current_success + current_failed;
    total > 0
        && (tried as i64) * 100 >= (total as i64) * DUPE_HOPELESS_TRIED_PERCENT
        && (current_success as i64) * 100 < (tried as i64) * DUPE_HOPELESS_ALIVE_PERCENT
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RsDeadDownloadEval {
    pub own: i32,
    pub tried: i32,
    pub dead_rate: i32,
    pub dead_run: i32,
    pub dead: i32,
    pub projected: i32,
    pub doomed: i32,
    pub hopeless: i32,
    pub far_below: i32,
    pub below: i32,
}

/// `QueueCoordinator::CheckDeadDownload` metrics (`QueueCoordinator.cpp`).
#[allow(clippy::too_many_arguments)]
pub fn eval_dead_download(
    current_success: i32,
    dupe_recovered: i32,
    current_failed: i32,
    file_count: i32,
    dupe_failed_run: i32,
    par_cannot_cover_exact: bool,
    critical: i32,
    par_edge_margin: i32,
    files_tried: i32,
    probing_sec: i32,
) -> RsDeadDownloadEval {
    let own = (current_success - dupe_recovered.max(0)).max(0);
    let failed = current_failed;
    let tried = own + failed;

    let dead_rate =
        failed >= DEAD_DOWNLOAD_FAILURES + file_count && (own as i64) * 100 < (failed as i64);
    let dead_run = dupe_failed_run >= DEAD_RUN_FAILURES && par_cannot_cover_exact;
    let dead = dead_rate || dead_run;

    let projected = if tried > 0 {
        ((own as i64) * 1000 / (tried as i64)) as i32
    } else {
        1000
    };
    let doomed = !dead
        && tried >= PROJECTED_FAILURE_SAMPLE
        && files_tried >= 3
        && projected < critical + par_edge_margin;

    let hopeless = own == 0 && failed >= PROJECTED_FAILURE_SAMPLE && probing_sec >= SLOW_PROBE_SEC;
    let far_below = projected * 2 < critical;
    let below = projected < critical && tried >= PARK_BELOW_CRITICAL_SAMPLE;

    RsDeadDownloadEval {
        own,
        tried,
        dead_rate: i32::from(dead_rate),
        dead_run: i32::from(dead_run),
        dead: i32::from(dead),
        projected,
        doomed: i32::from(doomed),
        hopeless: i32::from(hopeless),
        far_below: i32::from(far_below),
        below: i32::from(below),
    }
}

/// `QueueCoordinator::SideFile` (`QueueCoordinator.cpp`).
pub fn is_side_file(filename: &[u8], ext_cleanup_disk: &[u8]) -> bool {
    let Some(dot_pos) = filename.iter().rposition(|&b| b == b'.') else {
        return false;
    };
    let extension = &filename[dot_pos..];
    const BUILTIN_SIDES: &[&[u8]] = &[
        b".nfo", b".sfv", b".txt", b".url", b".srr", b".srs", b".nzb", b".jpg", b".jpeg", b".png",
    ];
    for &side in BUILTIN_SIDES {
        if strcasecmp_bytes(extension, side) {
            return true;
        }
    }
    for token in ext_cleanup_disk.split(|&b| b == b',' || b == b';' || b == b' ') {
        if token.is_empty() {
            continue;
        }
        if strcasecmp_bytes(extension, token) && !strcasecmp_bytes(token, b".par2") {
            return true;
        }
    }
    false
}

/// `QueueCoordinator::ResetHangingDownloads` (`QueueCoordinator.cpp`).
#[inline]
pub fn is_hanging_download(
    now: i64,
    last_update: i64,
    article_timeout: i32,
    is_running: bool,
) -> bool {
    is_running && article_timeout > 0 && (now - last_update) > (article_timeout as i64) + 1
}

/// Tie-breaking comparison for duplicate file sizes and indices (`QueueCoordinator::CheckDupeFileInfos`).
#[inline]
pub fn is_smaller_dupe_file(size1: i64, idx1: i32, size2: i64, idx2: i32) -> bool {
    size1 < size2 || (size1 == size2 && idx2 < idx1)
}

/// Identifies indices of files in `filenames` (`0..filenames.len()`) whose filename
/// appears exactly twice and which lose the size/index tie-breaker
/// (`QueueCoordinator::CheckDupeFileInfos` in `daemon/queue/QueueCoordinator.cpp`).
pub fn find_dupe_file_indices(
    filenames: &[&[u8]],
    sizes: &[i64],
    out_dupe_indices: &mut [usize],
) -> usize {
    let n = filenames.len().min(sizes.len());
    let mut by_name: std::collections::HashMap<&[u8], Vec<(i32, usize)>> =
        std::collections::HashMap::with_capacity(n);
    for (idx, &name) in filenames.iter().enumerate().take(n) {
        by_name
            .entry(name)
            .or_default()
            .push(((idx + 1) as i32, idx));
    }
    let mut written = 0usize;
    for (idx, &name) in filenames.iter().enumerate().take(n) {
        let Some(same) = by_name.get(name) else {
            continue;
        };
        if same.len() != 2 {
            continue;
        }
        let (self_pos, self_idx) = if same[0].1 == idx { same[0] } else { same[1] };
        let (other_pos, other_idx) = if same[0].1 == idx { same[1] } else { same[0] };
        if is_smaller_dupe_file(sizes[self_idx], self_pos, sizes[other_idx], other_pos)
            && written < out_dupe_indices.len()
        {
            out_dupe_indices[written] = idx;
            written += 1;
        }
    }
    written
}

/// Computes `(posted_min_ago, start_in_min)` for the `PropagationDelay` log message in `QueueCoordinator::GetNextArticle`.
#[inline]
pub fn calc_propagation_wait_log_mins(
    file_time: i64,
    propagation_delay: i32,
    cur_date: i64,
) -> (i32, i32) {
    let wait_sec = (file_time as i32)
        .wrapping_add(propagation_delay)
        .wrapping_sub(cur_date as i32);
    let posted_min = (cur_date.wrapping_sub(file_time) as i32) / 60;
    let start_min = (wait_sec / 60).wrapping_add(1);
    (posted_min, start_min)
}

/// Applies article completion success counters to file and NZB counters in `QueueCoordinator::ArticleCompleted`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn apply_success_article_counters(
    article_size: i64,
    is_par_file: bool,
    file_success_size: i64,
    nzb_cur_success_size: i64,
    nzb_par_cur_success_size: i64,
    file_success_articles: i32,
    nzb_cur_success_articles: i32,
) -> (i64, i64, i64, i32, i32) {
    let par_add = if is_par_file { article_size } else { 0 };
    (
        file_success_size.wrapping_add(article_size),
        nzb_cur_success_size.wrapping_add(article_size),
        nzb_par_cur_success_size.wrapping_add(par_add),
        file_success_articles.wrapping_add(1),
        nzb_cur_success_articles.wrapping_add(1),
    )
}

/// Updates the file and NZB failed-article counters in `QueueCoordinator::ArticleCompleted`.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn apply_failed_article_counters(
    part_number: i32,
    par_file: i32,
    article_size: i32,
    dupe_failed_run: i32,
    file_failed_size: i64,
    nzb_cur_failed_size: i64,
    nzb_par_cur_failed_size: i64,
    file_failed_articles: i32,
    nzb_cur_failed_articles: i32,
) -> (i32, i64, i64, i64, i32, i32) {
    let next_dupe_failed_run = if part_number != 1 && par_file == 0 {
        dupe_failed_run.wrapping_add(1)
    } else {
        dupe_failed_run
    };
    let size64 = article_size as i64;
    let next_file_failed_size = file_failed_size.wrapping_add(size64);
    let next_nzb_cur_failed_size = nzb_cur_failed_size.wrapping_add(size64);
    let next_nzb_par_cur_failed_size =
        nzb_par_cur_failed_size.wrapping_add(if par_file != 0 { size64 } else { 0 });
    let next_file_failed_articles = file_failed_articles.wrapping_add(1);
    let next_nzb_cur_failed_articles = nzb_cur_failed_articles.wrapping_add(1);
    (
        next_dupe_failed_run,
        next_file_failed_size,
        next_nzb_cur_failed_size,
        next_nzb_par_cur_failed_size,
        next_file_failed_articles,
        next_nzb_cur_failed_articles,
    )
}

/// Selects the duplicate donor ID for a completed article in `QueueCoordinator::ArticleCompleted`.
#[inline]
pub fn select_dupe_donor_id(
    dupe_fallback_round: i32,
    donors: &[i32],
    msg_id: &[u8],
    orig_msg_id: &[u8],
) -> i32 {
    let source = dupe_fallback_round.wrapping_sub(1);
    if source >= 0 && (source as usize) < donors.len() && msg_id != orig_msg_id {
        donors[source as usize]
    } else {
        0
    }
}

/// Evaluates whether a finished article counts as a proven duplicate recovery and whether `DupeCutover` should be triggered.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn eval_dupe_recovery_and_cutover(
    dupe_fallback_round: i32,
    dupe_proactive: bool,
    msg_id: &[u8],
    orig_msg_id: &[u8],
    cur_file_recovered: i32,
    cur_nzb_recovered: i32,
    dupe_cutover: bool,
    cutover_threshold: i32,
) -> (bool, i32, i32, bool) {
    let recovered = dupe_fallback_round > 0
        && !dupe_proactive
        && !orig_msg_id.is_empty()
        && msg_id != orig_msg_id;
    if !recovered {
        return (false, cur_file_recovered, cur_nzb_recovered, false);
    }
    let next_file_rec = cur_file_recovered.wrapping_add(1);
    let next_nzb_rec = cur_nzb_recovered.wrapping_add(1);
    let trigger_cutover = !dupe_cutover && next_file_rec >= cutover_threshold;
    (true, next_file_rec, next_nzb_rec, trigger_cutover)
}

/// Advances remaining sizes and completed article count when an article finishes without retry in `QueueCoordinator::ArticleCompleted`.
#[inline]
pub fn advance_article_completion_counters(
    rem_file_size: i64,
    rem_nzb_size: i64,
    paused_size: i64,
    article_size: i32,
    is_paused: bool,
    completed_articles: i32,
    total_articles: i32,
) -> (i64, i64, i64, i32, bool) {
    let sz = i64::from(article_size);
    let new_rem_file = rem_file_size.wrapping_sub(sz);
    let new_rem_nzb = rem_nzb_size.wrapping_sub(sz);
    let new_paused = if is_paused {
        paused_size.wrapping_sub(sz)
    } else {
        paused_size
    };
    let new_completed = completed_articles.wrapping_add(1);
    let file_completed = total_articles == new_completed;
    (
        new_rem_file,
        new_rem_nzb,
        new_paused,
        new_completed,
        file_completed,
    )
}

/// Evaluates `(file_completed, complete_file_parts)` after health and dead-download checks in `QueueCoordinator::ArticleCompleted`.
#[inline]
pub fn eval_file_completion_and_assembly(
    file_completed: bool,
    parking: bool,
    active_downloads: i32,
    dupe_deleted: bool,
    file_deleted: bool,
    finished_before_cancel: bool,
) -> (bool, bool) {
    let fc = if parking && active_downloads == 1 && !dupe_deleted {
        true
    } else {
        file_completed
    };
    let complete_parts = fc && (!file_deleted || parking || finished_before_cancel);
    (fc, complete_parts)
}

/// Applies counter adjustments when `CompleteFileParts()` fails in `QueueCoordinator::ArticleCompleted`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn apply_assembly_failure_counters(
    success_articles: i32,
    success_size: i64,
    is_par_file: bool,
    inout_file_failed_articles: &mut i32,
    inout_file_failed_size: &mut i64,
    inout_nzb_cur_failed_articles: &mut i32,
    inout_nzb_cur_success_articles: &mut i32,
    inout_nzb_cur_failed_size: &mut i64,
    inout_nzb_cur_success_size: &mut i64,
    inout_nzb_par_cur_failed_size: &mut i64,
    inout_nzb_par_cur_success_size: &mut i64,
) {
    *inout_file_failed_articles = inout_file_failed_articles.wrapping_add(success_articles);
    *inout_file_failed_size = inout_file_failed_size.wrapping_add(success_size);
    *inout_nzb_cur_failed_articles = inout_nzb_cur_failed_articles.wrapping_add(success_articles);
    *inout_nzb_cur_success_articles = inout_nzb_cur_success_articles
        .wrapping_sub(success_articles)
        .max(0);
    *inout_nzb_cur_failed_size = inout_nzb_cur_failed_size.wrapping_add(success_size);
    *inout_nzb_cur_success_size = inout_nzb_cur_success_size.wrapping_sub(success_size).max(0);
    if is_par_file {
        *inout_nzb_par_cur_failed_size = inout_nzb_par_cur_failed_size.wrapping_add(success_size);
        *inout_nzb_par_cur_success_size = inout_nzb_par_cur_success_size
            .wrapping_sub(success_size)
            .max(0);
    }
}

/// Deduplicates, sorts, and formats a comma-separated list of donor IDs (`QueueCoordinator::RejectBorrowedArticles`).
pub fn format_donor_id_list(donor_ids: &[i32]) -> (Vec<i32>, Vec<u8>) {
    let mut unique: Vec<i32> = donor_ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let mut out = String::new();
    for (i, id) in unique.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&id.to_string());
    }
    (unique, out.into_bytes())
}

/// Evaluates `(delete_data, save_partial)` flags for `DiskState` operations in `QueueCoordinator::DeleteFileInfo`.
#[inline]
pub fn eval_delete_file_diskstate_flags(
    file_status: i32,
    cf_success: i32,
    cf_partial: i32,
    file_deleted: i32,
    parking: i32,
    completed: i32,
) -> (bool, bool) {
    let delete_data = file_status == cf_success || (file_deleted != 0 && parking == 0);
    let save_partial = file_status == cf_partial && (completed != 0 || parking != 0);
    (delete_data, save_partial)
}

/// Demotes a finished article's counters back to failed in `QueueCoordinator::CountAsFailed`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn demote_article_counters(
    article_size: i32,
    is_par_file: bool,
    dupe_proactive: bool,
    msg_id: &[u8],
    orig_msg_id: &[u8],
    inout_file_success_size: &mut i64,
    inout_file_failed_size: &mut i64,
    inout_file_success_articles: &mut i32,
    inout_file_failed_articles: &mut i32,
    inout_nzb_cur_success_size: &mut i64,
    inout_nzb_cur_failed_size: &mut i64,
    inout_nzb_par_cur_success_size: &mut i64,
    inout_nzb_par_cur_failed_size: &mut i64,
    inout_nzb_cur_success_articles: &mut i32,
    inout_nzb_cur_failed_articles: &mut i32,
    inout_file_dupe_recovered: &mut i32,
    inout_nzb_dupe_recovered: &mut i32,
) {
    let sz = i64::from(article_size);
    *inout_file_success_size = inout_file_success_size.wrapping_sub(sz);
    *inout_file_failed_size = inout_file_failed_size.wrapping_add(sz);
    *inout_file_success_articles = inout_file_success_articles.wrapping_sub(1);
    *inout_file_failed_articles = inout_file_failed_articles.wrapping_add(1);
    *inout_nzb_cur_success_size = inout_nzb_cur_success_size.wrapping_sub(sz);
    *inout_nzb_cur_failed_size = inout_nzb_cur_failed_size.wrapping_add(sz);
    if is_par_file {
        *inout_nzb_par_cur_success_size = inout_nzb_par_cur_success_size.wrapping_sub(sz);
        *inout_nzb_par_cur_failed_size = inout_nzb_par_cur_failed_size.wrapping_add(sz);
    }
    *inout_nzb_cur_success_articles = inout_nzb_cur_success_articles.wrapping_sub(1);
    *inout_nzb_cur_failed_articles = inout_nzb_cur_failed_articles.wrapping_add(1);

    if !dupe_proactive && !orig_msg_id.is_empty() && msg_id != orig_msg_id {
        *inout_file_dupe_recovered = inout_file_dupe_recovered.wrapping_sub(1);
        *inout_nzb_dupe_recovered = inout_nzb_dupe_recovered.wrapping_sub(1);
    }
}

/// Evaluates whether `QueueCoordinator::CheckHealth` should return early without taking action.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn should_skip_health_check(
    health_check: i32,
    hc_none: i32,
    hc_park: i32,
    par_scan: i32,
    ps_dupe: i32,
    health_paused: bool,
    delete_status: i32,
    ds_health: i32,
    health: i32,
    critical_health: i32,
    success_articles: i32,
    total_articles: i32,
) -> bool {
    health_check == hc_none
        || health_paused
        || delete_status == ds_health
        || health >= critical_health
        || (par_scan == ps_dupe
            && health_check == hc_park
            && total_articles > 0
            && i64::from(success_articles) * 100 / i64::from(total_articles) > 10)
}

/// Evaluates whether `other` is a size twin of `nzbInfo` (`QueueCoordinator::HasSizeTwin`).
pub fn is_size_twin(
    is_self: bool,
    same_name_or_key: bool,
    self_size: i64,
    other_size: i64,
    other_par_size: i64,
    largest_self_file: i64,
    other_file_sizes: &[i64],
) -> bool {
    if is_self || !same_name_or_key {
        return false;
    }
    if size_near(self_size, other_size)
        || (other_par_size > 0 && size_near(self_size, other_size - other_par_size))
    {
        return true;
    }
    other_file_sizes
        .iter()
        .any(|&fs| size_near(largest_self_file, fs))
}

/// Evaluates whether a queued duplicate entry is considered active (`QueueCoordinator::HasActiveDupe`).
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn is_queued_dupe_active(
    deleting: bool,
    delete_status: i32,
    ds_none: i32,
    active_downloads: i32,
    has_post_info: bool,
    post_working: bool,
    post_stage: i32,
    pt_queued: i32,
    extra_priority: bool,
    self_completed_empty: bool,
    direct_unpack_status: i32,
    ns_running: i32,
) -> bool {
    if (deleting || delete_status != ds_none) && active_downloads == 0 {
        return false;
    }
    let is_active_download = active_downloads > 0;
    let is_post_processing = has_post_info
        && (post_working || post_stage != pt_queued || extra_priority || self_completed_empty);
    let is_direct_unpacking = direct_unpack_status == ns_running;
    is_active_download || is_post_processing || is_direct_unpacking
}

/// Determines whether `QueueCoordinator::CheckDupeFailover` should defer failover and whether `DupeFailoverChecked` should be updated.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn should_defer_dupe_failover(
    fallback_enabled: bool,
    attempted: i32,
    recovered: i32,
    failover_sample: i32,
    defer_state: i32,
    dp_deferred: i32,
    par_cannot_cover: bool,
    failed_articles: i32,
    failover_checked: i32,
) -> (bool, bool) {
    if fallback_enabled
        && attempted > 0
        && (attempted < failover_sample || recovered.saturating_mul(2) >= attempted)
    {
        return (true, false);
    }
    if fallback_enabled && attempted == 0 && defer_state == dp_deferred && !par_cannot_cover {
        return (true, false);
    }
    if failover_checked >= 0 && failed_articles.wrapping_sub(failover_checked) < failover_sample {
        return (true, false);
    }
    (false, true)
}

/// Determines whether `QueueCoordinator::CheckDeadDownload` should park an unhealthy download that has no backup in history.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn should_park_dead_without_backup(
    fallback_enabled: bool,
    defer_state: i32,
    dp_deferred: i32,
    attempted: i32,
    recovered: i32,
    tried: i32,
    projected_sample: i32,
    files_tried: i32,
    far_below: bool,
    below: bool,
) -> bool {
    let borrowing = fallback_enabled
        && (defer_state == dp_deferred
            || (attempted > 0 && recovered.saturating_mul(2) >= attempted));
    tried >= projected_sample && files_tried >= 3 && (far_below || below) && !borrowing
}

/// Determines whether `QueueCoordinator::CheckParDamage` should wait for duplicate article borrowing before failing over.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn should_wait_for_borrowing_on_par_damage(
    fallback_enabled: bool,
    raw_article: bool,
    dupe_mode: i32,
    dm_force: i32,
    has_par2: bool,
    defer_state: i32,
    dp_deferred: i32,
    attempted: i32,
    failover_sample: i32,
    recovered: i32,
) -> bool {
    if fallback_enabled && !raw_article && dupe_mode != dm_force && has_par2 {
        defer_state == dp_deferred
            || attempted < failover_sample
            || recovered.saturating_mul(2) >= attempted
    } else {
        false
    }
}

/// Formats PAR damage summary string for `QueueCoordinator::CheckParDamage`.
pub fn format_par_damage_summary(
    certain: bool,
    damaged_blocks: i32,
    projected_blocks: i32,
    block_size: i64,
    recovery_blocks: i32,
) -> Vec<u8> {
    let blocks = if certain {
        damaged_blocks
    } else {
        projected_blocks
    };
    let mb = (block_size as f64) / 1024.0 / 1024.0;
    let suffix = if certain { "" } else { " at the rate so far" };
    format!(
        "{blocks} par2 block(s) of {mb:.1} MB damaged{suffix}, its par2-files hold {recovery_blocks} recovery block(s)"
    )
    .into_bytes()
}

/// Evaluates `(discard_par_probe_article, discard_article_infos)` in `QueueCoordinator::DiscardDirectRename`.
#[inline]
pub fn eval_discard_direct_rename_file(
    is_par_file: bool,
    completed_articles: i32,
    active_downloads: i32,
    server_mode: bool,
    has_articles: bool,
    continue_partial: bool,
    cached_articles: i32,
) -> (bool, bool) {
    let discard_par = is_par_file && completed_articles == 1 && active_downloads == 0;
    let discard_infos = server_mode
        && has_articles
        && continue_partial
        && active_downloads == 0
        && cached_articles == 0;
    (discard_par, discard_infos)
}

/// Validates a file entry in `QueueCoordinator::SplitQueueEntries`.
/// Returns `0` if valid, `1` if already (partially) downloaded, `2` if in post-process stage, `3` if from a different collection.
#[inline]
pub fn validate_split_file_state(
    active_downloads: i32,
    completed_articles: i32,
    has_post_info: bool,
    same_nzb: bool,
) -> i32 {
    if active_downloads > 0 || completed_articles > 0 {
        1
    } else if has_post_info {
        2
    } else if !same_nzb {
        3
    } else {
        0
    }
}

/// Counter bundle for `QueueCoordinator::MergeQueueEntries` and `QueueCoordinator::SplitQueueEntries`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NzbgetRsNzbMergeSplitCounters {
    pub file_count: i32,
    pub active_downloads: i32,
    pub full_content_hash: u32,
    pub filtered_content_hash: u32,
    pub size: i64,
    pub remaining_size: i64,
    pub paused_file_count: i32,
    pub paused_size: i64,
    pub success_size: i64,
    pub current_success_size: i64,
    pub failed_size: i64,
    pub current_failed_size: i64,
    pub par_size: i64,
    pub par_success_size: i64,
    pub par_current_success_size: i64,
    pub par_failed_size: i64,
    pub par_current_failed_size: i64,
    pub remaining_par_count: i32,
    pub total_articles: i32,
    pub success_articles: i32,
    pub failed_articles: i32,
    pub current_success_articles: i32,
    pub current_failed_articles: i32,
    pub min_time: i64,
    pub max_time: i64,
    pub downloaded_size: i64,
    pub download_sec: i32,
    pub download_start_time: i64,
}

/// File counter snapshot for `QueueCoordinator::SplitQueueEntries`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NzbgetRsSplitFileCounters {
    pub size: i64,
    pub remaining_size: i64,
    pub success_size: i64,
    pub failed_size: i64,
    pub missed_size: i64,
    pub total_articles: i32,
    pub missed_articles: i32,
    pub success_articles: i32,
    pub failed_articles: i32,
    pub par_file: i32,
    pub paused: i32,
}

/// Accumulates `src` counters into `dest` for `QueueCoordinator::MergeQueueEntries`.
pub fn merge_counters(
    dest: &mut NzbgetRsNzbMergeSplitCounters,
    src: &NzbgetRsNzbMergeSplitCounters,
) {
    dest.file_count = dest.file_count.wrapping_add(src.file_count);
    dest.active_downloads = dest.active_downloads.wrapping_add(src.active_downloads);
    dest.full_content_hash = 0;
    dest.filtered_content_hash = 0;

    dest.size = dest.size.wrapping_add(src.size);
    dest.remaining_size = dest.remaining_size.wrapping_add(src.remaining_size);
    dest.paused_file_count = dest.paused_file_count.wrapping_add(src.paused_file_count);
    dest.paused_size = dest.paused_size.wrapping_add(src.paused_size);

    dest.success_size = dest.success_size.wrapping_add(src.success_size);
    dest.current_success_size = dest
        .current_success_size
        .wrapping_add(src.current_success_size);
    dest.failed_size = dest.failed_size.wrapping_add(src.failed_size);
    dest.current_failed_size = dest
        .current_failed_size
        .wrapping_add(src.current_failed_size);

    dest.par_size = dest.par_size.wrapping_add(src.par_size);
    dest.par_success_size = dest.par_success_size.wrapping_add(src.par_success_size);
    dest.par_current_success_size = dest
        .par_current_success_size
        .wrapping_add(src.par_current_success_size);
    dest.par_failed_size = dest.par_failed_size.wrapping_add(src.par_failed_size);
    dest.par_current_failed_size = dest
        .par_current_failed_size
        .wrapping_add(src.par_current_failed_size);
    dest.remaining_par_count = dest
        .remaining_par_count
        .wrapping_add(src.remaining_par_count);

    dest.total_articles = dest.total_articles.wrapping_add(src.total_articles);
    dest.success_articles = dest.success_articles.wrapping_add(src.success_articles);
    dest.failed_articles = dest.failed_articles.wrapping_add(src.failed_articles);
    dest.current_success_articles = dest
        .current_success_articles
        .wrapping_add(src.current_success_articles);
    dest.current_failed_articles = dest
        .current_failed_articles
        .wrapping_add(src.current_failed_articles);

    if src.min_time < dest.min_time {
        dest.min_time = src.min_time;
    }
    if src.max_time > dest.max_time {
        dest.max_time = src.max_time;
    }

    dest.downloaded_size = dest.downloaded_size.wrapping_add(src.downloaded_size);
    dest.download_sec = dest.download_sec.wrapping_add(src.download_sec);
    if !((dest.download_start_time > 0 && dest.download_start_time < src.download_start_time)
        || src.download_start_time == 0)
    {
        dest.download_start_time = src.download_start_time;
    }
}

/// Transfers one file's counters from `src` to `dst` in `QueueCoordinator::SplitQueueEntries`.
pub fn split_transfer_file(
    src: &mut NzbgetRsNzbMergeSplitCounters,
    dst: &mut NzbgetRsNzbMergeSplitCounters,
    f: &NzbgetRsSplitFileCounters,
) {
    let failed_and_missed_size = f.failed_size.wrapping_add(f.missed_size);
    let failed_and_missed_articles = f.failed_articles.wrapping_add(f.missed_articles);

    src.file_count = src.file_count.wrapping_sub(1);
    src.size = src.size.wrapping_sub(f.size);
    src.remaining_size = src.remaining_size.wrapping_sub(f.remaining_size);
    src.current_success_size = src.current_success_size.wrapping_sub(f.success_size);
    src.current_failed_size = src.current_failed_size.wrapping_sub(failed_and_missed_size);
    src.total_articles = src.total_articles.wrapping_sub(f.total_articles);
    src.failed_articles = src.failed_articles.wrapping_sub(f.missed_articles);
    src.current_success_articles = src
        .current_success_articles
        .wrapping_sub(f.success_articles);
    src.current_failed_articles = src
        .current_failed_articles
        .wrapping_sub(failed_and_missed_articles);

    dst.file_count = dst.file_count.wrapping_add(1);
    dst.size = dst.size.wrapping_add(f.size);
    dst.remaining_size = dst.remaining_size.wrapping_add(f.remaining_size);
    dst.current_success_size = dst.current_success_size.wrapping_add(f.success_size);
    dst.current_failed_size = dst.current_failed_size.wrapping_add(failed_and_missed_size);
    dst.total_articles = dst.total_articles.wrapping_add(f.total_articles);
    dst.failed_articles = dst.failed_articles.wrapping_add(f.missed_articles);
    dst.current_success_articles = dst
        .current_success_articles
        .wrapping_add(f.success_articles);
    dst.current_failed_articles = dst
        .current_failed_articles
        .wrapping_add(failed_and_missed_articles);

    if f.par_file != 0 {
        src.par_size = src.par_size.wrapping_sub(f.size);
        src.par_current_success_size = src.par_current_success_size.wrapping_sub(f.success_size);
        src.par_current_failed_size = src
            .par_current_failed_size
            .wrapping_sub(failed_and_missed_size);
        src.remaining_par_count = src.remaining_par_count.wrapping_sub(1);

        dst.par_size = dst.par_size.wrapping_add(f.size);
        dst.par_current_success_size = dst.par_current_success_size.wrapping_add(f.success_size);
        dst.par_current_failed_size = dst
            .par_current_failed_size
            .wrapping_add(failed_and_missed_size);
        dst.remaining_par_count = dst.remaining_par_count.wrapping_add(1);
    }

    if f.paused != 0 {
        src.paused_file_count = src.paused_file_count.wrapping_sub(1);
        src.paused_size = src.paused_size.wrapping_sub(f.remaining_size);

        dst.paused_file_count = dst.paused_file_count.wrapping_add(1);
        dst.paused_size = dst.paused_size.wrapping_add(f.remaining_size);
    }
}

/// Computes the updated NZB and file counters when discarding downloaded articles
/// in `QueueCoordinator::DiscardDownloadedArticles`.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn discard_downloaded_articles_counters(
    nzb_rem_size: i64,
    nzb_paused_size: i64,
    nzb_cur_succ_art: i32,
    nzb_cur_succ_size: i64,
    nzb_par_cur_succ_size: i64,
    nzb_cur_fail_art: i32,
    nzb_cur_fail_size: i64,
    nzb_par_cur_fail_size: i64,
    file_paused: i32,
    file_succ_art: i32,
    file_succ_size: i64,
    file_fail_art: i32,
    file_fail_size: i64,
    file_size: i64,
    file_missed_size: i64,
) -> (i64, i64, i32, i64, i64, i32, i64, i64, i64) {
    let delta_size = file_succ_size.wrapping_add(file_fail_size);
    let next_nzb_rem_size = nzb_rem_size.wrapping_add(delta_size);
    let next_nzb_paused_size = if file_paused != 0 {
        nzb_paused_size.wrapping_add(delta_size)
    } else {
        nzb_paused_size
    };
    let next_nzb_cur_succ_art = nzb_cur_succ_art.wrapping_sub(file_succ_art);
    let next_nzb_cur_succ_size = nzb_cur_succ_size.wrapping_sub(file_succ_size);
    let next_nzb_par_cur_succ_size = nzb_par_cur_succ_size.wrapping_sub(file_succ_size);
    let next_nzb_cur_fail_art = nzb_cur_fail_art.wrapping_sub(file_fail_art);
    let next_nzb_cur_fail_size = nzb_cur_fail_size.wrapping_sub(file_fail_size);
    let next_nzb_par_cur_fail_size = nzb_par_cur_fail_size.wrapping_sub(file_fail_size);
    let next_file_rem_size = file_size.wrapping_sub(file_missed_size);
    (
        next_nzb_rem_size,
        next_nzb_paused_size,
        next_nzb_cur_succ_art,
        next_nzb_cur_succ_size,
        next_nzb_par_cur_succ_size,
        next_nzb_cur_fail_art,
        next_nzb_cur_fail_size,
        next_nzb_par_cur_fail_size,
        next_file_rem_size,
    )
}

/// Helper to convert a `*const c_char` into `&[u8]`.
///
/// # Safety
/// `ptr` must be null or a valid null-terminated C string.
pub unsafe fn cstr_to_bytes<'a>(ptr: *const c_char) -> &'a [u8] {
    if ptr.is_null() {
        &[]
    } else {
        unsafe { CStr::from_ptr(ptr).to_bytes() }
    }
}

#[cfg(unix)]
const DUPE_FAILOVER_SAMPLE: i32 = 32;
#[cfg(unix)]
const BACKUP_LEAD: i32 = 100;
#[cfg(unix)]
const DUPE_PROBE_SAMPLE_COUNT: i32 = 10;
#[cfg(unix)]
const DUPE_PROBE_MIN_ARTICLES: usize = 4;
#[cfg(unix)]
const DUPE_PROBE_MIN_ALIVE_SAMPLES: i32 = 2;

#[cfg(unix)]
pub struct NzbgetRsQueueCoordinator {
    downloads_limit: std::sync::atomic::AtomicI32,
    server_config_generation: std::sync::atomic::AtomicI32,
    propagation_wait_logged_id: std::sync::atomic::AtomicI32,
    donor_cache: std::sync::Mutex<crate::dupefallback::DonorCacheState>,
    par_loaders_stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    par_loaders: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>>,
    pub(crate) stopped: std::sync::atomic::AtomicBool,
    has_more_jobs: std::sync::atomic::AtomicBool,
    state_changed: std::sync::atomic::AtomicBool,
    mass_edit: std::sync::atomic::AtomicBool,
    want_save: std::sync::atomic::AtomicBool,
    history_changed: std::sync::atomic::AtomicBool,
    wait_lock: (std::sync::Mutex<bool>, std::sync::Condvar),
    active_downloads:
        std::sync::Mutex<Vec<std::sync::Arc<crate::articledl::NzbgetRsArticleDownloader>>>,
    active_dl_threads: (std::sync::Mutex<usize>, std::sync::Condvar),
}

#[cfg(unix)]
impl Drop for NzbgetRsQueueCoordinator {
    fn drop(&mut self) {
        self.par_loaders_stopped
            .store(true, std::sync::atomic::Ordering::Release);
        let handles: Vec<_> = self
            .par_loaders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        for h in handles {
            let _ = h.join();
        }
    }
}

#[cfg(unix)]
impl Default for NzbgetRsQueueCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(unix)]
fn snapshot_file_articles(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    file_ptr: usize,
    len: usize,
) -> Vec<crate::queuesubsys_host::ArticleSnap> {
    let mut v = Vec::with_capacity(len);
    for i in 0..len {
        v.push(host.article_at(file_ptr, i));
    }
    v
}

#[cfg(unix)]
fn pack_merge_split_counters(
    n: &crate::queuesubsys_host::NzbSnap,
) -> NzbgetRsNzbMergeSplitCounters {
    NzbgetRsNzbMergeSplitCounters {
        file_count: n.file_count,
        active_downloads: n.active_downloads,
        full_content_hash: n.full_content_hash,
        filtered_content_hash: n.filtered_content_hash,
        size: n.size,
        remaining_size: n.remaining_size,
        paused_file_count: n.paused_file_count,
        paused_size: n.paused_size,
        success_size: n.success_size,
        current_success_size: n.current_success_size,
        failed_size: n.failed_size,
        current_failed_size: n.current_failed_size,
        par_size: n.par_size,
        par_success_size: n.par_success_size,
        par_current_success_size: n.par_current_success_size,
        par_failed_size: n.par_failed_size,
        par_current_failed_size: n.par_current_failed_size,
        remaining_par_count: n.remaining_par_count,
        total_articles: n.total_articles,
        success_articles: n.success_articles,
        failed_articles: n.failed_articles,
        current_success_articles: n.current_success_articles,
        current_failed_articles: n.current_failed_articles,
        min_time: n.min_time,
        max_time: n.max_time,
        downloaded_size: n.downloaded_size,
        download_sec: n.download_sec,
        download_start_time: n.download_start_time,
    }
}

#[cfg(unix)]
fn join_path(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dir.len() + 1 + name.len());
    out.extend_from_slice(dir);
    out.push(crate::paths::PATH_SEPARATOR);
    out.extend_from_slice(name);
    out
}

#[cfg(unix)]
fn backup_alive_permille(host: &crate::queuesubsys_host::QueueSubsysHost, nzb_ptr: usize) -> i32 {
    let mut best = -1i32;
    for key in [b"DupeAlive".as_slice(), b"DupeHealth".as_slice()] {
        if let Some(val) = host.nzb_get_param(nzb_ptr, key) {
            best = best.max(crate::dupecoord::parse_alive_permille_bytes(&val));
        }
    }
    best
}

#[cfg(unix)]
impl NzbgetRsQueueCoordinator {
    pub fn new() -> Self {
        Self {
            downloads_limit: std::sync::atomic::AtomicI32::new(0),
            server_config_generation: std::sync::atomic::AtomicI32::new(0),
            propagation_wait_logged_id: std::sync::atomic::AtomicI32::new(0),
            donor_cache: std::sync::Mutex::new(crate::dupefallback::DonorCacheState::default()),
            par_loaders_stopped: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            par_loaders: std::sync::Mutex::new(Vec::new()),
            stopped: std::sync::atomic::AtomicBool::new(false),
            has_more_jobs: std::sync::atomic::AtomicBool::new(true),
            state_changed: std::sync::atomic::AtomicBool::new(false),
            mass_edit: std::sync::atomic::AtomicBool::new(false),
            want_save: std::sync::atomic::AtomicBool::new(false),
            history_changed: std::sync::atomic::AtomicBool::new(false),
            wait_lock: (std::sync::Mutex::new(false), std::sync::Condvar::new()),
            active_downloads: std::sync::Mutex::new(Vec::new()),
            active_dl_threads: (std::sync::Mutex::new(0), std::sync::Condvar::new()),
        }
    }

    pub(crate) fn spawn_par_loader(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_id: i32,
        par_files: Vec<Vec<u8>>,
    ) {
        let mut guard = self.par_loaders.lock().unwrap_or_else(|e| e.into_inner());
        let mut alive = Vec::with_capacity(guard.len() + 1);
        for h in guard.drain(..) {
            if h.is_finished() {
                let _ = h.join();
            } else {
                alive.push(h);
            }
        }
        let ctx_addr = host.ctx as usize;
        let cb = host.cb;
        let stopped = std::sync::Arc::clone(&self.par_loaders_stopped);
        if let Ok(handle) = std::thread::Builder::new().spawn(move || {
            let loader_host = crate::queuesubsys_host::QueueSubsysHost::new(
                ctx_addr as *mut std::ffi::c_void,
                cb,
            );
            crate::directrenamer::run_par_loader(loader_host, &stopped, nzb_id, par_files);
        }) {
            alive.push(handle);
        }
        *guard = alive;
    }

    fn with_donor_cache<R>(
        &self,
        f: impl FnOnce(&mut crate::dupefallback::DonorCacheState) -> R,
    ) -> R {
        let mut guard = self.donor_cache.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    pub(crate) fn init(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        self.par_loaders_stopped
            .store(false, std::sync::atomic::Ordering::Release);
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Creating QueueCoordinator",
        );
        host.coord_dq_init();
        crate::dupeprobe::probe_reset();
        crate::twin::reset_all();
    }

    pub(crate) fn destroy(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Destroying QueueCoordinator",
        );
        self.par_loaders_stopped
            .store(true, std::sync::atomic::Ordering::Release);
        let loader_handles: Vec<_> = self
            .par_loaders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        for h in loader_handles {
            let _ = h.join();
        }
        crate::dupeprobe::probe_wait_all();
        crate::twin::wait_all();
        host.delete_all_active_downloads();
        host.coord_dq_final();
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"QueueCoordinator destroyed",
        );
    }

    pub(crate) fn stop(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        self.par_loaders_stopped
            .store(true, std::sync::atomic::Ordering::Release);
        crate::dupeprobe::probe_stop_all();
        crate::twin::stop_all_jobs();
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Stopping ArticleDownloads",
        );
        host.with_locked_queue(|_| {
            let n = host.active_downloads_len();
            for i in 0..n {
                let ad = host.active_download_at(i);
                host.ad_action(ad.ad_ptr, crate::queuesubsys_host::AD_ACT_STOP, 0);
            }
        });
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"ArticleDownloads are notified",
        );
    }

    fn load(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.with_locked_queue(|dq_ptr| {
            let opts = host.opts();
            let mut stat_loaded = true;
            let mut perfect_server_match = true;
            let mut queue_loaded = false;

            if opts.server_mode {
                let (sl, pm) = host.stat_meter_load();
                stat_loaded = sl;
                perfect_server_match = pm;
                if host.disk_queue_exists() {
                    queue_loaded = host.disk_load_queue(dq_ptr);
                } else {
                    host.disk_discard_queue();
                }
            }

            if queue_loaded && stat_loaded {
                host.disk_cleanup_temp_dir(dq_ptr);
            }

            if queue_loaded && host.disk_legacy_queue_read() {
                host.log(
                    crate::queuesubsys_host::MK_INFO,
                    b"Saving the queue and history of an earlier build in the upstream format",
                );
                host.dq_history_changed(dq_ptr);
                host.dq_save(dq_ptr);
            }

            if queue_loaded && stat_loaded && !perfect_server_match {
                host.log(
                    crate::queuesubsys_host::MK_DEBUG,
                    b"Changes in section <NEWS SERVERS> of config file detected, resaving queue",
                );
                host.stat_meter_save();
                host.dq_history_changed(dq_ptr);
                host.dq_save(dq_ptr);

                if opts.server_mode {
                    let qlen = host.queue_len(dq_ptr);
                    for i in 0..qlen {
                        let nzb_ptr = host.queue_at(dq_ptr, i);
                        if opts.continue_partial {
                            let flen = host.nzb_file_len(nzb_ptr);
                            for j in 0..flen {
                                let fp = host.nzb_file_at(nzb_ptr, j);
                                let f = host.file_snap(fp);
                                if f.articles_len > 0 {
                                    host.disk_save_file_state(fp, false);
                                }
                            }
                        }
                        for cf in host.nzb_completed_files(nzb_ptr) {
                            if (cf.status == crate::queuesubsys_host::CF_PARTIAL
                                || cf.status == crate::queuesubsys_host::CF_FAILURE)
                                && cf.id > 0
                            {
                                host.disk_resave_completed_file_state(cf.id);
                            }
                        }
                    }
                }
            }

            host.coord_dq_loaded();
        });
    }

    fn adjust_downloads_limit(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        let cur_gen = host.server_pool_generation();
        if self
            .server_config_generation
            .load(std::sync::atomic::Ordering::Relaxed)
            == cur_gen
        {
            return;
        }
        let (levels, actives, conns) = host.server_pool_servers();
        let limit = calc_downloads_limit(&levels, &actives, &conns);
        self.downloads_limit
            .store(limit, std::sync::atomic::Ordering::Relaxed);
        self.server_config_generation
            .store(cur_gen, std::sync::atomic::Ordering::Relaxed);
    }

    fn wait_jobs(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"QueueCoordinator: waiting for Downloads to complete",
        );
        loop {
            let mut empty = false;
            host.with_locked_queue(|_| {
                empty = host.active_downloads_len() == 0;
            });
            if empty {
                break;
            }
            host.sleep_ms(100);
            self.reset_hanging_downloads(host);
        }
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"QueueCoordinator: Downloads are completed",
        );
    }

    pub(crate) fn run(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Entering QueueCoordinator-loop",
        );
        self.load(host);
        self.adjust_downloads_limit(host);
        let mut was_standby = true;
        let mut article_downloads_running = false;
        let mut last_reset: i64 = 0;
        host.stat_meter_interval_check();
        let mut wait_interval: i32 = 100;

        while !host.self_is_stopped() {
            let mut downloads_checked = false;
            let mut download_started = false;

            host.with_locked_queue(|dq_ptr| {
                let opts = host.opts();
                let next_pair = self.get_next_article(host, &opts, dq_ptr);
                let has_more_articles = next_pair.is_some();
                let active_len = host.active_downloads_len();
                article_downloads_running = active_len > 0;
                host.self_set_has_more_jobs(has_more_articles || article_downloads_running);

                let dl_limit = self
                    .downloads_limit
                    .load(std::sync::atomic::Ordering::Relaxed);
                let effective_limit = host.effective_downloads_limit(dl_limit);
                let can_proceed = !host.self_is_stopped() && (active_len as i32) < effective_limit;

                if let Some((file, art)) = next_pair {
                    if can_proceed && (!opts.download_held || file.extra_priority) {
                        let nzb = host.nzb_snap(file.nzb_ptr, false);
                        let conn_ptr = host.server_pool_get_connection(
                            nzb.has_desired_server,
                            nzb.desired_server_id,
                        );
                        if conn_ptr != 0 {
                            downloads_checked = true;
                            article_downloads_running = true;
                            download_started = true;
                            self.start_dead_pick_probe(host, &opts, dq_ptr, &nzb);
                            self.start_article_download(host, &opts, &nzb, &file, &art, conn_ptr);
                        } else if nzb.has_desired_server {
                            let mut dbg = b"Could not start download for ".to_vec();
                            dbg.extend_from_slice(&nzb.name);
                            dbg.extend_from_slice(
                                format!(
                                    ": desired server {} has no available connections",
                                    nzb.desired_server_id
                                )
                                .as_bytes(),
                            );
                            host.log(crate::queuesubsys_host::MK_DEBUG, &dbg);
                        }
                    }
                }
            });

            if !downloads_checked {
                host.with_locked_queue(|_| {
                    article_downloads_running = host.active_downloads_len() > 0;
                });
            }

            let standby = !article_downloads_running;
            if standby != was_standby {
                host.stat_meter_enter_leave_standby(standby);
                host.work_state_set_downloading(!standby);
                was_standby = standby;
                if standby {
                    self.save_all_partial_state(host);
                }
            }

            if standby {
                wait_interval = (wait_interval * 2).min(2000);
                host.self_standby_wait(wait_interval);
            } else {
                let sleep_interval = if download_started { 0 } else { 5 };
                host.sleep_ms(sleep_interval);
                host.stat_meter_add_zero_speed();
                wait_interval = 100;
            }

            let now = host.opts().cur_time;
            if last_reset != now {
                host.server_pool_close_unused();
                self.reset_hanging_downloads(host);
                if !standby {
                    self.save_all_partial_state(host);
                }
                host.stat_meter_interval_check();
                host.log_interval_check();
                self.adjust_downloads_limit(host);
                host.set_standby_mode(standby);
                last_reset = host.opts().cur_time;
            }
        }

        self.wait_jobs(host);
        self.save_all_partial_state(host);
        self.save_queue_if_changed(host);
        self.save_all_file_state(host);

        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Exiting QueueCoordinator-loop",
        );
    }

    pub(crate) fn add_nzb_file_to_queue(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        uptr_addr: usize,
        url_ptr: usize,
        add_first: bool,
    ) -> usize {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Adding NZBFile to queue",
        );
        let mut out_ptr = 0usize;
        host.with_locked_queue(|dq_ptr| {
            let opts = host.opts();
            // Read raw NzbInfo* from std::unique_ptr<NzbInfo>* at uptr_addr
            let raw_nzb_ptr = unsafe { *(uptr_addr as *const usize) };
            let mut added_nzb_ptr = raw_nzb_ptr;

            host.dq_notify(
                dq_ptr,
                crate::queuesubsys_host::ASPECT_NZB_FOUND,
                added_nzb_ptr,
                0,
            );

            let nzb = host.nzb_snap(added_nzb_ptr, false);
            let delete_status = nzb.delete_status;

            if delete_status != crate::queuesubsys_host::DS_NONE {
                let flen = host.nzb_file_len(added_nzb_ptr);
                let mut all_paused = flen > 0;
                for i in 0..flen {
                    let fp = host.nzb_file_at(added_nzb_ptr, i);
                    let f = host.file_snap(fp);
                    all_paused &= f.paused;
                    if opts.server_mode {
                        host.disk_discard_file(f.id, true, false, false);
                    }
                }
                host.nzb_set_int(
                    added_nzb_ptr,
                    crate::queuesubsys_host::NZB_INT_DELETE_PAUSED,
                    i32::from(all_paused),
                );
            }

            if delete_status == crate::queuesubsys_host::DS_NONE {
                if opts.dupe_check && nzb.dupe_mode != crate::queuesubsys_host::DM_FORCE {
                    self.check_dupe_file_infos(host, added_nzb_ptr);
                }
                host.queue_insert_owned(dq_ptr, uptr_addr, url_ptr, add_first);
            } else {
                host.queue_insert_owned(dq_ptr, uptr_addr, 0, true);
            }

            if url_ptr != 0 {
                let url_snap = host.nzb_snap(url_ptr, false);
                host.nzb_set_int(
                    added_nzb_ptr,
                    crate::queuesubsys_host::NZB_INT_ID,
                    url_snap.id,
                );
                host.queue_remove(dq_ptr, url_ptr);
            }

            let cur_nzb = host.nzb_snap(added_nzb_ptr, false);
            if delete_status == crate::queuesubsys_host::DS_NONE {
                let mut msg = b"Collection ".to_vec();
                msg.extend_from_slice(&cur_nzb.name);
                msg.extend_from_slice(b" added to queue");
                host.nzb_print_message(added_nzb_ptr, crate::queuesubsys_host::MK_INFO, &msg);
            }

            if delete_status != crate::queuesubsys_host::DS_MANUAL {
                host.dq_notify(
                    dq_ptr,
                    crate::queuesubsys_host::ASPECT_NZB_ADDED,
                    added_nzb_ptr,
                    0,
                );
            }

            if delete_status != crate::queuesubsys_host::DS_NONE {
                let check_id = cur_nzb.id;
                host.queue_remove(dq_ptr, added_nzb_ptr);
                if host.history_find_by_id(dq_ptr, check_id) == 0 {
                    added_nzb_ptr = 0;
                }
            }

            host.dq_save(dq_ptr);
            out_ptr = added_nzb_ptr;
        });
        out_ptr
    }

    pub(crate) fn check_dupe_file_infos(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_ptr: usize,
    ) {
        host.log(crate::queuesubsys_host::MK_DEBUG, b"CheckDupeFileInfos");
        if nzb_ptr == 0 {
            return;
        }
        let flen = host.nzb_file_len(nzb_ptr);
        if flen == 0 {
            return;
        }
        let opts = host.opts();
        let mut snaps = Vec::with_capacity(flen);
        let mut sizes = Vec::with_capacity(flen);
        for i in 0..flen {
            let f = host.file_snap(host.nzb_file_at(nzb_ptr, i));
            sizes.push(f.size);
            snaps.push(f);
        }
        let names: Vec<&[u8]> = snaps.iter().map(|f| f.filename.as_slice()).collect();
        let mut dupes = vec![0usize; flen];
        let count = find_dupe_file_indices(&names, &sizes, &mut dupes);
        for &idx in &dupes[..count] {
            let f = &snaps[idx];
            let mut msg = b"File \"".to_vec();
            msg.extend_from_slice(&f.filename);
            msg.extend_from_slice(b"\" appears twice in collection, adding only the biggest file");
            host.log(crate::queuesubsys_host::MK_WARNING, &msg);
            host.nzb_action(
                nzb_ptr,
                crate::queuesubsys_host::NZB_ACT_UPDATE_DELETED_STATS,
                f.ptr,
            );
            host.nzb_remove_file(nzb_ptr, f.ptr);
            if opts.server_mode {
                host.disk_discard_file(f.id, true, false, false);
            }
        }
    }

    fn discard_temp_files(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        file: &crate::queuesubsys_host::FileSnap,
    ) {
        discard_temp_files_host(host, opts, file);
    }

    pub(crate) fn fail_over_dead_pick(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_id: i32,
        samples: i32,
        missing_servers: i32,
        active_servers: i32,
        recovered_at_start: i32,
    ) {
        let nzb_ptr = host.queue_find_by_id(dq_ptr, nzb_id);
        if nzb_ptr == 0 {
            return;
        }
        let nzb = host.nzb_snap(nzb_ptr, false);
        if nzb.deleting || nzb.parking || nzb.delete_status != crate::queuesubsys_host::DS_NONE {
            return;
        }
        let own_success = nzb.current_success_articles
            - (nzb.dupe_recovered_articles - recovered_at_start).max(0);
        if own_success >= DUPE_PROBE_MIN_ALIVE_SAMPLES {
            return;
        }
        let opts = host.opts();
        let backup = if crate::dupecoord::fails_over_snap(&opts, &nzb) {
            crate::dupecoord::find_dupe_backup_host(
                host,
                &opts,
                dq_ptr,
                nzb.ptr,
                &nzb.name,
                &nzb.dupe_key,
            )
        } else {
            None
        };
        let Some((_, backup_nzb)) = backup else {
            return;
        };
        if !crate::dupecoord::dupe_failover_warranted_inline(
            nzb.dupe_score,
            0,
            backup_nzb.dupe_score,
        ) {
            return;
        }
        let mut msg = b"Failing over ".to_vec();
        msg.extend_from_slice(&nzb.name);
        msg.extend_from_slice(b" to duplicate ");
        msg.extend_from_slice(&backup_nzb.name);
        msg.extend_from_slice(
            format!(
                ": none of {} sampled articles exists on any server ({} of {} servers answered definitively)",
                samples, missing_servers, active_servers
            )
            .as_bytes(),
        );
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
            crate::queuesubsys_host::DS_HEALTH,
        );
        host.dq_edit_entry(
            dq_ptr,
            nzb.id,
            crate::queuesubsys_host::EA_GROUP_PARK_DELETE,
            None,
        );
    }

    pub(crate) fn delete_queue_entry(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        file_ptr: usize,
    ) -> bool {
        host.file_set_int(file_ptr, crate::queuesubsys_host::FILE_INT_DELETED, 1);
        let mut downloading = false;
        let n = host.active_downloads_len();
        for i in 0..n {
            let ad = host.active_download_at(i);
            if ad.file_ptr == file_ptr {
                downloading = true;
                host.ad_action(ad.ad_ptr, crate::queuesubsys_host::AD_ACT_STOP, 0);
            }
        }
        if !downloading {
            self.delete_file_info(host, dq_ptr, file_ptr, false);
        }
        downloading
    }

    pub(crate) fn set_queue_entry_category(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_ptr: usize,
        category: &[u8],
    ) -> bool {
        let nzb = host.nzb_snap(nzb_ptr, false);
        if nzb.has_post_info {
            let mut msg = b"Could not change category for ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b". File in post-process-stage");
            host.log(crate::queuesubsys_host::MK_ERROR, &msg);
            return false;
        }
        let old_dest_dir = nzb.dest_dir;
        host.nzb_set_str(nzb_ptr, crate::queuesubsys_host::NZB_STR_CATEGORY, category);
        host.nzb_action(nzb_ptr, crate::queuesubsys_host::NZB_ACT_BUILD_DEST_DIR, 0);
        let new_nzb = host.nzb_snap(nzb_ptr, false);
        new_nzb.dest_dir == old_dest_dir || host.move_completed_files(nzb_ptr, &old_dest_dir)
    }

    pub(crate) fn set_queue_entry_name(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_ptr: usize,
        name: &[u8],
    ) -> bool {
        let nzb = host.nzb_snap(nzb_ptr, false);
        if nzb.has_post_info {
            let mut msg = b"Could not rename ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b". File in post-process-stage");
            host.log(crate::queuesubsys_host::MK_ERROR, &msg);
            return false;
        }
        if name.is_empty() {
            let mut msg = b"Could not rename ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b". The new name cannot be empty");
            host.log(crate::queuesubsys_host::MK_ERROR, &msg);
            return false;
        }
        let nice = crate::downloadinfo::make_nice_nzb_name(name, false);
        host.nzb_set_str(nzb_ptr, crate::queuesubsys_host::NZB_STR_NAME, &nice);

        if nzb.kind == crate::queuesubsys_host::NK_URL {
            let mut fn_buf = nice;
            fn_buf.extend_from_slice(b".nzb");
            host.nzb_set_str(nzb_ptr, crate::queuesubsys_host::NZB_STR_FILENAME, &fn_buf);
            return true;
        }

        let old_dest_dir = nzb.dest_dir;
        host.nzb_action(nzb_ptr, crate::queuesubsys_host::NZB_ACT_BUILD_DEST_DIR, 0);
        let new_nzb = host.nzb_snap(nzb_ptr, false);
        new_nzb.dest_dir == old_dest_dir || host.move_completed_files(nzb_ptr, &old_dest_dir)
    }

    pub(crate) fn merge_queue_entries(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        dest_ptr: usize,
        src_ptr: usize,
    ) -> bool {
        let dest = host.nzb_snap(dest_ptr, false);
        let src = host.nzb_snap(src_ptr, false);
        if dest.has_post_info || src.has_post_info {
            let mut msg = b"Could not merge ".to_vec();
            msg.extend_from_slice(&dest.name);
            msg.extend_from_slice(b" and ");
            msg.extend_from_slice(&src.name);
            msg.extend_from_slice(b". File in post-process-stage");
            host.log(crate::queuesubsys_host::MK_ERROR, &msg);
            return false;
        }
        if dest.kind == crate::queuesubsys_host::NK_URL
            || src.kind == crate::queuesubsys_host::NK_URL
        {
            let mut msg = b"Could not merge ".to_vec();
            msg.extend_from_slice(&dest.name);
            msg.extend_from_slice(b" and ");
            msg.extend_from_slice(&src.name);
            msg.extend_from_slice(b". URLs cannot be merged");
            host.log(crate::queuesubsys_host::MK_ERROR, &msg);
            return false;
        }

        let old_dest_dir = src.dest_dir.clone();
        host.nzb_set_str(
            src_ptr,
            crate::queuesubsys_host::NZB_STR_CATEGORY,
            &dest.category,
        );
        host.nzb_set_str(
            src_ptr,
            crate::queuesubsys_host::NZB_STR_DEST_DIR,
            &dest.dest_dir,
        );
        if old_dest_dir != dest.dest_dir {
            let _ = host.move_completed_files(src_ptr, &old_dest_dir);
        }

        let mut dc = pack_merge_split_counters(&dest);
        let sc = pack_merge_split_counters(&src);
        merge_counters(&mut dc, &sc);

        host.nzb_merge_lists_and_stats(dest_ptr, src_ptr);
        host.nzb_apply_merge_split_counters(dest_ptr, &dc);

        let mut merged_qf = dest.queued_filename;
        merged_qf.push(b'|');
        merged_qf.extend_from_slice(&src.queued_filename);
        host.nzb_set_str(
            dest_ptr,
            crate::queuesubsys_host::NZB_STR_QUEUED_FILENAME,
            &merged_qf,
        );

        for param in [
            b"DupeFiles".as_slice(),
            b"DupeKind".as_slice(),
            b"DupeSampled".as_slice(),
            b"DupeSampledOf".as_slice(),
        ] {
            host.nzb_set_param(dest_ptr, param, None);
        }

        host.disk_discard_files(src_ptr);
        crate::dupefallback::par_damage_forget_all(src.id);
        host.queue_remove(dq_ptr, src_ptr);
        true
    }

    pub(crate) fn split_queue_entries(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        file_ptrs: &[usize],
        name: &[u8],
    ) -> bool {
        let mut src_nzb_ptr = 0usize;
        let mut file_snaps = Vec::with_capacity(file_ptrs.len());

        for &fp in file_ptrs {
            let f = host.file_snap(fp);
            let f_nzb = host.nzb_snap(f.nzb_ptr, false);
            let same_src = src_nzb_ptr == 0 || f.nzb_ptr == src_nzb_ptr;
            let v = validate_split_file_state(
                f.active_downloads,
                f.completed_articles,
                f_nzb.has_post_info,
                same_src,
            );
            if v != 0 {
                let mut msg = b"Could not split ".to_vec();
                msg.extend_from_slice(&f.filename);
                msg.extend_from_slice(if v == 1 {
                    b". File is already (partially) downloaded"
                } else if v == 2 {
                    b". File in post-process-stage"
                } else {
                    b". The files belong to different collections"
                });
                host.log(crate::queuesubsys_host::MK_ERROR, &msg);
                return false;
            }
            src_nzb_ptr = f.nzb_ptr;
            file_snaps.push(f);
        }

        if src_nzb_ptr == 0 {
            return false;
        }

        let src_snap = host.nzb_snap(src_nzb_ptr, false);
        let (uptr_addr, new_nzb_ptr) = host.alloc_new_nzb_uptr();

        host.nzb_set_str(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_STR_FILENAME,
            &src_snap.filename,
        );
        host.nzb_set_str(new_nzb_ptr, crate::queuesubsys_host::NZB_STR_NAME, name);
        host.nzb_set_str(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_STR_CATEGORY,
            &src_snap.category,
        );
        host.nzb_set_int(new_nzb_ptr, crate::queuesubsys_host::NZB_INT_FULL_HASH, 0);
        host.nzb_set_int(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_INT_FILTERED_HASH,
            0,
        );
        host.nzb_set_int(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_INT_PRIORITY,
            src_snap.priority,
        );
        host.nzb_action(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_ACT_BUILD_DEST_DIR,
            0,
        );
        host.nzb_set_str(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_STR_QUEUED_FILENAME,
            &src_snap.queued_filename,
        );
        host.nzb_copy_params(new_nzb_ptr, src_nzb_ptr);

        let new_init_snap = host.nzb_snap(new_nzb_ptr, false);
        let mut sc = pack_merge_split_counters(&src_snap);
        let mut nc = pack_merge_split_counters(&new_init_snap);
        sc.full_content_hash = 0;
        sc.filtered_content_hash = 0;

        for f in &file_snaps {
            host.dq_notify(
                dq_ptr,
                crate::queuesubsys_host::ASPECT_FILE_DELETED,
                src_nzb_ptr,
                f.ptr,
            );
            host.nzb_transfer_split_file(src_nzb_ptr, new_nzb_ptr, f.ptr);
            let fc = NzbgetRsSplitFileCounters {
                size: f.size,
                remaining_size: f.remaining_size,
                success_size: f.success_size,
                failed_size: f.failed_size,
                missed_size: f.missed_size,
                total_articles: f.total_articles,
                missed_articles: f.missed_articles,
                success_articles: f.success_articles,
                failed_articles: f.failed_articles,
                par_file: i32::from(f.par_file),
                paused: i32::from(f.paused),
            };
            split_transfer_file(&mut sc, &mut nc, &fc);
        }

        host.nzb_apply_merge_split_counters(src_nzb_ptr, &sc);
        host.nzb_apply_merge_split_counters(new_nzb_ptr, &nc);

        host.nzb_action(
            new_nzb_ptr,
            crate::queuesubsys_host::NZB_ACT_UPDATE_MIN_MAX_TIME,
            0,
        );
        if host.nzb_completed_files(src_nzb_ptr).is_empty() {
            host.nzb_action(
                src_nzb_ptr,
                crate::queuesubsys_host::NZB_ACT_UPDATE_MIN_MAX_TIME,
                0,
            );
        }

        if host.nzb_file_len(src_nzb_ptr) == 0 {
            host.disk_discard_files(src_nzb_ptr);
            crate::dupefallback::par_damage_forget_all(src_snap.id);
            host.queue_remove(dq_ptr, src_nzb_ptr);
        }

        host.finish_new_nzb_uptr(dq_ptr, uptr_addr);
        true
    }

    pub(crate) fn log_debug_info(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.with_locked_queue(|dq_ptr| {
            let opts = host.opts();
            host.log(crate::queuesubsys_host::MK_INFO, b"   ---------- Queue");
            let (remaining, remaining_forced) = host.dq_calc_remaining_size(dq_ptr);
            let rem_mb = (remaining as f64) / 1024.0 / 1024.0;
            let forced_mb = (remaining_forced as f64) / 1024.0 / 1024.0;
            host.log(
                crate::queuesubsys_host::MK_INFO,
                format!(
                    "     Remaining: {:.1} MB, Forced: {:.1} MB",
                    rem_mb, forced_mb
                )
                .as_bytes(),
            );
            let dl_state = if opts.pause_download {
                "paused"
            } else if opts.temp_pause_download {
                "temp-paused"
            } else {
                "active"
            };
            let pp_state = if opts.pause_post_process {
                "paused"
            } else {
                "active"
            };
            let sc_state = if opts.pause_scan { "paused" } else { "active" };
            host.log(
                crate::queuesubsys_host::MK_INFO,
                format!(
                    "     Download: {}, Post-process: {}, Scan: {}",
                    dl_state, pp_state, sc_state
                )
                .as_bytes(),
            );
            host.log(
                crate::queuesubsys_host::MK_INFO,
                b"   ---------- QueueCoordinator",
            );
            let active_n = host.active_downloads_len();
            let dl_limit = self
                .downloads_limit
                .load(std::sync::atomic::Ordering::Relaxed);
            host.log(
                crate::queuesubsys_host::MK_INFO,
                format!("    Active Downloads: {}, Limit: {}", active_n, dl_limit).as_bytes(),
            );
            for i in 0..active_n {
                let ad = host.active_download_at(i);
                host.ad_action(ad.ad_ptr, crate::queuesubsys_host::AD_ACT_LOG_DEBUG, 0);
            }
        });
    }

    fn get_next_article(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
    ) -> Option<(
        crate::queuesubsys_host::FileSnap,
        crate::queuesubsys_host::ArticleSnap,
    )> {
        let mut checked_files: Vec<usize> = Vec::new();
        let cur_date = opts.cur_time;

        loop {
            let mut best_file: Option<(
                crate::queuesubsys_host::NzbSnap,
                crate::queuesubsys_host::FileSnap,
            )> = None;
            let mut prop_wait_file: Option<(
                crate::queuesubsys_host::NzbSnap,
                crate::queuesubsys_host::FileSnap,
            )> = None;

            let qlen = host.queue_len(dq_ptr);
            for i in 0..qlen {
                let nzb_ptr = host.queue_at(dq_ptr, i);
                let nzb = host.nzb_snap(nzb_ptr, false);
                let flen = host.nzb_file_len(nzb_ptr);

                let nzb_higher = match &best_file {
                    None => true,
                    Some((best_nzb, _)) => {
                        if nzb.has_extra_priority == best_nzb.has_extra_priority {
                            nzb.priority > best_nzb.priority
                        } else {
                            nzb.has_extra_priority && !best_nzb.has_extra_priority
                        }
                    }
                };

                let nzb_paused = (flen as i32) == nzb.paused_file_count;

                if nzb_higher
                    && !nzb_paused
                    && !self.has_active_dupe(host, dq_ptr, &nzb)
                    && ((!opts.pause_download && !opts.quota_reached) || nzb.force_priority)
                {
                    let propagation_delay = opts.propagation_delay;
                    for j in 0..flen {
                        let fp = host.nzb_file_at(nzb_ptr, j);
                        if checked_files.contains(&fp) {
                            continue;
                        }
                        let f = host.file_snap(fp);
                        if f.paused || f.deleted {
                            continue;
                        }
                        let prop_wait = is_propagation_wait(f.time, propagation_delay, cur_date);
                        if !prop_wait {
                            let higher = match &best_file {
                                None => true,
                                Some((best_nzb, bf)) => {
                                    (f.extra_priority == bf.extra_priority
                                        && nzb.priority > best_nzb.priority)
                                        || (f.extra_priority && !bf.extra_priority)
                                }
                            };
                            if higher {
                                best_file = Some((nzb.clone(), f));
                            }
                        } else if prop_wait_file
                            .as_ref()
                            .is_none_or(|(_, pw)| f.time < pw.time)
                        {
                            prop_wait_file = Some((nzb.clone(), f));
                        }
                    }
                }
            }

            let Some((mut nzb, mut file)) = best_file else {
                if let Some((pw_nzb, pw_file)) = prop_wait_file {
                    let prev_logged = self
                        .propagation_wait_logged_id
                        .load(std::sync::atomic::Ordering::Relaxed);
                    if pw_file.id != prev_logged {
                        self.propagation_wait_logged_id
                            .store(pw_file.id, std::sync::atomic::Ordering::Relaxed);
                        let (posted_min, start_min) = calc_propagation_wait_log_mins(
                            pw_file.time,
                            opts.propagation_delay,
                            cur_date,
                        );
                        let mut msg = b"Holding ".to_vec();
                        msg.extend_from_slice(&pw_nzb.name);
                        msg.push(crate::paths::PATH_SEPARATOR);
                        msg.extend_from_slice(&pw_file.filename);
                        msg.extend_from_slice(
                            format!(
                                " due to PropagationDelay: posted {} min ago, download starts in {} min",
                                posted_min, start_min
                            )
                            .as_bytes(),
                        );
                        host.nzb_print_message(
                            pw_nzb.ptr,
                            crate::queuesubsys_host::MK_DETAIL,
                            &msg,
                        );
                    }
                }
                break;
            };

            if opts.direct_rename
                && nzb.direct_rename_status <= crate::queuesubsys_host::TS_RUNNING
                && !nzb.all_first
            {
                if let Some((f1, a1)) = self.get_next_first_article(host, opts, &nzb) {
                    self.propagation_wait_logged_id
                        .store(0, std::sync::atomic::Ordering::Relaxed);
                    return Some((f1, a1));
                }
            }

            if file.articles_len == 0 && opts.server_mode {
                host.disk_load_articles(file.ptr);
                self.load_partial_state(host, opts, &nzb, file.ptr);
                file = host.file_snap(file.ptr);
            }

            if let Some((idx, mut art)) = host.file_first_undefined_article(file.ptr) {
                if file.dupe_cutover && art.dupe_fallback_round == 0 {
                    art.dupe_proactive = true;
                    host.article_set_int(
                        art.ptr,
                        crate::queuesubsys_host::ART_INT_DUPE_PROACTIVE,
                        1,
                    );
                    let ok = self.with_donor_cache(|cache| {
                        crate::dupefallback::try_fallback_host(
                            cache, host, opts, dq_ptr, &mut nzb, &mut file, idx, &mut art,
                        )
                    });
                    art.dupe_proactive = ok;
                    host.article_set_int(
                        art.ptr,
                        crate::queuesubsys_host::ART_INT_DUPE_PROACTIVE,
                        i32::from(ok),
                    );
                }
                self.propagation_wait_logged_id
                    .store(0, std::sync::atomic::Ordering::Relaxed);
                return Some((file, art));
            }

            if checked_files.capacity() < 100 {
                checked_files.reserve(100);
            }
            checked_files.push(file.ptr);
        }

        None
    }

    fn get_next_first_article(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> Option<(
        crate::queuesubsys_host::FileSnap,
        crate::queuesubsys_host::ArticleSnap,
    )> {
        let flen = host.nzb_file_len(nzb.ptr);
        for i in 0..flen {
            let fp = host.nzb_file_at(nzb.ptr, i);
            let mut f = host.file_snap(fp);
            if !f.filename_confirmed {
                if f.articles_len == 0 && opts.server_mode {
                    host.disk_load_articles(fp);
                    self.load_partial_state(host, opts, nzb, fp);
                    f = host.file_snap(fp);
                }
                if f.articles_len > 0 {
                    let a0 = host.article_at(fp, 0);
                    if a0.status == crate::queuesubsys_host::AI_UNDEFINED {
                        host.nzb_set_int(
                            nzb.ptr,
                            crate::queuesubsys_host::NZB_INT_DIRECT_RENAME_STATUS,
                            crate::queuesubsys_host::TS_RUNNING,
                        );
                        return Some((f, a0));
                    }
                }
            }
        }
        host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_ALL_FIRST, 1);
        None
    }

    fn start_article_download(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        nzb: &crate::queuesubsys_host::NzbSnap,
        file: &crate::queuesubsys_host::FileSnap,
        art: &crate::queuesubsys_host::ArticleSnap,
        conn_ptr: usize,
    ) {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Starting new ArticleDownloader",
        );
        let attach_analyzer = art.part_number == 1
            && !opts.raw_article
            && (opts.direct_rename
                || opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE);

        let mut info_name = nzb.name.clone();
        info_name.push(crate::paths::PATH_SEPARATOR);
        info_name.extend_from_slice(&file.filename);
        info_name
            .extend_from_slice(format!(" [{}/{}]", art.part_number, file.articles_len).as_bytes());

        host.start_article_download(file.ptr, art.ptr, conn_ptr, attach_analyzer, &info_name);
    }

    fn unpause_streaming_par_volumes(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb_ptr: usize,
        nzb_id: i32,
    ) {
        loop {
            let ids = crate::parchecker::streaming_session_take_unpause_ids_bounded(nzb_id, 32);
            if ids.is_empty() {
                break;
            }
            let flen = host.nzb_file_len(nzb_ptr);
            for uid in ids {
                for i in 0..flen {
                    let qf_ptr = host.nzb_file_at(nzb_ptr, i);
                    let qf = host.file_snap(qf_ptr);
                    if (qf.id as u64) == uid && qf.paused && !qf.deleted {
                        host.file_set_int(qf_ptr, crate::queuesubsys_host::FILE_INT_PAUSED, 0);
                        host.file_set_int(
                            qf_ptr,
                            crate::queuesubsys_host::FILE_INT_EXTRA_PRIORITY,
                            1,
                        );
                        break;
                    }
                }
            }
        }
    }

    pub(crate) fn article_completed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        ad_ptr: usize,
    ) {
        host.log(crate::queuesubsys_host::MK_DEBUG, b"Article downloaded");
        let ad = host.ad_snap(ad_ptr);
        let file_ptr = ad.file_ptr;
        let mut complete_file_parts = false;

        host.with_locked_queue(|dq_ptr| {
            let opts = host.opts();
            let mut file = host.file_snap(file_ptr);
            let mut nzb = host.nzb_snap(file.nzb_ptr, false);
            let articles_len = file.articles_len;
            let Some((art_idx, mut art)) = host.file_find_article(file_ptr, ad.article_ptr) else {
                self.delete_downloader(host, host.self_download_queue_ptr(), ad_ptr, false);
                return;
            };

            let mut retry = false;
            let mut file_completed = false;

            let check_aligned = ad.status == crate::queuesubsys_host::AD_FINISHED
                && art.dupe_fallback_round > 0
                && ad.decoded_file_size > 0;
            let misplaced = check_aligned
                && !crate::dupefallback::segment_aligned_at(
                    host,
                    file_ptr,
                    articles_len,
                    art_idx,
                    &art,
                    file.decoded_file_size,
                );
            if misplaced {
                self.discard_article_segment(host, &mut file, &art);
                let mut msg = b"Discarding misaligned article ".to_vec();
                msg.extend_from_slice(&file.filename);
                msg.extend_from_slice(
                    format!(" [{}/{}] from duplicate", art.part_number, articles_len).as_bytes(),
                );
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
            }

            if ad.status == crate::queuesubsys_host::AD_FINISHED && !misplaced {
                art.status = crate::queuesubsys_host::AI_FINISHED;
                host.article_set_int(
                    art.ptr,
                    crate::queuesubsys_host::ART_INT_STATUS,
                    crate::queuesubsys_host::AI_FINISHED,
                );
                if art.dupe_fallback_round == 0
                    || (!art.dupe_original_message_id.is_empty()
                        && art.message_id == art.dupe_original_message_id)
                {
                    nzb.dupe_failed_run = 0;
                    host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_DUPE_FAILED_RUN, 0);
                }

                let (fs, ns, ps, fa, na) = apply_success_article_counters(
                    i64::from(art.size),
                    file.par_file,
                    file.success_size,
                    nzb.current_success_size,
                    nzb.par_current_success_size,
                    file.success_articles,
                    nzb.current_success_articles,
                );
                file.success_size = fs;
                nzb.current_success_size = ns;
                nzb.par_current_success_size = ps;
                file.success_articles = fa;
                nzb.current_success_articles = na;
                host.file_set_i64(file.ptr, crate::queuesubsys_host::FILE_I64_SUCCESS_SIZE, fs);
                host.nzb_set_i64(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_I64_CUR_SUCCESS_SIZE,
                    ns,
                );
                host.nzb_set_i64(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_I64_PAR_CUR_SUCCESS_SIZE,
                    ps,
                );
                host.file_set_int(
                    file.ptr,
                    crate::queuesubsys_host::FILE_INT_SUCCESS_ARTICLES,
                    fa,
                );
                host.nzb_set_int(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_INT_CUR_SUCCESS_ARTICLES,
                    na,
                );

                let previous_file_size = file.decoded_file_size;
                if crate::dupefallback::merge_decoded_file_size_host(
                    host,
                    &mut file,
                    ad.decoded_file_size,
                ) {
                    let mut msg = b"Articles of ".to_vec();
                    msg.extend_from_slice(&file.filename);
                    msg.extend_from_slice(
                        format!(
                            " declare different file sizes ({} vs {}); ignoring the declared size",
                            previous_file_size, ad.decoded_file_size
                        )
                        .as_bytes(),
                    );
                    host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
                }

                let (_, source_donors) = host.article_pinned_sources(art.ptr);
                let donor_id = select_dupe_donor_id(
                    art.dupe_fallback_round,
                    &source_donors,
                    &art.message_id,
                    &art.dupe_original_message_id,
                );
                art.dupe_donor_id = donor_id;
                host.article_set_int(
                    art.ptr,
                    crate::queuesubsys_host::ART_INT_DUPE_DONOR_ID,
                    donor_id,
                );

                let (rec, next_file_rec, next_nzb_rec, trig_cutover) =
                    eval_dupe_recovery_and_cutover(
                        art.dupe_fallback_round,
                        art.dupe_proactive,
                        &art.message_id,
                        &art.dupe_original_message_id,
                        file.dupe_recovered_articles,
                        nzb.dupe_recovered_articles,
                        file.dupe_cutover,
                        crate::dupefallback::CUTOVER_THRESHOLD,
                    );
                if rec {
                    file.dupe_recovered_articles = next_file_rec;
                    nzb.dupe_recovered_articles = next_nzb_rec;
                    host.file_set_int(
                        file.ptr,
                        crate::queuesubsys_host::FILE_INT_DUPE_RECOVERED_ARTICLES,
                        next_file_rec,
                    );
                    host.nzb_set_int(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_INT_DUPE_RECOVERED_ARTICLES,
                        next_nzb_rec,
                    );
                    if trig_cutover {
                        file.dupe_cutover = true;
                        host.file_set_int(
                            file.ptr,
                            crate::queuesubsys_host::FILE_INT_DUPE_CUTOVER,
                            1,
                        );
                        let mut msg = b"Leading with duplicate collections for ".to_vec();
                        msg.extend_from_slice(&file.filename);
                        msg.extend_from_slice(b" (primary is missing many articles)");
                        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
                    }
                }

                crate::dupefallback::register_lead_success_at(
                    host,
                    &mut file,
                    articles_len,
                    art_idx,
                    &art,
                );

                if file.dupe_recovered_articles > 0 && ad.decoded_file_size > 0 {
                    self.demote_misaligned_dupe_neighbors(
                        host,
                        &mut nzb,
                        &mut file,
                        articles_len,
                        art_idx,
                    );
                }

                if !file.par_file
                    && art.segment_offset >= 0
                    && art.segment_size > 0
                    && crate::parchecker::streaming_session_has_session(nzb.id)
                {
                    let seg_len = art.segment_size as usize;
                    let seg_opt =
                        host.article_cache_snapshot_segment(file.ptr, art.ptr, seg_len);
                    let _ = crate::parchecker::streaming_session_on_article_finished_by_name(
                        nzb.id,
                        &file.filename,
                        art.segment_offset as u64,
                        art.segment_size as u64,
                        art.crc,
                        seg_opt.as_deref(),
                    );
                    Self::unpause_streaming_par_volumes(host, nzb.ptr, nzb.id);
                }
            } else if (ad.status == crate::queuesubsys_host::AD_FAILED || misplaced)
                && self.with_donor_cache(|cache| {
                    crate::dupefallback::try_fallback_host(
                        cache,
                        host,
                        &opts,
                        dq_ptr,
                        &mut nzb,
                        &mut file,
                        art_idx,
                        &mut art,
                    )
                })
            {
                art.status = crate::queuesubsys_host::AI_UNDEFINED;
                host.article_set_int(
                    art.ptr,
                    crate::queuesubsys_host::ART_INT_STATUS,
                    crate::queuesubsys_host::AI_UNDEFINED,
                );
                retry = true;
                if art.part_number == 1 {
                    nzb.all_first = false;
                    host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_ALL_FIRST, 0);
                }
            } else if ad.status == crate::queuesubsys_host::AD_FAILED
                || ad.status == crate::queuesubsys_host::AD_FATAL_ERROR
                || misplaced
            {
                art.status = crate::queuesubsys_host::AI_FAILED;
                host.article_set_int(
                    art.ptr,
                    crate::queuesubsys_host::ART_INT_STATUS,
                    crate::queuesubsys_host::AI_FAILED,
                );
                let (dfr, ff_sz, nf_sz, npf_sz, ff_art, nf_art) = apply_failed_article_counters(
                    art.part_number,
                    file.par_file as i32,
                    art.size,
                    nzb.dupe_failed_run,
                    file.failed_size,
                    nzb.current_failed_size,
                    nzb.par_current_failed_size,
                    file.failed_articles,
                    nzb.current_failed_articles,
                );
                nzb.dupe_failed_run = dfr;
                file.failed_articles = ff_art;
                nzb.current_failed_articles = nf_art;
                file.failed_size = ff_sz;
                nzb.current_failed_size = nf_sz;
                nzb.par_current_failed_size = npf_sz;
                host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_DUPE_FAILED_RUN, dfr);
                host.file_set_int(
                    file.ptr,
                    crate::queuesubsys_host::FILE_INT_FAILED_ARTICLES,
                    ff_art,
                );
                host.nzb_set_int(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_INT_CUR_FAILED_ARTICLES,
                    nf_art,
                );
                host.file_set_i64(file.ptr, crate::queuesubsys_host::FILE_I64_FAILED_SIZE, ff_sz);
                host.nzb_set_i64(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_I64_CUR_FAILED_SIZE,
                    nf_sz,
                );
                host.nzb_set_i64(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_I64_PAR_CUR_FAILED_SIZE,
                    npf_sz,
                );

                if !file.par_file
                    && crate::parchecker::streaming_session_has_session(nzb.id)
                    && art.segment_size > 0
                    && art.segment_offset >= 0
                {
                    let _ =
                        crate::parchecker::streaming_session_on_article_failed_permanently_by_name(
                            nzb.id,
                            &file.filename,
                            art.segment_offset as u64,
                            art.segment_size as u64,
                        );
                    Self::unpause_streaming_par_volumes(host, nzb.ptr, nzb.id);
                }
            } else if ad.status == crate::queuesubsys_host::AD_RETRY {
                art.status = crate::queuesubsys_host::AI_UNDEFINED;
                host.article_set_int(
                    art.ptr,
                    crate::queuesubsys_host::ART_INT_STATUS,
                    crate::queuesubsys_host::AI_UNDEFINED,
                );
                retry = true;
                if art.part_number == 1 {
                    nzb.all_first = false;
                    host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_ALL_FIRST, 0);
                }
            }

            if ad.status != crate::queuesubsys_host::AD_RETRY {
                host.ad_action(
                    ad_ptr,
                    crate::queuesubsys_host::AD_ACT_ADD_SERVER_STATS,
                    0,
                );
            }

            // Refresh paused state in case UnpauseStreamingParVolumes changed it
            file = host.file_snap(file_ptr);
            nzb = host.nzb_snap(file.nzb_ptr, false);

            if !retry {
                let (rf, rn, pz, comp, fc) = advance_article_completion_counters(
                    file.remaining_size,
                    nzb.remaining_size,
                    nzb.paused_size,
                    art.size,
                    file.paused,
                    file.completed_articles,
                    articles_len as i32,
                );
                file.remaining_size = rf;
                nzb.remaining_size = rn;
                nzb.paused_size = pz;
                file.completed_articles = comp;
                file.partial_changed = true;
                file_completed = fc;
                host.file_set_i64(file.ptr, crate::queuesubsys_host::FILE_I64_REMAINING_SIZE, rf);
                host.nzb_set_i64(nzb.ptr, crate::queuesubsys_host::NZB_I64_REMAINING_SIZE, rn);
                host.nzb_set_i64(nzb.ptr, crate::queuesubsys_host::NZB_I64_PAUSED_SIZE, pz);
                host.file_set_int(
                    file.ptr,
                    crate::queuesubsys_host::FILE_INT_COMPLETED_ARTICLES,
                    comp,
                );
                host.file_set_int(file.ptr, crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED, 1);
            }

            if !file.filename_confirmed
                && !misplaced
                && ad.status == crate::queuesubsys_host::AD_FINISHED
                && ad.has_article_filename
            {
                let use_from_article = opts.file_naming == crate::queuesubsys_host::NF_ARTICLE
                    || (opts.file_naming == crate::queuesubsys_host::NF_AUTO
                        && !crate::deobfuscation::is_excessively_obfuscated(&ad.article_filename)
                        && !nzb.many_dupe_files);
                if use_from_article {
                    host.file_set_str(
                        file.ptr,
                        crate::queuesubsys_host::FILE_STR_FILENAME,
                        Some(&ad.article_filename),
                    );
                    host.file_action(
                        file.ptr,
                        crate::queuesubsys_host::FILE_ACT_MAKE_VALID_FILENAME,
                        0,
                    );
                    file = host.file_snap(file.ptr);
                }
                file.filename_confirmed = true;
                host.file_set_int(
                    file.ptr,
                    crate::queuesubsys_host::FILE_INT_FILENAME_CONFIRMED,
                    1,
                );
                let mut dupe_file_exists = false;
                if opts.dupe_check
                    && nzb.dupe_mode != crate::queuesubsys_host::DM_FORCE
                    && !nzb.many_dupe_files
                {
                    let check_path = join_path(&nzb.dest_dir, &file.filename);
                    dupe_file_exists =
                        crate::fsops::file_exists(&check_path, &crate::fsops::posix_stat);
                }
                if dupe_file_exists {
                    let mut msg = b"File \"".to_vec();
                    msg.extend_from_slice(&file.filename);
                    msg.extend_from_slice(
                        b"\" seems to be duplicate, cancelling download and deleting file from queue",
                    );
                    host.log(crate::queuesubsys_host::MK_WARNING, &msg);
                    file_completed = false;
                    file.dupe_deleted = true;
                    host.file_set_int(file.ptr, crate::queuesubsys_host::FILE_INT_DUPE_DELETED, 1);
                    self.delete_queue_entry(host, dq_ptr, file.ptr);
                    file = host.file_snap(file.ptr);
                }
            }

            if ad.has_content_analyzer
                && !misplaced
                && ad.status == crate::queuesubsys_host::AD_FINISHED
            {
                let fc = if ad.analyzed_par_file {
                    crate::queuesubsys_host::FC_PAR2
                } else {
                    crate::queuesubsys_host::FC_OTHER
                };
                file.first_content = fc;
                host.file_set_int(file.ptr, crate::queuesubsys_host::FILE_INT_FIRST_CONTENT, fc);
                if opts.direct_rename {
                    crate::directrenamer::article_downloaded_host(
                        self,
                        host,
                        &opts,
                        dq_ptr,
                        file.ptr,
                        art.size,
                        ad_ptr,
                    );
                }
            }

            nzb = host.nzb_snap(file.nzb_ptr, false);
            let next_dl_size = nzb.downloaded_size + ad.downloaded_size;
            host.nzb_set_i64(
                nzb.ptr,
                crate::queuesubsys_host::NZB_I64_DOWNLOADED_SIZE,
                next_dl_size,
            );

            file = host.file_snap(file.ptr);
            let finished_before_cancel = file_completed && !file.deleted;

            self.check_health(host, &opts, dq_ptr, file.ptr);
            self.check_dead_download(host, &opts, dq_ptr, nzb.ptr);

            file = host.file_snap(file.ptr);
            nzb = host.nzb_snap(file.nzb_ptr, false);

            let (fc_out, cp_out) = eval_file_completion_and_assembly(
                file_completed,
                nzb.parking,
                file.active_downloads,
                file.dupe_deleted,
                file.deleted,
                finished_before_cancel,
            );
            file_completed = fc_out;
            complete_file_parts = cp_out;

            if file_completed
                && file.success_articles == file.total_articles
                && (opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE
                    || file.dupe_recovered_articles > 0
                    || file.dupe_attempted_articles > 0)
            {
                let mut fresh_articles = snapshot_file_articles(host, file.ptr, file.articles_len);
                self.validate_completed_file_tiling(
                    host,
                    &mut nzb,
                    &mut file,
                    &mut fresh_articles,
                );
            }

            if !complete_file_parts {
                self.delete_downloader(host, host.self_download_queue_ptr(), ad_ptr, false);
            }
        });

        if complete_file_parts {
            if !host.ad_action(
                ad_ptr,
                crate::queuesubsys_host::AD_ACT_COMPLETE_FILE_PARTS,
                0,
            ) {
                host.with_locked_queue(|dq_ptr| {
                    let opts = host.opts();
                    let file = host.file_snap(file_ptr);
                    let nzb = host.nzb_snap(file.nzb_ptr, false);
                    let mut f_fa = file.failed_articles;
                    let mut f_fs = file.failed_size;
                    let mut n_fa = nzb.current_failed_articles;
                    let mut n_sa = nzb.current_success_articles;
                    let mut n_fs = nzb.current_failed_size;
                    let mut n_ss = nzb.current_success_size;
                    let mut np_fs = nzb.par_current_failed_size;
                    let mut np_ss = nzb.par_current_success_size;
                    apply_assembly_failure_counters(
                        file.success_articles,
                        file.success_size,
                        file.par_file,
                        &mut f_fa,
                        &mut f_fs,
                        &mut n_fa,
                        &mut n_sa,
                        &mut n_fs,
                        &mut n_ss,
                        &mut np_fs,
                        &mut np_ss,
                    );
                    host.file_set_int(
                        file.ptr,
                        crate::queuesubsys_host::FILE_INT_FAILED_ARTICLES,
                        f_fa,
                    );
                    host.file_set_int(
                        file.ptr,
                        crate::queuesubsys_host::FILE_INT_SUCCESS_ARTICLES,
                        0,
                    );
                    host.file_set_i64(
                        file.ptr,
                        crate::queuesubsys_host::FILE_I64_FAILED_SIZE,
                        f_fs,
                    );
                    host.file_set_i64(file.ptr, crate::queuesubsys_host::FILE_I64_SUCCESS_SIZE, 0);
                    host.nzb_set_int(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_INT_CUR_FAILED_ARTICLES,
                        n_fa,
                    );
                    host.nzb_set_int(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_INT_CUR_SUCCESS_ARTICLES,
                        n_sa,
                    );
                    host.nzb_set_i64(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_I64_CUR_FAILED_SIZE,
                        n_fs,
                    );
                    host.nzb_set_i64(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_I64_CUR_SUCCESS_SIZE,
                        n_ss,
                    );
                    host.nzb_set_i64(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_I64_PAR_CUR_FAILED_SIZE,
                        np_fs,
                    );
                    host.nzb_set_i64(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_I64_PAR_CUR_SUCCESS_SIZE,
                        np_ss,
                    );
                    host.file_action(
                        file.ptr,
                        crate::queuesubsys_host::FILE_ACT_FAIL_FINISHED_ARTICLES,
                        0,
                    );
                    self.discard_temp_files(host, &opts, &file);
                    self.delete_downloader(host, dq_ptr, ad_ptr, true);
                });
                return;
            }

            host.file_set_int(
                file_ptr,
                crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED,
                0,
            );

            let (mut path, dest_dir, filename, articles, daf_mode) = {
                let mut tup = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), 0i32);
                host.with_locked_queue(|_| {
                    let opts = host.opts();
                    if opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE {
                        let f = host.file_snap(file_ptr);
                        let n = host.nzb_snap(f.nzb_ptr, false);
                        let arts = snapshot_file_articles(host, file_ptr, f.articles_len);
                        tup = (
                            f.output_filename,
                            n.dest_dir,
                            f.filename,
                            arts,
                            opts.dupe_article_fallback,
                        );
                    }
                });
                tup
            };

            if daf_mode != crate::queuesubsys_host::DAF_NONE
                && (path.is_empty() || !crate::fsops::file_exists(&path, &crate::fsops::posix_stat))
            {
                path = join_path(&dest_dir, &filename);
            }
            let wrong_indices = if daf_mode != crate::queuesubsys_host::DAF_NONE {
                crate::dupefallback::borrowed_par2_mismatches_slice(
                    &articles, &path, &dest_dir, &filename,
                )
            } else {
                Vec::new()
            };

            host.with_locked_queue(|dq_ptr| {
                if !wrong_indices.is_empty() {
                    let mut f = host.file_snap(file_ptr);
                    let mut n = host.nzb_snap(f.nzb_ptr, false);
                    let mut fresh_arts = snapshot_file_articles(host, file_ptr, f.articles_len);
                    self.reject_borrowed_articles(
                        host,
                        &mut n,
                        &mut f,
                        &mut fresh_arts,
                        &wrong_indices,
                    );
                }
                self.delete_downloader(host, dq_ptr, ad_ptr, true);
            });
        }
    }

    fn reject_borrowed_articles(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &mut crate::queuesubsys_host::NzbSnap,
        file: &mut crate::queuesubsys_host::FileSnap,
        articles: &mut [crate::queuesubsys_host::ArticleSnap],
        wrong_indices: &[usize],
    ) {
        let mut donor_ids = Vec::with_capacity(wrong_indices.len());
        let mut rejected_count = 0usize;
        for &idx in wrong_indices {
            if idx >= articles.len() {
                continue;
            }
            rejected_count += 1;
            donor_ids.push(articles[idx].dupe_donor_id);
            if articles[idx].status == crate::queuesubsys_host::AI_FINISHED {
                self.count_as_failed(host, nzb, file, &mut articles[idx]);
            }
        }
        let (unique_ids, formatted_names) = format_donor_id_list(&donor_ids);
        for id in unique_ids {
            host.nzb_blocked_donor_op(nzb.ptr, true, id);
        }
        let mut msg = format!("{} article(s) of ", rejected_count).into_bytes();
        msg.extend_from_slice(&file.filename);
        msg.extend_from_slice(
            b" borrowed from duplicates don't match its par2 checksums: counted as failed, and duplicate(s) ",
        );
        msg.extend_from_slice(&formatted_names);
        msg.extend_from_slice(b" lend this download no more articles");
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
    }

    fn delete_downloader(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        ad_ptr: usize,
        file_completed: bool,
    ) {
        let ad = host.ad_snap(ad_ptr);
        let file = host.file_snap(ad.file_ptr);
        let nzb = host.nzb_snap(file.nzb_ptr, false);
        let delete_file_obj = file_completed || (file.deleted && file.active_downloads == 1);

        host.remove_active_download(ad_ptr);

        let next_f_ad = file.active_downloads - 1;
        let next_n_ad = nzb.active_downloads - 1;
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_ACTIVE_DOWNLOADS,
            next_f_ad,
        );
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_ACTIVE_DOWNLOADS,
            next_n_ad,
        );

        if delete_file_obj {
            let nzb_id = nzb.id;
            self.delete_file_info(host, dq_ptr, file.ptr, file_completed);
            let found_nzb = host.queue_find_by_id(dq_ptr, nzb_id);
            if found_nzb != 0 {
                host.nzb_set_int(found_nzb, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
                host.dq_save_changed(dq_ptr);
            }
        } else if next_n_ad == 0 {
            host.prepost_check_download_completed(dq_ptr, nzb.ptr);
        }
    }

    fn delete_file_info(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        file_ptr: usize,
        completed: bool,
    ) {
        while host.article_cache_file_busy(file_ptr) {
            host.sleep_ms(5);
        }

        let opts = host.opts();
        let file = host.file_snap(file_ptr);
        let nzb = host.nzb_snap(file.nzb_ptr, false);
        let parking = nzb.parking;
        let file_deleted = file.deleted;

        host.file_set_int(file.ptr, crate::queuesubsys_host::FILE_INT_DELETED, 1);

        if completed && file.dupe_attempted_articles > 0 {
            let mut msg = format!(
                "Recovered {} of {} missing article(s) of ",
                file.dupe_recovered_articles, file.dupe_attempted_articles
            )
            .into_bytes();
            msg.extend_from_slice(&file.filename);
            msg.extend_from_slice(b" from duplicate collections");
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
        }

        if completed || nzb.deleting {
            host.nzb_action(
                nzb.ptr,
                crate::queuesubsys_host::NZB_ACT_UPDATE_COMPLETED_STATS,
                file.ptr,
            );
        } else {
            host.nzb_action(
                nzb.ptr,
                crate::queuesubsys_host::NZB_ACT_UPDATE_DELETED_STATS,
                file.ptr,
            );
        }

        let file_status = classify_completed_file_status(
            file.total_articles,
            file.success_articles,
            file.missed_articles,
            file.failed_articles,
        );

        if opts.server_mode {
            let (del_data, save_part) = eval_delete_file_diskstate_flags(
                file_status,
                crate::queuesubsys_host::CF_SUCCESS,
                crate::queuesubsys_host::CF_PARTIAL,
                file_deleted as i32,
                parking as i32,
                completed as i32,
            );
            host.disk_discard_file(file.id, del_data, true, false);
            if save_part {
                host.disk_save_file_state(file.ptr, true);
            }
        }

        if !completed {
            self.discard_temp_files(host, &opts, &file);
        }

        if completed || parking {
            let out_exists = !file.output_filename.is_empty()
                && crate::fsops::file_exists(&file.output_filename, &crate::fsops::posix_stat);
            let filename = resolve_completed_filename(
                completed,
                file_status,
                &file.output_filename,
                &file.filename,
                &nzb.dest_dir,
                crate::paths::PATH_SEPARATOR,
                out_exists,
            );

            let articles = snapshot_file_articles(host, file.ptr, file.articles_len);
            let fresh_nzb = host.nzb_snap(nzb.ptr, false);

            if completed {
                crate::dupefallback::par_damage_file_completed_host(
                    host, &fresh_nzb, &file, &articles, &filename,
                );
            }

            let is_incomplete = completed
                && (file_status == crate::queuesubsys_host::CF_PARTIAL
                    || file_status == crate::queuesubsys_host::CF_FAILURE);
            let is_side = is_incomplete && is_side_file(&filename, &opts.ext_cleanup_disk);
            if is_side {
                let mut msg = b"Not repairing side file ".to_vec();
                msg.extend_from_slice(&filename);
                msg.extend_from_slice(b" from duplicates");
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
            }
            if is_incomplete
                && !is_side
                && crate::dupefallback::build_repair_job_host(
                    host, &opts, &fresh_nzb, &file, &articles, &filename,
                )
            {
                let mut msg = b"Queueing stream repair of ".to_vec();
                msg.extend_from_slice(&filename);
                if file_status == crate::queuesubsys_host::CF_FAILURE {
                    msg.extend_from_slice(b" (no article available) from duplicate collections");
                } else {
                    msg.extend_from_slice(b" from duplicate collections");
                }
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);

                let flen = host.nzb_file_len(nzb.ptr);
                let mut more_files_queued = false;
                for i in 0..flen {
                    let qf_ptr = host.nzb_file_at(nzb.ptr, i);
                    if qf_ptr != file.ptr {
                        let qf = host.file_snap(qf_ptr);
                        if !qf.deleted {
                            more_files_queued = true;
                            break;
                        }
                    }
                }
                let latest_nzb = host.nzb_snap(nzb.ptr, false);
                if opts.dupe_article_fallback == crate::queuesubsys_host::DAF_LIVE
                    && more_files_queued
                    && !latest_nzb.has_live_repair_thread
                {
                    host.stream_repair_start_live(nzb.ptr);
                }
            }

            let file_crc = if file_status == crate::queuesubsys_host::CF_SUCCESS {
                file.crc
            } else {
                0
            };
            host.nzb_push_completed_file(nzb.ptr, file.ptr, &filename, file_status, file_crc);
        }

        if opts.direct_rename {
            crate::directrenamer::file_downloaded_host(self, host, &opts, dq_ptr, file.ptr);
        }

        let check_nzb = host.nzb_snap(nzb.ptr, false);
        if check_nzb.direct_rename_status == crate::queuesubsys_host::TS_RUNNING
            && !check_nzb.deleting
            && check_nzb.is_download_completed
        {
            self.discard_direct_rename(host, &opts, nzb.ptr);
        }

        let removed_action = if completed && !file_deleted {
            crate::queuesubsys_host::ASPECT_FILE_COMPLETED
        } else {
            crate::queuesubsys_host::ASPECT_FILE_DELETED
        };
        host.nzb_remove_file_and_notify(dq_ptr, nzb.ptr, file.ptr, removed_action);
    }

    fn demote_misaligned_dupe_neighbors(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &mut crate::queuesubsys_host::NzbSnap,
        file: &mut crate::queuesubsys_host::FileSnap,
        articles_len: usize,
        index: usize,
    ) {
        let mut neighbors = [None, None];
        if index > 0 {
            neighbors[0] = Some(index - 1);
        }
        if index + 1 < articles_len {
            neighbors[1] = Some(index + 1);
        }
        for nb_opt in neighbors {
            let Some(nb_idx) = nb_opt else {
                continue;
            };
            let mut nb = host.article_at(file.ptr, nb_idx);
            if nb.status != crate::queuesubsys_host::AI_FINISHED
                || nb.dupe_fallback_round <= 0
                || crate::dupefallback::segment_aligned_at(
                    host,
                    file.ptr,
                    articles_len,
                    nb_idx,
                    &nb,
                    file.decoded_file_size,
                )
            {
                continue;
            }
            let part_num = nb.part_number;
            self.demote_finished_article(host, nzb, file, &mut nb);
            let mut msg = b"Discarding misaligned article ".to_vec();
            msg.extend_from_slice(&file.filename);
            msg.extend_from_slice(
                format!(" [{}/{}] from duplicate", part_num, articles_len).as_bytes(),
            );
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
        }
    }

    fn validate_completed_file_tiling(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &mut crate::queuesubsys_host::NzbSnap,
        file: &mut crate::queuesubsys_host::FileSnap,
        articles: &mut [crate::queuesubsys_host::ArticleSnap],
    ) {
        if file.success_articles != file.total_articles {
            return;
        }
        if !crate::dupefallback::has_donor_articles_slice(articles) {
            return;
        }
        let Some(untiled_idx) =
            crate::dupefallback::first_untiled_article_slice(articles, file.decoded_file_size)
        else {
            return;
        };
        let part_num = articles[untiled_idx].part_number;
        let total_len = articles.len();
        self.demote_finished_article(host, nzb, file, &mut articles[untiled_idx]);
        let mut msg = b"Discarding misaligned article ".to_vec();
        msg.extend_from_slice(&file.filename);
        msg.extend_from_slice(
            format!(
                " [{}/{}]: decoded bytes do not tile the file (leaving to par-repair)",
                part_num, total_len
            )
            .as_bytes(),
        );
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
    }

    fn count_as_failed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &mut crate::queuesubsys_host::NzbSnap,
        file: &mut crate::queuesubsys_host::FileSnap,
        art: &mut crate::queuesubsys_host::ArticleSnap,
    ) {
        art.status = crate::queuesubsys_host::AI_FAILED;
        host.article_set_int(
            art.ptr,
            crate::queuesubsys_host::ART_INT_STATUS,
            crate::queuesubsys_host::AI_FAILED,
        );
        demote_article_counters(
            art.size,
            file.par_file,
            art.dupe_proactive,
            &art.message_id,
            &art.dupe_original_message_id,
            &mut file.success_size,
            &mut file.failed_size,
            &mut file.success_articles,
            &mut file.failed_articles,
            &mut nzb.current_success_size,
            &mut nzb.current_failed_size,
            &mut nzb.par_current_success_size,
            &mut nzb.par_current_failed_size,
            &mut nzb.current_success_articles,
            &mut nzb.current_failed_articles,
            &mut file.dupe_recovered_articles,
            &mut nzb.dupe_recovered_articles,
        );
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_SUCCESS_ARTICLES,
            file.success_articles,
        );
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_FAILED_ARTICLES,
            file.failed_articles,
        );
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_CUR_SUCCESS_ARTICLES,
            nzb.current_success_articles,
        );
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_CUR_FAILED_ARTICLES,
            nzb.current_failed_articles,
        );
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_DUPE_RECOVERED_ARTICLES,
            file.dupe_recovered_articles,
        );
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_DUPE_RECOVERED_ARTICLES,
            nzb.dupe_recovered_articles,
        );
        host.file_set_i64(
            file.ptr,
            crate::queuesubsys_host::FILE_I64_SUCCESS_SIZE,
            file.success_size,
        );
        host.file_set_i64(
            file.ptr,
            crate::queuesubsys_host::FILE_I64_FAILED_SIZE,
            file.failed_size,
        );
        host.nzb_set_i64(
            nzb.ptr,
            crate::queuesubsys_host::NZB_I64_CUR_SUCCESS_SIZE,
            nzb.current_success_size,
        );
        host.nzb_set_i64(
            nzb.ptr,
            crate::queuesubsys_host::NZB_I64_CUR_FAILED_SIZE,
            nzb.current_failed_size,
        );
        host.nzb_set_i64(
            nzb.ptr,
            crate::queuesubsys_host::NZB_I64_PAR_CUR_SUCCESS_SIZE,
            nzb.par_current_success_size,
        );
        host.nzb_set_i64(
            nzb.ptr,
            crate::queuesubsys_host::NZB_I64_PAR_CUR_FAILED_SIZE,
            nzb.par_current_failed_size,
        );
    }

    fn demote_finished_article(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &mut crate::queuesubsys_host::NzbSnap,
        file: &mut crate::queuesubsys_host::FileSnap,
        art: &mut crate::queuesubsys_host::ArticleSnap,
    ) {
        self.discard_article_segment(host, file, art);
        self.count_as_failed(host, nzb, file, art);
        if crate::dupefallback::register_lead_failure_host(host, file, art) {
            let mut msg = b"Switching lead duplicate collection for ".to_vec();
            msg.extend_from_slice(&file.filename);
            msg.extend_from_slice(b" (the lead duplicate is missing many articles)");
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
        }
        file.partial_changed = true;
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED,
            1,
        );
    }

    fn discard_article_segment(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        file: &mut crate::queuesubsys_host::FileSnap,
        art: &crate::queuesubsys_host::ArticleSnap,
    ) {
        host.article_cache_discard_segment(file.ptr, art.ptr);
        file.cached_articles = host.file_snap(file.ptr).cached_articles;
    }

    fn save_queue_if_changed(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        if !host.opts().server_mode {
            return;
        }
        host.with_locked_queue(|dq_ptr| {
            let qlen = host.queue_len(dq_ptr);
            let mut has_changes = false;
            for i in 0..qlen {
                let nzb = host.nzb_snap(host.queue_at(dq_ptr, i), false);
                has_changes |= nzb.changed;
            }
            if has_changes {
                host.dq_save(dq_ptr);
            }
        });
    }

    fn save_all_partial_state(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        let opts = host.opts();
        if !opts.server_mode || !opts.continue_partial {
            return;
        }
        host.with_locked_queue(|dq_ptr| {
            let qlen = host.queue_len(dq_ptr);
            for i in 0..qlen {
                let nzb_ptr = host.queue_at(dq_ptr, i);
                let flen = host.nzb_file_len(nzb_ptr);
                for j in 0..flen {
                    self.save_partial_state(host, host.nzb_file_at(nzb_ptr, j));
                }
            }
            host.dq_save_changed(dq_ptr);
        });
    }

    fn save_partial_state(&self, host: &crate::queuesubsys_host::QueueSubsysHost, file_ptr: usize) {
        save_partial_state_host(host, file_ptr);
    }

    fn load_partial_state(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        nzb: &crate::queuesubsys_host::NzbSnap,
        file_ptr: usize,
    ) {
        let file = host.file_snap(file_ptr);
        if file.partial_state == crate::queuesubsys_host::PARTIAL_PARTIAL {
            host.disk_load_file_state(file.ptr, false);
            let f2 = host.file_snap(file.ptr);
            if (opts.direct_write || f2.force_direct_write) && !f2.output_initialized {
                let out_tmp = join_path(&nzb.dest_dir, format!("{}.out.tmp", f2.id).as_bytes());
                if crate::fsops::file_exists(&out_tmp, &crate::fsops::posix_stat) {
                    host.file_set_str(
                        file.ptr,
                        crate::queuesubsys_host::FILE_STR_OUTPUT_FILENAME,
                        Some(&out_tmp),
                    );
                    host.file_set_int(
                        file.ptr,
                        crate::queuesubsys_host::FILE_INT_OUTPUT_INITIALIZED,
                        1,
                    );
                } else if f2.force_direct_write {
                    let out_fn = join_path(&nzb.dest_dir, &f2.filename);
                    if crate::fsops::file_exists(&out_fn, &crate::fsops::posix_stat) {
                        host.file_set_str(
                            file.ptr,
                            crate::queuesubsys_host::FILE_STR_OUTPUT_FILENAME,
                            Some(&out_fn),
                        );
                        host.file_set_int(
                            file.ptr,
                            crate::queuesubsys_host::FILE_INT_OUTPUT_INITIALIZED,
                            1,
                        );
                    }
                }
            }
        } else if file.partial_state == crate::queuesubsys_host::PARTIAL_COMPLETED {
            host.disk_load_file_state(file.ptr, true);
            let f2 = host.file_snap(file.ptr);
            let out_fn = join_path(&nzb.dest_dir, &f2.filename);
            host.file_set_str(
                file.ptr,
                crate::queuesubsys_host::FILE_STR_OUTPUT_FILENAME,
                Some(&out_fn),
            );
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_OUTPUT_INITIALIZED,
                1,
            );
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_FORCE_DIRECT_WRITE,
                1,
            );
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_FILENAME_CONFIRMED,
                1,
            );
        }
    }

    fn save_all_file_state(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        if host.opts().server_mode && host.coord_dq_state_changed() {
            host.with_locked_queue(|dq_ptr| {
                host.disk_save_all_file_infos(dq_ptr);
            });
        }
    }

    fn check_health(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        file_ptr: usize,
    ) {
        let file = host.file_snap(file_ptr);
        let nzb = host.nzb_snap(file.nzb_ptr, false);
        let dl_crit = self.download_critical_health(host, opts, dq_ptr, &nzb);
        if should_skip_health_check(
            opts.health_check,
            crate::queuesubsys_host::HC_NONE,
            crate::queuesubsys_host::HC_PARK,
            opts.par_scan,
            crate::queuesubsys_host::PS_DUPE,
            nzb.health_paused,
            nzb.delete_status,
            crate::queuesubsys_host::DS_HEALTH,
            nzb.health,
            dl_crit,
            nzb.success_articles,
            nzb.total_articles,
        ) {
            return;
        }

        if opts.health_check == crate::queuesubsys_host::HC_PAUSE {
            let mut msg = b"Pausing ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(
                format!(
                    " due to health {:.1}% below critical {:.1}%",
                    (nzb.health as f64) / 10.0,
                    (nzb.critical_health_opt as f64) / 10.0
                )
                .as_bytes(),
            );
            host.log(crate::queuesubsys_host::MK_WARNING, &msg);
            host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_HEALTH_PAUSED, 1);
            host.dq_edit_entry(
                dq_ptr,
                nzb.id,
                crate::queuesubsys_host::EA_GROUP_PAUSE,
                None,
            );
        } else if opts.health_check == crate::queuesubsys_host::HC_DELETE
            || opts.health_check == crate::queuesubsys_host::HC_PARK
        {
            let mut msg = b"Cancelling download and deleting ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(
                format!(
                    " due to health {:.1}% below critical {:.1}%",
                    (nzb.health as f64) / 10.0,
                    (nzb.critical_health_opt as f64) / 10.0
                )
                .as_bytes(),
            );
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
            host.nzb_set_int(
                nzb.ptr,
                crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                crate::queuesubsys_host::DS_HEALTH,
            );
            host.dq_edit_entry(
                dq_ptr,
                nzb.id,
                if opts.health_check == crate::queuesubsys_host::HC_PARK {
                    crate::queuesubsys_host::EA_GROUP_PARK_DELETE
                } else {
                    crate::queuesubsys_host::EA_GROUP_DELETE
                },
                None,
            );
        } else if opts.health_check == crate::queuesubsys_host::HC_DUPE {
            self.check_dupe_failover(host, opts, dq_ptr, &nzb);
        }
    }

    fn download_critical_health(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> i32 {
        if opts.health_check == crate::queuesubsys_host::HC_DUPE
            && nzb.critical_health_exact == 1000
            && Self::certainly_parless(host, nzb)
            && !Self::has_size_twin(host, dq_ptr, nzb)
        {
            return 1000;
        }
        nzb.critical_health_opt
    }

    fn certainly_parless(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> bool {
        if crate::dupefallback::has_par2_host(host, nzb) {
            return false;
        }
        let mut files = 0i32;
        let flen = host.nzb_file_len(nzb.ptr);
        for i in 0..flen {
            let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
            if !is_data_filename(&f.filename) {
                return false;
            }
            files += 1;
        }
        for cf in host.nzb_completed_files(nzb.ptr) {
            if !is_data_filename(&cf.filename) {
                return false;
            }
            files += 1;
        }
        files > 0
    }

    fn has_size_twin(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> bool {
        let mut largest = 0i64;
        let flen = host.nzb_file_len(nzb.ptr);
        for i in 0..flen {
            let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
            largest = largest.max(f.size);
        }
        if largest <= 0 {
            return false;
        }
        let check_other = |other_ptr: usize| -> bool {
            if other_ptr == 0 || other_ptr == nzb.ptr {
                return false;
            }
            let other = host.nzb_snap(other_ptr, false);
            let same_key = crate::dupecoord::same_name_or_key(
                &other.name,
                &other.dupe_key,
                &nzb.name,
                &nzb.dupe_key,
            );
            let oflen = host.nzb_file_len(other_ptr);
            let mut sizes = Vec::with_capacity(oflen);
            for j in 0..oflen {
                sizes.push(host.file_snap(host.nzb_file_at(other_ptr, j)).size);
            }
            is_size_twin(
                other_ptr == nzb.ptr,
                same_key,
                nzb.size,
                other.size,
                other.par_size,
                largest,
                &sizes,
            )
        };
        let qlen = host.queue_len(dq_ptr);
        for i in 0..qlen {
            if check_other(host.queue_at(dq_ptr, i)) {
                return true;
            }
        }
        let hlen = host.history_len(dq_ptr);
        for i in 0..hlen {
            let hi = host.history_at(dq_ptr, i);
            if hi.kind == crate::queuesubsys_host::HK_NZB && check_other(hi.inner_ptr) {
                return true;
            }
        }
        false
    }

    fn has_active_dupe(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> bool {
        if nzb.dupe_key.is_empty() {
            return false;
        }
        if nzb.has_extra_priority
            || (nzb.has_post_info
                && (nzb.post_working || nzb.post_stage != crate::queuesubsys_host::PT_QUEUED))
        {
            return false;
        }
        let self_completed_empty = host.nzb_completed_files(nzb.ptr).is_empty();
        let qlen = host.queue_len(dq_ptr);
        for i in 0..qlen {
            let qp = host.queue_at(dq_ptr, i);
            if qp == 0 || qp == nzb.ptr {
                continue;
            }
            let queued = host.nzb_snap(qp, false);
            if !queued.dupe_key.is_empty() && queued.dupe_key.eq_ignore_ascii_case(&nzb.dupe_key) {
                if is_queued_dupe_active(
                    queued.deleting,
                    queued.delete_status,
                    crate::queuesubsys_host::DS_NONE,
                    queued.active_downloads,
                    queued.has_post_info,
                    queued.post_working,
                    queued.post_stage,
                    crate::queuesubsys_host::PT_QUEUED,
                    queued.has_extra_priority,
                    self_completed_empty,
                    queued.direct_unpack_status,
                    crate::queuesubsys_host::NS_RUNNING,
                ) {
                    return true;
                }
            }
        }
        false
    }

    fn check_dupe_failover(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) {
        if nzb.deleting || nzb.parking {
            return;
        }
        let attempted = nzb.dupe_attempted_articles + nzb.dupe_unsourced_articles;
        let recovered = nzb.dupe_recovered_articles;
        let failed = nzb.current_failed_articles;
        let (defer, upd_checked) = should_defer_dupe_failover(
            opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE,
            attempted,
            recovered,
            DUPE_FAILOVER_SAMPLE,
            nzb.dupe_par_defer_state,
            crate::queuesubsys_host::DP_DEFERRED,
            crate::dupefallback::par_cannot_cover_snap(nzb, true),
            failed,
            nzb.dupe_failover_checked,
        );
        if defer {
            return;
        }
        if upd_checked {
            host.nzb_set_int(
                nzb.ptr,
                crate::queuesubsys_host::NZB_INT_DUPE_FAILOVER_CHECKED,
                failed,
            );
        }

        let backup = if crate::dupecoord::fails_over_snap(opts, nzb) {
            crate::dupecoord::find_dupe_backup_host(
                host,
                opts,
                dq_ptr,
                nzb.ptr,
                &nzb.name,
                &nzb.dupe_key,
            )
        } else {
            None
        };

        let dl_crit = self.download_critical_health(host, opts, dq_ptr, nzb);
        let effective_failover_health = if dl_crit > nzb.critical_health_opt {
            0
        } else {
            nzb.health
        };

        if let Some((_, backup_nzb)) = backup {
            if crate::dupecoord::dupe_failover_warranted_inline(
                nzb.dupe_score,
                effective_failover_health,
                backup_nzb.dupe_score,
            ) {
                let mut msg = b"Failing over ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b" to duplicate ");
                msg.extend_from_slice(&backup_nzb.name);
                msg.extend_from_slice(
                    format!(
                        ": health {:.1}% below critical {:.1}%, {} of {} missing article(s) recovered from duplicates",
                        (nzb.health as f64) / 10.0,
                        (dl_crit as f64) / 10.0,
                        recovered,
                        attempted
                    )
                    .as_bytes(),
                );
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
                host.nzb_set_int(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                    crate::queuesubsys_host::DS_HEALTH,
                );
                host.dq_edit_entry(
                    dq_ptr,
                    nzb.id,
                    crate::queuesubsys_host::EA_GROUP_PARK_DELETE,
                    None,
                );
                return;
            }
        }

        if download_hopeless(
            nzb.total_articles,
            nzb.current_success_articles,
            nzb.current_failed_articles,
        ) {
            let tried = nzb.current_success_articles + nzb.current_failed_articles;
            let reason = crate::dupecoord::no_backup_reason_host(host, opts, dq_ptr, nzb);
            let mut msg = b"Parking ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(
                format!(
                    ": health {:.1}% below critical {:.1}%, {} of {} missing article(s) recovered from duplicates, {} of {} tried article(s) exist and no better duplicate in history (",
                    (nzb.health as f64) / 10.0,
                    (dl_crit as f64) / 10.0,
                    recovered,
                    attempted,
                    nzb.current_success_articles,
                    tried
                )
                .as_bytes(),
            );
            msg.extend_from_slice(&reason);
            msg.push(b')');
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
            host.nzb_set_int(
                nzb.ptr,
                crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                crate::queuesubsys_host::DS_HEALTH,
            );
            host.dq_edit_entry(
                dq_ptr,
                nzb.id,
                crate::queuesubsys_host::EA_GROUP_PARK_DELETE,
                None,
            );
        }
    }

    fn check_dead_download(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb_ptr: usize,
    ) {
        let nzb = host.nzb_snap(nzb_ptr, false);
        if opts.health_check != crate::queuesubsys_host::HC_DUPE
            || !opts.dupe_check
            || !crate::dupecoord::fails_over_snap(opts, &nzb)
            || nzb.kind != crate::queuesubsys_host::NK_NZB
            || nzb.deleting
            || nzb.parking
            || nzb.delete_status != crate::queuesubsys_host::DS_NONE
        {
            return;
        }

        let critical = self.download_critical_health(host, opts, dq_ptr, &nzb);
        let probing = crate::dupeprobe::probe_probing_for(nzb.id);
        let failed = nzb.current_failed_articles;
        let files_tried = Self::files_tried(host, &nzb);
        let eval = eval_dead_download(
            nzb.current_success_articles,
            nzb.dupe_recovered_articles,
            failed,
            nzb.file_count,
            nzb.dupe_failed_run,
            crate::dupefallback::par_cannot_cover_snap(&nzb, false),
            critical,
            crate::dupefallback::PAR_EDGE_MARGIN,
            files_tried,
            probing,
        );
        let dead = eval.dead != 0;
        let doomed = eval.doomed != 0;
        let hopeless = eval.hopeless != 0;
        let dead_rate = eval.dead_rate != 0;
        let far_below = eval.far_below != 0;
        let below = eval.below != 0;

        if !dead && !doomed && self.check_par_damage(host, opts, dq_ptr, &nzb) {
            return;
        }
        if (!dead && !doomed) || (probing >= 0 && !hopeless) {
            return;
        }

        let backup = crate::dupecoord::find_dupe_backup_host(
            host,
            opts,
            dq_ptr,
            nzb.ptr,
            &nzb.name,
            &nzb.dupe_key,
        );
        let Some((_, backup_nzb)) = backup else {
            let attempted = nzb.dupe_attempted_articles + nzb.dupe_unsourced_articles;
            if should_park_dead_without_backup(
                opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE,
                nzb.dupe_par_defer_state,
                crate::queuesubsys_host::DP_DEFERRED,
                attempted,
                nzb.dupe_recovered_articles,
                eval.tried,
                PROJECTED_FAILURE_SAMPLE,
                files_tried,
                far_below,
                below,
            ) {
                let reason = crate::dupecoord::no_backup_reason_host(host, opts, dq_ptr, &nzb);
                let mut msg = b"Parking ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(
                    format!(
                        ": {} of {} tried article(s) of its own arrived, projected health {:.1}% {} critical {:.1}%, and no duplicate in history to fail over to (",
                        eval.own,
                        eval.tried,
                        (eval.projected as f64) / 10.0,
                        if far_below { "far below" } else { "below" },
                        (critical as f64) / 10.0
                    )
                    .as_bytes(),
                );
                msg.extend_from_slice(&reason);
                msg.push(b')');
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
                host.nzb_set_int(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                    crate::queuesubsys_host::DS_HEALTH,
                );
                host.dq_edit_entry(
                    dq_ptr,
                    nzb.id,
                    crate::queuesubsys_host::EA_GROUP_PARK_DELETE,
                    None,
                );
            }
            return;
        };

        if dead {
            if !crate::dupecoord::dupe_failover_warranted_inline(
                nzb.dupe_score,
                0,
                backup_nzb.dupe_score,
            ) {
                return;
            }
            let mut msg = b"Failing over ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" to duplicate ");
            msg.extend_from_slice(&backup_nzb.name);
            if dead_rate {
                msg.extend_from_slice(
                    format!(
                        ": {} of its own articles downloaded, {} failed",
                        eval.own, failed
                    )
                    .as_bytes(),
                );
            } else {
                msg.extend_from_slice(
                    format!(
                        ": {} of its articles failed in a row ({} of {} tried arrived)",
                        nzb.dupe_failed_run, eval.own, eval.tried
                    )
                    .as_bytes(),
                );
            }
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
        } else {
            let backup_alive = backup_alive_permille(host, backup_nzb.ptr);
            if backup_alive < eval.projected + BACKUP_LEAD
                || backup_alive < critical
                || !crate::dupecoord::dupe_failover_warranted_inline(
                    nzb.dupe_score,
                    eval.projected,
                    backup_nzb.dupe_score,
                )
            {
                return;
            }
            let mut msg = b"Failing over ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" to duplicate ");
            msg.extend_from_slice(&backup_nzb.name);
            msg.extend_from_slice(
                format!(
                    ": {} of {} tried article(s) of its own arrived, projected health {:.1}% {} critical {:.1}%; the duplicate is {:.0}% alive",
                    eval.own,
                    eval.tried,
                    (eval.projected as f64) / 10.0,
                    if eval.projected < critical {
                        "below"
                    } else {
                        "near"
                    },
                    (critical as f64) / 10.0,
                    (backup_alive as f64) / 10.0
                )
                .as_bytes(),
            );
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
        }
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
            crate::queuesubsys_host::DS_HEALTH,
        );
        host.dq_edit_entry(
            dq_ptr,
            nzb.id,
            crate::queuesubsys_host::EA_GROUP_PARK_DELETE,
            None,
        );
    }

    fn check_par_damage(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> bool {
        let verdict = crate::dupefallback::par_damage_judge_host(host, nzb);
        let certain = verdict.certain();
        if !certain && !verdict.projected() {
            return false;
        }
        if should_wait_for_borrowing_on_par_damage(
            opts.dupe_article_fallback != crate::queuesubsys_host::DAF_NONE,
            opts.raw_article,
            nzb.dupe_mode,
            crate::queuesubsys_host::DM_FORCE,
            crate::dupefallback::has_par2_host(host, nzb),
            nzb.dupe_par_defer_state,
            crate::queuesubsys_host::DP_DEFERRED,
            nzb.dupe_attempted_articles + nzb.dupe_unsourced_articles,
            DUPE_FAILOVER_SAMPLE,
            nzb.dupe_recovered_articles,
        ) {
            return false;
        }

        let qlen = host.queue_len(dq_ptr);
        for i in 0..qlen {
            let qp = host.queue_at(dq_ptr, i);
            if qp == 0 || qp == nzb.ptr {
                continue;
            }
            let queued = host.nzb_snap(qp, false);
            if queued.paused_size < queued.remaining_size
                && crate::dupecoord::same_name_or_key(
                    &queued.name,
                    &queued.dupe_key,
                    &nzb.name,
                    &nzb.dupe_key,
                )
            {
                return false;
            }
        }

        let backup = crate::dupecoord::find_dupe_backup_host(
            host,
            opts,
            dq_ptr,
            nzb.ptr,
            &nzb.name,
            &nzb.dupe_key,
        );
        let damage = format_par_damage_summary(
            certain,
            verdict.damaged_blocks,
            verdict.projected_blocks,
            verdict.block_size as i64,
            verdict.recovery_blocks,
        );
        let Some((_, backup_nzb)) = backup else {
            if crate::pardamage::first_report(nzb.id) {
                let mut msg = nzb.name.clone();
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&damage);
                msg.extend_from_slice(b"; no duplicate in history to fail over to");
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
            }
            return false;
        };

        let backup_alive = backup_alive_permille(host, backup_nzb.ptr);
        if (!certain && backup_alive < 999)
            || !crate::dupecoord::dupe_failover_warranted_inline(
                nzb.dupe_score,
                0,
                backup_nzb.dupe_score,
            )
        {
            return false;
        }

        let mut msg = b"Failing over ".to_vec();
        msg.extend_from_slice(&nzb.name);
        msg.extend_from_slice(b" to duplicate ");
        msg.extend_from_slice(&backup_nzb.name);
        msg.extend_from_slice(b": ");
        msg.extend_from_slice(&damage);
        msg.extend_from_slice(b", par-repair can't fix it");
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &msg);
        crate::dupefallback::par_damage_forget_all(nzb.id);
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
            crate::queuesubsys_host::DS_HEALTH,
        );
        host.dq_edit_entry(
            dq_ptr,
            nzb.id,
            crate::queuesubsys_host::EA_GROUP_PARK_DELETE,
            None,
        );
        true
    }

    fn files_tried(
        host: &crate::queuesubsys_host::QueueSubsysHost,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) -> i32 {
        let mut files = host.nzb_completed_files(nzb.ptr).len() as i32;
        let flen = host.nzb_file_len(nzb.ptr);
        for i in 0..flen {
            let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
            if f.success_articles + f.failed_articles > 0 {
                files += 1;
            }
        }
        files
    }

    fn start_dead_pick_probe(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        dq_ptr: usize,
        nzb: &crate::queuesubsys_host::NzbSnap,
    ) {
        if opts.health_check != crate::queuesubsys_host::HC_DUPE
            || !opts.dupe_check
            || nzb.dead_pick_probed
            || !crate::dupecoord::fails_over_snap(opts, nzb)
            || nzb.kind != crate::queuesubsys_host::NK_NZB
            || nzb.deleting
            || nzb.parking
            || nzb.delete_status != crate::queuesubsys_host::DS_NONE
        {
            return;
        }
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_DEAD_PICK_PROBED,
            1,
        );
        if nzb.current_success_articles > 0 {
            return;
        }
        if crate::dupecoord::find_dupe_backup_host(
            host,
            opts,
            dq_ptr,
            nzb.ptr,
            &nzb.name,
            &nzb.dupe_key,
        )
        .is_none()
        {
            return;
        }

        let flen = host.nzb_file_len(nzb.ptr);
        let mut files: Vec<crate::queuesubsys_host::FileSnap> = Vec::new();
        for pass in 0..2 {
            if !files.is_empty() {
                break;
            }
            for i in 0..flen {
                let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
                if !f.deleted && !f.paused && f.total_articles > 0 && (pass == 1 || !f.par_file) {
                    files.push(f);
                }
            }
        }

        let total: usize = files.iter().map(|f| f.total_articles.max(0) as usize).sum();
        if total < DUPE_PROBE_MIN_ARTICLES {
            return;
        }

        let indexes = crate::dupeprobe::sample_indexes(total, DUPE_PROBE_SAMPLE_COUNT);
        let mut samples: Vec<(Vec<u8>, Vec<Vec<u8>>)> = Vec::with_capacity(indexes.len());
        for pos in indexes {
            let mut offset = 0usize;
            for f in &mut files {
                let count = f.total_articles.max(0) as usize;
                if pos >= offset + count {
                    offset += count;
                    continue;
                }
                if f.articles_len == 0 && opts.server_mode {
                    host.disk_load_articles(f.ptr);
                    self.load_partial_state(host, opts, nzb, f.ptr);
                    *f = host.file_snap(f.ptr);
                }
                let idx = pos - offset;
                if idx < f.articles_len {
                    let art = host.article_at(f.ptr, idx);
                    let groups = host.file_groups(f.ptr);
                    samples.push((art.message_id, groups));
                }
                break;
            }
        }
        if samples.len() < DUPE_PROBE_MIN_ARTICLES {
            return;
        }

        let mut probe = Box::new(crate::dupeprobe::NzbgetRsDupeProbe::new(
            nzb.id,
            false,
            nzb.dupe_recovered_articles,
        ));
        for (msg_id, groups) in samples {
            probe.add_sample(msg_id, groups);
        }
        #[cfg(unix)]
        if host.use_active {
            crate::dupeprobe::launch_probe_rs(probe);
        }
        #[cfg(not(unix))]
        let _ = probe;
    }

    fn reset_hanging_downloads(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        let opts = host.opts();
        if opts.article_timeout == 0 {
            return;
        }
        host.with_locked_queue(|_| {
            let tm = host.opts().cur_time;
            let n = host.active_downloads_len();
            for i in 0..n {
                let ad = host.active_download_at(i);
                if is_hanging_download(
                    tm,
                    ad.last_update_time,
                    opts.article_timeout,
                    ad.status == crate::queuesubsys_host::AD_RUNNING,
                ) {
                    host.ad_action(ad.ad_ptr, crate::queuesubsys_host::AD_ACT_CANCEL_HANGING, 0);
                }
            }
        });
    }

    fn discard_direct_rename(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
        nzb_ptr: usize,
    ) {
        discard_direct_rename_host(host, opts, nzb_ptr);
    }

    pub(crate) fn wakeup(&self) {
        crate::log::log_active_bytes(
            crate::queuesubsys_host::MK_DEBUG,
            b"Waking up QueueCoordinator",
        );
        let mut g = self.wait_lock.0.lock().unwrap_or_else(|e| e.into_inner());
        self.has_more_jobs
            .store(true, std::sync::atomic::Ordering::SeqCst);
        *g = true;
        self.wait_lock.1.notify_all();
    }

    pub(crate) fn standby_wait(&self, wait_ms: i32) {
        let mut g = self.wait_lock.0.lock().unwrap_or_else(|e| e.into_inner());
        if !*g && !self.stopped.load(std::sync::atomic::Ordering::SeqCst) && wait_ms > 0 {
            g = self
                .wait_lock
                .1
                .wait_timeout(g, std::time::Duration::from_millis(wait_ms as u64))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        *g = false;
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn set_has_more_jobs(&self, more: bool) {
        self.has_more_jobs
            .store(more, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn has_more_jobs(&self) -> bool {
        self.has_more_jobs.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn dq_state_changed(&self) -> bool {
        self.state_changed.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn dq_history_changed(&self) {
        self.history_changed
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn dq_save(&self, host: &crate::queuesubsys_host::QueueSubsysHost, dq_ptr: usize) {
        if self.mass_edit.load(std::sync::atomic::Ordering::SeqCst) {
            self.want_save
                .store(true, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        if host.opts().server_mode {
            let hc = self
                .history_changed
                .load(std::sync::atomic::Ordering::SeqCst);
            let _ = crate::diskstate_format::active_disk_save_queue(host, dq_ptr, hc);
            self.state_changed
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let qlen = host.queue_len(dq_ptr);
        for i in 0..qlen {
            let nzb_ptr = host.queue_at(dq_ptr, i);
            host.nzb_set_int(nzb_ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 0);
        }
        self.want_save
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.history_changed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.wakeup();
    }

    pub(crate) fn dq_save_changed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
    ) {
        if host.opts().server_mode {
            let _ = crate::diskstate_format::active_disk_save_progress(host, dq_ptr);
            self.state_changed
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub(crate) fn dq_edit_entry(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        id: i32,
        action: i32,
        args: Option<&[u8]>,
    ) -> bool {
        crate::queueeditor::edit_entry_host(self, host, dq_ptr, id, action, args)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dq_edit_list(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        ids: Option<&[i32]>,
        names: Option<&[&[u8]]>,
        match_mode: i32,
        action: i32,
        args: Option<&[u8]>,
    ) -> bool {
        self.mass_edit
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let ret = crate::queueeditor::edit_list_host(
            self, host, dq_ptr, ids, names, match_mode, action, args,
        );
        self.mass_edit
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if self.want_save.load(std::sync::atomic::Ordering::SeqCst) {
            self.dq_save(host, dq_ptr);
        }
        ret
    }

    fn find_ad(
        &self,
        ad_ptr: usize,
    ) -> Option<std::sync::Arc<crate::articledl::NzbgetRsArticleDownloader>> {
        let g = self
            .active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        g.iter()
            .find(|a| (std::sync::Arc::as_ptr(a) as usize) == ad_ptr)
            .cloned()
    }

    pub(crate) fn active_downloads_len(&self) -> usize {
        self.active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    pub(crate) fn active_download_at(
        &self,
        idx: usize,
    ) -> crate::queuesubsys_host::RunningDownloadSnap {
        let ad_opt = self
            .active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(idx)
            .cloned();
        match ad_opt {
            Some(ad) => crate::queuesubsys_host::RunningDownloadSnap {
                ad_ptr: std::sync::Arc::as_ptr(&ad) as usize,
                file_ptr: ad.file_ptr,
                status: ad.status(),
                last_update_time: ad.last_update_time(),
            },
            None => crate::queuesubsys_host::RunningDownloadSnap {
                ad_ptr: 0,
                file_ptr: 0,
                status: 0,
                last_update_time: 0,
            },
        }
    }

    pub(crate) fn ad_snap(&self, ad_ptr: usize) -> crate::queuesubsys_host::ActiveDownloadSnap {
        match self.find_ad(ad_ptr) {
            Some(ad) => ad.snap_for_coord(),
            None => crate::queuesubsys_host::ActiveDownloadSnap::default(),
        }
    }

    pub(crate) fn ad_finish_analyzer(&self, ad_ptr: usize) -> Option<(bool, Vec<u8>, Vec<u8>)> {
        self.find_ad(ad_ptr).and_then(|ad| ad.finish_analyzer())
    }

    pub(crate) fn ad_action(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        ad_ptr: usize,
        action: i32,
    ) -> bool {
        let Some(ad) = self.find_ad(ad_ptr) else {
            return false;
        };
        match action {
            crate::queuesubsys_host::AD_ACT_STOP => {
                ad.stop();
                false
            }
            crate::queuesubsys_host::AD_ACT_LOG_DEBUG => {
                ad.log_debug();
                false
            }
            crate::queuesubsys_host::AD_ACT_ADD_SERVER_STATS => {
                ad.merge_server_stats(host);
                false
            }
            crate::queuesubsys_host::AD_ACT_COMPLETE_FILE_PARTS => ad.complete_file_parts(host),
            crate::queuesubsys_host::AD_ACT_CANCEL_HANGING => {
                ad.cancel_hanging();
                false
            }
            _ => false,
        }
    }

    pub(crate) fn remove_active_download(&self, ad_ptr: usize) {
        let mut g = self
            .active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(pos) = g
            .iter()
            .position(|a| (std::sync::Arc::as_ptr(a) as usize) == ad_ptr)
        {
            g.remove(pos);
        }
    }

    pub(crate) fn delete_all_active_downloads(&self) {
        let mut cnt = self
            .active_dl_threads
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        while *cnt > 0 {
            cnt = self
                .active_dl_threads
                .1
                .wait(cnt)
                .unwrap_or_else(|e| e.into_inner());
        }
        drop(cnt);
        let drained: Vec<_> = self
            .active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        drop(drained);
    }

    pub(crate) fn spawn_article_download(
        self: &std::sync::Arc<Self>,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        file_ptr: usize,
        article_ptr: usize,
        conn_ptr: usize,
        with_analyzer: bool,
        info_name: &[u8],
    ) {
        let ad = std::sync::Arc::new(crate::articledl::NzbgetRsArticleDownloader::new_for_queue(
            file_ptr,
            article_ptr,
            with_analyzer,
            crate::ffi::build_parcheck_enabled(),
            conn_ptr,
            info_name.to_vec(),
        ));
        let ad_ptr = std::sync::Arc::as_ptr(&ad) as usize;
        host.article_set_int(
            article_ptr,
            crate::queuesubsys_host::ART_INT_STATUS,
            ARTICLE_RUNNING,
        );
        let f = host.file_snap(file_ptr);
        host.file_set_int(
            file_ptr,
            crate::queuesubsys_host::FILE_INT_ACTIVE_DOWNLOADS,
            f.active_downloads + 1,
        );
        let n = host.nzb_snap(f.nzb_ptr, false);
        host.nzb_set_int(
            f.nzb_ptr,
            crate::queuesubsys_host::NZB_INT_ACTIVE_DOWNLOADS,
            n.active_downloads + 1,
        );
        self.active_downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(std::sync::Arc::clone(&ad));
        {
            let mut cnt = self
                .active_dl_threads
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *cnt += 1;
        }
        let coord_cl = std::sync::Arc::clone(self);
        if std::thread::Builder::new()
            .name("article-dl".into())
            .spawn(move || {
                struct ActiveDlGuard(std::sync::Arc<NzbgetRsQueueCoordinator>);
                impl Drop for ActiveDlGuard {
                    fn drop(&mut self) {
                        let mut cnt = self
                            .0
                            .active_dl_threads
                            .0
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        *cnt = cnt.saturating_sub(1);
                        self.0.active_dl_threads.1.notify_all();
                    }
                }
                let _cnt_guard = ActiveDlGuard(std::sync::Arc::clone(&coord_cl));
                {
                    let _tg = crate::ffi::RustThreadGuard::enter(&ad.stopped);
                    if let Some(qh) = crate::ffi::queue_host() {
                        ad.run_with_coord(&qh, &coord_cl);
                    }
                }
                drop(ad);
                crate::ffi::stop_ssl_thread();
            })
            .is_err()
        {
            self.remove_active_download(ad_ptr);
            // The worker never ran, so hand back the connection run() reserved for it.
            if conn_ptr != 0 {
                if let Some(pool) = crate::net::active_server_pool() {
                    pool.free_connection(conn_ptr as *mut crate::net::PooledNntpConn, false);
                }
            }
            host.article_set_int(
                article_ptr,
                crate::queuesubsys_host::ART_INT_STATUS,
                ARTICLE_UNDEFINED,
            );
            host.file_set_int(
                file_ptr,
                crate::queuesubsys_host::FILE_INT_ACTIVE_DOWNLOADS,
                f.active_downloads,
            );
            host.nzb_set_int(
                f.nzb_ptr,
                crate::queuesubsys_host::NZB_INT_ACTIVE_DOWNLOADS,
                n.active_downloads,
            );
            let mut cnt = self
                .active_dl_threads
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *cnt = cnt.saturating_sub(1);
            self.active_dl_threads.1.notify_all();
        }
    }

    pub(crate) fn on_article_finished(
        &self,
        ad: &std::sync::Arc<crate::articledl::NzbgetRsArticleDownloader>,
        host: &crate::queuesubsys_host::QueueSubsysHost,
    ) {
        host.log(
            crate::queuesubsys_host::MK_DEBUG,
            b"Notification from ArticleDownloader received",
        );
        let st = ad.status();
        if st == crate::queuesubsys_host::AD_FINISHED
            || st == crate::queuesubsys_host::AD_FAILED
            || st == crate::queuesubsys_host::AD_RETRY
            || st == crate::queuesubsys_host::AD_FATAL_ERROR
        {
            self.article_completed(host, std::sync::Arc::as_ptr(ad) as usize);
        }
    }
}

#[cfg(unix)]
static DAEMON_QC_INST: std::sync::Mutex<Option<std::sync::Arc<NzbgetRsQueueCoordinator>>> =
    std::sync::Mutex::new(None);
#[cfg(unix)]
static DAEMON_QC_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(unix)]
static DAEMON_QC_HANDLE: std::sync::Mutex<Option<std::thread::JoinHandle<()>>> =
    std::sync::Mutex::new(None);

#[cfg(unix)]
pub(crate) fn create_daemon_queue_coord() {
    let qc = std::sync::Arc::new(NzbgetRsQueueCoordinator::new());
    crate::ffi::set_active_queue_coord(Some(std::sync::Arc::downgrade(&qc)));
    if let Some(lg) = crate::log::active_log() {
        lg.register_debuggable(5);
    }
    if let Some(qh) = crate::ffi::queue_host() {
        qc.init(&qh);
    }
    if let Ok(mut g) = DAEMON_QC_INST.lock() {
        *g = Some(qc);
    }
}

#[cfg(unix)]
pub(crate) fn start_daemon_queue_coord() {
    let mut hg = DAEMON_QC_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
    if hg.is_some() {
        return;
    }
    let Some(qc) = DAEMON_QC_INST.lock().ok().and_then(|g| g.clone()) else {
        return;
    };
    qc.stopped.store(false, std::sync::atomic::Ordering::SeqCst);
    DAEMON_QC_RUNNING.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Ok(h) = std::thread::Builder::new()
        .name("queue-coord".into())
        .spawn(move || {
            struct RunningReset;
            impl Drop for RunningReset {
                fn drop(&mut self) {
                    DAEMON_QC_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
                }
            }
            let _reset = RunningReset;
            let _tg = crate::ffi::RustThreadGuard::enter(&qc.stopped);
            if let Some(qh) = crate::ffi::queue_host() {
                qc.run(&qh);
            }
        })
    {
        *hg = Some(h);
    } else {
        DAEMON_QC_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(unix)]
pub(crate) fn stop_daemon_queue_coord(log_stopping: bool) {
    let Some(qc) = DAEMON_QC_INST.lock().ok().and_then(|g| g.clone()) else {
        return;
    };
    if log_stopping {
        crate::log::log_active_bytes(
            crate::queuesubsys_host::MK_DEBUG,
            b"Stopping QueueCoordinator",
        );
    } else if qc.is_stopped() {
        return;
    }
    qc.stopped.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Some(qh) = crate::ffi::queue_host() {
        qc.stop(&qh);
    }
    let mut g = qc.wait_lock.0.lock().unwrap_or_else(|e| e.into_inner());
    *g = true;
    qc.wait_lock.1.notify_all();
}

#[cfg(unix)]
pub(crate) fn is_daemon_queue_coord_running() -> bool {
    DAEMON_QC_RUNNING.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(unix)]
pub(crate) fn daemon_queue_coord_has_more_jobs() -> bool {
    DAEMON_QC_INST
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .is_some_and(|qc| qc.has_more_jobs())
}

#[cfg(unix)]
pub(crate) fn shutdown_daemon_queue_coord() {
    stop_daemon_queue_coord(false);
    if let Some(h) = DAEMON_QC_HANDLE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        let _ = h.join();
    }
    DAEMON_QC_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    let qc_opt = DAEMON_QC_INST
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(qc) = qc_opt {
        if let Some(qh) = crate::ffi::queue_host() {
            qc.destroy(&qh);
        }
        if let Some(lg) = crate::log::active_log() {
            lg.unregister_debuggable(5);
        }
        crate::ffi::set_active_queue_coord(None);
    }
}

#[cfg(unix)]
pub(crate) fn notify_from_workstate() {
    if let Some(qc) = crate::ffi::active_queue_coord() {
        crate::log::log_active_bytes(
            crate::queuesubsys_host::MK_DEBUG,
            b"Notification from WorkState received",
        );
        qc.wakeup();
    }
}

#[cfg(unix)]
pub(crate) fn direct_rename_completed_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    nzb_ptr: usize,
) {
    let flen = host.nzb_file_len(nzb_ptr);
    for i in 0..flen {
        let fp = host.nzb_file_at(nzb_ptr, i);
        let f = host.file_snap(fp);
        if opts.server_mode && f.articles_len > 0 {
            host.disk_save_file(fp);
        }
    }

    discard_direct_rename_host(host, opts, nzb_ptr);

    let nzb = host.nzb_snap(nzb_ptr, false);
    host.nzb_set_int(
        nzb_ptr,
        crate::queuesubsys_host::NZB_INT_DIRECT_RENAME_STATUS,
        crate::queuesubsys_host::TS_SUCCESS,
    );

    if opts.par_check != crate::queuesubsys_host::PC_FORCE {
        host.dq_edit_entry(
            dq_ptr,
            nzb.id,
            crate::queuesubsys_host::EA_GROUP_RESUME,
            None,
        );
        host.dq_edit_entry(
            dq_ptr,
            nzb.id,
            crate::queuesubsys_host::EA_GROUP_PAUSE_ALL_PARS,
            None,
        );
    }

    if opts.reorder_files {
        let mut msg = b"Reordering files for ".to_vec();
        msg.extend_from_slice(&nzb.name);
        host.nzb_print_message(nzb_ptr, crate::queuesubsys_host::MK_INFO, &msg);
        host.dq_edit_entry(
            dq_ptr,
            nzb.id,
            crate::queuesubsys_host::EA_GROUP_SORT_FILES,
            None,
        );
    }

    host.nzb_set_int(nzb_ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
    host.dq_save_changed(dq_ptr);

    host.dq_notify(
        dq_ptr,
        crate::queuesubsys_host::ASPECT_NZB_NAMED,
        nzb_ptr,
        0,
    );
}

#[cfg(unix)]
fn discard_direct_rename_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb_ptr: usize,
) {
    let mut discarded_size = 0i64;
    let mut discarded_count = 0i32;
    let flen = host.nzb_file_len(nzb_ptr);

    for i in 0..flen {
        let fp = host.nzb_file_at(nzb_ptr, i);
        let f = host.file_snap(fp);
        let (discard_par, discard_infos) = eval_discard_direct_rename_file(
            f.par_file,
            f.completed_articles,
            f.active_downloads,
            opts.server_mode,
            f.articles_len > 0,
            opts.continue_partial,
            f.cached_articles,
        );

        if discard_par && host.file_try_acquire_flush_lock(fp) {
            discarded_size += f.success_size;
            discarded_count += 1;
            discard_downloaded_articles_host(host, opts, nzb_ptr, fp);
            host.file_release_flush_lock(fp);
        }

        let f_after = host.file_snap(fp);
        if discard_infos && f_after.articles_len > 0 {
            let nzb = host.nzb_snap(nzb_ptr, false);
            let mut dbg = b"Discarding article infos for ".to_vec();
            dbg.extend_from_slice(&nzb.name);
            dbg.push(b'/');
            dbg.extend_from_slice(&f_after.filename);
            host.log(crate::queuesubsys_host::MK_DEBUG, &dbg);
            host.file_set_int(fp, crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED, 1);
            save_partial_state_host(host, fp);
            host.file_action(fp, crate::queuesubsys_host::FILE_ACT_CLEAR_ARTICLES, 0);
        }
    }

    if discarded_size > 0 {
        let fmt_sz = crate::util::format_size(discarded_size);
        let mut msg = b"Discarded ".to_vec();
        msg.extend_from_slice(&fmt_sz);
        msg.extend_from_slice(
            format!(" from {} files used for direct renaming", discarded_count).as_bytes(),
        );
        host.nzb_print_message(nzb_ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
    }
}

#[cfg(unix)]
fn discard_downloaded_articles_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb_ptr: usize,
    file_ptr: usize,
) {
    host.file_action(
        file_ptr,
        crate::queuesubsys_host::FILE_ACT_SUBTRACT_AND_CLEAR_SERVER_STATS,
        nzb_ptr,
    );
    let nzb = host.nzb_snap(nzb_ptr, false);
    let file = host.file_snap(file_ptr);
    let (
        next_nzb_rem_size,
        next_nzb_paused_size,
        next_nzb_cur_succ_art,
        next_nzb_cur_succ_size,
        next_nzb_par_cur_succ_size,
        next_nzb_cur_fail_art,
        next_nzb_cur_fail_size,
        next_nzb_par_cur_fail_size,
        next_file_rem_size,
    ) = discard_downloaded_articles_counters(
        nzb.remaining_size,
        nzb.paused_size,
        nzb.current_success_articles,
        nzb.current_success_size,
        nzb.par_current_success_size,
        nzb.current_failed_articles,
        nzb.current_failed_size,
        nzb.par_current_failed_size,
        i32::from(file.paused),
        file.success_articles,
        file.success_size,
        file.failed_articles,
        file.failed_size,
        file.size,
        file.missed_size,
    );

    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_REMAINING_SIZE,
        next_nzb_rem_size,
    );
    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_PAUSED_SIZE,
        next_nzb_paused_size,
    );
    host.nzb_set_int(
        nzb_ptr,
        crate::queuesubsys_host::NZB_INT_CUR_SUCCESS_ARTICLES,
        next_nzb_cur_succ_art,
    );
    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_CUR_SUCCESS_SIZE,
        next_nzb_cur_succ_size,
    );
    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_PAR_CUR_SUCCESS_SIZE,
        next_nzb_par_cur_succ_size,
    );
    host.file_set_i64(file_ptr, crate::queuesubsys_host::FILE_I64_SUCCESS_SIZE, 0);
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_SUCCESS_ARTICLES,
        0,
    );
    host.nzb_set_int(
        nzb_ptr,
        crate::queuesubsys_host::NZB_INT_CUR_FAILED_ARTICLES,
        next_nzb_cur_fail_art,
    );
    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_CUR_FAILED_SIZE,
        next_nzb_cur_fail_size,
    );
    host.nzb_set_i64(
        nzb_ptr,
        crate::queuesubsys_host::NZB_I64_PAR_CUR_FAILED_SIZE,
        next_nzb_par_cur_fail_size,
    );
    host.file_set_i64(file_ptr, crate::queuesubsys_host::FILE_I64_FAILED_SIZE, 0);
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_FAILED_ARTICLES,
        0,
    );
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_COMPLETED_ARTICLES,
        0,
    );
    host.file_set_i64(
        file_ptr,
        crate::queuesubsys_host::FILE_I64_REMAINING_SIZE,
        next_file_rem_size,
    );

    discard_temp_files_host(host, opts, &file);
    host.disk_discard_file(file.id, false, true, false);

    host.file_set_str(
        file_ptr,
        crate::queuesubsys_host::FILE_STR_OUTPUT_FILENAME,
        None,
    );
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_OUTPUT_INITIALIZED,
        0,
    );
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_CACHED_ARTICLES,
        0,
    );
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED,
        0,
    );
    host.file_set_int(
        file_ptr,
        crate::queuesubsys_host::FILE_INT_PARTIAL_STATE,
        crate::queuesubsys_host::PARTIAL_NONE,
    );

    if opts.server_mode {
        host.file_action(
            file_ptr,
            crate::queuesubsys_host::FILE_ACT_CLEAR_ARTICLES,
            0,
        );
    } else {
        host.file_action(
            file_ptr,
            crate::queuesubsys_host::FILE_ACT_RESET_ARTICLES_NONSERV,
            0,
        );
    }
}

#[cfg(unix)]
fn discard_temp_files_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    file: &crate::queuesubsys_host::FileSnap,
) {
    let unlink_bytes = |b: &[u8]| {
        let c = crate::queuesubsys_host::c_bytes(b);
        if let Ok(cs) = CStr::from_bytes_with_nul(&c) {
            let _ = crate::fsops::delete_file(cs);
        }
    };
    host.file_action(
        file.ptr,
        crate::queuesubsys_host::FILE_ACT_DISCARD_TEMP_ARTICLES,
        usize::from(!opts.direct_write && !file.force_direct_write),
    );
    if opts.direct_write && !file.output_filename.is_empty() && !file.force_direct_write {
        unlink_bytes(&file.output_filename);
        if file.is_hard_linked && !file.hard_link_path.is_empty() {
            unlink_bytes(&file.hard_link_path);
        }
    }
}

#[cfg(unix)]
fn save_partial_state_host(host: &crate::queuesubsys_host::QueueSubsysHost, file_ptr: usize) {
    let file = host.file_snap(file_ptr);
    if file.partial_changed {
        let mut dbg = b"Saving partial state for ".to_vec();
        dbg.extend_from_slice(&file.filename);
        host.log(crate::queuesubsys_host::MK_DEBUG, &dbg);
        if file.partial_state == crate::queuesubsys_host::PARTIAL_COMPLETED {
            host.disk_discard_file(file.id, false, false, true);
        }
        host.disk_save_file_state(file.ptr, false);
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_PARTIAL_CHANGED,
            0,
        );
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_PARTIAL_STATE,
            crate::queuesubsys_host::PARTIAL_PARTIAL,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_coordinator_helpers() {
        assert!(is_data_filename(b"Movie.2026.mkv"));
        assert!(is_data_filename(b"archive.r00"));
        assert!(is_data_filename(b"split.001"));
        assert!(!is_data_filename(b"checksum.par2"));

        assert!(is_side_file(b"release.NFO", b""));
        assert!(is_side_file(b"sample.srs", b""));
        assert!(is_side_file(b"extra.sfv", b".par2, .nzb"));
        assert!(!is_side_file(b"repair.par2", b".par2, .nfo"));

        let eval = eval_dead_download(0, 0, 250, 10, 45, true, 900, 20, 4, 15);
        assert_eq!(eval.dead_rate, 1);
        assert_eq!(eval.dead_run, 1);
        assert_eq!(eval.dead, 1);
        assert_eq!(eval.hopeless, 1);

        let names: &[&[u8]] = &[b"a.rar", b"b.rar", b"a.rar"];
        let sizes: &[i64] = &[100, 200, 300];
        let mut out = [0usize; 3];
        assert_eq!(find_dupe_file_indices(names, sizes, &mut out), 1);
        assert_eq!(out[0], 0);

        let (uniq, formatted) = format_donor_id_list(&[3, 1, 3, 2]);
        assert_eq!(uniq, vec![1, 2, 3]);
        assert_eq!(formatted, b"1, 2, 3");

        let mut d = NzbgetRsNzbMergeSplitCounters {
            file_count: 2,
            size: 100,
            min_time: 50,
            max_time: 60,
            ..Default::default()
        };
        let s = NzbgetRsNzbMergeSplitCounters {
            file_count: 3,
            size: 200,
            min_time: 40,
            max_time: 80,
            ..Default::default()
        };
        merge_counters(&mut d, &s);
        assert_eq!(
            (d.file_count, d.size, d.min_time, d.max_time),
            (5, 300, 40, 80)
        );
    }

    #[test]
    fn propagation_wait_disabled_test() {
        let now: i64 = 1_760_000_000;
        assert!(!is_propagation_wait(now, 0, now));
        assert!(!is_propagation_wait(now + 60, 0, now));
    }

    #[test]
    fn propagation_wait_test() {
        let now: i64 = 1_760_000_000;
        let delay: i32 = 15 * 60;

        // posted just now or within the delay: held back
        assert!(is_propagation_wait(now, delay, now));
        assert!(is_propagation_wait(now - 14 * 60, delay, now));
        assert!(is_propagation_wait(now - delay as i64, delay, now));

        // posted before the delay: downloadable
        assert!(!is_propagation_wait(now - delay as i64 - 1, delay, now));
        assert!(!is_propagation_wait(now - 3600, delay, now));
    }

    #[test]
    fn propagation_wait_per_file_test() {
        let now: i64 = 1_760_000_000;
        let delay: i32 = 15 * 60;
        let mkv_posted = now - 3600;
        let nfo_posted = now - 60;

        assert!(!is_propagation_wait(mkv_posted, delay, now));
        assert!(is_propagation_wait(nfo_posted, delay, now));
        assert!(!is_propagation_wait(
            nfo_posted,
            delay,
            nfo_posted + delay as i64 + 1
        ));
    }

    #[cfg(unix)]
    #[test]
    fn reset_hanging_downloads_uses_minimal_running_snapshot() {
        use std::ffi::{c_char, c_int, c_void};
        struct State {
            cancelled: Vec<usize>,
            saw_non_null_strs_on_running_snap: bool,
        }
        unsafe extern "C" fn cb(
            ctx: *mut c_void,
            op: c_int,
            a1: c_int,
            _a2: c_int,
            p1: usize,
            p2: usize,
            _s1: *const c_char,
            _s2: *const c_char,
            ints: *mut c_int,
            i64s: *mut i64,
            strs: *mut *const c_char,
        ) -> usize {
            let st = &mut *(ctx as *mut State);
            match op {
                1 => {
                    *ints.add(13) = 30; // article_timeout
                    *i64s = 1_000; // cur_time
                    0
                }
                20 => {
                    let f: unsafe extern "C" fn(*mut c_void, usize) = std::mem::transmute(p1);
                    f(p2 as *mut c_void, 0xDEAD);
                    0
                }
                50 => {
                    if a1 < 0 {
                        return 2;
                    }
                    if !strs.is_null() {
                        st.saw_non_null_strs_on_running_snap = true;
                    }
                    if a1 == 0 {
                        *ints = crate::queuesubsys_host::AD_RUNNING;
                        *i64s = 0x100;
                        *i64s.add(1) = 990; // fresh: not hanging
                        0xAD01
                    } else {
                        *ints = crate::queuesubsys_host::AD_RUNNING;
                        *i64s = 0x200;
                        *i64s.add(1) = 900; // stale > 30s: hanging
                        0xAD02
                    }
                }
                51 => {
                    if a1 == crate::queuesubsys_host::AD_ACT_CANCEL_HANGING {
                        st.cancelled.push(p1);
                    }
                    0
                }
                _ => 0,
            }
        }
        let mut st = State {
            cancelled: Vec::new(),
            saw_non_null_strs_on_running_snap: false,
        };
        let host = crate::queuesubsys_host::QueueSubsysHost::new_mock(
            &mut st as *mut State as *mut c_void,
            cb,
        );
        let coord = NzbgetRsQueueCoordinator::new();
        coord.reset_hanging_downloads(&host);
        assert!(!st.saw_non_null_strs_on_running_snap);
        assert_eq!(st.cancelled, vec![0xAD02]);
    }
}
