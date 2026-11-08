//! End-to-end Rust implementation of `DupeCoordinator` (`daemon/queue/DupeCoordinator.cpp`):
//! duplicate name/key matching (`same_name_or_key`), on-disk verification (`files_on_disk`),
//! failover threshold & policy (`failover_warranted`, `fails_over`, `parse_alive_permille_cstr`),
//! backup candidate selection (`select_dupe_backup`), no-backup diagnostics
//! (`no_backup_reason`, `format_no_backup_reason`), skip message formatting
//! (`format_dupe_skip_message`), and status classification (`status_from_text`).

use std::collections::BTreeSet;
use std::ffi::{c_char, CStr};
use std::path::Path;

pub(crate) fn parse_f64_prefix(bytes: &[u8]) -> Option<f64> {
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let start = i;
    let mut neg = false;
    if bytes[i] == b'-' {
        neg = true;
        i += 1;
    } else if bytes[i] == b'+' {
        i += 1;
    }
    let rest = &bytes[i..];
    if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(b"inf") {
        return Some(if neg { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(b"nan") {
        return Some(f64::NAN);
    }
    let mut has_digits = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        has_digits = true;
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            has_digits = true;
            i += 1;
        }
    }
    if !has_digits {
        return None;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut exp_i = i + 1;
        if exp_i < bytes.len() && (bytes[exp_i] == b'+' || bytes[exp_i] == b'-') {
            exp_i += 1;
        }
        let exp_digits_start = exp_i;
        while exp_i < bytes.len() && bytes[exp_i].is_ascii_digit() {
            exp_i += 1;
        }
        if exp_i > exp_digits_start {
            i = exp_i;
        }
    }
    std::str::from_utf8(&bytes[start..i]).ok()?.parse::<f64>().ok()
}

#[inline]
pub fn strcasecmp_bytes(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[inline]
pub fn strncasecmp_prefix(haystack: &[u8], prefix: &[u8]) -> bool {
    haystack.len() >= prefix.len() && haystack[..prefix.len()].eq_ignore_ascii_case(prefix)
}

pub const DUPE_MODE_SCORE: i32 = 0;
pub const DUPE_MODE_ALL: i32 = 1;
pub const DUPE_MODE_FORCE: i32 = 2;

pub const HEALTH_CHECK_PAUSE: i32 = 0;
pub const HEALTH_CHECK_DELETE: i32 = 1;
pub const HEALTH_CHECK_PARK: i32 = 2;
pub const HEALTH_CHECK_NONE: i32 = 3;
pub const HEALTH_CHECK_DUPE: i32 = 4;

pub const DELETE_STATUS_NONE: i32 = 0;
pub const DELETE_STATUS_MANUAL: i32 = 1;
pub const DELETE_STATUS_HEALTH: i32 = 2;
pub const DELETE_STATUS_DUPE: i32 = 3;
pub const DELETE_STATUS_BAD: i32 = 4;
pub const DELETE_STATUS_GOOD: i32 = 5;
pub const DELETE_STATUS_COPY: i32 = 6;
pub const DELETE_STATUS_SCAN: i32 = 7;

pub const MARK_STATUS_NONE: i32 = 0;
pub const MARK_STATUS_BAD: i32 = 1;
pub const MARK_STATUS_GOOD: i32 = 2;
pub const MARK_STATUS_SUCCESS: i32 = 3;

pub const DUP_STATUS_UNDEFINED: i32 = 0;
pub const DUP_STATUS_SUCCESS: i32 = 1;
pub const DUP_STATUS_FAILED: i32 = 2;
pub const DUP_STATUS_DELETED: i32 = 3;
pub const DUP_STATUS_DUPE: i32 = 4;
pub const DUP_STATUS_BAD: i32 = 5;
pub const DUP_STATUS_GOOD: i32 = 6;

pub const HISTORY_KIND_UNKNOWN: i32 = 0;
pub const HISTORY_KIND_NZB: i32 = 1;
pub const HISTORY_KIND_URL: i32 = 2;
pub const HISTORY_KIND_DUP: i32 = 3;

pub const DUPE_STATUS_NONE: i32 = 0;
pub const DUPE_STATUS_QUEUED: i32 = 1;
pub const DUPE_STATUS_DOWNLOADING: i32 = 2;
pub const DUPE_STATUS_SUCCESS: i32 = 4;
pub const DUPE_STATUS_WARNING: i32 = 8;
pub const DUPE_STATUS_FAILURE: i32 = 16;

pub use crate::postrepair::{NzbgetRsDupeHistoryCandidate, NzbgetRsDupeQueueCandidate};

/// `DupeCoordinator::SameNameOrKey`
pub fn same_name_or_key(name1: &[u8], dupe_key1: &[u8], name2: &[u8], dupe_key2: &[u8]) -> bool {
    let has_dupe_keys = !dupe_key1.is_empty() && !dupe_key2.is_empty();
    (has_dupe_keys && strcasecmp_bytes(dupe_key1, dupe_key2))
        || (!has_dupe_keys && strcasecmp_bytes(name1, name2))
}

/// `DupeCoordinator::FilesOnDisk`:
/// Checks whether `final_dir` (or `dest_dir` if `final_dir` is empty) exists and
/// contains at least one non-empty regular file that is not a side file.
pub fn files_on_disk(final_dir: &[u8], dest_dir: &[u8], ext_cleanup_disk: &[u8]) -> bool {
    let dir = if !final_dir.is_empty() {
        final_dir
    } else {
        dest_dir
    };
    if dir.is_empty() {
        return false;
    }
    #[cfg(unix)]
    let dir_path = {
        use std::os::unix::ffi::OsStrExt;
        Path::new(std::ffi::OsStr::from_bytes(dir))
    };
    #[cfg(not(unix))]
    let dir_owned = String::from_utf8_lossy(dir).into_owned();
    #[cfg(not(unix))]
    let dir_path = Path::new(&dir_owned);

    let Ok(entries) = std::fs::read_dir(dir_path) else {
        return false;
    };
    for entry in entries.flatten() {
        let fname = entry.file_name();
        #[cfg(unix)]
        let fname_bytes = {
            use std::os::unix::ffi::OsStrExt;
            fname.as_bytes()
        };
        #[cfg(not(unix))]
        let fname_str = fname.to_string_lossy();
        #[cfg(not(unix))]
        let fname_bytes = fname_str.as_bytes();

        if crate::queuecoord::is_side_file(fname_bytes, ext_cleanup_disk) {
            continue;
        }
        if let Ok(meta) = std::fs::metadata(entry.path()) {
            if !meta.is_dir() && meta.len() > 0 {
                return true;
            }
        }
    }
    false
}

/// `DupeCoordinator::DupeFailoverWarranted`
pub fn failover_warranted(item_score: i32, health: i32, backup_score: i32) -> bool {
    let clamped_health = health.clamp(0, 1000);
    let warranted = if item_score > 0 {
        ((item_score as i64) * (clamped_health as i64) / 1000) as i32
    } else {
        0
    };
    warranted == 0 || backup_score >= warranted
}

/// `DupeCoordinator::AlivePermille` string value parser
pub unsafe fn parse_alive_permille_cstr(value: *const c_char) -> i32 {
    if value.is_null() {
        return -1;
    }
    let Some(percent) = parse_f64_prefix(CStr::from_ptr(value).to_bytes()) else {
        return -1;
    };
    if percent.is_nan() {
        return -1;
    }
    let clamped = percent.clamp(0.0, 100.0);
    (clamped * 10.0).round() as i32
}

/// `DupeCoordinator::FailsOver`
pub fn fails_over(dupe_check: bool, dupe_mode: i32, health_check: i32) -> bool {
    dupe_check
        && (dupe_mode == DUPE_MODE_SCORE
            || (dupe_mode == DUPE_MODE_FORCE && health_check == HEALTH_CHECK_DUPE))
}

/// `DupeCoordinator::NoBackupReason` formatter
pub fn format_no_backup_reason(
    total: i32,
    not_backup: i32,
    forced: i32,
    no_file: i32,
    unhealthy: i32,
    bad: i32,
    dead: i32,
) -> Vec<u8> {
    format!(
        "{total} other item(s) of the key in history: {not_backup} failed/succeeded/deleted, {forced} forced, \
{no_file} without nzb-file, {unhealthy} unhealthy, {bad} marked bad, {dead} found dead; the rest scored below a queued or \
successful duplicate, or were copies of this download"
    )
    .into_bytes()
}

/// `DupeCoordinator::NoBackupReason` end-to-end evaluator.
pub fn no_backup_reason<F>(
    fails_over_flag: bool,
    dupe_mode: i32,
    history: &[NzbgetRsDupeHistoryCandidate],
    mut file_exists_cb: F,
) -> Vec<u8>
where
    F: FnMut(usize) -> bool,
{
    if !fails_over_flag {
        return if dupe_mode == DUPE_MODE_FORCE {
            b"its DupeMode is force".to_vec()
        } else {
            b"its DupeMode is all".to_vec()
        };
    }
    let mut total = 0;
    let mut not_backup = 0;
    let mut forced = 0;
    let mut no_file = 0;
    let mut unhealthy = 0;
    let mut bad = 0;
    let mut dead = 0;
    for (idx, h) in history.iter().enumerate() {
        if h.kind != HISTORY_KIND_NZB || h.same_name_or_key == 0 {
            continue;
        }
        total += 1;
        if h.delete_status != DELETE_STATUS_DUPE && h.delete_status != DELETE_STATUS_COPY {
            not_backup += 1;
        } else if h.dupe_mode == DUPE_MODE_FORCE {
            forced += 1;
        } else if !file_exists_cb(idx) {
            no_file += 1;
        } else if h.health < h.critical_health {
            unhealthy += 1;
        } else if h.mark_status == MARK_STATUS_BAD {
            bad += 1;
        } else if h.alive_permille == 0 {
            dead += 1;
        }
    }
    format_no_backup_reason(total, not_backup, forced, no_file, unhealthy, bad, dead)
}

/// Formats the duplicate skip warning message in `DupeCoordinator::NzbFound`
/// (both queue and history skip variants).
pub fn format_dupe_skip_message(
    name: &[u8],
    other_name: &[u8],
    in_history: bool,
    same_content: bool,
    good: bool,
) -> Vec<u8> {
    if !in_history {
        let mut out = Vec::with_capacity(name.len() + other_name.len() + 48);
        out.extend_from_slice(b"Skipping duplicate ");
        out.extend_from_slice(name);
        if name == other_name {
            out.extend_from_slice(b", already queued");
        } else {
            out.extend_from_slice(b", already queued as ");
            out.extend_from_slice(other_name);
        }
        out
    } else {
        let reason: &[u8] = if same_content {
            b"exactly same content (the same posting, no new articles)"
        } else if good {
            b"good status"
        } else {
            b"success status"
        };
        let mut out = Vec::with_capacity(name.len() + other_name.len() + reason.len() + 48);
        out.extend_from_slice(b"Skipping duplicate ");
        out.extend_from_slice(name);
        if name == other_name {
            out.extend_from_slice(b", found in history with ");
        } else {
            out.extend_from_slice(b", found in history ");
            out.extend_from_slice(other_name);
            out.extend_from_slice(b" with ");
        }
        out.extend_from_slice(reason);
        out
    }
}

/// `DupeCoordinator::GetDupeStatus` text-status classifier
pub fn status_from_text(text_status: &[u8]) -> i32 {
    if strncasecmp_prefix(text_status, b"SUCCESS") {
        DUPE_STATUS_SUCCESS
    } else if strncasecmp_prefix(text_status, b"FAILURE") {
        DUPE_STATUS_FAILURE
    } else if strncasecmp_prefix(text_status, b"WARNING") {
        DUPE_STATUS_WARNING
    } else {
        DUPE_STATUS_NONE
    }
}

/// `DupeCoordinator::FindDupeBackup` full decision algorithm.
/// Returns the winning `0..history.len()` index, or `-1` if no backup qualifies.
pub fn select_dupe_backup<F>(
    has_self_nzb: bool,
    self_full_content_hash: u32,
    dupe_health: bool,
    history: &[NzbgetRsDupeHistoryCandidate],
    queue: &[NzbgetRsDupeQueueCandidate],
    mut file_exists_cb: F,
) -> i32
where
    F: FnMut(usize) -> bool,
{
    let mut dupe_found = false;
    let mut history_score = 0i32;

    for h in history {
        let mut good_dupe = false;

        if h.kind == HISTORY_KIND_NZB
            && h.dupe_mode != DUPE_MODE_FORCE
            && h.is_dupe_success != 0
            && h.same_name_or_key != 0
        {
            if !dupe_found || h.dupe_score > history_score {
                history_score = h.dupe_score;
            }
            dupe_found = true;
            good_dupe = h.mark_status == MARK_STATUS_GOOD;
        }

        if h.kind == HISTORY_KIND_DUP
            && h.dupe_mode != DUPE_MODE_FORCE
            && (h.dup_status == DUP_STATUS_SUCCESS || h.dup_status == DUP_STATUS_GOOD)
            && h.same_name_or_key != 0
        {
            if !dupe_found || h.dupe_score > history_score {
                history_score = h.dupe_score;
            }
            dupe_found = true;
            good_dupe = h.dup_status == DUP_STATUS_GOOD;
        }

        if good_dupe {
            return -1;
        }
    }

    let mut queue_dupe = false;
    let mut queue_score = 0i32;
    for q in queue {
        if q.is_self == 0
            && q.is_nzb_kind != 0
            && q.dupe_mode != DUPE_MODE_FORCE
            && q.same_name_or_key != 0
            && (!queue_dupe || q.dupe_score > queue_score)
        {
            queue_score = q.dupe_score;
            queue_dupe = true;
        }
    }

    let mut failed_content: BTreeSet<u32> = BTreeSet::new();
    if dupe_health {
        if has_self_nzb {
            failed_content.insert(self_full_content_hash);
        }
        for h in history {
            if h.kind == HISTORY_KIND_NZB
                && h.full_content_hash > 0
                && h.is_dupe_success == 0
                && h.delete_status != DELETE_STATUS_DUPE
                && h.delete_status != DELETE_STATUS_COPY
                && h.delete_status != DELETE_STATUS_MANUAL
            {
                failed_content.insert(h.full_content_hash);
            }
        }
    }

    let mut best_idx: Option<usize> = None;
    for (idx, h) in history.iter().enumerate() {
        let has_candidate = h.kind == HISTORY_KIND_NZB || h.kind == HISTORY_KIND_URL;
        let backup_status = has_candidate
            && (h.delete_status == DELETE_STATUS_DUPE
                || (dupe_health
                    && h.kind == HISTORY_KIND_NZB
                    && h.delete_status == DELETE_STATUS_COPY))
            && !(dupe_health
                && h.full_content_hash > 0
                && failed_content.contains(&h.full_content_hash));

        let candidate_score = if has_candidate { h.dupe_score } else { 0 };
        let better = has_candidate
            && match best_idx {
                None => true,
                Some(prev) => {
                    let prev_h = &history[prev];
                    candidate_score > prev_h.dupe_score
                        || (candidate_score == prev_h.dupe_score
                            && h.alive_permille > prev_h.alive_permille)
                }
            };

        if has_candidate
            && h.dupe_mode != DUPE_MODE_FORCE
            && backup_status
            && better
            && !(dupe_health && h.alive_permille == 0)
            && (h.kind != HISTORY_KIND_NZB || file_exists_cb(idx))
            && h.health >= h.critical_health
            && h.mark_status != MARK_STATUS_BAD
            && (!dupe_found || h.dupe_score > history_score)
            && (!queue_dupe || h.dupe_score > queue_score)
            && h.same_name_or_key != 0
        {
            best_idx = Some(idx);
        }
    }

    best_idx.map(|i| i as i32).unwrap_or(-1)
}

/// Returns true when `RarLister::FindLargestFile` in `DupeMatcher.cpp` should continue waiting for `unrar` (`is_running && start_time + timeout_sec > cur_time && start_time >= cur_time`).
pub fn dupematcher_should_wait_rar_lister(
    is_running: bool,
    start_time: i64,
    timeout_sec: i32,
    cur_time: i64,
) -> bool {
    is_running && start_time + i64::from(timeout_sec) > cur_time && start_time >= cur_time
}

/// `DupeCoordinator::DownloadedOnDisk` candidate check.
#[allow(clippy::too_many_arguments)]
pub fn is_downloaded_on_disk_candidate(
    health_check: i32,
    self_dupe_key: &[u8],
    is_self: bool,
    item_delete_status: i32,
    item_is_dupe_success: bool,
    item_mark_status: i32,
    item_dupe_key: &[u8],
    item_final_dir: &[u8],
    item_dest_dir: &[u8],
    ext_cleanup_disk: &[u8],
) -> bool {
    let dupe_health = health_check == HEALTH_CHECK_DUPE || health_check == HEALTH_CHECK_PARK;
    if !dupe_health || self_dupe_key.is_empty() || is_self {
        return false;
    }
    item_delete_status == DELETE_STATUS_NONE
        && item_is_dupe_success
        && item_mark_status != MARK_STATUS_BAD
        && strcasecmp_bytes(item_dupe_key, self_dupe_key)
        && files_on_disk(item_final_dir, item_dest_dir, ext_cleanup_disk)
}

/// `DupeCoordinator::GetDupeStatus` per-queue-item bitmask accumulator.
pub fn accumulate_queue_dupe_status(
    current_statuses: i32,
    name: &[u8],
    dupe_key: &[u8],
    item_name: &[u8],
    item_dupe_key: &[u8],
    success_articles: i32,
    failed_articles: i32,
) -> i32 {
    if !same_name_or_key(name, dupe_key, item_name, item_dupe_key) {
        return current_statuses;
    }
    if success_articles + failed_articles > 0 {
        current_statuses | DUPE_STATUS_DOWNLOADING
    } else {
        current_statuses | DUPE_STATUS_QUEUED
    }
}

/// `DupeCoordinator::GetDupeStatus` per-history-item bitmask accumulator.
#[allow(clippy::too_many_arguments)]
pub fn accumulate_history_dupe_status(
    current_statuses: i32,
    kind: i32,
    name: &[u8],
    dupe_key: &[u8],
    item_name: &[u8],
    item_dupe_key: &[u8],
    text_status: &[u8],
    dup_status: i32,
) -> i32 {
    if kind == HISTORY_KIND_NZB && same_name_or_key(name, dupe_key, item_name, item_dupe_key) {
        return current_statuses | status_from_text(text_status);
    }
    if kind == HISTORY_KIND_DUP && same_name_or_key(name, dupe_key, item_name, item_dupe_key) {
        if dup_status == DUP_STATUS_SUCCESS || dup_status == DUP_STATUS_GOOD {
            return current_statuses | DUPE_STATUS_SUCCESS;
        }
        if dup_status == DUP_STATUS_FAILED || dup_status == DUP_STATUS_BAD {
            return current_statuses | DUPE_STATUS_FAILURE;
        }
    }
    current_statuses
}

/// `DupeCoordinator::HistoryCleanup` candidate predicate.
#[allow(clippy::too_many_arguments)]
pub fn should_cleanup_history_dupe(
    kind: i32,
    dupe_mode: i32,
    delete_status: i32,
    is_self: bool,
    item_name: &[u8],
    item_dupe_key: &[u8],
    target_name: &[u8],
    target_dupe_key: &[u8],
) -> bool {
    !is_self
        && (kind == HISTORY_KIND_NZB || kind == HISTORY_KIND_URL)
        && dupe_mode != DUPE_MODE_FORCE
        && delete_status == DELETE_STATUS_DUPE
        && same_name_or_key(item_name, item_dupe_key, target_name, target_dupe_key)
}

/// `DupeCoordinator::ListHistoryDupes` candidate predicate.
pub fn should_list_history_dupe(
    self_dupe_mode: i32,
    kind: i32,
    item_dupe_mode: i32,
    item_name: &[u8],
    item_dupe_key: &[u8],
    self_name: &[u8],
    self_dupe_key: &[u8],
) -> bool {
    self_dupe_mode != DUPE_MODE_FORCE
        && kind == HISTORY_KIND_NZB
        && item_dupe_mode != DUPE_MODE_FORCE
        && same_name_or_key(item_name, item_dupe_key, self_name, self_dupe_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn dupe_coord_basics() {
        assert!(same_name_or_key(
            b"Movie.A",
            b"imdb:123",
            b"Movie.B",
            b"IMDB:123"
        ));
        assert!(!same_name_or_key(
            b"Movie.A",
            b"imdb:123",
            b"Movie.A",
            b"imdb:456"
        ));
        assert!(failover_warranted(90, 300, 27));
        assert!(!failover_warranted(90, 300, 26));
        assert_eq!(status_from_text(b"SUCCESS/ALL"), DUPE_STATUS_SUCCESS);
        assert_eq!(
            format_dupe_skip_message(b"Show.A", b"Show.A", false, true, false),
            b"Skipping duplicate Show.A, already queued"
        );
    }

    #[test]
    fn dupe_coordinator_full_suite_port_from_cpp() {
        // 1. DupeFailoverWarrantedTest
        assert!(failover_warranted(90, 300, 27));
        assert!(!failover_warranted(90, 300, 26));
        assert!(failover_warranted(90, 0, 0));
        assert!(failover_warranted(90, 0, -5));
        assert!(failover_warranted(-10, 500, -20));
        assert!(failover_warranted(90, 1000, 90));
        assert!(!failover_warranted(90, 1000, 89));
        assert!(failover_warranted(0, 1000, 0));
        assert!(failover_warranted(-10, 500, 0));
        assert!(!failover_warranted(90, 1500, 89));
        assert!(failover_warranted(90, -7, 0));

        // 2. DupeSuccessMatchesUnrepairedHealthFailure
        let health = crate::downloadinfo::calc_health(1000, 0, 40, 0);
        let crit_est = crate::downloadinfo::calc_critical_health(1000, 0, 0, true);
        let crit_strict = crate::downloadinfo::calc_critical_health(1000, 0, 0, false);
        assert_eq!(health, 960);
        assert_eq!(crit_est, 850);
        assert_eq!(crit_strict, 1000);
        assert_eq!(
            crate::downloadinfo::make_text_status_cstr(
                0,
                MARK_STATUS_NONE,
                DELETE_STATUS_NONE,
                crate::downloadinfo::PAR_SKIPPED,
                crate::downloadinfo::UNPACK_NONE,
                crate::downloadinfo::MOVE_NONE,
                crate::downloadinfo::URL_NONE,
                crate::downloadinfo::SCRIPT_STATUS_SUCCESS,
                health,
                crit_strict,
            ),
            b"FAILURE/HEALTH\0"
        );
        assert!(!crate::downloadinfo::is_dupe_success(
            MARK_STATUS_NONE,
            DELETE_STATUS_NONE,
            crate::downloadinfo::PAR_SKIPPED,
            crate::downloadinfo::UNPACK_NONE,
            crate::downloadinfo::URL_NONE,
            health,
            crit_est,
            crit_strict,
            true,
        ));
        // A verified repair (PAR_SUCCESS) or explicit good mark takes precedence
        assert!(crate::downloadinfo::is_dupe_success(
            MARK_STATUS_NONE,
            DELETE_STATUS_NONE,
            crate::downloadinfo::PAR_SUCCESS,
            crate::downloadinfo::UNPACK_NONE,
            crate::downloadinfo::URL_NONE,
            health,
            crit_est,
            crit_strict,
            true,
        ));
        assert!(crate::downloadinfo::is_dupe_success(
            MARK_STATUS_GOOD,
            DELETE_STATUS_NONE,
            crate::downloadinfo::PAR_SKIPPED,
            crate::downloadinfo::UNPACK_NONE,
            crate::downloadinfo::URL_NONE,
            health,
            crit_est,
            crit_strict,
            true,
        ));
        let full_health = crate::downloadinfo::calc_health(1000, 0, 0, 0);
        assert!(crate::downloadinfo::is_dupe_success(
            MARK_STATUS_NONE,
            DELETE_STATUS_NONE,
            crate::downloadinfo::PAR_SKIPPED,
            crate::downloadinfo::UNPACK_NONE,
            crate::downloadinfo::URL_NONE,
            full_health,
            crit_est,
            crit_strict,
            true,
        ));

        // 3. DupeAlivePermilleTest
        let alive = |val: Option<&str>| -> i32 {
            match val {
                None => unsafe { parse_alive_permille_cstr(std::ptr::null()) },
                Some(s) => {
                    let c = CString::new(s).unwrap();
                    unsafe { parse_alive_permille_cstr(c.as_ptr()) }
                }
            }
        };
        assert_eq!(alive(None), -1);
        assert_eq!(alive(Some("100")), 1000);
        assert_eq!(alive(Some("99.6")), 996);
        assert_eq!(alive(Some("0")), 0);
        assert_eq!(alive(Some("")), -1);
        assert_eq!(alive(Some("abc")), -1);
        assert_eq!(alive(Some("-5")), 0);
        assert_eq!(alive(Some("1000")), 1000);
        assert_eq!(alive(Some("nan")), -1);

        // 4. DupeCoordinatorFindDupeBackupTest
        let make_backup = |name: &[u8], key: &[u8], score: i32| -> NzbgetRsDupeHistoryCandidate {
            NzbgetRsDupeHistoryCandidate {
                kind: HISTORY_KIND_NZB,
                dupe_mode: DUPE_MODE_SCORE,
                dupe_score: score,
                is_dupe_success: 0,
                mark_status: MARK_STATUS_NONE,
                delete_status: DELETE_STATUS_DUPE,
                dup_status: DUP_STATUS_UNDEFINED,
                full_content_hash: 0,
                alive_permille: -1,
                health: 1000,
                critical_health: 1000,
                same_name_or_key: i32::from(same_name_or_key(name, key, b"Rel.2026.1080p-A", b"imdb:1")),
            }
        };

        let mut history: Vec<(NzbgetRsDupeHistoryCandidate, bool)> = Vec::new();
        let mut queue = vec![NzbgetRsDupeQueueCandidate {
            is_self: 1,
            is_nzb_kind: 1,
            dupe_mode: DUPE_MODE_SCORE,
            dupe_score: 10,
            same_name_or_key: 1,
        }];

        let run_find = |hist: &[(NzbgetRsDupeHistoryCandidate, bool)],
                        q: &[NzbgetRsDupeQueueCandidate]|
         -> i32 {
            let candidates: Vec<NzbgetRsDupeHistoryCandidate> =
                hist.iter().map(|(c, _)| *c).collect();
            select_dupe_backup(true, 0, false, &candidates, q, |idx| hist[idx].1)
        };

        // nothing in history
        assert_eq!(run_find(&history, &queue), -1);

        // the highest-scored healthy backup of the same key wins; other keys are ignored
        history.insert(0, (make_backup(b"Rel.2026.1080p-B", b"imdb:1", 50), true));
        history.insert(0, (make_backup(b"Rel.2026.1080p-C", b"imdb:1", 90), true));
        history.insert(0, (make_backup(b"Other.2026-X", b"imdb:2", 100), true));
        let winner = run_find(&history, &queue);
        assert!(winner >= 0);
        assert_eq!(history[winner as usize].0.dupe_score, 90);

        // a backup whose source nzb-file is gone can't be downloaded again
        history.insert(0, (make_backup(b"Rel.2026.1080p-Z", b"imdb:1", 95), false));
        let winner = run_find(&history, &queue);
        assert!(winner >= 0);
        assert_eq!(history[winner as usize].0.dupe_score, 90);
        history[0].0.same_name_or_key = 0;

        // a backup marked bad is no candidate
        history[2].0.mark_status = MARK_STATUS_BAD;
        let winner = run_find(&history, &queue);
        assert!(winner >= 0);
        assert_eq!(history[winner as usize].0.dupe_score, 50);

        // a queued duplicate with a higher score than every backup blocks them
        queue.push(NzbgetRsDupeQueueCandidate {
            is_self: 0,
            is_nzb_kind: 1,
            dupe_mode: DUPE_MODE_SCORE,
            dupe_score: 60,
            same_name_or_key: 1,
        });
        assert_eq!(run_find(&history, &queue), -1);

        // the item itself never counts as its own rival
        queue[0].dupe_score = 99;
        queue.pop();
        let winner = run_find(&history, &queue);
        assert!(winner >= 0);
        assert_eq!(history[winner as usize].0.dupe_score, 50);

        // a good-duplicate in history ends the search
        let mut good = make_backup(b"Rel.2026.1080p-G", b"imdb:1", 1);
        good.delete_status = DELETE_STATUS_NONE;
        good.mark_status = MARK_STATUS_GOOD;
        good.is_dupe_success = 1;
        history.insert(0, (good, true));
        assert_eq!(run_find(&history, &queue), -1);

        // 5. RustDupeCoordinatorFfiParity
        assert!(same_name_or_key(b"Movie.A", b"imdb:123", b"Movie.B", b"IMDB:123"));
        assert!(!same_name_or_key(b"Movie.A", b"imdb:123", b"Movie.A", b"imdb:456"));
        assert!(same_name_or_key(b"Movie.A", b"", b"movie.a", b""));
        assert!(failover_warranted(1000, 300, 300));
        assert!(!failover_warranted(1000, 300, 299));
        assert_eq!(alive(Some("96.5")), 965);
        assert_eq!(alive(Some("invalid")), -1);
        assert_eq!(status_from_text(b"SUCCESS/ALL"), DUPE_STATUS_SUCCESS);
        assert_eq!(status_from_text(b"FAILURE/PAR"), DUPE_STATUS_FAILURE);
        assert_eq!(status_from_text(b"WARNING/SCRIPT"), DUPE_STATUS_WARNING);
        let text = String::from_utf8(format_no_backup_reason(5, 1, 1, 1, 1, 0, 1)).unwrap();
        assert!(text.contains("5 other item(s) of the key in history:"));
        assert_eq!(crate::downloadinfo::calc_health(1000, 0, 100, 0), 900);
        assert_eq!(crate::downloadinfo::calc_critical_health(1100, 100, 0, true), 900);
        assert_eq!(
            crate::artwritercore::calc_write_buffer_size(500, 1024 * 1024),
            500 * 1024
        );
        assert!(crate::queuecoord::is_data_filename(b"Show.S01E01.mkv"));
        assert!(!crate::queuecoord::is_data_filename(b"Show.S01E01.par2"));
        assert!(crate::queuecoord::size_near(10_000, 10_025));
        assert!(!crate::queuecoord::size_near(10_000, 10_035));
        assert!(crate::queuecoord::download_hopeless(1000, 5, 95));
        assert_eq!(
            crate::queueeditor::parse_dupe_mode(b"score"),
            Some(DUPE_MODE_SCORE)
        );
        assert_eq!(
            crate::queueeditor::parse_dupe_mode(b"all"),
            Some(DUPE_MODE_ALL)
        );
        assert_eq!(
            crate::queueeditor::parse_dupe_mode(b"force"),
            Some(DUPE_MODE_FORCE)
        );
        assert_eq!(crate::queueeditor::parse_dupe_mode(b"bad"), None);
    }
}
