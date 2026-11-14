//! End-to-end Rust implementation of `DownloadInfo`, `NzbInfo`, `FileInfo`,
//! `CompletedFile`, `ServerStatList`, and `ScriptStatusList`
//! (`daemon/queue/DownloadInfo.cpp`).

use std::ffi::CStr;

#[inline]
fn c_lower(b: u8) -> u8 {
    b.to_ascii_lowercase()
}

pub const SCRIPT_STATUS_NONE: i32 = 0;
pub const SCRIPT_STATUS_FAILURE: i32 = 1;
pub const SCRIPT_STATUS_SUCCESS: i32 = 2;

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

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsDownloadCurrentStatsAcc {
    pub remaining_size: i64,
    pub current_success_size: i64,
    pub current_failed_size: i64,
    pub paused_size: i64,
    pub par_current_success_size: i64,
    pub par_current_failed_size: i64,
    pub current_success_articles: i32,
    pub current_failed_articles: i32,
    pub extra_priority: i32,
    pub paused_file_count: i32,
    pub remaining_par_count: i32,
    pub _pad: i32,
}

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

/// Applies a `ServerStatList::EStatOperation` (`soSet`, `soAdd`, `soSubtract`) to a server's article counters.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn server_stat_apply_op(
    cur_success: i32,
    cur_failed: i32,
    delta_success: i32,
    delta_failed: i32,
    stat_op: i32,
    so_set: i32,
    so_add: i32,
    so_subtract: i32,
) -> (i32, i32) {
    if stat_op == so_set {
        (delta_success, delta_failed)
    } else if stat_op == so_add {
        (
            cur_success.wrapping_add(delta_success),
            cur_failed.wrapping_add(delta_failed),
        )
    } else if stat_op == so_subtract {
        (
            cur_success.wrapping_sub(delta_success),
            cur_failed.wrapping_sub(delta_failed),
        )
    } else {
        (cur_success, cur_failed)
    }
}

/// Checks whether an NZB filename is an absolute path (`/` or `\` prefix, or `X:\` / `X:/` drive).
pub fn is_absolute_nzb_name(nzb_name: &[u8]) -> bool {
    if nzb_name.starts_with(b"/") || nzb_name.starts_with(b"\\") {
        return true;
    }
    nzb_name.len() >= 3
        && nzb_name[0].is_ascii_alphabetic()
        && nzb_name[1] == b':'
        && (nzb_name[2] == b'\\' || nzb_name[2] == b'/')
}

/// Strips a trailing `.nzb` extension (case-insensitive) when `remove_ext` is true.
pub fn strip_nzb_ext(nicename: &[u8], remove_ext: bool) -> &[u8] {
    if remove_ext && nicename.len() >= 4 {
        let split_at = nicename.len() - 4;
        if nicename[split_at..].eq_ignore_ascii_case(b".nzb") {
            return &nicename[..split_at];
        }
    }
    nicename
}

/// `NzbInfo::MakeNiceNzbName` (`DownloadInfo.cpp`):
/// Strips directory components only when `nzb_filename` is an absolute path,
/// optionally removes a trailing `.nzb` extension, sanitizes the path segment,
/// and falls back to `"nzb"` if empty.
pub fn make_nice_nzb_name(nzb_filename: &[u8], remove_ext: bool) -> Vec<u8> {
    let base = if is_absolute_nzb_name(nzb_filename) {
        &nzb_filename[crate::paths::base_file_name(nzb_filename)..]
    } else {
        nzb_filename
    };
    let stripped = strip_nzb_ext(base, remove_ext);
    let valid = crate::paths::sanitize_path_segment(stripped);
    if valid.is_empty() {
        b"nzb".to_vec()
    } else {
        valid
    }
}

