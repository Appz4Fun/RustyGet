//! End-to-end Rust implementation of `DupeCoordinator` (`daemon/queue/DupeCoordinator.cpp`):
//! duplicate name/key matching (`same_name_or_key`), on-disk verification (`files_on_disk`),
//! failover threshold & policy (`dupe_failover_warranted_inline`, `fails_over_snap`, `parse_alive_permille_bytes`),
//! backup candidate selection (`select_dupe_backup`), no-backup diagnostics
//! (`no_backup_reason`, `format_no_backup_reason`), skip message formatting
//! (`format_dupe_skip_message`), and status classification (`status_from_text`).

use crate::queuesubsys_host::NzbRef;
use crate::status::{DeleteStatus, DupStatus, DupeMode, MarkStatus};
use std::collections::BTreeSet;
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
        if h.delete_status != DeleteStatus::Dupe && h.delete_status != DeleteStatus::Copy {
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
/// Returns the winning `0..history.len()` index, or `None` if no backup qualifies.
pub fn select_dupe_backup<F>(
    has_self_nzb: bool,
    self_full_content_hash: u32,
    dupe_health: bool,
    history: &[NzbgetRsDupeHistoryCandidate],
    queue: &[NzbgetRsDupeQueueCandidate],
    mut file_exists_cb: F,
) -> Option<usize>
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
            return None;
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
                && h.delete_status != DeleteStatus::Dupe
                && h.delete_status != DeleteStatus::Copy
                && h.delete_status != DeleteStatus::Manual
            {
                failed_content.insert(h.full_content_hash);
            }
        }
    }

    let mut best_idx: Option<usize> = None;
    for (idx, h) in history.iter().enumerate() {
        let has_candidate = h.kind == HISTORY_KIND_NZB || h.kind == HISTORY_KIND_URL;
        let backup_status = has_candidate
            && (h.delete_status == DeleteStatus::Dupe
                || (dupe_health
                    && h.kind == HISTORY_KIND_NZB
                    && h.delete_status == DeleteStatus::Copy))
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

    best_idx
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
/// as copies, which failover skips. `None` when a download of the key is
/// queued, one already succeeded, or no backup is alive.
pub fn select_rescue_backup<F>(
    history: &[NzbgetRsDupeHistoryCandidate],
    queue: &[NzbgetRsDupeQueueCandidate],
    mut file_exists_cb: F,
) -> Option<usize>
where
    F: FnMut(usize) -> bool,
{
    if queue
        .iter()
        .any(|q| q.is_self == 0 && q.is_nzb_kind != 0 && q.same_name_or_key != 0)
    {
        return None;
    }
    let succeeded = history.iter().any(|h| {
        h.same_name_or_key != 0
            && ((h.kind == HISTORY_KIND_NZB && h.is_dupe_success != 0)
                || (h.kind == HISTORY_KIND_DUP
                    && (h.dup_status == DupStatus::Success || h.dup_status == DupStatus::Good)))
    });
    if succeeded {
        return None;
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
            && (h.delete_status == DeleteStatus::Dupe || h.delete_status == DeleteStatus::Copy)
            && h.alive_permille != 0
            && h.health >= h.critical_health
            && h.mark_status != MarkStatus::Bad
            && best.is_none_or(|(_, b)| rank > b)
            && file_exists_cb(idx)
        {
            best = Some((idx, rank));
        }
    }
    best.map(|(idx, _)| idx)
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
    nzb_ptr: NzbRef,
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
pub(crate) fn locked_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: NzbRef,
) -> bool {
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
        if hi.kind != crate::queuesubsys_host::HK_NZB || hi.nzb().is_none() {
            continue;
        }
        if hi.nzb() == nzb.ptr {
            continue;
        }
        let item = host.nzb_snap(hi.nzb(), false);
        if item.delete_status == DeleteStatus::None
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
            delete_status: DeleteStatus::None,
            dup_status: DupStatus::Undefined,
            full_content_hash: 0,
            alive_permille: 0,
            health: 0,
            critical_health: 0,
            same_name_or_key: 0,
            donor_kind: crate::donorkind::DonorKind::Unknown as i32,
            locked: 0,
        };
        let mut qfn = Vec::new();
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.nzb().is_some() {
            let item = host.nzb_snap(hi.nzb(), false);
            c.dupe_mode = item.dupe_mode;
            c.mark_status = item.mark_status;
            c.delete_status = tried_delete_status(host, &item);
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
    nzb_ptr: NzbRef,
    nzb_name: &[u8],
    dupe_key: &[u8],
) -> DupeCandidates {
    let self_snap = if nzb_ptr.is_some() {
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
            delete_status: DeleteStatus::None,
            dup_status: DupStatus::Undefined,
            full_content_hash: 0,
            alive_permille: 0,
            health: 0,
            critical_health: 0,
            same_name_or_key: 0,
            donor_kind: crate::donorkind::DonorKind::Unknown as i32,
            locked: 0,
        };
        let mut qfn = Vec::new();
        if (hi.kind == crate::queuesubsys_host::HK_NZB
            || hi.kind == crate::queuesubsys_host::HK_URL)
            && hi.inner_ptr != 0
        {
            let item = host.nzb_snap(hi.nzb(), false);
            c.dupe_mode = item.dupe_mode;
            c.dupe_score = item.dupe_score;
            c.is_dupe_success = i32::from(item.is_dupe_success);
            c.mark_status = item.mark_status;
            c.delete_status = tried_delete_status(host, &item);
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
            is_self: i32::from(qp == nzb_ptr && nzb_ptr.is_some()),
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
    nzb_ptr: NzbRef,
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
    if let Some(hi) = winner.and_then(|i| history_entries.get(i)) {
        let hi = hi.clone();
        let nzb_snap = if hi.inner_ptr != 0 {
            host.nzb_snap(hi.nzb(), false)
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
    nzb_ptr: NzbRef,
    nzb_name: &[u8],
    dupe_key: &[u8],
) {
    if let Some((hi, backup_nzb)) =
        find_dupe_backup_host(host, opts, dq_ptr, nzb_ptr, nzb_name, dupe_key)
    {
        // A copy whose archive needs a password nobody has can never be extracted: downloading
        // it after this one failed only fails again.
        if locked_host(host, backup_nzb.ptr) {
            let mut msg = b"Not returning duplicate ".to_vec();
            msg.extend_from_slice(&backup_nzb.name);
            msg.extend_from_slice(b" for ");
            msg.extend_from_slice(nzb_name);
            msg.extend_from_slice(b": its archive needs a password its nzb does not carry");
            host.log(crate::queuesubsys_host::MK_INFO, &msg);
            return;
        }
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
    nzb_ptr: NzbRef,
) {
    if dq_ptr == 0 || nzb_ptr.is_none() {
        return;
    }
    let nzb = host.nzb_snap(nzb_ptr, false);
    if !host.opts().dupe_check
        || nzb.kind != crate::queuesubsys_host::NK_NZB
        || nzb.is_dupe_success
        || nzb.delete_status != DeleteStatus::Health
        || nzb.dupe_key.is_empty()
        || !(crate::options::dupe_watch()
            || crate::options::active_options().is_some_and(|o| o.scalars().dupe_search != 0)
            // a fleet always looks after its copies: one of them must download
            || is_fleet_member(host, &nzb))
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
    let Some((winner, hi)) = winner.and_then(|i| Some((i, history_entries.get(i)?))) else {
        return;
    };
    if hi.inner_ptr == 0 {
        return;
    }
    let backup = host.nzb_snap(hi.nzb(), false);
    if locked_host(host, backup.ptr) {
        return;
    }
    let alive = history_candidates[winner].alive_permille;
    // DupeSearchDryRun only covers what DupeSearch itself would do.
    let dry_run = crate::options::active_options()
        .is_some_and(|o| o.scalars().dupe_search != 0 && o.scalars().dupe_search_dry_run != 0);
    let mut msg = b"Rescue: ".to_vec();
    msg.extend_from_slice(&crate::releasename::log_name(&nzb.name));
    msg.extend_from_slice(b" failed its health check with nothing of its key queued: ");
    msg.extend_from_slice(if dry_run {
        b"dry run, would return backup ".as_slice()
    } else {
        b"returning backup ".as_slice()
    });
    msg.extend_from_slice(&crate::releasename::log_name(&backup.name));
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
            let item = host.nzb_snap(hi.nzb(), false);
            if item.dupe_mode != DupeMode::Force
                && item.delete_status == DeleteStatus::Dupe
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

/// The nzb parameter that keeps a download out of the group of its copies
/// ([`join_copy_group_host`]): `no` from the web UI's Add dialog when its
/// "Add copies of the same release as one fleet" box is off.
pub const JOIN_PARAM: &[u8] = b"DupeJoin";

/// Whether a download may join, or be joined by, the copies of its release.
#[cfg(unix)]
fn joins_copy_group(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    item: &crate::queuesubsys_host::NzbSnap,
) -> bool {
    item.kind == crate::queuesubsys_host::NK_NZB
        && item.dupe_mode != DupeMode::Force
        && !host
            .nzb_get_param(item.ptr, JOIN_PARAM)
            .is_some_and(|v| v.eq_ignore_ascii_case(b"no"))
}

/// The DupeScore the next copy of a group gets: one below the lowest copy
/// (`DupePrimaryScore` for the first), never below 1, so the copy added first
/// stays the one that downloads and the rest keep the order they came in.
pub fn next_group_score(scores: &[i32]) -> i32 {
    match scores.iter().copied().filter(|&s| s > 0).min() {
        Some(lowest) => (lowest - 1).max(1),
        None => crate::dupewatch::primary_score(),
    }
}

/// A download added without a DupeKey joins the copies of its release that
/// are queued or in history without one (or already joined): its name, without
/// the suffix a download manager added (`_5`, ` (1)`, `.nzb.2.queued`, see
/// [`crate::releasename::group_key`]), names the same release. Every one of
/// them gets the DupeKey `dupes:<normalized name>` and a DupeScore in the order
/// they came, so the duplicate check below keeps one downloading and the rest
/// as backups, and TwinCheck labels each backup a twin or an alt.
///
/// Strict, so it never stops or deletes another release: the names must give
/// the same strict group name (resolution, source, edition and group kept,
/// see [`crate::releasename::group_name`]), and a copy joins only copies in
/// the same category and the same DupeMode that carry no DupeKey of their own
/// (a user's or a client's key is never replaced). The queued copies move to
/// the front only when `move_to_top` and a real fleet forms (another copy of
/// it queued).
#[cfg(unix)]
pub(crate) fn join_copy_group_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb: &mut crate::queuesubsys_host::NzbSnap,
    move_to_top: bool,
) {
    if !nzb.dupe_key.is_empty() || !joins_copy_group(host, nzb) {
        return;
    }
    let key = crate::releasename::group_key(&nzb.name);
    if key.is_empty() {
        return;
    }
    // (item, already in the group)
    let mut siblings: Vec<(crate::queuesubsys_host::NzbSnap, bool)> = Vec::new();
    let (category, mode) = (nzb.category.clone(), nzb.dupe_mode);
    let mut consider = |item: crate::queuesubsys_host::NzbSnap| {
        if item.ptr == nzb.ptr
            || !joins_copy_group(host, &item)
            || item.category != category
            || item.dupe_mode != mode
        {
            return;
        }
        if item.dupe_key.eq_ignore_ascii_case(&key) {
            siblings.push((item, true));
        } else if item.dupe_key.is_empty()
            && crate::releasename::group_key(&item.name).eq_ignore_ascii_case(&key)
        {
            siblings.push((item, false));
        }
    };
    for i in 0..host.queue_len(dq_ptr) {
        let qp = host.queue_at(dq_ptr, i);
        if qp.is_some() {
            consider(host.nzb_snap(qp, false));
        }
    }
    for i in 0..host.history_len(dq_ptr) {
        let hi = host.history_at(dq_ptr, i);
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.nzb().is_some() {
            consider(host.nzb_snap(hi.nzb(), false));
        }
    }
    if siblings.is_empty() {
        return;
    }

    let mut scores: Vec<i32> = siblings
        .iter()
        .filter(|(s, _)| s.dupe_score > 0)
        .map(|(s, _)| s.dupe_score)
        .collect();
    let mut history_changed = false;
    for (sib, joined) in &siblings {
        if *joined {
            continue;
        }
        host.nzb_set_str(sib.ptr, crate::queuesubsys_host::NZB_STR_DUPE_KEY, &key);
        if sib.dupe_score == 0 {
            let score = next_group_score(&scores);
            host.nzb_set_int(sib.ptr, crate::queuesubsys_host::NZB_INT_DUPE_SCORE, score);
            scores.push(score);
        }
        history_changed = true;
    }
    nzb.dupe_key = key;
    host.nzb_set_str(nzb.ptr, crate::queuesubsys_host::NZB_STR_DUPE_KEY, &nzb.dupe_key);
    if nzb.dupe_score == 0 {
        nzb.dupe_score = next_group_score(&scores);
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_DUPE_SCORE,
            nzb.dupe_score,
        );
    }
    // a real fleet (another copy of it queued) goes to the front of the queue
    let queued_ids: Vec<i32> = siblings
        .iter()
        .filter(|(sib, _)| {
            !sib.deleting
                && sib.delete_status == DeleteStatus::None
                && (0..host.queue_len(dq_ptr)).any(|i| host.queue_at(dq_ptr, i) == sib.ptr)
        })
        .map(|(sib, _)| sib.id)
        .collect();
    if move_to_top {
        for id in &queued_ids {
            host.dq_edit_entry(dq_ptr, *id, crate::queuesubsys_host::EA_GROUP_MOVE_TOP, None);
        }
    }
    let mut msg = b"Grouping ".to_vec();
    msg.extend_from_slice(&crate::releasename::log_name(&nzb.name));
    msg.extend_from_slice(
        format!(
            " with {} other copy(ies) of the same release: DupeKey ",
            siblings.len()
        )
        .as_bytes(),
    );
    msg.extend_from_slice(&nzb.dupe_key);
    msg.extend_from_slice(format!(", DupeScore {}", nzb.dupe_score).as_bytes());
    host.log(crate::queuesubsys_host::MK_INFO, &msg);
    if history_changed {
        host.dq_history_changed(dq_ptr);
    }
}

/// A group key stored by an earlier build that grouped by the raw name can
/// hold the words of its `{{password}}` (that build read `{ }` as separators:
/// `dupes:movie.2024.grp.s3cret`). Each download whose name holds a password
/// and whose key is such a key gets the key its name gives now
/// ([`crate::releasename::rebuilt_group_key`]), in the queue and in history.
#[cfg(unix)]
fn scrub_password_keys_host(host: &crate::queuesubsys_host::QueueSubsysHost, dq_ptr: usize) {
    let mut items = Vec::new();
    for i in 0..host.queue_len(dq_ptr) {
        let qp = host.queue_at(dq_ptr, i);
        if qp.is_some() {
            items.push(host.nzb_snap(qp, false));
        }
    }
    for i in 0..host.history_len(dq_ptr) {
        let hi = host.history_at(dq_ptr, i);
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.nzb().is_some() {
            items.push(host.nzb_snap(hi.nzb(), false));
        }
    }
    let mut changed = false;
    for item in items {
        if let Some(clean) = crate::releasename::rebuilt_group_key(&item.name, &item.dupe_key) {
            host.nzb_set_str(item.ptr, crate::queuesubsys_host::NZB_STR_DUPE_KEY, &clean);
            changed = true;
        }
    }
    if changed {
        host.dq_history_changed(dq_ptr);
        host.dq_save(dq_ptr);
    }
}

/// Groups the queued downloads without a DupeKey with the copies of their
/// release ([`join_copy_group_host`]): a queue loaded from an earlier run, or
/// filled while the grouping did not exist, gets the keys a download added
/// now would get. Each download is looked at once (`checked` holds the ids
/// seen, and 0 once the keys of an earlier build were scrubbed, see
/// [`scrub_password_keys_host`]); called with the queue loaded. Nothing is
/// moved to history here: the copies already downloading keep downloading,
/// and failover and TwinCheck see them as one fleet from now on.
#[cfg(unix)]
pub(crate) fn join_queued_copy_groups_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    checked: &mut std::collections::BTreeSet<i32>,
) {
    if dq_ptr == 0 || !host.opts().dupe_check {
        return;
    }
    // once, the first time the loaded queue is looked at (empty or not)
    if checked.insert(0) {
        scrub_password_keys_host(host, dq_ptr);
    }
    // by id, from the end: the last copy joins the earlier ones, which are
    // scored in queue order, so the copy queued first keeps the top score; the
    // queue is never reordered after a restart
    let queued: Vec<i32> = (0..host.queue_len(dq_ptr))
        .rev()
        .map(|i| host.queue_at(dq_ptr, i))
        .filter(|p| p.is_some())
        .map(|p| host.nzb_snap(p, false).id)
        .collect();
    // forget the ids no longer queued, so the set stays the queue's size
    let queued_set: std::collections::BTreeSet<i32> = queued.iter().copied().collect();
    checked.retain(|id| *id == 0 || queued_set.contains(id));
    let ids: Vec<i32> = queued.into_iter().filter(|id| !checked.contains(id)).collect();
    checked.extend(ids.iter().copied());
    for id in ids {
        let qp = host.queue_find_by_id(dq_ptr, id);
        if qp.is_none() {
            continue;
        }
        let mut item = host.nzb_snap(qp, false);
        if item.dupe_key.is_empty() && !item.deleting && item.delete_status == DeleteStatus::None {
            join_copy_group_host(host, dq_ptr, &mut item, false);
        }
    }
}

#[cfg(unix)]
pub(crate) fn nzb_found_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb_ptr: NzbRef,
) {
    if dq_ptr == 0 || nzb_ptr.is_none() {
        return;
    }
    let opts = host.opts();
    let mut nzb = host.nzb_snap(nzb_ptr, false);

    let mut dbg = b"Checking duplicates for ".to_vec();
    dbg.extend_from_slice(&nzb.name);
    host.log(crate::queuesubsys_host::MK_DEBUG, &dbg);

    join_copy_group_host(host, dq_ptr, &mut nzb, true);

    if nzb.kind == crate::queuesubsys_host::NK_NZB && nzb.dupe_mode != DupeMode::Force {
        if let Some(downloaded) = downloaded_on_disk_host(host, &opts, dq_ptr, &nzb) {
            let mut msg = b"Not downloading ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(&downloaded.name);
            msg.extend_from_slice(b" is downloaded and on disk");
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
            host.nzb_set_delete_status(nzb.ptr, DeleteStatus::Dupe);
            return;
        }
    }

    let qlen = host.queue_len(dq_ptr);
    for i in 0..qlen {
        let qp = host.queue_at(dq_ptr, i);
        if qp.is_none() || qp == nzb.ptr {
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
                host.nzb_set_delete_status(nzb.ptr, DeleteStatus::Manual);
                host.history_delete_disk_files(nzb.ptr);
            } else {
                host.nzb_set_delete_status(nzb.ptr, DeleteStatus::Copy);
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
            if hi.kind == crate::queuesubsys_host::HK_NZB && hi.nzb().is_some() {
                let hn = host.nzb_snap(hi.nzb(), false);
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
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.nzb().is_some() {
            let item = host.nzb_snap(hi.nzb(), false);
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
            if hi.kind == crate::queuesubsys_host::HK_NZB && hi.nzb().is_some() {
                let item = host.nzb_snap(hi.nzb(), false);
                if (item.full_content_hash > 0 && item.full_content_hash == nzb.full_content_hash)
                    || (item.filtered_content_hash > 0
                        && item.filtered_content_hash == nzb.filtered_content_hash)
                {
                    let ds = tried_delete_status(host, &item);
                    if ds != DeleteStatus::Dupe
                        && ds != DeleteStatus::Copy
                        && ds != DeleteStatus::Manual
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
                if qp.is_none() || qp == nzb.ptr {
                    continue;
                }
                let q = host.nzb_snap(qp, false);
                if q.delete_status == DeleteStatus::None
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
                let item = host.nzb_snap(hi.nzb(), false);
                if item.dupe_mode != DupeMode::Force
                    && same_name_or_key(&item.name, &item.dupe_key, &nzb.name, &nzb.dupe_key)
                    && nzb.dupe_score <= item.dupe_score
                    && item.is_dupe_success
                {
                    host.nzb_set_delete_status(nzb.ptr, DeleteStatus::Dupe);
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
            host.nzb_set_delete_status(nzb.ptr, DeleteStatus::Manual);
            host.log(crate::queuesubsys_host::MK_WARNING, &message);
            host.history_delete_disk_files(nzb.ptr);
        } else {
            let ds = if same_content {
                DeleteStatus::Copy
            } else {
                DeleteStatus::Good
            };
            host.nzb_set_delete_status(nzb.ptr, ds);
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
            if qp.is_none() || qp == nzb.ptr {
                index += 1;
                continue;
            }
            let q = host.nzb_snap(qp, false);
            if q.delete_status == DeleteStatus::None
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
                    host.nzb_set_delete_status(nzb.ptr, DeleteStatus::Dupe);
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
                    host.nzb_set_delete_status(q.ptr, DeleteStatus::Dupe);
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
    nzb_ptr: NzbRef,
) {
    if dq_ptr == 0 || nzb_ptr.is_none() {
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
        && nzb.delete_status == DeleteStatus::None
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
            if qp.is_none() || qp == nzb.ptr {
                continue;
            }
            let queued = host.nzb_snap(qp, false);
            if queued.kind == crate::queuesubsys_host::NK_NZB
                && !queued.deleting
                && queued.delete_status == DeleteStatus::None
                && queued.dupe_mode != DupeMode::Force
                && queued.dupe_key.eq_ignore_ascii_case(&nzb.dupe_key)
            {
                let mut msg = b"Stopping ".to_vec();
                msg.extend_from_slice(&queued.name);
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b" is downloaded and on disk");
                host.nzb_print_message(queued.ptr, crate::queuesubsys_host::MK_INFO, &msg);
                host.nzb_set_delete_status(queued.ptr, DeleteStatus::Dupe);
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

/// The nzb parameter of a copy that failed its health check while other
/// copies of its fleet were left, and was kept as a donor
/// ([`keep_as_donor_host`]): the share of its tried articles that arrived, in
/// percent. Failover never picks such a copy again.
pub const PARKED_PARAM: &[u8] = b"DupeParked";

/// The nzb parameter with the health a copy was judged on when it was parked
/// from a sample (projected over the articles tried), in percent: the
/// download's own `Health` counts every untried article as good.
pub const PROJECTED_PARAM: &[u8] = b"DupeProjected";

/// Adds to a parking message how many other copies of the download's key
/// (or name) are still queued: they are its fleet, and the parked copy stays
/// in it as a donor ([`keep_as_donor_host`]).
#[cfg(unix)]
pub(crate) fn push_queued_copies_note(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb: &crate::queuesubsys_host::NzbSnap,
    msg: &mut Vec<u8>,
) {
    let mut queued = 0usize;
    for i in 0..host.queue_len(dq_ptr) {
        let qp = host.queue_at(dq_ptr, i);
        if qp.is_none() || qp == nzb.ptr {
            continue;
        }
        let q = host.nzb_snap(qp, false);
        if q.kind == crate::queuesubsys_host::NK_NZB
            && !q.deleting
            && q.delete_status == DeleteStatus::None
            && same_name_or_key(&q.name, &q.dupe_key, &nzb.name, &nzb.dupe_key)
        {
            queued += 1;
        }
    }
    if queued > 0 {
        msg.extend_from_slice(
            format!(
                "; {queued} other copy(ies) of it still queued, so it stays in their fleet as a donor"
            )
            .as_bytes(),
        );
    }
}

/// The delete status failover and the duplicate check judge a history item
/// by: a copy kept as a donor after its health check ([`PARKED_PARAM`]) is
/// filed as a backup (DUPE) but was tried and failed, so they see it as
/// parked on health: never a failover target, and its posting counts as failed.
#[cfg(unix)]
pub(crate) fn tried_delete_status(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    item: &crate::queuesubsys_host::NzbSnap,
) -> DeleteStatus {
    if item.delete_status == DeleteStatus::Dupe
        && host.nzb_get_param(item.ptr, PARKED_PARAM).is_some()
    {
        DeleteStatus::Health
    } else {
        item.delete_status
    }
}

/// Whether a download belongs to a fleet: copies grouped by name (a `dupes:`
/// key) or sent together (`appendfleet`, the `DupeFleet` rank). A plain key a
/// user or client set makes no fleet: a failed download of it stays FAILURE.
#[cfg(unix)]
pub(crate) fn is_fleet_member(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> bool {
    !nzb.dupe_key.is_empty()
        && (crate::dupesearch_coord::is_group_key(&nzb.dupe_key)
            || host.nzb_get_param(nzb.ptr, b"DupeFleet").is_some())
}

/// The other copies of a download's fleet that can still complete it: queued
/// ones, and backups in history not yet tried (DUPE, not kept as a parked
/// donor, not found dead). None left means the fleet failed: its last copy
/// stays FAILURE/HEALTH instead of being kept as a donor.
#[cfg(unix)]
pub(crate) fn live_fleet_copies(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> usize {
    let mut live = 0usize;
    for i in 0..host.queue_len(dq_ptr) {
        let qp = host.queue_at(dq_ptr, i);
        if qp.is_none() || qp == nzb.ptr {
            continue;
        }
        let q = host.nzb_snap(qp, false);
        if q.kind == crate::queuesubsys_host::NK_NZB
            && !q.deleting
            && q.delete_status == DeleteStatus::None
            && q.dupe_key.eq_ignore_ascii_case(&nzb.dupe_key)
        {
            live += 1;
        }
    }
    for i in 0..host.history_len(dq_ptr) {
        let hi = host.history_at(dq_ptr, i);
        if hi.kind != crate::queuesubsys_host::HK_NZB || hi.nzb().is_none() || hi.nzb() == nzb.ptr {
            continue;
        }
        let h = host.nzb_snap(hi.nzb(), false);
        if !h.dupe_key.eq_ignore_ascii_case(&nzb.dupe_key) {
            continue;
        }
        if h.is_dupe_success && h.delete_status == DeleteStatus::None {
            return 0; // the fleet is done
        }
        // only a backup failover or the rescue would actually pick: not tried,
        // not dead, not marked bad, healthy enough, its nzb-file still there and
        // not locked
        if tried_delete_status(host, &h) == DeleteStatus::Dupe
            && alive_permille_host(host, h.ptr) != 0
            && h.mark_status != MarkStatus::Bad
            && h.health >= h.critical_health_opt
            && !locked_host(host, h.ptr)
            && !h.queued_filename.is_empty()
            && crate::fsops::file_exists(&h.queued_filename, &crate::fsops::posix_stat)
        {
            live += 1;
        }
    }
    live
}

/// The share of tried articles that arrived, in percent with one decimal.
pub fn arrived_percent(success: i32, failed: i32) -> f64 {
    let tried = i64::from(success.max(0)) + i64::from(failed.max(0));
    if tried == 0 {
        return 0.0;
    }
    ((i64::from(success.max(0)) * 1000 + tried / 2) / tried) as f64 / 10.0
}

/// A copy that failed its health check (parked) while its fleet is still open
/// stays in it as a low-priority donor instead of a failure: its other copies
/// can still borrow the articles it has. It is filed as a backup (DUPE) with
/// its measured health as `DupeAlive` and [`PARKED_PARAM`], keeps its nzb and
/// files, and failover never picks it again. A copy with nothing alive is
/// labelled dead (`DupeAlive` 0). The fleet is open while another copy of its
/// key is queued or waits in history and none succeeded; once one succeeds,
/// the backups go to the old history as usual.
#[cfg(unix)]
pub(crate) fn keep_as_donor_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb_ptr: NzbRef,
) {
    if dq_ptr == 0 || nzb_ptr.is_none() || !host.opts().dupe_check {
        return;
    }
    let nzb = host.nzb_snap(nzb_ptr, false);
    if nzb.kind != crate::queuesubsys_host::NK_NZB
        || nzb.is_dupe_success
        || nzb.delete_status != DeleteStatus::Health
        || !is_fleet_member(host, &nzb)
    {
        return;
    }
    let others = live_fleet_copies(host, dq_ptr, &nzb);
    if others == 0 {
        // the last copy standing: the fleet failed, and shows it
        return;
    }
    let alive = host
        .nzb_get_param(nzb.ptr, PROJECTED_PARAM)
        .and_then(|v| parse_f64_prefix(&v))
        .unwrap_or_else(|| {
            arrived_percent(nzb.current_success_articles, nzb.current_failed_articles)
        })
        .clamp(0.0, 100.0);
    // "0" as the fleet writes a dead copy, so every reader of DupeAlive agrees
    let alive_text = if alive <= 0.0 {
        "0".to_string()
    } else {
        format!("{alive:.1}")
    };
    host.nzb_set_delete_status(nzb.ptr, DeleteStatus::Dupe);
    host.nzb_set_param(nzb.ptr, PARKED_PARAM, Some(alive_text.as_bytes()));
    host.nzb_set_param(nzb.ptr, b"DupeAlive", Some(alive_text.as_bytes()));
    let mut msg = b"Keeping ".to_vec();
    msg.extend_from_slice(&crate::releasename::log_name(&nzb.name));
    if alive > 0.0 {
        msg.extend_from_slice(
            format!(
                " as a donor of its fleet ({others} other copy(ies)): {alive_text}% of its tried articles arrived"
            )
            .as_bytes(),
        );
    } else {
        msg.extend_from_slice(
            format!(
                " in its fleet ({others} other copy(ies)) as dead: none of its tried articles arrived; its nzb stays"
            )
            .as_bytes(),
        );
    }
    host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
    host.dq_history_changed(dq_ptr);
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
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.nzb().is_some() {
            let nzb = host.nzb_snap(hi.nzb(), false);
            host.nzb_set_mark_status(hi.nzb(), mark_status);
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
            host.dup_edit(hi.inner_ptr, crate::qdup::DupEdit::Status(ds));
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
        return_best_dupe_host(host, &opts, dq_ptr, NzbRef::NONE, &hi.name, &dupe_key);
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
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.nzb().is_some() {
            let item = host.nzb_snap(hi.nzb(), true);
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
    nzb_ptr: NzbRef,
) -> Vec<NzbRef> {
    let mut out = Vec::new();
    if dq_ptr == 0 || nzb_ptr.is_none() {
        return out;
    }
    let nzb = host.nzb_snap(nzb_ptr, false);
    if nzb.dupe_mode == DupeMode::Force {
        return out;
    }
    let hlen = host.history_len(dq_ptr);
    for i in 0..hlen {
        let hi = host.history_at(dq_ptr, i);
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.nzb().is_some() {
            let item = host.nzb_snap(hi.nzb(), false);
            if item.dupe_mode != DupeMode::Force
                && same_name_or_key(&item.name, &item.dupe_key, &nzb.name, &nzb.dupe_key)
            {
                out.push(hi.nzb());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_scores_follow_the_order_copies_came() {
        assert_eq!(next_group_score(&[]), crate::dupewatch::primary_score());
        assert_eq!(next_group_score(&[0, 0]), crate::dupewatch::primary_score());
        assert_eq!(next_group_score(&[100]), 99);
        assert_eq!(next_group_score(&[100, 99, 98]), 97);
        assert_eq!(next_group_score(&[1]), 1);
        assert!((arrived_percent(19, 21) - 47.5).abs() < 1e-9);
        assert_eq!(arrived_percent(0, 200), 0.0);
        assert_eq!(arrived_percent(0, 0), 0.0);
    }

    #[test]
    fn unknown_dupe_mode_matches_no_mode_check() {
        for raw in [3, -1] {
            let mode = DupeMode::from(raw);
            assert_eq!(i32::from(mode), raw);
            // Not Score, so the reason names DupeMode all.
            assert_eq!(
                no_backup_reason(false, mode, &[], |_| true),
                b"its DupeMode is all".to_vec()
            );
        }
    }

    fn rescue_candidate(
        mode: DupeMode,
        status: DeleteStatus,
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
            donor_kind: crate::donorkind::DonorKind::Unknown as i32,
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
            rescue_candidate(DupeMode::Force, DeleteStatus::Health, 1_000_000, 300),
            rescue_candidate(DupeMode::Force, DeleteStatus::Dupe, 999_080, 900),
            rescue_candidate(DupeMode::Score, DeleteStatus::Copy, 999_050, 990),
            rescue_candidate(DupeMode::Force, DeleteStatus::Dupe, 999_001, 0),
        ];
        let queue = [rescue_self()];
        // failover skips FORCE backups and (without HealthCheck=dupe) copies
        assert_eq!(
            select_dupe_backup(true, 0, false, &history, &queue, |_| true), None
        );
        // the rescue takes the wholest one
        assert_eq!(select_rescue_backup(&history, &queue, |_| true), Some(2));
        // its nzb-file is gone: the next wholest
        assert_eq!(select_rescue_backup(&history, &queue, |i| i != 2), Some(1));
        // a dead backup is never returned
        assert_eq!(select_rescue_backup(&history[3..], &queue, |_| true), None);
    }

    #[test]
    fn rescue_orders_by_alive_then_score() {
        let history = [
            rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_090, -1),
            rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_020, 500),
            rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_030, 500),
        ];
        let queue = [rescue_self()];
        assert_eq!(select_rescue_backup(&history, &queue, |_| true), Some(2));
        // unmeasured backups only when nothing measured is alive
        assert_eq!(select_rescue_backup(&history[..1], &queue, |_| true), Some(0));
    }

    #[test]
    fn rescue_orders_twins_then_borrowable_alts() {
        let mut twin = rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_010, 980);
        twin.donor_kind = crate::donorkind::DonorKind::Twin as i32;
        let mut borrowable = rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_090, 990);
        borrowable.donor_kind = crate::donorkind::DonorKind::Borrowable as i32;
        let mut unborrowable = rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_095, 1000);
        unborrowable.donor_kind = crate::donorkind::DonorKind::Unborrowable as i32;
        let queue = [rescue_self()];
        let history = [unborrowable, borrowable, twin];
        assert_eq!(select_rescue_backup(&history, &queue, |_| true), Some(2));
        assert_eq!(select_rescue_backup(&history[..2], &queue, |_| true), Some(1));
        // a weak twin (90% alive, under 97.5%) comes after the healthy ALTs
        let mut weak_twin = twin;
        weak_twin.alive_permille = 900;
        let history = [weak_twin, unborrowable, borrowable];
        assert_eq!(select_rescue_backup(&history, &queue, |_| true), Some(2));
    }

    #[test]
    fn rescue_takes_a_healthy_copy_before_a_weak_cheaper_kind() {
        // as the fleet ranks them: a 13%-alive copy of unknown packaging never
        // goes ahead of a 100%-alive unborrowable one
        let mut weak = rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_090, 130);
        weak.donor_kind = crate::donorkind::DonorKind::Unknown as i32;
        let mut healthy = rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_010, 1000);
        healthy.donor_kind = crate::donorkind::DonorKind::Unborrowable as i32;
        let queue = [rescue_self()];
        assert_eq!(select_rescue_backup(&[weak, healthy], &queue, |_| true), Some(1));
    }

    #[test]
    fn a_locked_backup_goes_after_every_extractable_one() {
        // live fleet 2026-11-19: a 100%-alive copy with encrypted headers and
        // no password ranked ahead of an 84%-alive plain one
        let mut locked = rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_090, 1000);
        locked.donor_kind = crate::donorkind::DonorKind::Unborrowable as i32;
        locked.locked = 1;
        let mut plain = rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_010, 840);
        plain.donor_kind = crate::donorkind::DonorKind::Borrowable as i32;
        let unmeasured = rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_000, -1);
        let queue = [rescue_self()];
        assert_eq!(select_rescue_backup(&[locked, plain], &queue, |_| true), Some(1));
        assert_eq!(
            select_rescue_backup(&[locked, unmeasured], &queue, |_| true), Some(1)
        );
        // nothing else alive: the locked copy is the last resort
        assert_eq!(select_rescue_backup(&[locked], &queue, |_| true), Some(0));
        // the same password-less copy with its password known ranks by health
        let mut with_password = locked;
        with_password.locked = 0;
        assert_eq!(
            select_rescue_backup(&[with_password, plain], &queue, |_| true), Some(0)
        );
        // failover too passes over it despite its higher score, and takes it
        // when it is the only backup
        assert_eq!(
            select_dupe_backup(true, 0, true, &[locked, plain], &queue, |_| true), Some(1)
        );
        assert_eq!(
            select_dupe_backup(true, 0, true, &[plain, locked], &queue, |_| true), Some(0)
        );
        assert_eq!(
            select_dupe_backup(true, 0, true, &[locked], &queue, |_| true), Some(0)
        );
    }

    #[test]
    fn rescue_leaves_a_key_that_is_queued_or_done() {
        let backup = rescue_candidate(DupeMode::Force, DeleteStatus::Dupe, 999_080, 900);
        let mut queued = rescue_self();
        queued.is_self = 0;
        assert_eq!(
            select_rescue_backup(&[backup], &[rescue_self(), queued], |_| true), None
        );
        let mut done = rescue_candidate(DupeMode::Score, DeleteStatus::None, 999_000, -1);
        done.is_dupe_success = 1;
        assert_eq!(
            select_rescue_backup(&[backup, done], &[rescue_self()], |_| true), None
        );
        let mut dup = done;
        dup.kind = HISTORY_KIND_DUP;
        dup.is_dupe_success = 0;
        dup.dup_status = DupStatus::Success;
        assert_eq!(
            select_rescue_backup(&[backup, dup], &[rescue_self()], |_| true), None
        );
        // another key's success does not count
        let mut other = done;
        other.same_name_or_key = 0;
        assert_eq!(
            select_rescue_backup(&[backup, other], &[rescue_self()], |_| true), Some(0)
        );
    }

    #[test]
    fn rescue_skips_bad_or_unhealthy_backups() {
        let mut bad = rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_080, 900);
        bad.mark_status = MarkStatus::Bad;
        let mut sick = rescue_candidate(DupeMode::Score, DeleteStatus::Dupe, 999_070, 900);
        sick.health = 800;
        let mut manual = rescue_candidate(DupeMode::Score, DeleteStatus::Manual, 999_060, 900);
        manual.health = 1000;
        assert_eq!(
            select_rescue_backup(&[bad, sick, manual], &[rescue_self()], |_| true), None
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
        assert_eq!(status_from_text(b"SUCCESS/ALL"), DUPE_STATUS_SUCCESS);
        assert_eq!(
            format_dupe_skip_message(b"Show.A", b"Show.A", false, true, false),
            b"Skipping duplicate Show.A, already queued"
        );
    }

    #[test]
    fn dupe_coordinator_full_suite_port_from_cpp() {

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
                DeleteStatus::None,
                crate::status::NzbParStatus::Skipped,
                crate::status::UnpackStatus::None,
                crate::status::MoveStatus::None,
                crate::status::UrlStatus::None,
                crate::status::ScriptStatus::Success,
                health,
                crit_strict,
            ),
            b"FAILURE/HEALTH\0"
        );
        assert!(!crate::downloadinfo::is_dupe_success(
            MarkStatus::None,
            DeleteStatus::None,
            crate::status::NzbParStatus::Skipped,
            crate::status::UnpackStatus::None,
            crate::status::UrlStatus::None,
            health,
            crit_est,
            crit_strict,
            true,
        ));
        // A verified repair (NzbParStatus::Success) or explicit good mark takes precedence
        assert!(crate::downloadinfo::is_dupe_success(
            MarkStatus::None,
            DeleteStatus::None,
            crate::status::NzbParStatus::Success,
            crate::status::UnpackStatus::None,
            crate::status::UrlStatus::None,
            health,
            crit_est,
            crit_strict,
            true,
        ));
        assert!(crate::downloadinfo::is_dupe_success(
            MarkStatus::Good,
            DeleteStatus::None,
            crate::status::NzbParStatus::Skipped,
            crate::status::UnpackStatus::None,
            crate::status::UrlStatus::None,
            health,
            crit_est,
            crit_strict,
            true,
        ));
        let full_health = crate::downloadinfo::calc_health(1000, 0, 0, 0);
        assert!(crate::downloadinfo::is_dupe_success(
            MarkStatus::None,
            DeleteStatus::None,
            crate::status::NzbParStatus::Skipped,
            crate::status::UnpackStatus::None,
            crate::status::UrlStatus::None,
            full_health,
            crit_est,
            crit_strict,
            true,
        ));

        // 3. DupeAlivePermilleTest
        let alive = |s: &str| parse_alive_permille_bytes(s.as_bytes());
        assert_eq!(alive("100"), 1000);
        assert_eq!(alive("99.6"), 996);
        assert_eq!(alive("0"), 0);
        assert_eq!(alive(""), -1);
        assert_eq!(alive("abc"), -1);
        assert_eq!(alive("-5"), 0);
        assert_eq!(alive("1000"), 1000);
        assert_eq!(alive("nan"), -1);

        // 4. DupeCoordinatorFindDupeBackupTest
        let make_backup = |name: &[u8], key: &[u8], score: i32| -> NzbgetRsDupeHistoryCandidate {
            NzbgetRsDupeHistoryCandidate {
                kind: HISTORY_KIND_NZB,
                dupe_mode: DupeMode::Score,
                dupe_score: score,
                is_dupe_success: 0,
                mark_status: MarkStatus::None,
                delete_status: DeleteStatus::Dupe,
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
                donor_kind: crate::donorkind::DonorKind::Unknown as i32,
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
         -> Option<usize> {
            let candidates: Vec<NzbgetRsDupeHistoryCandidate> =
                hist.iter().map(|(c, _)| *c).collect();
            select_dupe_backup(true, 0, false, &candidates, q, |idx| hist[idx].1)
        };

        // nothing in history
        assert_eq!(run_find(&history, &queue), None);

        // the highest-scored healthy backup of the same key wins; other keys are ignored
        history.insert(0, (make_backup(b"Rel.2026.1080p-B", b"imdb:1", 50), true));
        history.insert(0, (make_backup(b"Rel.2026.1080p-C", b"imdb:1", 90), true));
        history.insert(0, (make_backup(b"Other.2026-X", b"imdb:2", 100), true));
        let winner = run_find(&history, &queue).expect("a backup qualifies");
        assert_eq!(history[winner].0.dupe_score, 90);

        // a backup whose source nzb-file is gone can't be downloaded again
        history.insert(0, (make_backup(b"Rel.2026.1080p-Z", b"imdb:1", 95), false));
        let winner = run_find(&history, &queue).expect("a backup qualifies");
        assert_eq!(history[winner].0.dupe_score, 90);
        history[0].0.same_name_or_key = 0;

        // a backup marked bad is no candidate
        history[2].0.mark_status = MarkStatus::Bad;
        let winner = run_find(&history, &queue).expect("a backup qualifies");
        assert_eq!(history[winner].0.dupe_score, 50);

        // a queued duplicate with a higher score than every backup blocks them
        queue.push(NzbgetRsDupeQueueCandidate {
            is_self: 0,
            is_nzb_kind: 1,
            dupe_mode: DupeMode::Score,
            dupe_score: 60,
            same_name_or_key: 1,
        });
        assert_eq!(run_find(&history, &queue), None);

        // the item itself never counts as its own rival
        queue[0].dupe_score = 99;
        queue.pop();
        let winner = run_find(&history, &queue).expect("a backup qualifies");
        assert_eq!(history[winner].0.dupe_score, 50);

        // a good-duplicate in history ends the search
        let mut good = make_backup(b"Rel.2026.1080p-G", b"imdb:1", 1);
        good.delete_status = DeleteStatus::None;
        good.mark_status = MarkStatus::Good;
        good.is_dupe_success = 1;
        history.insert(0, (good, true));
        assert_eq!(run_find(&history, &queue), None);

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
        assert_eq!(alive("96.5"), 965);
        assert_eq!(alive("invalid"), -1);
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
    }
}
