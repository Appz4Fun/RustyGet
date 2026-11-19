//! End-to-end Rust implementation of `DupeCoordinator` (`daemon/queue/DupeCoordinator.cpp`):
//! duplicate name/key matching (`same_name_or_key`), on-disk verification (`files_on_disk`),
//! failover threshold & policy (`failover_warranted`, `fails_over`, `parse_alive_permille_cstr`),
//! backup candidate selection (`select_dupe_backup`), no-backup diagnostics
//! (`no_backup_reason`, `format_no_backup_reason`), skip message formatting
//! (`format_dupe_skip_message`), and status classification (`status_from_text`).

use crate::status::{DupStatus, DupeMode, MarkStatus};
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
        return Some(if neg {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
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
    std::str::from_utf8(&bytes[start..i])
        .ok()?
        .parse::<f64>()
        .ok()
}

#[inline]
pub fn strcasecmp_bytes(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[inline]
pub fn strncasecmp_prefix(haystack: &[u8], prefix: &[u8]) -> bool {
    haystack.len() >= prefix.len() && haystack[..prefix.len()].eq_ignore_ascii_case(prefix)
}

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
pub fn fails_over(dupe_check: bool, dupe_mode: DupeMode, health_check: i32) -> bool {
    dupe_check
        && (dupe_mode == DupeMode::Score
            || (dupe_mode == DupeMode::Force && health_check == HEALTH_CHECK_DUPE))
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
    dupe_mode: DupeMode,
    history: &[NzbgetRsDupeHistoryCandidate],
    mut file_exists_cb: F,
) -> Vec<u8>
where
    F: FnMut(usize) -> bool,
{
    if !fails_over_flag {
        return if dupe_mode == DupeMode::Force {
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
        } else if h.dupe_mode == DupeMode::Force {
            forced += 1;
        } else if !file_exists_cb(idx) {
            no_file += 1;
        } else if h.health < h.critical_health {
            unhealthy += 1;
        } else if h.mark_status == MarkStatus::Bad {
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
            && h.dupe_mode != DupeMode::Force
            && h.is_dupe_success != 0
            && h.same_name_or_key != 0
        {
            if !dupe_found || h.dupe_score > history_score {
                history_score = h.dupe_score;
            }
            dupe_found = true;
            good_dupe = h.mark_status == MarkStatus::Good;
        }

        if h.kind == HISTORY_KIND_DUP
            && h.dupe_mode != DupeMode::Force
            && (h.dup_status == DupStatus::Success || h.dup_status == DupStatus::Good)
            && h.same_name_or_key != 0
        {
            if !dupe_found || h.dupe_score > history_score {
                history_score = h.dupe_score;
            }
            dupe_found = true;
            good_dupe = h.dup_status == DupStatus::Good;
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
            && q.dupe_mode != DupeMode::Force
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
                    // a locked backup (encrypted headers, no password) only
                    // when no backup that can be extracted qualifies
                    let prev_h = &history[prev];
                    h.locked < prev_h.locked
                        || (h.locked == prev_h.locked
                            && (candidate_score > prev_h.dupe_score
                                || (candidate_score == prev_h.dupe_score
                                    && h.alive_permille > prev_h.alive_permille)))
                }
            };

        if has_candidate
            && h.dupe_mode != DupeMode::Force
            && backup_status
            && better
            && !(dupe_health && h.alive_permille == 0)
            && (h.kind != HISTORY_KIND_NZB || file_exists_cb(idx))
            && h.health >= h.critical_health
            && h.mark_status != MarkStatus::Bad
            && (!dupe_found || h.dupe_score > history_score)
            && (!queue_dupe || h.dupe_score > queue_score)
            && h.same_name_or_key != 0
        {
            best_idx = Some(idx);
        }
    }

    best_idx.map(|i| i as i32).unwrap_or(-1)
}

/// The history backup to send back to the queue after a download failed its
/// health check with nothing of its key left in the queue, in the fleet's
/// order: backups at least
/// [`crate::dupesearch_nzb::FLEET_HEALTHY_ALIVE_PERMILLE`] alive first, then
/// weak ones, then unmeasured ones; within each TWINs, then borrowable,
/// unknown and unborrowable ALTs (`DupeCopyKind`, see
/// [`crate::donorkind`]), each the wholest live backup by its measured
/// `DupeAlive` share and then its score. A locked backup (archive headers
/// encrypted, no password: [`crate::donorkind::LOCKED_PARAM`]) can never be
/// extracted and goes after all of them, whatever its health. Unlike
/// `select_dupe_backup` it takes backups in DupeMode FORCE and backups filed
/// as copies, which failover skips. `-1` when a download of the key is
/// queued, one already succeeded, or no backup is alive.
pub fn select_rescue_backup<F>(
    history: &[NzbgetRsDupeHistoryCandidate],
    queue: &[NzbgetRsDupeQueueCandidate],
    mut file_exists_cb: F,
) -> i32
where
    F: FnMut(usize) -> bool,
{
    if queue
        .iter()
        .any(|q| q.is_self == 0 && q.is_nzb_kind != 0 && q.same_name_or_key != 0)
    {
        return -1;
    }
    let succeeded = history.iter().any(|h| {
        h.same_name_or_key != 0
            && ((h.kind == HISTORY_KIND_NZB && h.is_dupe_success != 0)
                || (h.kind == HISTORY_KIND_DUP
                    && (h.dup_status == DupStatus::Success || h.dup_status == DupStatus::Good)))
    });
    if succeeded {
        return -1;
    }
    let mut best: Option<(usize, (i32, i32, i32, i32, i32))> = None;
    for (idx, h) in history.iter().enumerate() {
        // healthy copies first, then weak, then unmeasured ones (as the fleet
        // ranks them); within each TWINs, then borrowable, unknown and
        // unborrowable ALTs, then by measured health and score
        let health_tier = if h.alive_permille < 0 {
            0
        } else if h.alive_permille < crate::dupesearch_nzb::FLEET_HEALTHY_ALIVE_PERMILLE {
            1
        } else {
            2
        };
        let rank = (
            -h.locked,
            health_tier,
            -h.donor_kind,
            h.alive_permille.max(-1),
            h.dupe_score,
        );
        if h.kind == HISTORY_KIND_NZB
            && h.same_name_or_key != 0
            && (h.delete_status == DELETE_STATUS_DUPE || h.delete_status == DELETE_STATUS_COPY)
            && h.alive_permille != 0
            && h.health >= h.critical_health
            && h.mark_status != MarkStatus::Bad
            && best.is_none_or(|(_, b)| rank > b)
            && file_exists_cb(idx)
        {
            best = Some((idx, rank));
        }
    }
    best.map_or(-1, |(idx, _)| idx as i32)
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
    item_mark_status: MarkStatus,
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
        && item_mark_status != MarkStatus::Bad
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
    dup_status: DupStatus,
) -> i32 {
    if kind == HISTORY_KIND_NZB && same_name_or_key(name, dupe_key, item_name, item_dupe_key) {
        return current_statuses | status_from_text(text_status);
    }
    if kind == HISTORY_KIND_DUP && same_name_or_key(name, dupe_key, item_name, item_dupe_key) {
        if dup_status == DupStatus::Success || dup_status == DupStatus::Good {
            return current_statuses | DUPE_STATUS_SUCCESS;
        }
        if dup_status == DupStatus::Failed || dup_status == DupStatus::Bad {
            return current_statuses | DUPE_STATUS_FAILURE;
        }
    }
    current_statuses
}

/// `DupeCoordinator::HistoryCleanup` candidate predicate.
#[allow(clippy::too_many_arguments)]
pub fn should_cleanup_history_dupe(
    kind: i32,
    dupe_mode: DupeMode,
    delete_status: i32,
    is_self: bool,
    item_name: &[u8],
    item_dupe_key: &[u8],
    target_name: &[u8],
    target_dupe_key: &[u8],
) -> bool {
    !is_self
        && (kind == HISTORY_KIND_NZB || kind == HISTORY_KIND_URL)
        && dupe_mode != DupeMode::Force
        && delete_status == DELETE_STATUS_DUPE
        && same_name_or_key(item_name, item_dupe_key, target_name, target_dupe_key)
}

/// `DupeCoordinator::ListHistoryDupes` candidate predicate.
pub fn should_list_history_dupe(
    self_dupe_mode: DupeMode,
    kind: i32,
    item_dupe_mode: DupeMode,
    item_name: &[u8],
    item_dupe_key: &[u8],
    self_name: &[u8],
    self_dupe_key: &[u8],
) -> bool {
    self_dupe_mode != DupeMode::Force
        && kind == HISTORY_KIND_NZB
        && item_dupe_mode != DupeMode::Force
        && same_name_or_key(item_name, item_dupe_key, self_name, self_dupe_key)
}

/// `DupeCoordinator::NzbFound` content hash equality check.
#[inline]
pub fn same_content_hash(full1: u32, filtered1: u32, full2: u32, filtered2: u32) -> bool {
    (full1 > 0 && full1 == full2) || (filtered1 > 0 && filtered1 == filtered2)
}

/// `DupeCoordinator::NzbFound` property-inheritance match predicate:
/// returns true when `self_name == other_name` and `other` has a non-empty `other_key` or non-zero `other_score`.
#[inline]
pub fn should_inherit_dupe_props(
    self_name: &[u8],
    other_name: &[u8],
    other_key: &[u8],
    other_score: i32,
) -> bool {
    self_name == other_name && (!other_key.is_empty() || other_score != 0)
}

/// `DupeCoordinator::NzbFound` history duplicate match evaluator.
/// Returns `Some((same_content, good))` when the history item causes `self` to be skipped.
#[allow(clippy::too_many_arguments)]
pub fn eval_history_dupe_match(
    kind: i32,
    self_full_hash: u32,
    self_filtered_hash: u32,
    self_dupe_mode: DupeMode,
    self_dupe_score: i32,
    self_name: &[u8],
    self_dupe_key: &[u8],
    item_full_hash: u32,
    item_filtered_hash: u32,
    item_dupe_mode: DupeMode,
    item_mark_status: MarkStatus,
    item_dup_status: DupStatus,
    item_dupe_score: i32,
    item_name: &[u8],
    item_dupe_key: &[u8],
) -> Option<(bool, bool)> {
    if (kind == HISTORY_KIND_NZB || kind == HISTORY_KIND_DUP)
        && same_content_hash(
            self_full_hash,
            self_filtered_hash,
            item_full_hash,
            item_filtered_hash,
        )
    {
        return Some((true, false));
    }
    if kind == HISTORY_KIND_NZB
        && item_dupe_mode != DupeMode::Force
        && item_mark_status == MarkStatus::Good
        && same_name_or_key(item_name, item_dupe_key, self_name, self_dupe_key)
    {
        return Some((false, true));
    }
    if kind == HISTORY_KIND_DUP
        && item_dupe_mode != DupeMode::Force
        && (item_dup_status == DupStatus::Good
            || (self_dupe_mode == DupeMode::Score
                && item_dup_status == DupStatus::Success
                && self_dupe_score <= item_dupe_score))
        && same_name_or_key(item_name, item_dupe_key, self_name, self_dupe_key)
    {
        return Some((false, item_dup_status == DupStatus::Good));
    }
    None
}

/// `DupeCoordinator::NzbFound` `HealthCheck=dupe` same-content history outcome accumulator.
#[allow(clippy::too_many_arguments)]
pub fn accum_hc_dupe_same_content_outcome(
    kind: i32,
    self_full_hash: u32,
    self_filtered_hash: u32,
    item_full_hash: u32,
    item_filtered_hash: u32,
    item_delete_status: i32,
    item_is_dupe_success: bool,
    item_dup_status: DupStatus,
    succeeded: &mut bool,
    failed: &mut bool,
) {
    if !same_content_hash(
        self_full_hash,
        self_filtered_hash,
        item_full_hash,
        item_filtered_hash,
    ) {
        return;
    }
    if kind == HISTORY_KIND_NZB {
        if item_delete_status != DELETE_STATUS_DUPE
            && item_delete_status != DELETE_STATUS_COPY
            && item_delete_status != DELETE_STATUS_MANUAL
        {
            if item_is_dupe_success {
                *succeeded = true;
            } else {
                *failed = true;
            }
        }
    } else if kind == HISTORY_KIND_DUP {
        if item_dup_status == DupStatus::Success || item_dup_status == DupStatus::Good {
            *succeeded = true;
        } else if item_dup_status == DupStatus::Failed || item_dup_status == DupStatus::Bad {
            *failed = true;
        }
    }
}

/// `DupeCoordinator::NzbFound` recent-history score-duplicate skip predicate.
#[allow(clippy::too_many_arguments)]
pub fn should_skip_recent_history_score_dupe(
    same_content: bool,
    good: bool,
    self_dupe_mode: DupeMode,
    self_dupe_score: i32,
    self_name: &[u8],
    self_dupe_key: &[u8],
    kind: i32,
    item_dupe_mode: DupeMode,
    item_dupe_score: i32,
    item_is_dupe_success: bool,
    item_name: &[u8],
    item_dupe_key: &[u8],
) -> bool {
    !same_content
        && !good
        && self_dupe_mode == DupeMode::Score
        && (kind == HISTORY_KIND_NZB || kind == HISTORY_KIND_URL)
        && item_dupe_mode != DupeMode::Force
        && same_name_or_key(item_name, item_dupe_key, self_name, self_dupe_key)
        && self_dupe_score <= item_dupe_score
        && item_is_dupe_success
}

/// `DupeCoordinator::NzbFound` queue score-duplicate evaluator:
/// - `0`: not a matching duplicate or no action
/// - `1`: skip `self` (`dsDupe`) and return
/// - `2`: move `queued` with lower score to history (`dsDupe` + `eaGroupDelete`)
#[allow(clippy::too_many_arguments)]
pub fn eval_queue_score_dupe(
    is_self: bool,
    queued_delete_status: i32,
    queued_is_nzb: bool,
    both_are_url: bool,
    queued_dupe_mode: DupeMode,
    queued_name: &[u8],
    queued_dupe_key: &[u8],
    self_name: &[u8],
    self_dupe_key: &[u8],
    self_dupe_score: i32,
    queued_dupe_score: i32,
    health_check: i32,
    queued_is_redownload_auto: bool,
    queued_cur_success_articles: i32,
    queued_health: i32,
    queued_crit_health: i32,
    queued_has_post_info: bool,
) -> i32 {
    if is_self
        || queued_delete_status != DELETE_STATUS_NONE
        || (!queued_is_nzb && !both_are_url)
        || queued_dupe_mode == DupeMode::Force
        || !same_name_or_key(queued_name, queued_dupe_key, self_name, self_dupe_key)
    {
        return 0;
    }
    let keep_returned = health_check == HEALTH_CHECK_DUPE
        && queued_is_redownload_auto
        && queued_cur_success_articles > 0
        && queued_health >= queued_crit_health;
    if self_dupe_score <= queued_dupe_score || keep_returned {
        return 1;
    }
    if !queued_has_post_info {
        return 2;
    }
    0
}

/// `DupeCoordinator::NzbCompleted` predicate for stopping other queued items of the same key.
#[allow(clippy::too_many_arguments)]
pub fn is_other_to_stop_on_completed(
    is_self: bool,
    queued_is_nzb: bool,
    queued_deleting: bool,
    queued_delete_status: i32,
    queued_dupe_mode: DupeMode,
    queued_dupe_key: &[u8],
    self_dupe_key: &[u8],
) -> bool {
    !is_self
        && queued_is_nzb
        && !queued_deleting
        && queued_delete_status == DELETE_STATUS_NONE
        && queued_dupe_mode != DupeMode::Force
        && strcasecmp_bytes(queued_dupe_key, self_dupe_key)
}

/// `DupeCoordinator::HistoryMark` status/action resolver.
pub fn resolve_history_mark(
    mark_status: MarkStatus,
    dupe_check: bool,
    kind: i32,
    dupe_mode: DupeMode,
) -> (&'static [u8], DupStatus, i32) {
    let mark_name: &'static [u8] = if mark_status == MarkStatus::Bad {
        b"bad\0"
    } else if mark_status == MarkStatus::Good {
        b"good\0"
    } else if mark_status == MarkStatus::Success {
        b"success\0"
    } else {
        b"NONE\0"
    };
    let dup_status = if mark_status == MarkStatus::Good {
        DupStatus::Good
    } else if mark_status == MarkStatus::Success {
        DupStatus::Success
    } else {
        DupStatus::Bad
    };
    if !dupe_check
        || ((kind == HISTORY_KIND_NZB || kind == HISTORY_KIND_DUP) && dupe_mode == DupeMode::Force)
    {
        return (mark_name, dup_status, 0);
    }
    let post_action = if mark_status == MarkStatus::Good {
        1
    } else if mark_status == MarkStatus::Bad {
        2
    } else {
        0
    };
    (mark_name, dup_status, post_action)
}

#[cfg(unix)]
#[inline]
pub(crate) fn dupe_failover_warranted_inline(
    item_score: i32,
    health: i32,
    backup_score: i32,
) -> bool {
    backup_score > item_score || health < 1000
}

#[cfg(unix)]
pub(crate) fn parse_alive_permille_bytes(bytes: &[u8]) -> i32 {
    let Some(percent) = parse_f64_prefix(bytes) else {
        return -1;
    };
    if percent.is_nan() {
        return -1;
    }
    let clamped = percent.clamp(0.0, 100.0);
    (clamped * 10.0).round() as i32
}

#[cfg(unix)]
pub(crate) fn alive_permille_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
) -> i32 {
    match host.nzb_get_param(nzb_ptr, b"DupeAlive") {
        Some(v) => parse_alive_permille_bytes(&v),
        None => -1,
    }
}