/// `NzbInfo::MakeNiceUrlName` (`DownloadInfo.cpp`):
/// Parses `url_cstr` via `crate::url::parse_url` and formats either
/// `"<nice_nzb_name> @ <host>"`, `"<host><resource>"`, or raw `url_cstr`.
pub fn make_nice_url_name(url_cstr: Option<&CStr>, nzb_filename: &[u8]) -> Vec<u8> {
    let url_bytes = url_cstr.map(CStr::to_bytes).unwrap_or(b"");
    let parsed = url_cstr.map(crate::url::parse_url).unwrap_or_default();
    let host = parsed
        .host
        .and_then(|(s, l)| url_bytes.get(s..s + l))
        .unwrap_or(b"");
    if !nzb_filename.is_empty() {
        let nicename = make_nice_nzb_name(nzb_filename, true);
        let mut out = Vec::with_capacity(nicename.len() + host.len() + 3);
        out.extend_from_slice(&nicename);
        out.extend_from_slice(b" @ ");
        out.extend_from_slice(host);
        out
    } else if parsed.valid {
        let resource = parsed
            .resource
            .and_then(|(s, l)| url_bytes.get(s..s + l))
            .unwrap_or(b"/");
        let mut out = Vec::with_capacity(host.len() + resource.len());
        out.extend_from_slice(host);
        out.extend_from_slice(resource);
        out
    } else {
        url_bytes.to_vec()
    }
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

/// `NzbInfo::BuildFinalDirName` (`DownloadInfo.cpp`).
pub fn build_final_dir_name(
    default_dest_dir: &[u8],
    category_dest_dir: &[u8],
    category: &[u8],
    append_category_dir: bool,
    name: &[u8],
    nzb_id: i32,
) -> Vec<u8> {
    let (mut out, use_category) = if !category.is_empty() && !category_dest_dir.is_empty() {
        (category_dest_dir.to_vec(), false)
    } else {
        (default_dest_dir.to_vec(), !category.is_empty())
    };
    if append_category_dir && use_category {
        let category_dir = crate::paths::sanitize_relative_path(category);
        if !category_dir.is_empty() {
            out.push(crate::paths::PATH_SEPARATOR);
            out.extend_from_slice(&category_dir);
        }
    }
    out.push(crate::paths::PATH_SEPARATOR);
    if name.is_empty() {
        out.extend_from_slice(format!("nzb-{nzb_id}").as_bytes());
    } else {
        out.extend_from_slice(&dir_name_of(name, 0));
    }
    out
}

/// `NzbInfo::BuildDestDirName` (`DownloadInfo.cpp`).
pub fn build_dest_dir_name(
    inter_dir: &[u8],
    default_dest_dir: &[u8],
    category_dest_dir: &[u8],
    category: &[u8],
    append_category_dir: bool,
    name: &[u8],
    nzb_id: i32,
) -> Vec<u8> {
    if inter_dir.is_empty() {
        build_final_dir_name(
            default_dest_dir,
            category_dest_dir,
            category,
            append_category_dir,
            name,
            nzb_id,
        )
    } else {
        let id_suffix = format!(".#{nzb_id}");
        let dir_name = dir_name_of(name, id_suffix.len());
        let mut out = Vec::with_capacity(inter_dir.len() + 1 + dir_name.len() + id_suffix.len());
        out.extend_from_slice(inter_dir);
        out.push(crate::paths::PATH_SEPARATOR);
        out.extend_from_slice(&dir_name);
        out.extend_from_slice(id_suffix.as_bytes());
        out
    }
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

/// Updates download timing state in `NzbInfo::SetActiveDownloads`.
/// Returns `(download_sec, download_start_time, download_start_sec, changed)`.
pub fn calc_active_downloads_sec(
    old_active: i32,
    new_active: i32,
    is_nzb_kind: bool,
    cur_time: i64,
    download_sec: i32,
    download_start_time: i64,
    download_start_sec: i32,
) -> (i32, i64, i32, bool) {
    if ((old_active == 0 && new_active > 0) || (old_active > 0 && new_active == 0)) && is_nzb_kind {
        if new_active > 0 {
            (download_sec, cur_time, download_sec, false)
        } else {
            let elapsed = cur_time.wrapping_sub(download_start_time) as i32;
            (
                download_start_sec.wrapping_add(elapsed),
                0,
                download_start_sec,
                true,
            )
        }
    } else if new_active > 0 {
        let elapsed = cur_time.wrapping_sub(download_start_time) as i32;
        (
            download_start_sec.wrapping_add(elapsed),
            download_start_time,
            download_start_sec,
            true,
        )
    } else {
        (download_sec, download_start_time, download_start_sec, false)
    }
}

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

/// `NzbInfo::UpdateCurrentStats` per-file accumulation step (`DownloadInfo.cpp`).
#[allow(clippy::too_many_arguments)]
pub fn update_current_stats_step(
    acc: &mut RsDownloadCurrentStatsAcc,
    file_remaining_size: i64,
    file_success_articles: i32,
    file_failed_articles: i32,
    file_success_size: i64,
    file_failed_size: i64,
    file_extra_priority: bool,
    file_paused: bool,
    file_par_file: bool,
) {
    acc.remaining_size = acc.remaining_size.wrapping_add(file_remaining_size);
    acc.current_success_articles = acc
        .current_success_articles
        .wrapping_add(file_success_articles);
    acc.current_failed_articles = acc
        .current_failed_articles
        .wrapping_add(file_failed_articles);
    acc.current_success_size = acc.current_success_size.wrapping_add(file_success_size);
    acc.current_failed_size = acc.current_failed_size.wrapping_add(file_failed_size);
    if file_extra_priority {
        acc.extra_priority = acc.extra_priority.wrapping_add(1);
    }
    if file_paused {
        acc.paused_file_count = acc.paused_file_count.wrapping_add(1);
        acc.paused_size = acc.paused_size.wrapping_add(file_remaining_size);
    }
    if file_par_file {
        acc.remaining_par_count = acc.remaining_par_count.wrapping_add(1);
        acc.par_current_success_size = acc.par_current_success_size.wrapping_add(file_success_size);
        acc.par_current_failed_size = acc.par_current_failed_size.wrapping_add(file_failed_size);
    }
}

/// `NzbInfo::UpdateCompletedStats` (`DownloadInfo.cpp`).
#[allow(clippy::too_many_arguments)]
pub fn update_completed_stats(
    file_success_size: i64,
    file_failed_size: i64,
    file_failed_articles: i32,
    file_success_articles: i32,
    file_extra_priority: bool,
    file_par_file: bool,
    file_paused: bool,
    inout_success_size: &mut i64,
    inout_failed_size: &mut i64,
    inout_failed_articles: &mut i32,
    inout_success_articles: &mut i32,
    inout_extra_priority: &mut i32,
    inout_par_success_size: &mut i64,
    inout_par_failed_size: &mut i64,
    inout_remaining_par_count: &mut i32,
    inout_paused_file_count: &mut i32,
) {
    *inout_success_size = inout_success_size.wrapping_add(file_success_size);
    *inout_failed_size = inout_failed_size.wrapping_add(file_failed_size);
    *inout_failed_articles = inout_failed_articles.wrapping_add(file_failed_articles);
    *inout_success_articles = inout_success_articles.wrapping_add(file_success_articles);
    if file_extra_priority {
        *inout_extra_priority = inout_extra_priority.wrapping_sub(1);
    }
    if file_par_file {
        *inout_par_success_size = inout_par_success_size.wrapping_add(file_success_size);
        *inout_par_failed_size = inout_par_failed_size.wrapping_add(file_failed_size);
        *inout_remaining_par_count = inout_remaining_par_count.wrapping_sub(1);
    }
    if file_paused {
        *inout_paused_file_count = inout_paused_file_count.wrapping_sub(1);
    }
}

/// `NzbInfo::UpdateDeletedStats` (`DownloadInfo.cpp`).
#[allow(clippy::too_many_arguments)]
pub fn update_deleted_stats(
    file_size: i64,
    file_success_size: i64,
    file_failed_size: i64,
    file_missed_size: i64,
    file_total_articles: i32,
    file_success_articles: i32,
    file_failed_articles: i32,
    file_missed_articles: i32,
    file_remaining_size: i64,
    file_extra_priority: bool,
    file_par_file: bool,
    file_paused: bool,
    inout_file_count: &mut i32,
    inout_size: &mut i64,
    inout_cur_success_size: &mut i64,
    inout_failed_size: &mut i64,
    inout_failed_articles: &mut i32,
    inout_cur_failed_size: &mut i64,
    inout_total_articles: &mut i32,
    inout_cur_success_articles: &mut i32,
    inout_cur_failed_articles: &mut i32,
    inout_remaining_size: &mut i64,
    inout_extra_priority: &mut i32,
    inout_remaining_par_count: &mut i32,
    inout_par_size: &mut i64,
    inout_par_cur_success_size: &mut i64,
    inout_par_failed_size: &mut i64,
    inout_par_cur_failed_size: &mut i64,
    inout_paused_file_count: &mut i32,
    inout_paused_size: &mut i64,
) {
    *inout_file_count = inout_file_count.wrapping_sub(1);
    *inout_size = inout_size.wrapping_sub(file_size);
    *inout_cur_success_size = inout_cur_success_size.wrapping_sub(file_success_size);
    *inout_failed_size = inout_failed_size.wrapping_sub(file_missed_size);
    *inout_failed_articles = inout_failed_articles.wrapping_sub(file_missed_articles);
    *inout_cur_failed_size =
        inout_cur_failed_size.wrapping_sub(file_failed_size.wrapping_add(file_missed_size));
    *inout_total_articles = inout_total_articles.wrapping_sub(file_total_articles);
    *inout_cur_success_articles = inout_cur_success_articles.wrapping_sub(file_success_articles);
    *inout_cur_failed_articles = inout_cur_failed_articles
        .wrapping_sub(file_failed_articles.wrapping_add(file_missed_articles));
    *inout_remaining_size = inout_remaining_size.wrapping_sub(file_remaining_size);
    if file_extra_priority {
        *inout_extra_priority = inout_extra_priority.wrapping_sub(1);
    }
    if file_par_file {
        *inout_remaining_par_count = inout_remaining_par_count.wrapping_sub(1);
        *inout_par_size = inout_par_size.wrapping_sub(file_size);
        *inout_par_cur_success_size = inout_par_cur_success_size.wrapping_sub(file_success_size);
        *inout_par_failed_size = inout_par_failed_size.wrapping_sub(file_missed_size);
        *inout_par_cur_failed_size =
            inout_par_cur_failed_size.wrapping_sub(file_failed_size.wrapping_add(file_missed_size));
    }
    if file_paused {
        *inout_paused_file_count = inout_paused_file_count.wrapping_sub(1);
        *inout_paused_size = inout_paused_size.wrapping_sub(file_remaining_size);
    }
}

/// `NzbInfo::IsDownloadCompleted` (`DownloadInfo.cpp`):
/// Returns `true` when `active_downloads == 0` and no non-deleted file blocks completion
/// (when `ignore_paused_pars` is true, paused PAR files do not block completion).
pub fn is_download_completed_with<F>(
    active_downloads: i32,
    ignore_paused_pars: bool,
    file_count: usize,
    mut get_file: F,
) -> bool
where
    F: FnMut(usize) -> (bool, bool, bool),
{
    if active_downloads != 0 {
        return false;
    }
    for i in 0..file_count {
        let (paused, par_file, deleted) = get_file(i);
        if ((!paused || !ignore_paused_pars) || !par_file) && !deleted {
            return false;
        }
    }
    true
}

/// Slice helper for `NzbInfo::IsDownloadCompleted` over `(paused, par_file, deleted)` tuples.
pub fn is_download_completed(
    active_downloads: i32,
    ignore_paused_pars: bool,
    files: &[(bool, bool, bool)],
) -> bool {
    is_download_completed_with(active_downloads, ignore_paused_pars, files.len(), |i| {
        files[i]
    })
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

/// Pure-Rust representation of a `CompletedFile` entry for `NzbInfo::RenameCompletedFile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedFileRecord {
    pub id: i32,
    pub filename: Vec<u8>,
    pub origname: Vec<u8>,
    pub status: i32,
    pub crc: u32,
    pub par_file: bool,
    pub hash16k: Vec<u8>,
    pub par_set_id: Vec<u8>,
}

/// `NzbInfo::RenameCompletedFile` (`DownloadInfo.cpp`):
/// Iterates `count` completed file entries via `get_names(index) -> (filename, origname)`,
/// finds the first matching `old_name` via `completed_same_filename`, and calls
/// `set_names(index, new_filename, new_origname_if_was_empty)`.
pub fn rename_completed_file_with<G, S>(
    count: usize,
    old_name: &[u8],
    new_name: &[u8],
    mut get_names: G,
    mut set_names: S,
) -> bool
where
    G: FnMut(usize) -> (Vec<u8>, Vec<u8>),
    S: FnMut(usize, &[u8], Option<&[u8]>),
{
    if old_name.is_empty() || new_name.is_empty() {
        return false;
    }
    for i in 0..count {
        let (filename, origname) = get_names(i);
        if completed_same_filename(&filename, old_name) {
            let new_orig = if origname.is_empty() {
                Some(filename.as_slice())
            } else {
                None
            };
            set_names(i, new_name, new_orig);
            return true;
        }
    }
    false
}

/// `NzbInfo::RenameCompletedFile` (`DownloadInfo.cpp`) on a slice of `CompletedFileRecord`.
pub fn rename_completed_file(
    completed_files: &mut [CompletedFileRecord],
    old_name: &[u8],
    new_name: &[u8],
) -> bool {
    let mut matched = None;
    let ok = rename_completed_file_with(
        completed_files.len(),
        old_name,
        new_name,
        |i| {
            (
                completed_files[i].filename.clone(),
                completed_files[i].origname.clone(),
            )
        },
        |i, new_fn, new_orig| {
            matched = Some((i, new_fn.to_vec(), new_orig.map(<[u8]>::to_vec)));
        },
    );
    if let Some((i, new_fn, new_orig)) = matched {
        if let Some(orig) = new_orig {
            completed_files[i].origname = orig;
        }
        completed_files[i].filename = new_fn;
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_names_and_dirs() {
        assert_eq!(
            make_nice_nzb_name(b"/tmp/Show.S01E01.nzb", true),
            b"Show.S01E01"
        );
        assert_eq!(
            make_nice_nzb_name(b"[hash] [16/22] - Title.nzb", true),
            b"[hash] [16_22] - Title"
        );
        assert_eq!(
            make_nice_url_name(Some(c"https://indexer.example/get/123"), b"Show.nzb"),
            b"Show @ indexer.example"
        );
        assert_eq!(
            make_nice_url_name(Some(c"https://indexer.example/get/123"), b""),
            b"indexer.example/get/123"
        );
        let sep = crate::paths::PATH_SEPARATOR;
        assert_eq!(
            build_dest_dir_name(b"/inter", b"/dest", b"", b"", false, b"MyShow", 42),
            [b"/inter".as_slice(), &[sep], b"MyShow.#42"].concat()
        );
        assert_eq!(
            build_final_dir_name(b"/dest", b"", b"TV", true, b"MyShow", 42),
            [b"/dest".as_slice(), &[sep], b"TV", &[sep], b"MyShow"].concat()
        );
    }

    #[test]
    fn health_and_status() {
        assert_eq!(calc_health(1000, 100, 90, 0), 900);
        assert_eq!(calc_critical_health(1100, 100, 0, true), 900);
        assert_eq!(
            script_calc_total_status(&[SCRIPT_STATUS_SUCCESS, SCRIPT_STATUS_FAILURE]),
            SCRIPT_STATUS_FAILURE
        );
        assert!(completed_same_filename(b"sub\\file.rar", b"SUB/FILE.RAR"));
    }

    #[test]
    fn completed_file_full_suite_port_from_cpp() {
        // 1. CompletedFileSameFilenameTest
        let rec = b"DISORDER_2025-FLT/2c0837e5fa42c8cfb5d5e583168a2af4.mkv";
        assert!(completed_same_filename(
            rec,
            b"DISORDER_2025-FLT/2c0837e5fa42c8cfb5d5e583168a2af4.mkv"
        ));
        assert!(completed_same_filename(
            rec,
            b"DISORDER_2025-FLT\\2c0837e5fa42c8cfb5d5e583168a2af4.mkv"
        ));
        assert!(completed_same_filename(
            rec,
            b"disorder_2025-flt/2C0837E5FA42C8CFB5D5E583168A2AF4.MKV"
        ));
        assert!(!completed_same_filename(
            rec,
            b"2c0837e5fa42c8cfb5d5e583168a2af4.mkv"
        ));
        assert!(!completed_same_filename(rec, b"other.mkv"));
        assert!(!completed_same_filename(
            rec,
            b"OTHER_DIR/2c0837e5fa42c8cfb5d5e583168a2af4.mkv"
        ));

        let bare = b"5KzdcWdGVGUG83Q9jv8KXht4O2k57w.mkv";
        assert!(completed_same_filename(
            bare,
            b"5KzdcWdGVGUG83Q9jv8KXht4O2k57w.mkv"
        ));
        assert!(!completed_same_filename(
            bare,
            b"Some.Dir/5KzdcWdGVGUG83Q9jv8KXht4O2k57w.mkv"
        ));
        assert!(!completed_same_filename(
            bare,
            b"5KzdcWdGVGUG83Q9jv8KXht4O2k57w.mp4"
        ));

        // 2. RenameCompletedFileMatchesPathQualifiedRecordTest
        let mut completed_files = vec![CompletedFileRecord {
            id: 1,
            filename: b"parent/2c0837e5fa42c8cfb5d5e583168a2af4.mkv".to_vec(),
            origname: Vec::new(),
            status: 1,
            crc: 0,
            par_file: false,
            hash16k: Vec::new(),
            par_set_id: Vec::new(),
        }];
        assert!(rename_completed_file(
            &mut completed_files,
            b"parent/2c0837e5fa42c8cfb5d5e583168a2af4.mkv",
            b"parent/Some.Release.mkv"
        ));
        assert_eq!(completed_files[0].filename, b"parent/Some.Release.mkv");
        assert_eq!(
            completed_files[0].origname,
            b"parent/2c0837e5fa42c8cfb5d5e583168a2af4.mkv"
        );
    }
}
