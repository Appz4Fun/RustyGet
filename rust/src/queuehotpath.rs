//! Rust hot-path port for `DownloadInfo`, `ArticleWriter`, `ArticleCache`,
//! `MemoryMonitor`, `QueueCoordinator`, `HistoryCoordinator`, and `QueueEditor`.

use std::ffi::{c_char, c_int, CStr};

extern "C" {
    fn tolower(c: c_int) -> c_int;
}

#[inline]
fn c_lower(b: u8) -> u8 {
    unsafe { tolower(b as c_int) as u8 }
}

#[inline]
pub fn strcasecmp_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(&x, &y)| c_lower(x) == c_lower(y))
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
// DownloadInfo (daemon/queue/DownloadInfo.cpp)
// ============================================================================

pub const SCRIPT_STATUS_NONE: i32 = 0;
pub const SCRIPT_STATUS_FAILURE: i32 = 1;
pub const SCRIPT_STATUS_SUCCESS: i32 = 2;

/// `ScriptStatusList::CalcTotalStatus` (`DownloadInfo.cpp`):
/// Failure overrides Success; Success overrides None.
pub fn script_calc_total_status(statuses: &[i32]) -> i32 {
    let mut status = SCRIPT_STATUS_NONE;
    for &s in statuses {
        if (s == SCRIPT_STATUS_SUCCESS && status == SCRIPT_STATUS_NONE)
            || s == SCRIPT_STATUS_FAILURE
        {
            status = s;
        }
    }
    status
}

/// `DirNameOf` (`DownloadInfo.cpp`):
/// Truncates directory name to `255 - reserve` bytes on a UTF-8 codepoint boundary,
/// prepends `_` if starting with `.`, and strips trailing `.` or `' '` when truncated.
pub fn dir_name_of(name: &[u8], reserve: usize) -> Vec<u8> {
    let mut dir_name: Vec<u8> = if name.is_empty() {
        b"nzb".to_vec()
    } else {
        name.to_vec()
    };
    if dir_name.first() == Some(&b'.') {
        dir_name.insert(0, b'_');
    }
    let max_len = 255usize.saturating_sub(reserve);
    if dir_name.len() > max_len {
        let mut cut = max_len;
        while cut > 0 && (dir_name[cut] & 0xC0) == 0x80 {
            cut -= 1;
        }
        dir_name.truncate(cut);
        while let Some(&last) = dir_name.last() {
            if last == b'.' || last == b' ' {
                dir_name.pop();
            } else {
                break;
            }
        }
    }
    dir_name
}

/// `NzbInfo::CalcHealth` (`DownloadInfo.cpp`).
pub fn calc_health(
    size: i64,
    par_size: i64,
    current_failed_size: i64,
    par_current_failed_size: i64,
) -> i32 {
    let data_size = size - par_size;
    if current_failed_size == 0 || size == par_size || data_size <= 0 {
        return 1000;
    }
    let data_failed = current_failed_size - par_current_failed_size;
    let mut health = ((data_size - data_failed) * 1000 / data_size) as i32;
    if health == 1000 && data_failed > 0 {
        health = 999;
    }
    health
}

/// `NzbInfo::CalcCriticalHealth` (`DownloadInfo.cpp`).
pub fn calc_critical_health(
    size: i64,
    par_size: i64,
    par_current_failed_size: i64,
    allow_estimation: bool,
) -> i32 {
    if size == 0 {
        return 1000;
    }
    if size == par_size {
        return 0;
    }
    let good_par_size = par_size - par_current_failed_size;
    let denom = size - good_par_size;
    let mut critical_health = if denom != 0 {
        ((size - good_par_size * 2) * 1000 / denom) as i32
    } else {
        0
    };
    if good_par_size * 2 > size {
        critical_health = 0;
    } else if critical_health == 1000 && par_size > 0 {
        critical_health = 999;
    }
    if critical_health == 1000 && allow_estimation {
        critical_health = 850;
    }
    critical_health
}

/// `NzbInfo::UpdateMinMaxTime` (`DownloadInfo.cpp`).
pub fn calc_min_max_time(times: &[i64]) -> (i64, i64) {
    let mut min_time = 0i64;
    let mut max_time = 0i64;
    let mut first = true;
    for &t in times {
        if first {
            min_time = t;
            max_time = t;
            first = false;
        }
        if t > 0 {
            if t < min_time {
                min_time = t;
            }
            if t > max_time {
                max_time = t;
            }
        }
    }
    (min_time, max_time)
}