/// Whether a queued or parked item is locked now: its archive headers are
/// encrypted ([`crate::donorkind::LOCKED_PARAM`]) and no password is at hand
/// (its `*Unpack:Password`, or `UnpackPassFile`), see [`crate::donorkind::locked`].
#[cfg(unix)]
pub(crate) fn locked_host(host: &crate::queuesubsys_host::QueueSubsysHost, nzb_ptr: usize) -> bool {
    crate::donorkind::locked_params(
        host.nzb_get_param(nzb_ptr, crate::donorkind::LOCKED_PARAM)
            .as_deref(),
        host.nzb_get_param(nzb_ptr, crate::donorkind::PASSWORD_PARAM)
            .as_deref(),
    )
}

#[cfg(unix)]
pub(crate) fn fails_over_snap(
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> bool {
    opts.dupe_check
        && (nzb.dupe_mode == DupeMode::Score
            || (nzb.dupe_mode == DupeMode::Force
                && opts.health_check == crate::queuesubsys_host::HC_DUPE))
}

#[cfg(unix)]
fn downloaded_on_disk_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> Option<crate::queuesubsys_host::NzbSnap> {
    if (opts.health_check != crate::queuesubsys_host::HC_DUPE
        && opts.health_check != crate::queuesubsys_host::HC_PARK)
        || nzb.dupe_key.is_empty()
    {
        return None;
    }
    let hlen = host.history_len(dq_ptr);
    for i in 0..hlen {
        let hi = host.history_at(dq_ptr, i);
        if hi.kind != crate::queuesubsys_host::HK_NZB || hi.inner_ptr == 0 {
            continue;
        }
        if hi.inner_ptr == nzb.ptr {
            continue;
        }
        let item = host.nzb_snap(hi.inner_ptr, false);
        if item.delete_status == crate::queuesubsys_host::DS_NONE
            && item.is_dupe_success
            && item.mark_status != MarkStatus::Bad
            && item.dupe_key.eq_ignore_ascii_case(&nzb.dupe_key)
            && files_on_disk(&item.final_dir, &item.dest_dir, &opts.ext_cleanup_disk)
        {
            return Some(item);
        }
    }
    None
}

#[cfg(unix)]
pub(crate) fn no_backup_reason_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> Vec<u8> {
    let hlen = host.history_len(dq_ptr);
    let mut history_candidates = Vec::with_capacity(hlen);
    let mut queued_filenames: Vec<Vec<u8>> = Vec::with_capacity(hlen);
    for i in 0..hlen {
        let hi = host.history_at(dq_ptr, i);
        let mut c = NzbgetRsDupeHistoryCandidate {
            kind: hi.kind,
            dupe_mode: DupeMode::Score,
            dupe_score: 0,
            is_dupe_success: 0,
            mark_status: MarkStatus::None,
            delete_status: 0,
            dup_status: DupStatus::Undefined,
            full_content_hash: 0,
            alive_permille: 0,
            health: 0,
            critical_health: 0,
            same_name_or_key: 0,
            donor_kind: 2,
            locked: 0,
        };
        let mut qfn = Vec::new();
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.inner_ptr != 0 {
            let item = host.nzb_snap(hi.inner_ptr, false);
            c.dupe_mode = item.dupe_mode;
            c.mark_status = item.mark_status;
            c.delete_status = item.delete_status;
            c.alive_permille = alive_permille_host(host, item.ptr);
            c.donor_kind = crate::donorkind::DonorKind::from_param(
                host.nzb_get_param(item.ptr, crate::donorkind::KIND_PARAM)
                    .as_deref(),
            ) as i32;
            c.locked = i32::from(locked_host(host, item.ptr));
            c.health = item.health;
            c.critical_health = item.critical_health_opt;
            c.same_name_or_key = i32::from(
                item.ptr != nzb.ptr
                    && same_name_or_key(&item.name, &item.dupe_key, &nzb.name, &nzb.dupe_key),
            );
            qfn = item.queued_filename;
        }
        history_candidates.push(c);
        queued_filenames.push(qfn);
    }
    no_backup_reason(
        fails_over_snap(opts, nzb),
        nzb.dupe_mode,
        &history_candidates,
        |idx| {
            queued_filenames.get(idx).is_some_and(|p| {
                !p.is_empty() && crate::fsops::file_exists(p, &crate::fsops::posix_stat)
            })
        },
    )
}

#[cfg(unix)]
struct DupeCandidates {
    self_snap: Option<crate::queuesubsys_host::NzbSnap>,
    history_entries: Vec<crate::queuesubsys_host::HistEntrySnap>,
    history_candidates: Vec<NzbgetRsDupeHistoryCandidate>,
    queued_filenames: Vec<Vec<u8>>,
    queue_candidates: Vec<NzbgetRsDupeQueueCandidate>,
}

/// The history and queue items that `select_dupe_backup` and
/// `select_rescue_backup` choose from.
#[cfg(unix)]
fn collect_dupe_candidates(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb_ptr: usize,
    nzb_name: &[u8],
    dupe_key: &[u8],
) -> DupeCandidates {
    let self_snap = if nzb_ptr != 0 {
        Some(host.nzb_snap(nzb_ptr, false))
    } else {
        None
    };
    let hlen = host.history_len(dq_ptr);
    let mut history_entries = Vec::with_capacity(hlen);
    let mut history_candidates = Vec::with_capacity(hlen);
    let mut queued_filenames: Vec<Vec<u8>> = Vec::with_capacity(hlen);
    for i in 0..hlen {
        let hi = host.history_at(dq_ptr, i);
        let mut c = NzbgetRsDupeHistoryCandidate {
            kind: hi.kind,
            dupe_mode: DupeMode::Score,
            dupe_score: 0,
            is_dupe_success: 0,
            mark_status: MarkStatus::None,
            delete_status: 0,
            dup_status: DupStatus::Undefined,
            full_content_hash: 0,
            alive_permille: 0,
            health: 0,
            critical_health: 0,
            same_name_or_key: 0,
            donor_kind: 2,
            locked: 0,
        };
        let mut qfn = Vec::new();
        if (hi.kind == crate::queuesubsys_host::HK_NZB
            || hi.kind == crate::queuesubsys_host::HK_URL)
            && hi.inner_ptr != 0
        {
            let item = host.nzb_snap(hi.inner_ptr, false);
            c.dupe_mode = item.dupe_mode;
            c.dupe_score = item.dupe_score;
            c.is_dupe_success = i32::from(item.is_dupe_success);
            c.mark_status = item.mark_status;
            c.delete_status = item.delete_status;
            c.full_content_hash = item.full_content_hash;
            c.alive_permille = alive_permille_host(host, item.ptr);
            c.donor_kind = crate::donorkind::DonorKind::from_param(
                host.nzb_get_param(item.ptr, crate::donorkind::KIND_PARAM)
                    .as_deref(),
            ) as i32;
            c.locked = i32::from(locked_host(host, item.ptr));
            c.health = item.health;
            c.critical_health = item.critical_health_opt;
            c.same_name_or_key = i32::from(same_name_or_key(
                &item.name,
                &item.dupe_key,
                nzb_name,
                dupe_key,
            ));
            qfn = item.queued_filename;
        } else if hi.kind == crate::queuesubsys_host::HK_DUP && hi.inner_ptr != 0 {
            let dup = host.dup_snap(hi.inner_ptr);
            c.dupe_mode = dup.dupe_mode;
            c.dupe_score = dup.dupe_score;
            c.dup_status = dup.status;
            c.same_name_or_key = i32::from(same_name_or_key(
                &dup.name,
                &dup.dupe_key,
                nzb_name,
                dupe_key,
            ));
        }
        history_entries.push(hi);
        history_candidates.push(c);
        queued_filenames.push(qfn);
    }

    let qlen = host.queue_len(dq_ptr);
    let mut queue_candidates = Vec::with_capacity(qlen);
    for i in 0..qlen {
        let qp = host.queue_at(dq_ptr, i);
        let q = host.nzb_snap(qp, false);
        queue_candidates.push(NzbgetRsDupeQueueCandidate {
            is_self: i32::from(qp == nzb_ptr && nzb_ptr != 0),
            is_nzb_kind: i32::from(q.kind == crate::queuesubsys_host::NK_NZB),
            dupe_mode: q.dupe_mode,
            dupe_score: q.dupe_score,
            same_name_or_key: i32::from(same_name_or_key(&q.name, &q.dupe_key, nzb_name, dupe_key)),
        });
    }

    DupeCandidates {
        self_snap,
        history_entries,
        history_candidates,
        queued_filenames,
        queue_candidates,
    }
}

#[cfg(unix)]
pub(crate) fn find_dupe_backup_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    nzb_ptr: usize,
    nzb_name: &[u8],
    dupe_key: &[u8],
) -> Option<(
    crate::queuesubsys_host::HistEntrySnap,
    crate::queuesubsys_host::NzbSnap,
)> {
    let DupeCandidates {
        self_snap,
        history_entries,
        history_candidates,
        queued_filenames,
        queue_candidates,
    } = collect_dupe_candidates(host, dq_ptr, nzb_ptr, nzb_name, dupe_key);

    let winner = select_dupe_backup(
        self_snap.is_some(),
        self_snap.as_ref().map_or(0, |s| s.full_content_hash),
        opts.health_check == crate::queuesubsys_host::HC_DUPE,
        &history_candidates,
        &queue_candidates,
        |idx| {
            queued_filenames.get(idx).is_some_and(|p| {
                !p.is_empty() && crate::fsops::file_exists(p, &crate::fsops::posix_stat)
            })
        },
    );
    if winner >= 0 && (winner as usize) < history_entries.len() {
        let hi = history_entries[winner as usize].clone();
        let nzb_snap = if hi.inner_ptr != 0 {
            host.nzb_snap(hi.inner_ptr, false)
        } else {
            crate::queuesubsys_host::NzbSnap::default()
        };
        Some((hi, nzb_snap))
    } else {
        None
    }
}

