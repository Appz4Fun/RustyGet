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
    propagation_delay > 0
        && (post_time as i32).wrapping_add(propagation_delay) >= (cur_date as i32)
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
        b".mkv", b".mp4", b".avi", b".m4v", b".ts", b".m2ts", b".mov", b".wmv", b".mpg",
        b".mpeg", b".iso", b".rar", b".7z", b".zip", b".nfo", b".sfv", b".srr", b".srt",
        b".sub", b".idx", b".jpg", b".png", b".txt", b".flac", b".mp3", b".m4a",
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

    let dead_rate = failed >= DEAD_DOWNLOAD_FAILURES + file_count
        && (own as i64) * 100 < (failed as i64);
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
pub fn is_hanging_download(now: i64, last_update: i64, article_timeout: i32, is_running: bool) -> bool {
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
        by_name.entry(name).or_default().push(((idx + 1) as i32, idx));
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
    dest.current_success_size = dest.current_success_size.wrapping_add(src.current_success_size);
    dest.failed_size = dest.failed_size.wrapping_add(src.failed_size);
    dest.current_failed_size = dest.current_failed_size.wrapping_add(src.current_failed_size);

    dest.par_size = dest.par_size.wrapping_add(src.par_size);
    dest.par_success_size = dest.par_success_size.wrapping_add(src.par_success_size);
    dest.par_current_success_size = dest
        .par_current_success_size
        .wrapping_add(src.par_current_success_size);
    dest.par_failed_size = dest.par_failed_size.wrapping_add(src.par_failed_size);
    dest.par_current_failed_size = dest
        .par_current_failed_size
        .wrapping_add(src.par_current_failed_size);
    dest.remaining_par_count = dest.remaining_par_count.wrapping_add(src.remaining_par_count);

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
        assert_eq!((d.file_count, d.size, d.min_time, d.max_time), (5, 300, 40, 80));
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
}