pub const NZB_KIND_NZB: i32 = 0;
pub const NZB_KIND_URL: i32 = 1;

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

pub const MOVE_NONE: i32 = 0;
pub const MOVE_FAILURE: i32 = 1;
pub const MOVE_SUCCESS: i32 = 2;

pub const URL_NONE: i32 = 0;
pub const URL_RUNNING: i32 = 1;
pub const URL_FINISHED: i32 = 2;
pub const URL_FAILED: i32 = 3;
pub const URL_RETRY: i32 = 4;
pub const URL_SCAN_SKIPPED: i32 = 5;
pub const URL_SCAN_FAILED: i32 = 6;

/// `NzbInfo::IsDupeSuccess` (`DownloadInfo.cpp`).
#[allow(clippy::too_many_arguments)]
pub fn is_dupe_success(
    mark_status: i32,
    delete_status: i32,
    par_status: i32,
    unpack_status: i32,
    url_status: i32,
    health: i32,
    critical_health_est: i32,
    critical_health_exact: i32,
    health_check_dupe: bool,
) -> bool {
    let failure = mark_status != MARK_SUCCESS
        && mark_status != MARK_GOOD
        && (delete_status != DELETE_NONE
            || mark_status == MARK_BAD
            || par_status == PAR_FAILURE
            || unpack_status == UNPACK_FAILURE
            || unpack_status == UNPACK_PASSWORD
            || url_status == URL_FAILED
            || url_status == URL_SCAN_SKIPPED
            || url_status == URL_SCAN_FAILED
            || (par_status == PAR_SKIPPED
                && unpack_status == UNPACK_SKIPPED
                && health < critical_health_est)
            || (health_check_dupe
                && (par_status == PAR_NONE || par_status == PAR_SKIPPED)
                && (unpack_status == UNPACK_NONE || unpack_status == UNPACK_SKIPPED)
                && health < critical_health_exact));
    !failure
}