#[cfg(unix)]
fn return_best_dupe_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    nzb_ptr: usize,
    nzb_name: &[u8],
    dupe_key: &[u8],
) {
    if let Some((hi, backup_nzb)) =
        find_dupe_backup_host(host, opts, dq_ptr, nzb_ptr, nzb_name, dupe_key)
    {
        let mut msg = b"Found duplicate ".to_vec();
        msg.extend_from_slice(&backup_nzb.name);
        msg.extend_from_slice(b" for ");
        msg.extend_from_slice(nzb_name);
        host.log(crate::queuesubsys_host::MK_INFO, &msg);
        host.nzb_set_int(
            backup_nzb.ptr,
            crate::queuesubsys_host::NZB_INT_DUPE_HINT,
            crate::queuesubsys_host::DH_REDOWNLOAD_AUTO,
        );
        host.history_redownload(dq_ptr, hi.hi_ptr);
    }
}

/// Rescue (option `DupeWatch` or `DupeSearch`): a download failed its health
/// check and failover returned nothing, so no download of its key is left. Its backups in
/// DupeMode FORCE and those filed as copies are not failover candidates, so
/// the wholest live one goes back to the queue here.
/// Called for every completed download after the duplicate coordinator had
/// its turn, whatever the DupeMode: failover never runs for a pick in DupeMode
/// FORCE.
#[cfg(unix)]
pub(crate) fn rescue_on_health_failure_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb_ptr: usize,
) {
    if dq_ptr == 0 || nzb_ptr == 0 {
        return;
    }
    let nzb = host.nzb_snap(nzb_ptr, false);
    if !host.opts().dupe_check
        || nzb.kind != crate::queuesubsys_host::NK_NZB
        || nzb.is_dupe_success
        || nzb.delete_status != crate::queuesubsys_host::DS_HEALTH
        || nzb.dupe_key.is_empty()
        || !(crate::options::dupe_watch()
            || crate::options::active_options().is_some_and(|o| o.scalars().dupe_search != 0))
    {
        return;
    }
    rescue_dupe_host(host, dq_ptr, &nzb);
}

#[cfg(unix)]
fn rescue_dupe_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb: &crate::queuesubsys_host::NzbSnap,
) {
    let DupeCandidates {
        history_entries,
        history_candidates,
        queued_filenames,
        queue_candidates,
        ..
    } = collect_dupe_candidates(host, dq_ptr, nzb.ptr, &nzb.name, &nzb.dupe_key);
    let winner = select_rescue_backup(&history_candidates, &queue_candidates, |idx| {
        queued_filenames.get(idx).is_some_and(|p| {
            !p.is_empty() && crate::fsops::file_exists(p, &crate::fsops::posix_stat)
        })
    });
    let Some(hi) = usize::try_from(winner)
        .ok()
        .and_then(|i| history_entries.get(i))
    else {
        return;
    };
    if hi.inner_ptr == 0 {
        return;
    }
    let backup = host.nzb_snap(hi.inner_ptr, false);
    let alive = history_candidates[winner as usize].alive_permille;
    // DupeSearchDryRun only covers what DupeSearch itself would do.
    let dry_run = crate::options::active_options()
        .is_some_and(|o| o.scalars().dupe_search != 0 && o.scalars().dupe_search_dry_run != 0);
    let mut msg = b"Rescue: ".to_vec();
    msg.extend_from_slice(&nzb.name);
    msg.extend_from_slice(b" failed its health check with nothing of its key queued: ");
    msg.extend_from_slice(if dry_run {
        b"dry run, would return backup ".as_slice()
    } else {
        b"returning backup ".as_slice()
    });
    msg.extend_from_slice(&backup.name);
    if alive >= 0 {
        msg.extend_from_slice(format!(" (alive {}.{}%)", alive / 10, alive % 10).as_bytes());
    }
    host.log(crate::queuesubsys_host::MK_INFO, &msg);
    if dry_run {
        return;
    }
    host.nzb_set_int(
        backup.ptr,
        crate::queuesubsys_host::NZB_INT_DUPE_HINT,
        crate::queuesubsys_host::DH_REDOWNLOAD_AUTO,
    );
    host.history_redownload(dq_ptr, hi.hi_ptr);
}

#[cfg(unix)]
fn history_cleanup_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    mark_hi_ptr: usize,
    nzb_name: &[u8],
    dupe_key: &[u8],
) {
    let mut changed = false;
    let mut rindex = 0usize;
    loop {
        let hlen = host.history_len(dq_ptr);
        if rindex >= hlen {
            break;
        }
        let fwd_idx = hlen - 1 - rindex;
        let hi = host.history_at(dq_ptr, fwd_idx);
        let is_nzb_or_url = hi.kind == crate::queuesubsys_host::HK_NZB
            || hi.kind == crate::queuesubsys_host::HK_URL;
        if is_nzb_or_url && hi.inner_ptr != 0 && hi.hi_ptr != mark_hi_ptr {
            let item = host.nzb_snap(hi.inner_ptr, false);
            if item.dupe_mode != DupeMode::Force
                && item.delete_status == crate::queuesubsys_host::DS_DUPE
                && same_name_or_key(&item.name, &item.dupe_key, nzb_name, dupe_key)
            {
                host.history_hide(dq_ptr, hi.hi_ptr, rindex as i32);
                rindex += 1;
                changed = true;
                continue;
            }
        }
        rindex += 1;
    }
    if changed {
        host.dq_history_changed(dq_ptr);
        host.dq_save(dq_ptr);
    }
}