/// `NzbInfo::MakeTextStatus` (`DownloadInfo.cpp`):
/// Returns a static null-terminated status literal matching `MakeTextStatus`.
#[allow(clippy::too_many_arguments)]
pub fn make_text_status_cstr(
    kind: i32,
    mark_status: i32,
    delete_status: i32,
    par_status: i32,
    unpack_status: i32,
    move_status: i32,
    url_status: i32,
    script_status: i32,
    health: i32,
    critical_health: i32,
) -> &'static [u8] {
    if kind == NZB_KIND_NZB {
        if mark_status == MARK_BAD {
            return b"FAILURE/BAD\0";
        } else if mark_status == MARK_GOOD {
            return b"SUCCESS/GOOD\0";
        } else if mark_status == MARK_SUCCESS {
            return b"SUCCESS/MARK\0";
        } else if delete_status == DELETE_HEALTH {
            return b"FAILURE/HEALTH\0";
        } else if delete_status == DELETE_MANUAL {
            return b"DELETED/MANUAL\0";
        } else if delete_status == DELETE_DUPE {
            return b"DELETED/DUPE\0";
        } else if delete_status == DELETE_BAD {
            return b"FAILURE/BAD\0";
        } else if delete_status == DELETE_GOOD {
            return b"DELETED/GOOD\0";
        } else if delete_status == DELETE_COPY {
            return b"DELETED/COPY\0";
        } else if delete_status == DELETE_SCAN {
            return b"FAILURE/SCAN\0";
        } else if par_status == PAR_FAILURE {
            return b"FAILURE/PAR\0";
        } else if unpack_status == UNPACK_FAILURE {
            return b"FAILURE/UNPACK\0";
        } else if move_status == MOVE_FAILURE {
            return b"FAILURE/MOVE\0";
        } else if par_status == PAR_MANUAL {
            return b"WARNING/DAMAGED\0";
        } else if par_status == PAR_REPAIR_POSSIBLE {
            return b"WARNING/REPAIRABLE\0";
        } else if (par_status == PAR_NONE || par_status == PAR_SKIPPED)
            && (unpack_status == UNPACK_NONE || unpack_status == UNPACK_SKIPPED)
            && health < critical_health
        {
            return b"FAILURE/HEALTH\0";
        } else if (par_status == PAR_NONE || par_status == PAR_SKIPPED)
            && (unpack_status == UNPACK_NONE || unpack_status == UNPACK_SKIPPED)
            && health < 1000
            && health >= critical_health
        {
            return b"WARNING/HEALTH\0";
        } else if (par_status == PAR_NONE || par_status == PAR_SKIPPED)
            && (unpack_status == UNPACK_NONE || unpack_status == UNPACK_SKIPPED)
            && script_status != SCRIPT_STATUS_FAILURE
            && health == 1000
        {
            return b"SUCCESS/HEALTH\0";
        } else if unpack_status == UNPACK_SPACE {
            return b"WARNING/SPACE\0";
        } else if unpack_status == UNPACK_PASSWORD {
            return b"WARNING/PASSWORD\0";
        } else if (unpack_status == UNPACK_SUCCESS
            || ((unpack_status == UNPACK_NONE || unpack_status == UNPACK_SKIPPED)
                && par_status == PAR_SUCCESS))
            && script_status == SCRIPT_STATUS_SUCCESS
        {
            return b"SUCCESS/ALL\0";
        } else if unpack_status == UNPACK_SUCCESS && script_status == SCRIPT_STATUS_NONE {
            return b"SUCCESS/UNPACK\0";
        } else if par_status == PAR_SUCCESS && script_status == SCRIPT_STATUS_NONE {
            return b"SUCCESS/PAR\0";
        } else if script_status == SCRIPT_STATUS_FAILURE {
            return b"WARNING/SCRIPT\0";
        }
    } else if kind == NZB_KIND_URL {
        if delete_status == DELETE_MANUAL {
            return b"DELETED/MANUAL\0";
        } else if delete_status == DELETE_DUPE {
            return b"DELETED/DUPE\0";
        } else if delete_status == DELETE_GOOD {
            return b"DELETED/GOOD\0";
        } else {
            return match url_status {
                URL_FAILED => b"FAILURE/FETCH\0",
                URL_SCAN_SKIPPED => b"WARNING/SKIPPED\0",
                URL_SCAN_FAILED => b"FAILURE/SCAN\0",
                _ => b"FAILURE/INTERNAL_ERROR\0",
            };
        }
    }
    b"FAILURE/INTERNAL_ERROR\0"
}

/// `CompletedFile::SameFilename` (`DownloadInfo.cpp`):
/// Case-insensitive comparison normalizing `\` to `/`.
pub fn completed_same_filename(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(&c1, &c2)| {
        let n1 = if c1 == b'\\' { b'/' } else { c1 };
        let n2 = if c2 == b'\\' { b'/' } else { c2 };
        c_lower(n1) == c_lower(n2)
    })
}

// ============================================================================
// ArticleWriter, ArticleCache, and MemoryMonitor (daemon/nntp/ArticleWriter.cpp)
// ============================================================================

/// `ArticleWriter::SetWriteBuffer` (`ArticleWriter.cpp`).
pub fn calc_write_buffer_size(write_buffer_kb: i32, rec_size: i32) -> i32 {
    if write_buffer_kb <= 0 {
        return 0;
    }
    let max_bytes = write_buffer_kb.saturating_mul(1024);
    if rec_size > 0 && rec_size < max_bytes {
        rec_size
    } else {
        max_bytes
    }
}

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

pub const PRESSURE_NORMAL: i32 = 0;
pub const PRESSURE_CONSTRAINED: i32 = 1;
pub const PRESSURE_CRITICAL: i32 = 2;

/// `ArticleCache::Run` (`ArticleWriter.cpp`):
/// Evaluates whether the cache loop should call `CheckFlush` and with what flags.
#[allow(clippy::too_many_arguments)]
pub fn article_cache_should_flush(
    allocated: usize,
    effective_limit: usize,
    tier: i32,
    just_flushed: bool,
    reset_counter: i32,
    is_stopped: bool,
    direct_write: bool,
) -> (bool, bool, bool) {
    let is_critical = tier == PRESSURE_CRITICAL;
    let is_constrained = tier == PRESSURE_CONSTRAINED;
    let fill_threshold = if effective_limit > 0 {
        (effective_limit / 100) * 90
    } else {
        0
    };
    let need_emergency_flush = is_critical && allocated > 0;
    let need_constrained_flush = is_constrained && allocated > effective_limit;

    let should_flush = (just_flushed
        || reset_counter >= 1000
        || is_stopped
        || need_emergency_flush
        || need_constrained_flush
        || (direct_write && allocated >= fill_threshold))
        && allocated > 0;

    let flush_everything = is_stopped || need_emergency_flush || allocated >= fill_threshold;
    (should_flush, flush_everything, need_emergency_flush)
}