#[cfg(unix)]
pub(crate) fn nzb_found_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb_ptr: usize,
) {
    if dq_ptr == 0 || nzb_ptr == 0 {
        return;
    }
    let opts = host.opts();
    let mut nzb = host.nzb_snap(nzb_ptr, false);

    let mut dbg = b"Checking duplicates for ".to_vec();
    dbg.extend_from_slice(&nzb.name);
    host.log(crate::queuesubsys_host::MK_DEBUG, &dbg);

    if nzb.kind == crate::queuesubsys_host::NK_NZB && nzb.dupe_mode != DupeMode::Force {
        if let Some(downloaded) = downloaded_on_disk_host(host, &opts, dq_ptr, &nzb) {
            let mut msg = b"Not downloading ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(&downloaded.name);
            msg.extend_from_slice(b" is downloaded and on disk");
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
            host.nzb_set_int(
                nzb.ptr,
                crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                crate::queuesubsys_host::DS_DUPE,
            );
            return;
        }
    }

    let qlen = host.queue_len(dq_ptr);
    for i in 0..qlen {
        let qp = host.queue_at(dq_ptr, i);
        if qp == 0 || qp == nzb.ptr {
            continue;
        }
        let q = host.nzb_snap(qp, false);
        let same_content = (nzb.full_content_hash > 0
            && nzb.full_content_hash == q.full_content_hash)
            || (nzb.filtered_content_hash > 0
                && nzb.filtered_content_hash == q.filtered_content_hash);
        if same_content && nzb.kind == crate::queuesubsys_host::NK_NZB {
            let message = format_dupe_skip_message(&nzb.name, &q.name, false, true, false);
            if nzb.feed_id != 0 {
                host.log(crate::queuesubsys_host::MK_WARNING, &message);
                host.nzb_set_int(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                    crate::queuesubsys_host::DS_MANUAL,
                );
                host.history_delete_disk_files(nzb.ptr);
            } else {
                host.nzb_set_int(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                    crate::queuesubsys_host::DS_COPY,
                );
                host.nzb_add_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &message);
            }
            return;
        }
    }

    let try_inherit = |nzb: &mut crate::queuesubsys_host::NzbSnap,
                       other_name: &[u8],
                       other_key: &[u8],
                       other_score: i32,
                       source_label: &[u8]|
     -> bool {
        if nzb.name == other_name && (!other_key.is_empty() || other_score != 0) {
            nzb.dupe_key = other_key.to_vec();
            nzb.dupe_score = other_score;
            host.nzb_set_str(
                nzb.ptr,
                crate::queuesubsys_host::NZB_STR_DUPE_KEY,
                &nzb.dupe_key,
            );
            host.nzb_set_int(
                nzb.ptr,
                crate::queuesubsys_host::NZB_INT_DUPE_SCORE,
                nzb.dupe_score,
            );
            let mut msg = b"Assigning dupekey ".to_vec();
            msg.extend_from_slice(&nzb.dupe_key);
            msg.extend_from_slice(format!(" and dupescore {} to ", nzb.dupe_score).as_bytes());
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" from existing ");
            msg.extend_from_slice(source_label);
            msg.extend_from_slice(b" item with the same name");
            host.log(crate::queuesubsys_host::MK_INFO, &msg);
            return true;
        }
        false
    };

    if nzb.dupe_key.is_empty() && nzb.dupe_score == 0 {
        for i in 0..qlen {
            let qp = host.queue_at(dq_ptr, i);
            let q = host.nzb_snap(qp, false);
            if try_inherit(&mut nzb, &q.name, &q.dupe_key, q.dupe_score, b"queue") {
                break;
            }
        }
    }
    if nzb.dupe_key.is_empty() && nzb.dupe_score == 0 {
        let hlen = host.history_len(dq_ptr);
        for i in 0..hlen {
            let hi = host.history_at(dq_ptr, i);
            if hi.kind == crate::queuesubsys_host::HK_NZB && hi.inner_ptr != 0 {
                let hn = host.nzb_snap(hi.inner_ptr, false);
                if try_inherit(&mut nzb, &hn.name, &hn.dupe_key, hn.dupe_score, b"history") {
                    break;
                }
            } else if hi.kind == crate::queuesubsys_host::HK_DUP && hi.inner_ptr != 0 {
                let hd = host.dup_snap(hi.inner_ptr);
                if try_inherit(&mut nzb, &hd.name, &hd.dupe_key, hd.dupe_score, b"history") {
                    break;
                }
            }
        }
    }

    let mut skip = false;
    let mut good = false;
    let mut same_content = false;
    let mut dupe_name: Vec<u8> = Vec::new();

    let hlen = host.history_len(dq_ptr);
    for i in 0..hlen {
        let hi = host.history_at(dq_ptr, i);
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.inner_ptr != 0 {
            let item = host.nzb_snap(hi.inner_ptr, false);
            if (item.full_content_hash > 0 && item.full_content_hash == nzb.full_content_hash)
                || (item.filtered_content_hash > 0
                    && item.filtered_content_hash == nzb.filtered_content_hash)
            {
                skip = true;
                same_content = true;
                dupe_name = item.name;
                break;
            }
            if item.dupe_mode != DupeMode::Force
                && item.mark_status == MarkStatus::Good
                && same_name_or_key(&item.name, &item.dupe_key, &nzb.name, &nzb.dupe_key)
            {
                skip = true;
                good = true;
                dupe_name = item.name;
                break;
            }
        } else if hi.kind == crate::queuesubsys_host::HK_DUP && hi.inner_ptr != 0 {
            let dup = host.dup_snap(hi.inner_ptr);
            if (dup.full_content_hash > 0 && dup.full_content_hash == nzb.full_content_hash)
                || (dup.filtered_content_hash > 0
                    && dup.filtered_content_hash == nzb.filtered_content_hash)
            {
                skip = true;
                same_content = true;
                dupe_name = dup.name;
                break;
            }
            if dup.dupe_mode != DupeMode::Force
                && same_name_or_key(&dup.name, &dup.dupe_key, &nzb.name, &nzb.dupe_key)
                && (dup.status == DupStatus::Good
                    || (nzb.dupe_mode == DupeMode::Score
                        && dup.status == DupStatus::Success
                        && nzb.dupe_score <= dup.dupe_score))
            {
                skip = true;
                good = dup.status == DupStatus::Good;
                dupe_name = dup.name;
                break;
            }
        }
    }

    let mut return_backup = false;
    if skip
        && same_content
        && opts.health_check == crate::queuesubsys_host::HC_DUPE
        && nzb.kind == crate::queuesubsys_host::NK_NZB
        && nzb.dupe_hint == crate::queuesubsys_host::DH_NONE
    {
        let mut succeeded = false;
        let mut failed = false;
        for i in 0..hlen {
            let hi = host.history_at(dq_ptr, i);
            if hi.kind == crate::queuesubsys_host::HK_NZB && hi.inner_ptr != 0 {
                let item = host.nzb_snap(hi.inner_ptr, false);
                if (item.full_content_hash > 0 && item.full_content_hash == nzb.full_content_hash)
                    || (item.filtered_content_hash > 0
                        && item.filtered_content_hash == nzb.filtered_content_hash)
                {
                    if item.delete_status != crate::queuesubsys_host::DS_DUPE
                        && item.delete_status != crate::queuesubsys_host::DS_COPY
                        && item.delete_status != crate::queuesubsys_host::DS_MANUAL
                    {
                        if item.is_dupe_success {
                            succeeded = true;
                        } else {
                            failed = true;
                        }
                    }
                }
            } else if hi.kind == crate::queuesubsys_host::HK_DUP && hi.inner_ptr != 0 {
                let dup = host.dup_snap(hi.inner_ptr);
                if (dup.full_content_hash > 0 && dup.full_content_hash == nzb.full_content_hash)
                    || (dup.filtered_content_hash > 0
                        && dup.filtered_content_hash == nzb.filtered_content_hash)
                {
                    if dup.status == DupStatus::Success || dup.status == DupStatus::Good {
                        succeeded = true;
                    } else if dup.status == DupStatus::Failed || dup.status == DupStatus::Bad {
                        failed = true;
                    }
                }
            }
        }
        if !succeeded && !failed {
            let mut msg = b"Adding ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b": the same content waits in history only as an untried backup");
            host.log(crate::queuesubsys_host::MK_INFO, &msg);
            skip = false;
            same_content = false;
        } else if failed && !succeeded {
            let mut any_queued = false;
            let cur_qlen = host.queue_len(dq_ptr);
            for i in 0..cur_qlen {
                let qp = host.queue_at(dq_ptr, i);
                if qp == 0 || qp == nzb.ptr {
                    continue;
                }
                let q = host.nzb_snap(qp, false);
                if q.delete_status == crate::queuesubsys_host::DS_NONE
                    && same_name_or_key(&q.name, &q.dupe_key, &nzb.name, &nzb.dupe_key)
                {
                    any_queued = true;
                    break;
                }
            }
            return_backup = !any_queued;
        }
    }

    if !same_content && nzb.dupe_hint != crate::queuesubsys_host::DH_NONE {
        return;
    }

    if !same_content && !good && nzb.dupe_mode == DupeMode::Score {
        for i in 0..hlen {
            let hi = host.history_at(dq_ptr, i);
            if (hi.kind == crate::queuesubsys_host::HK_NZB
                || hi.kind == crate::queuesubsys_host::HK_URL)
                && hi.inner_ptr != 0
            {
                let item = host.nzb_snap(hi.inner_ptr, false);
                if item.dupe_mode != DupeMode::Force
                    && same_name_or_key(&item.name, &item.dupe_key, &nzb.name, &nzb.dupe_key)
                    && nzb.dupe_score <= item.dupe_score
                    && item.is_dupe_success
                {
                    host.nzb_set_int(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                        crate::queuesubsys_host::DS_DUPE,
                    );
                    let mut msg = b"Collection ".to_vec();
                    msg.extend_from_slice(&nzb.name);
                    msg.extend_from_slice(b" is a duplicate to ");
                    msg.extend_from_slice(&item.name);
                    host.log(crate::queuesubsys_host::MK_INFO, &msg);
                    return;
                }
            }
        }
    }

    if skip {
        let message = format_dupe_skip_message(&nzb.name, &dupe_name, true, same_content, good);
        if nzb.feed_id != 0 && nzb.dupe_hint == crate::queuesubsys_host::DH_NONE {
            host.nzb_set_int(
                nzb.ptr,
                crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                crate::queuesubsys_host::DS_MANUAL,
            );
            host.log(crate::queuesubsys_host::MK_WARNING, &message);
            host.history_delete_disk_files(nzb.ptr);
        } else {
            let ds = if same_content {
                crate::queuesubsys_host::DS_COPY
            } else {
                crate::queuesubsys_host::DS_GOOD
            };
            host.nzb_set_int(nzb.ptr, crate::queuesubsys_host::NZB_INT_DELETE_STATUS, ds);
            host.nzb_add_message(nzb.ptr, crate::queuesubsys_host::MK_WARNING, &message);
        }

        if return_backup {
            let mut msg = nzb.name.clone();
            msg.extend_from_slice(
                b" is the same posting as a failed download: fetching the best backup instead",
            );
            host.log(crate::queuesubsys_host::MK_INFO, &msg);
            return_best_dupe_host(host, &opts, dq_ptr, nzb.ptr, &nzb.name, &nzb.dupe_key);
        }
        return;
    }

    if nzb.dupe_mode == DupeMode::Score {
        let mut index = 0usize;
        loop {
            let cur_qlen = host.queue_len(dq_ptr);
            if index >= cur_qlen {
                break;
            }
            let qp = host.queue_at(dq_ptr, index);
            if qp == 0 || qp == nzb.ptr {
                index += 1;
                continue;
            }
            let q = host.nzb_snap(qp, false);
            if q.delete_status == crate::queuesubsys_host::DS_NONE
                && (q.kind == crate::queuesubsys_host::NK_NZB
                    || (q.kind == crate::queuesubsys_host::NK_URL
                        && nzb.kind == crate::queuesubsys_host::NK_URL))
                && q.dupe_mode != DupeMode::Force
                && same_name_or_key(&q.name, &q.dupe_key, &nzb.name, &nzb.dupe_key)
            {
                let keep_returned = opts.health_check == crate::queuesubsys_host::HC_DUPE
                    && q.dupe_hint == crate::queuesubsys_host::DH_REDOWNLOAD_AUTO
                    && q.current_success_articles > 0
                    && q.health >= q.critical_health_opt;
                if nzb.dupe_score <= q.dupe_score || keep_returned {
                    host.nzb_set_int(
                        nzb.ptr,
                        crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                        crate::queuesubsys_host::DS_DUPE,
                    );
                    let mut msg = b"Collection ".to_vec();
                    msg.extend_from_slice(&nzb.name);
                    msg.extend_from_slice(b" is a duplicate to ");
                    msg.extend_from_slice(&q.name);
                    host.log(crate::queuesubsys_host::MK_INFO, &msg);
                    return;
                } else if !q.has_post_info {
                    let mut msg = b"Moving collection ".to_vec();
                    msg.extend_from_slice(&q.name);
                    msg.extend_from_slice(b" with lower duplicate score to history");
                    host.log(crate::queuesubsys_host::MK_INFO, &msg);
                    host.nzb_set_int(
                        q.ptr,
                        crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                        crate::queuesubsys_host::DS_DUPE,
                    );
                    let old_size = cur_qlen;
                    host.dq_edit_entry(
                        dq_ptr,
                        q.id,
                        crate::queuesubsys_host::EA_GROUP_DELETE,
                        None,
                    );
                    let new_size = host.queue_len(dq_ptr);
                    if old_size == new_size {
                        index += 1;
                    }
                    continue;
                }
            }
            index += 1;
        }
    }
}

#[cfg(unix)]
pub(crate) fn nzb_completed_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb_ptr: usize,
) {
    if dq_ptr == 0 || nzb_ptr == 0 {
        return;
    }
    let opts = host.opts();
    let nzb = host.nzb_snap(nzb_ptr, false);

    let mut dbg = b"Processing duplicates for ".to_vec();
    dbg.extend_from_slice(&nzb.name);
    host.log(crate::queuesubsys_host::MK_DEBUG, &dbg);

    if fails_over_snap(&opts, &nzb) && !nzb.is_dupe_success {
        return_best_dupe_host(host, &opts, dq_ptr, nzb.ptr, &nzb.name, &nzb.dupe_key);
    }

    let dupe_health = opts.health_check == crate::queuesubsys_host::HC_DUPE
        || opts.health_check == crate::queuesubsys_host::HC_PARK;
    if dupe_health
        && !nzb.dupe_key.is_empty()
        && nzb.delete_status == crate::queuesubsys_host::DS_NONE
        && nzb.is_dupe_success
        && files_on_disk(&nzb.final_dir, &nzb.dest_dir, &opts.ext_cleanup_disk)
    {
        let delete_action = if opts.health_check == crate::queuesubsys_host::HC_PARK {
            crate::queuesubsys_host::EA_GROUP_PARK_DELETE
        } else {
            crate::queuesubsys_host::EA_GROUP_DUPE_DELETE
        };
        let mut others: Vec<(i32, bool, bool)> = Vec::new();
        let qlen = host.queue_len(dq_ptr);
        for i in 0..qlen {
            let qp = host.queue_at(dq_ptr, i);
            if qp == 0 || qp == nzb.ptr {
                continue;
            }
            let queued = host.nzb_snap(qp, false);
            if queued.kind == crate::queuesubsys_host::NK_NZB
                && !queued.deleting
                && queued.delete_status == crate::queuesubsys_host::DS_NONE
                && queued.dupe_mode != DupeMode::Force
                && queued.dupe_key.eq_ignore_ascii_case(&nzb.dupe_key)
            {
                let mut msg = b"Stopping ".to_vec();
                msg.extend_from_slice(&queued.name);
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b" is downloaded and on disk");
                host.nzb_print_message(queued.ptr, crate::queuesubsys_host::MK_INFO, &msg);
                host.nzb_set_int(
                    queued.ptr,
                    crate::queuesubsys_host::NZB_INT_DELETE_STATUS,
                    crate::queuesubsys_host::DS_DUPE,
                );
                // Its own folder only: without InterDir two copies of the same name share one,
                // and the stopped copy's completed files there may be this copy's release.
                let own_folder = queued.dest_dir != nzb.dest_dir
                    && queued.dest_dir != nzb.final_dir;
                others.push((queued.id, queued.has_post_info, own_folder));
            }
        }
        for (other_id, in_post, own_folder) in others {
            if in_post {
                // Its files go when its job ends, as the delete route would remove them (see
                // `prepost::note_stopped_for_on_disk`).
                if own_folder {
                    crate::prepost::note_stopped_for_on_disk(other_id);
                }
                // A post job is cancelled through the post-processor: only the edit-list route
                // hands EA_POST_DELETE to it (a single-entry edit drops it, and the repair of
                // the stopped copy ran on to its end).
                host.dq_edit_list_single_id(
                    dq_ptr,
                    other_id,
                    crate::queuesubsys_host::EA_POST_DELETE,
                    None,
                );
            } else {
                host.dq_edit_entry(dq_ptr, other_id, delete_action, None);
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn history_mark_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    hi_ptr: usize,
    mark_status: MarkStatus,
) {
    if dq_ptr == 0 || hi_ptr == 0 {
        return;
    }
    let opts = host.opts();
    let hi = host.hist_item_snap(hi_ptr);

    let mark_status_name: &[u8] = if mark_status == MarkStatus::Bad {
        b"bad"
    } else if mark_status == MarkStatus::Good {
        b"good"
    } else if mark_status == MarkStatus::Success {
        b"success"
    } else {
        b"NONE"
    };
    let mut msg = b"Marking ".to_vec();
    msg.extend_from_slice(&hi.name);
    msg.extend_from_slice(b" as ");
    msg.extend_from_slice(mark_status_name);
    host.log(crate::queuesubsys_host::MK_INFO, &msg);

    let (dupe_mode, dupe_key, item_name) =
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.inner_ptr != 0 {
            let nzb = host.nzb_snap(hi.inner_ptr, false);
            host.nzb_set_mark_status(hi.inner_ptr, mark_status);
            (nzb.dupe_mode, nzb.dupe_key, nzb.name)
        } else if hi.kind == crate::queuesubsys_host::HK_DUP && hi.inner_ptr != 0 {
            let dup = host.dup_snap(hi.inner_ptr);
            let ds = if mark_status == MarkStatus::Good {
                DupStatus::Good
            } else if mark_status == MarkStatus::Success {
                DupStatus::Success
            } else {
                DupStatus::Bad
            };
            host.dup_set_status(hi.inner_ptr, ds);
            (dup.dupe_mode, dup.dupe_key, dup.name)
        } else {
            let mut err = b"Could not mark ".to_vec();
            err.extend_from_slice(&hi.name);
            err.extend_from_slice(b" as bad: history item has wrong type");
            host.log(crate::queuesubsys_host::MK_ERROR, &err);
            return;
        };

    if !opts.dupe_check || dupe_mode == DupeMode::Force {
        return;
    }

    if mark_status == MarkStatus::Good {
        history_cleanup_host(host, dq_ptr, hi.hi_ptr, &item_name, &dupe_key);
    } else if mark_status == MarkStatus::Bad {
        return_best_dupe_host(host, &opts, dq_ptr, 0, &hi.name, &dupe_key);
    }
}

#[cfg(unix)]
pub(crate) fn get_dupe_status_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    name: &[u8],
    dupe_key: &[u8],
) -> i32 {
    if dq_ptr == 0 {
        return DUPE_STATUS_NONE;
    }
    let mut statuses = DUPE_STATUS_NONE;

    let qlen = host.queue_len(dq_ptr);
    for i in 0..qlen {
        let qp = host.queue_at(dq_ptr, i);
        let nzb = host.nzb_snap(qp, false);
        if same_name_or_key(name, dupe_key, &nzb.name, &nzb.dupe_key) {
            statuses |= if nzb.success_articles + nzb.failed_articles > 0 {
                DUPE_STATUS_DOWNLOADING
            } else {
                DUPE_STATUS_QUEUED
            };
        }
    }

    let hlen = host.history_len(dq_ptr);
    for i in 0..hlen {
        let hi = host.history_at(dq_ptr, i);
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.inner_ptr != 0 {
            let item = host.nzb_snap(hi.inner_ptr, true);
            if same_name_or_key(name, dupe_key, &item.name, &item.dupe_key) {
                statuses |= status_from_text(&item.text_status);
            }
        } else if hi.kind == crate::queuesubsys_host::HK_DUP && hi.inner_ptr != 0 {
            let dup = host.dup_snap(hi.inner_ptr);
            if same_name_or_key(name, dupe_key, &dup.name, &dup.dupe_key) {
                if dup.status == DupStatus::Success || dup.status == DupStatus::Good {
                    statuses |= DUPE_STATUS_SUCCESS;
                } else if dup.status == DupStatus::Failed || dup.status == DupStatus::Bad {
                    statuses |= DUPE_STATUS_FAILURE;
                }
            }
        }
    }

    statuses
}

#[cfg(unix)]
pub(crate) fn list_history_dupes_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb_ptr: usize,
) -> Vec<usize> {
    let mut out = Vec::new();
    if dq_ptr == 0 || nzb_ptr == 0 {
        return out;
    }
    let nzb = host.nzb_snap(nzb_ptr, false);
    if nzb.dupe_mode == DupeMode::Force {
        return out;
    }
    let hlen = host.history_len(dq_ptr);
    for i in 0..hlen {
        let hi = host.history_at(dq_ptr, i);
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.inner_ptr != 0 {
            let item = host.nzb_snap(hi.inner_ptr, false);
            if item.dupe_mode != DupeMode::Force
                && same_name_or_key(&item.name, &item.dupe_key, &nzb.name, &nzb.dupe_key)
            {
                out.push(hi.inner_ptr);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn unknown_dupe_mode_matches_no_mode_check() {
        for raw in [3, -1] {
            let mode = DupeMode::from(raw);
            assert_eq!(i32::from(mode), raw);
            // Not Score, so it never fails over; not Force, so a mark still
            // gets its post action and the reason names DupeMode all.
            assert!(!fails_over(true, mode, HEALTH_CHECK_DUPE));
            assert_eq!(
                resolve_history_mark(MarkStatus::Good, true, HISTORY_KIND_NZB, mode).2,
                1
            );
            assert_eq!(
                no_backup_reason(false, mode, &[], |_| true),
                b"its DupeMode is all".to_vec()
            );
        }
        assert!(fails_over(true, DupeMode::Score, 0));
        assert!(fails_over(true, DupeMode::Force, HEALTH_CHECK_DUPE));
        assert_eq!(
            resolve_history_mark(MarkStatus::Good, true, HISTORY_KIND_NZB, DupeMode::Force).2,
            0
        );
    }

    fn rescue_candidate(
        mode: DupeMode,
        status: i32,
        score: i32,
        alive_permille: i32,
    ) -> NzbgetRsDupeHistoryCandidate {
        NzbgetRsDupeHistoryCandidate {
            kind: HISTORY_KIND_NZB,
            dupe_mode: mode,
            dupe_score: score,
            is_dupe_success: 0,
            mark_status: MarkStatus::None,
            delete_status: status,
            dup_status: DupStatus::Undefined,
            full_content_hash: 0,
            alive_permille,
            health: 1000,
            critical_health: 900,
            same_name_or_key: 1,
            donor_kind: 2,
            locked: 0,
        }
    }

    fn rescue_self() -> NzbgetRsDupeQueueCandidate {
        NzbgetRsDupeQueueCandidate {
            is_self: 1,
            is_nzb_kind: 1,
            dupe_mode: DupeMode::Force,
            dupe_score: 1_000_000,
            same_name_or_key: 1,
        }
    }

    #[test]
    fn rescue_takes_force_backups_and_copies() {
        let history = [
            // the failed pick itself
            rescue_candidate(DupeMode::Force, DELETE_STATUS_HEALTH, 1_000_000, 300),
            rescue_candidate(DupeMode::Force, DELETE_STATUS_DUPE, 999_080, 900),
            rescue_candidate(DupeMode::Score, DELETE_STATUS_COPY, 999_050, 990),
            rescue_candidate(DupeMode::Force, DELETE_STATUS_DUPE, 999_001, 0),
        ];
        let queue = [rescue_self()];
        // failover skips FORCE backups and (without HealthCheck=dupe) copies
        assert_eq!(
            select_dupe_backup(true, 0, false, &history, &queue, |_| true),
            -1
        );
        // the rescue takes the wholest one
        assert_eq!(select_rescue_backup(&history, &queue, |_| true), 2);
        // its nzb-file is gone: the next wholest
        assert_eq!(select_rescue_backup(&history, &queue, |i| i != 2), 1);
        // a dead backup is never returned
        assert_eq!(select_rescue_backup(&history[3..], &queue, |_| true), -1);
    }

    #[test]
    fn rescue_orders_by_alive_then_score() {
        let history = [
            rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_090, -1),
            rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_020, 500),
            rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_030, 500),
        ];
        let queue = [rescue_self()];
        assert_eq!(select_rescue_backup(&history, &queue, |_| true), 2);
        // unmeasured backups only when nothing measured is alive
        assert_eq!(select_rescue_backup(&history[..1], &queue, |_| true), 0);
    }

    #[test]
    fn rescue_orders_twins_then_borrowable_alts() {
        let mut twin = rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_010, 980);
        twin.donor_kind = crate::donorkind::DonorKind::Twin as i32;
        let mut borrowable = rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_090, 990);
        borrowable.donor_kind = crate::donorkind::DonorKind::Borrowable as i32;
        let mut unborrowable = rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_095, 1000);
        unborrowable.donor_kind = crate::donorkind::DonorKind::Unborrowable as i32;
        let queue = [rescue_self()];
        let history = [unborrowable, borrowable, twin];
        assert_eq!(select_rescue_backup(&history, &queue, |_| true), 2);
        assert_eq!(select_rescue_backup(&history[..2], &queue, |_| true), 1);
        // a weak twin (90% alive, under 97.5%) comes after the healthy ALTs
        let mut weak_twin = twin;
        weak_twin.alive_permille = 900;
        let history = [weak_twin, unborrowable, borrowable];
        assert_eq!(select_rescue_backup(&history, &queue, |_| true), 2);
    }

    #[test]
    fn rescue_takes_a_healthy_copy_before_a_weak_cheaper_kind() {
        // as the fleet ranks them: a 13%-alive copy of unknown packaging never
        // goes ahead of a 100%-alive unborrowable one
        let mut weak = rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_090, 130);
        weak.donor_kind = crate::donorkind::DonorKind::Unknown as i32;
        let mut healthy = rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_010, 1000);
        healthy.donor_kind = crate::donorkind::DonorKind::Unborrowable as i32;
        let queue = [rescue_self()];
        assert_eq!(select_rescue_backup(&[weak, healthy], &queue, |_| true), 1);
    }

    #[test]
    fn a_locked_backup_goes_after_every_extractable_one() {
        // live fleet 2026-11-19: a 100%-alive copy with encrypted headers and
        // no password ranked ahead of an 84%-alive plain one
        let mut locked = rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_090, 1000);
        locked.donor_kind = crate::donorkind::DonorKind::Unborrowable as i32;
        locked.locked = 1;
        let mut plain = rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_010, 840);
        plain.donor_kind = crate::donorkind::DonorKind::Borrowable as i32;
        let unmeasured = rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_000, -1);
        let queue = [rescue_self()];
        assert_eq!(select_rescue_backup(&[locked, plain], &queue, |_| true), 1);
        assert_eq!(
            select_rescue_backup(&[locked, unmeasured], &queue, |_| true),
            1
        );
        // nothing else alive: the locked copy is the last resort
        assert_eq!(select_rescue_backup(&[locked], &queue, |_| true), 0);
        // the same password-less copy with its password known ranks by health
        let mut with_password = locked;
        with_password.locked = 0;
        assert_eq!(
            select_rescue_backup(&[with_password, plain], &queue, |_| true),
            0
        );
        // failover too passes over it despite its higher score, and takes it
        // when it is the only backup
        assert_eq!(
            select_dupe_backup(true, 0, true, &[locked, plain], &queue, |_| true),
            1
        );
        assert_eq!(
            select_dupe_backup(true, 0, true, &[plain, locked], &queue, |_| true),
            0
        );
        assert_eq!(
            select_dupe_backup(true, 0, true, &[locked], &queue, |_| true),
            0
        );
    }

    #[test]
    fn rescue_leaves_a_key_that_is_queued_or_done() {
        let backup = rescue_candidate(DupeMode::Force, DELETE_STATUS_DUPE, 999_080, 900);
        let mut queued = rescue_self();
        queued.is_self = 0;
        assert_eq!(
            select_rescue_backup(&[backup], &[rescue_self(), queued], |_| true),
            -1
        );
        let mut done = rescue_candidate(DupeMode::Score, DELETE_STATUS_NONE, 999_000, -1);
        done.is_dupe_success = 1;
        assert_eq!(
            select_rescue_backup(&[backup, done], &[rescue_self()], |_| true),
            -1
        );
        let mut dup = done;
        dup.kind = HISTORY_KIND_DUP;
        dup.is_dupe_success = 0;
        dup.dup_status = DupStatus::Success;
        assert_eq!(
            select_rescue_backup(&[backup, dup], &[rescue_self()], |_| true),
            -1
        );
        // another key's success does not count
        let mut other = done;
        other.same_name_or_key = 0;
        assert_eq!(
            select_rescue_backup(&[backup, other], &[rescue_self()], |_| true),
            0
        );
    }

    #[test]
    fn rescue_skips_bad_or_unhealthy_backups() {
        let mut bad = rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_080, 900);
        bad.mark_status = MarkStatus::Bad;
        let mut sick = rescue_candidate(DupeMode::Score, DELETE_STATUS_DUPE, 999_070, 900);
        sick.health = 800;
        let mut manual = rescue_candidate(DupeMode::Score, DELETE_STATUS_MANUAL, 999_060, 900);
        manual.health = 1000;
        assert_eq!(
            select_rescue_backup(&[bad, sick, manual], &[rescue_self()], |_| true),
            -1
        );
    }

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
                MarkStatus::None,
                DELETE_STATUS_NONE,
                crate::status::NzbParStatus::Skipped,
                crate::status::UnpackStatus::None,
                crate::status::MoveStatus::None,
                crate::downloadinfo::URL_NONE,
                crate::downloadinfo::SCRIPT_STATUS_SUCCESS,
                health,
                crit_strict,
            ),
            b"FAILURE/HEALTH\0"
        );
        assert!(!crate::downloadinfo::is_dupe_success(
            MarkStatus::None,
            DELETE_STATUS_NONE,
            crate::status::NzbParStatus::Skipped,
            crate::status::UnpackStatus::None,
            crate::downloadinfo::URL_NONE,
            health,
            crit_est,
            crit_strict,
            true,
        ));
        // A verified repair (NzbParStatus::Success) or explicit good mark takes precedence
        assert!(crate::downloadinfo::is_dupe_success(
            MarkStatus::None,
            DELETE_STATUS_NONE,
            crate::status::NzbParStatus::Success,
            crate::status::UnpackStatus::None,
            crate::downloadinfo::URL_NONE,
            health,
            crit_est,
            crit_strict,
            true,
        ));
        assert!(crate::downloadinfo::is_dupe_success(
            MarkStatus::Good,
            DELETE_STATUS_NONE,
            crate::status::NzbParStatus::Skipped,
            crate::status::UnpackStatus::None,
            crate::downloadinfo::URL_NONE,
            health,
            crit_est,
            crit_strict,
            true,
        ));
        let full_health = crate::downloadinfo::calc_health(1000, 0, 0, 0);
        assert!(crate::downloadinfo::is_dupe_success(
            MarkStatus::None,
            DELETE_STATUS_NONE,
            crate::status::NzbParStatus::Skipped,
            crate::status::UnpackStatus::None,
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
                dupe_mode: DupeMode::Score,
                dupe_score: score,
                is_dupe_success: 0,
                mark_status: MarkStatus::None,
                delete_status: DELETE_STATUS_DUPE,
                dup_status: DupStatus::Undefined,
                full_content_hash: 0,
                alive_permille: -1,
                health: 1000,
                critical_health: 1000,
                same_name_or_key: i32::from(same_name_or_key(
                    name,
                    key,
                    b"Rel.2026.1080p-A",
                    b"imdb:1",
                )),
                donor_kind: 2,
                locked: 0,
            }
        };

        let mut history: Vec<(NzbgetRsDupeHistoryCandidate, bool)> = Vec::new();
        let mut queue = vec![NzbgetRsDupeQueueCandidate {
            is_self: 1,
            is_nzb_kind: 1,
            dupe_mode: DupeMode::Score,
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
        history[2].0.mark_status = MarkStatus::Bad;
        let winner = run_find(&history, &queue);
        assert!(winner >= 0);
        assert_eq!(history[winner as usize].0.dupe_score, 50);

        // a queued duplicate with a higher score than every backup blocks them
        queue.push(NzbgetRsDupeQueueCandidate {
            is_self: 0,
            is_nzb_kind: 1,
            dupe_mode: DupeMode::Score,
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
        good.mark_status = MarkStatus::Good;
        good.is_dupe_success = 1;
        history.insert(0, (good, true));
        assert_eq!(run_find(&history, &queue), -1);

        // 5. RustDupeCoordinatorFfiParity
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
        assert_eq!(
            crate::downloadinfo::calc_critical_health(1100, 100, 0, true),
            900
        );
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
            Some(DupeMode::Score)
        );
        assert_eq!(
            crate::queueeditor::parse_dupe_mode(b"all"),
            Some(DupeMode::All)
        );
        assert_eq!(
            crate::queueeditor::parse_dupe_mode(b"force"),
            Some(DupeMode::Force)
        );
        assert_eq!(crate::queueeditor::parse_dupe_mode(b"bad"), None);

        // 6. NzbFound / NzbCompleted / HistoryMark evaluators
        assert!(same_content_hash(42, 0, 42, 99));
        assert!(same_content_hash(0, 77, 12, 77));
        assert!(!same_content_hash(0, 0, 0, 0));
        assert!(should_inherit_dupe_props(
            b"Show.A", b"Show.A", b"imdb:1", 0
        ));
        assert!(!should_inherit_dupe_props(b"Show.A", b"Show.A", b"", 0));
        assert_eq!(
            eval_history_dupe_match(
                HISTORY_KIND_NZB,
                100,
                0,
                DupeMode::Score,
                10,
                b"A",
                b"k",
                100,
                0,
                DupeMode::Score,
                MarkStatus::None,
                DupStatus::Undefined,
                10,
                b"B",
                b"k2"
            ),
            Some((true, false))
        );
        assert_eq!(
            eval_queue_score_dupe(
                false,
                DELETE_STATUS_NONE,
                true,
                false,
                DupeMode::Score,
                b"Show.B",
                b"imdb:1",
                b"Show.A",
                b"imdb:1",
                50,
                80,
                HEALTH_CHECK_NONE,
                false,
                0,
                1000,
                900,
                false
            ),
            1
        );
        assert_eq!(
            eval_queue_score_dupe(
                false,
                DELETE_STATUS_NONE,
                true,
                false,
                DupeMode::Score,
                b"Show.B",
                b"imdb:1",
                b"Show.A",
                b"imdb:1",
                90,
                50,
                HEALTH_CHECK_NONE,
                false,
                0,
                1000,
                900,
                false
            ),
            2
        );
        assert_eq!(
            resolve_history_mark(MarkStatus::Good, true, HISTORY_KIND_NZB, DupeMode::Score),
            (&b"good\0"[..], DupStatus::Good, 1)
        );
        assert_eq!(
            resolve_history_mark(MarkStatus::Bad, true, HISTORY_KIND_DUP, DupeMode::Score),
            (&b"bad\0"[..], DupStatus::Bad, 2)
        );
    }
}