/// `MemoryMonitor::UpdateState` (`ArticleWriter.cpp`):
/// Immediate escalation on higher pressure (1 sample) and 2-sample hysteresis on de-escalation.
/// Returns `(next_tier, transitioned)`.
pub fn memory_monitor_step(
    raw_sample: i32,
    current_tier: i32,
    candidate_tier: &mut i32,
    consecutive_samples: &mut i32,
) -> (i32, bool) {
    let mut next_tier = current_tier;
    let mut transitioned = false;

    if raw_sample > current_tier {
        next_tier = raw_sample;
        *candidate_tier = raw_sample;
        *consecutive_samples = 0;
        transitioned = true;
    } else if raw_sample < current_tier {
        if raw_sample == *candidate_tier {
            *consecutive_samples += 1;
            if *consecutive_samples >= 2 {
                next_tier = raw_sample;
                *consecutive_samples = 0;
                transitioned = true;
            }
        } else {
            *candidate_tier = raw_sample;
            *consecutive_samples = 1;
        }
    } else {
        *candidate_tier = raw_sample;
        *consecutive_samples = 0;
    }

    (next_tier, transitioned)
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

// ============================================================================
// HistoryCoordinator (daemon/queue/HistoryCoordinator.cpp)
// ============================================================================

/// `HistoryCoordinator::AddToHistory` (`HistoryCoordinator.cpp`):
/// Determines `cleanupParkedFiles`.
pub fn should_cleanup_parked_files(
    par_status: i32,
    unpack_status: i32,
    failed_size: i64,
    par_failed_size: i64,
    delete_status: i32,
    parking: bool,
    unpack_cleaned_up_disk: bool,
) -> bool {
    let mut cleanup = ((par_status == PAR_SUCCESS || par_status == PAR_REPAIR_POSSIBLE)
        && unpack_status != UNPACK_FAILURE
        && unpack_status != UNPACK_SPACE
        && unpack_status != UNPACK_PASSWORD)
        || (unpack_status == UNPACK_SUCCESS && par_status != PAR_FAILURE)
        || (unpack_status <= UNPACK_SKIPPED
            && par_status != PAR_FAILURE
            && (failed_size - par_failed_size) == 0)
        || (delete_status != DELETE_NONE);

    cleanup &= !parking;
    cleanup |= unpack_cleaned_up_disk;
    cleanup
}

pub const DUP_STATUS_UNDEFINED: i32 = 0;
pub const DUP_STATUS_SUCCESS: i32 = 1;
pub const DUP_STATUS_FAILED: i32 = 2;
pub const DUP_STATUS_DELETED: i32 = 3;
pub const DUP_STATUS_DUPE: i32 = 4;
pub const DUP_STATUS_BAD: i32 = 5;
pub const DUP_STATUS_GOOD: i32 = 6;

/// `HistoryCoordinator::HistoryHide` (`HistoryCoordinator.cpp`):
/// Maps `NzbInfo` mark/delete/success statuses to `DupInfo::EStatus`.
pub fn classify_dup_status(mark_status: i32, delete_status: i32, is_dupe_success: bool) -> i32 {
    if mark_status == MARK_GOOD {
        DUP_STATUS_GOOD
    } else if mark_status == MARK_BAD {
        DUP_STATUS_BAD
    } else if mark_status == MARK_SUCCESS {
        DUP_STATUS_SUCCESS
    } else if delete_status == DELETE_DUPE {
        DUP_STATUS_DUPE
    } else if delete_status == DELETE_MANUAL
        || delete_status == DELETE_GOOD
        || delete_status == DELETE_COPY
    {
        DUP_STATUS_DELETED
    } else if is_dupe_success {
        DUP_STATUS_SUCCESS
    } else {
        DUP_STATUS_FAILED
    }
}

pub const ARTICLE_UNDEFINED: i32 = 0;
pub const ARTICLE_RUNNING: i32 = 1;
pub const ARTICLE_FINISHED: i32 = 2;
pub const ARTICLE_FAILED: i32 = 3;

pub const PARTIAL_STATE_NONE: i32 = 0;
pub const PARTIAL_STATE_PARTIAL: i32 = 1;
pub const PARTIAL_STATE_COMPLETED: i32 = 2;

/// `HistoryCoordinator::ResetArticles` (`HistoryCoordinator.cpp`):
/// Determines whether an article should be reset to `aiUndefined` during `HistoryRetry`.
pub fn should_reset_article(
    article_status: i32,
    partial_state: i32,
    all_failed: bool,
    reset_failed: bool,
) -> bool {
    (article_status == ARTICLE_FAILED && (reset_failed || partial_state == PARTIAL_STATE_NONE))
        || (article_status == ARTICLE_UNDEFINED && reset_failed && all_failed)
        || (article_status == ARTICLE_FINISHED && partial_state == PARTIAL_STATE_NONE)
}

pub const DUPE_MODE_SCORE: i32 = 0;
pub const DUPE_MODE_ALL: i32 = 1;
pub const DUPE_MODE_FORCE: i32 = 2;

/// `HistoryCoordinator::HistorySetDupeParam` & `QueueEditor::SetNzbDupeParam`:
/// Parses `"SCORE"`, `"ALL"`, `"FORCE"` case-insensitively.
pub fn parse_dupe_mode(text: &[u8]) -> Option<i32> {
    if strcasecmp_bytes(text, b"SCORE") {
        Some(DUPE_MODE_SCORE)
    } else if strcasecmp_bytes(text, b"ALL") {
        Some(DUPE_MODE_ALL)
    } else if strcasecmp_bytes(text, b"FORCE") {
        Some(DUPE_MODE_FORCE)
    } else {
        None
    }
}

// ============================================================================
// QueueEditor (daemon/queue/QueueEditor.cpp)
// ============================================================================

pub const SORT_CRITERIA_NAME: i32 = 0;
pub const SORT_CRITERIA_SIZE: i32 = 1;
pub const SORT_CRITERIA_REMAINING_SIZE: i32 = 2;
pub const SORT_CRITERIA_AGE: i32 = 3;
pub const SORT_CRITERIA_CATEGORY: i32 = 4;
pub const SORT_CRITERIA_PRIORITY: i32 = 5;

pub const SORT_ORDER_ASCENDING: i32 = 0;
pub const SORT_ORDER_DESCENDING: i32 = 1;
pub const SORT_ORDER_AUTO: i32 = 2;

/// `GroupSorter::Execute` (`QueueEditor.cpp`):
/// Parses sort specification (`name[+-]`, `size[+-]`, `left[+-]`, `age[+-]`, `category[+-]`, `priority[+-]`).
pub fn parse_sort_spec(sort: &[u8]) -> Option<(i32, i32)> {
    if sort.is_empty() {
        return None;
    }
    let last_ch = *sort.last().unwrap();
    let (base, order) = if last_ch == b'+' {
        (&sort[..sort.len() - 1], SORT_ORDER_ASCENDING)
    } else if last_ch == b'-' {
        (&sort[..sort.len() - 1], SORT_ORDER_DESCENDING)
    } else {
        (sort, SORT_ORDER_AUTO)
    };

    let criteria = if strcasecmp_bytes(base, b"name") {
        SORT_CRITERIA_NAME
    } else if strcasecmp_bytes(base, b"size") {
        SORT_CRITERIA_SIZE
    } else if strcasecmp_bytes(base, b"left") {
        SORT_CRITERIA_REMAINING_SIZE
    } else if strcasecmp_bytes(base, b"age") {
        SORT_CRITERIA_AGE
    } else if strcasecmp_bytes(base, b"category") {
        SORT_CRITERIA_CATEGORY
    } else if strcasecmp_bytes(base, b"priority") {
        SORT_CRITERIA_PRIORITY
    } else {
        return None;
    };

    Some((criteria, order))
}

/// `QueueEditor::PrepareList` (`QueueEditor.cpp`):
/// Computes `workOffset` and updated `lastDestPos` when moving multiple items in queue order.
pub fn clamp_move_offset(
    index: i32,
    nr_entries: i32,
    offset: i32,
    last_dest_pos: i32,
) -> (i32, i32) {
    let mut work_offset = offset;
    let dest_pos = index + work_offset;
    if last_dest_pos == -1 {
        if dest_pos < 0 {
            work_offset = -index;
        } else if dest_pos > nr_entries - 1 {
            work_offset = nr_entries - 1 - index;
        }
    } else if work_offset < 0 && dest_pos <= last_dest_pos {
        work_offset = last_dest_pos - index + 1;
    } else if work_offset > 0 && dest_pos >= last_dest_pos {
        work_offset = last_dest_pos - index - 1;
    }
    let new_last_dest_pos = index + work_offset;
    (work_offset, new_last_dest_pos)
}

/// `QueueEditor::PausePars` (`QueueEditor.cpp`):
/// Sets `out_pause[i] = 1` for every file in the group that `PausePars` should pause.
pub fn select_pars_to_pause(
    is_par: &[u8],
    has_vol: &[u8],
    sizes: &[i64],
    extra_pars_only: bool,
    out_pause: &mut [u8],
) {
    let n = is_par
        .len()
        .min(has_vol.len())
        .min(sizes.len())
        .min(out_pause.len());
    for p in out_pause[..n].iter_mut() {
        *p = 0;
    }
    if !extra_pars_only {
        for i in 0..n {
            if is_par[i] != 0 {
                out_pause[i] = 1;
            }
        }
        return;
    }

    let mut has_main_par = false;
    let mut vols: Vec<usize> = Vec::new();
    for i in 0..n {
        if is_par[i] != 0 {
            if has_vol[i] != 0 {
                vols.push(i);
            } else {
                has_main_par = true;
            }
        }
    }

    if has_main_par {
        for &idx in &vols {
            out_pause[idx] = 1;
        }
    } else {
        let mut smallest: Option<usize> = None;
        for &idx in &vols {
            match smallest {
                None => smallest = Some(idx),
                Some(prev) => {
                    if sizes[prev] > sizes[idx] {
                        out_pause[prev] = 1;
                        smallest = Some(idx);
                    } else {
                        out_pause[idx] = 1;
                    }
                }
            }
        }
    }
}

/// `QueueEditor::SortGroupFiles` (`QueueEditor.cpp`) comparator:
/// Returns `true` if `fileInfo1` should be ordered strictly before `fileInfo2`.
pub fn compare_inner_files(
    name1: &[u8],
    par1: bool,
    size1: i64,
    name2: &[u8],
    par2: bool,
    size2: i64,
    same_par_collection: bool,
) -> bool {
    if !par1 && !par2 {
        let len1 = name1.len();
        let len2 = name2.len();
        let ext1 = name1.iter().rposition(|&b| b == b'.').map(|p| &name1[p..]);
        let ext2 = name2.iter().rposition(|&b| b == b'.').map(|p| &name2[p..]);
        if let (Some(e1), Some(e2)) = (ext1, ext2) {
            if len1 == len2
                && e1.len() == 4
                && e2.len() == 4
                && name1[..len1 - 4] == name2[..len2 - 4]
            {
                if e1 == b".rar"
                    && e2[1] == b'r'
                    && e2[2].is_ascii_digit()
                    && e2[3].is_ascii_digit()
                {
                    return true;
                } else if e2 == b".rar"
                    && e1[1] == b'r'
                    && e1[2].is_ascii_digit()
                    && e1[3].is_ascii_digit()
                {
                    return false;
                }
            }
        }
    } else if par1 && par2 && same_par_collection {
        return size1 < size2;
    } else if !par1 && par2 {
        return true;
    } else if par1 && !par2 {
        return false;
    }

    name1 < name2
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
    fn test_script_status_and_dir_name_of() {
        assert_eq!(script_calc_total_status(&[]), SCRIPT_STATUS_NONE);
        assert_eq!(
            script_calc_total_status(&[SCRIPT_STATUS_NONE, SCRIPT_STATUS_SUCCESS]),
            SCRIPT_STATUS_SUCCESS
        );
        assert_eq!(
            script_calc_total_status(&[
                SCRIPT_STATUS_SUCCESS,
                SCRIPT_STATUS_FAILURE,
                SCRIPT_STATUS_SUCCESS
            ]),
            SCRIPT_STATUS_FAILURE
        );

        assert_eq!(dir_name_of(b"", 0), b"nzb");
        assert_eq!(dir_name_of(b".hidden", 0), b"_.hidden");

        // Multi-byte UTF-8 boundary truncation test: 254 'a' bytes + 2-byte 'é' (0xC3 0xA9) = 256 bytes
        let mut long_utf8 = vec![b'a'; 254];
        long_utf8.extend_from_slice("é".as_bytes());
        let truncated = dir_name_of(&long_utf8, 0);
        assert_eq!(truncated.len(), 254);
    }

    #[test]
    fn test_calc_health_and_critical_health() {
        assert_eq!(calc_health(1000, 100, 0, 0), 1000);
        // 1 byte failed out of 1_000_000 -> rounds to 1000, clamped to 999
        assert_eq!(calc_health(1_000_000, 0, 1, 0), 999);
        assert_eq!(calc_health(1000, 100, 90, 0), 900);

        assert_eq!(calc_critical_health(0, 0, 0, true), 1000);
        assert_eq!(calc_critical_health(1000, 0, 0, true), 850);
        assert_eq!(calc_critical_health(1000, 0, 0, false), 1000);
        assert_eq!(calc_critical_health(1000, 600, 0, false), 0);
    }

    #[test]
    fn test_make_text_status_and_is_dupe_success() {
        let s = make_text_status_cstr(
            NZB_KIND_NZB,
            MARK_NONE,
            DELETE_NONE,
            PAR_SUCCESS,
            UNPACK_SUCCESS,
            MOVE_SUCCESS,
            URL_NONE,
            SCRIPT_STATUS_SUCCESS,
            1000,
            900,
        );
        assert_eq!(s, b"SUCCESS/ALL\0");

        assert!(!is_dupe_success(
            MARK_NONE,
            DELETE_NONE,
            PAR_NONE,
            UNPACK_NONE,
            URL_NONE,
            950,
            850,
            1000,
            true
        ));
        assert!(is_dupe_success(
            MARK_NONE,
            DELETE_NONE,
            PAR_NONE,
            UNPACK_NONE,
            URL_NONE,
            950,
            850,
            1000,
            false
        ));
    }

    #[test]
    fn test_article_writer_and_memory_monitor() {
        assert_eq!(calc_write_buffer_size(1024, 500_000), 500_000);
        assert_eq!(calc_write_buffer_size(512, 600_000), 512 * 1024);

        let mut cand = PRESSURE_NORMAL;
        let mut consec = 0;
        let (next1, trans1) =
            memory_monitor_step(PRESSURE_CRITICAL, PRESSURE_NORMAL, &mut cand, &mut consec);
        assert_eq!(next1, PRESSURE_CRITICAL);
        assert!(trans1);

        // De-escalation requires 2 consecutive samples
        let (next2, trans2) =
            memory_monitor_step(PRESSURE_NORMAL, PRESSURE_CRITICAL, &mut cand, &mut consec);
        assert_eq!(next2, PRESSURE_CRITICAL);
        assert!(!trans2);
        let (next3, trans3) =
            memory_monitor_step(PRESSURE_NORMAL, PRESSURE_CRITICAL, &mut cand, &mut consec);
        assert_eq!(next3, PRESSURE_NORMAL);
        assert!(trans3);
    }

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
    }

    #[test]
    fn test_history_and_queue_editor_helpers() {
        assert_eq!(
            classify_dup_status(MARK_NONE, DELETE_MANUAL, false),
            DUP_STATUS_DELETED
        );
        assert_eq!(
            classify_dup_status(MARK_GOOD, DELETE_NONE, false),
            DUP_STATUS_GOOD
        );

        assert_eq!(
            parse_sort_spec(b"priority+"),
            Some((SORT_CRITERIA_PRIORITY, SORT_ORDER_ASCENDING))
        );
        assert_eq!(
            parse_sort_spec(b"left"),
            Some((SORT_CRITERIA_REMAINING_SIZE, SORT_ORDER_AUTO))
        );

        let mut pause = [0u8; 3];
        select_pars_to_pause(&[1, 1, 1], &[1, 1, 1], &[500, 100, 300], true, &mut pause);
        assert_eq!(pause, [1, 0, 1]);

        assert!(compare_inner_files(
            b"movie.rar",
            false,
            100,
            b"movie.r00",
            false,
            100,
            false
        ));
        assert!(!compare_inner_files(
            b"movie.r00",
            false,
            100,
            b"movie.rar",
            false,
            100,
            false
        ));
    }
}
