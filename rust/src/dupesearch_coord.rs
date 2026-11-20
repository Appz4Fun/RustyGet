//! Native Rust coordinator for `DupeSearch`, `Fleet`, `HttpGet`, `Newznab::Search`,
//! and `NntpHealthServer`, replacing `DonorHealth`, `ReleaseName`, `DupeUtil`,
//! `NzbSummary`, `NzbReader`, `HttpGet`, `Newznab`, `Posting`, `NntpHealthServer`,
//! `DeadPostings`, `NzbFetcher`, `DonorScore`, `DupeIdentity`, `DupeSearch`, and
//! `Fleet` in `daemon/util/Util.h`.

use crate::queuesubsys_host::NzbRef;
use crate::cstr;
use crate::options::OptStr;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::deadpostings::DeadPostingsStore;
use crate::dupesearch_nzb::{
    build_newznab_queries, build_newznab_url_percent, check_pending_identity, donor_check_posting,
    donor_check_postings, donor_dead_probe, donor_now_ms, donor_reset, donor_stop_all,
    donor_stopping, donor_wait_all, dupe_bytes_to_path, dupe_identity_read, dupe_read_all_bytes,
    dupe_util_lower, dupe_write_atomic_bytes, dupesearch_calc_expand_budget_ms,
    dupesearch_classify_search_retry_reason, dupesearch_eval_post_place_retry,
    dupesearch_format_pending_dir, dupesearch_format_rejected_outcome,
    dupesearch_is_better_parent_candidate, dupesearch_is_rank_only_queued_member,
    dupesearch_parse_pending_dir_id, eval_fleet_member_health, eval_fleet_outcome_reason,
    eval_member_rerank, eval_pick_gone, fleet_calc_member_add_score, fleet_clamp_timeout_sec,
    fleet_ensure_nzb_ext, fleet_format_fetch_error, fleet_format_summary_log,
    fleet_member_skip_reason, fleet_update_key_score_bounds, fleet_visit_pipe_names,
    http_get_format_temp_path, http_get_read_and_delete, lower_key, mark_searching_pending,
    mask_newznab_text, newznab_format_query_label, newznab_format_search_info_name,
    parse_atoi_bytes, parse_dupesearch_state_line, parse_newznab_response, parse_nzb_summary,
    parse_pending_meta, placement_available, rank_fleet_candidates, save_pending_finish,
    save_pending_header, save_pending_item, saved_history_backup, sort_verified_postings,
    DonorAnswer, DonorExchangeError, DonorHealthResult, DonorOptions, DonorPosting, DonorRequest,
    DonorSampleOptions, DonorServer, DonorServerExchange, FetchedArticleResult, NewznabResult,
    NzbFetchReason, NzbFetched, NzbFetcherCore, NzbFetcherStats, NzbSummaryData, DONOR_MIN_KNOWN,
};
use crate::dupewatch::{
    build_item_flags, dead_donor, decide, effective_alive, format_searched_state_line, higher_peer,
    is_live_fleet_peer, par_doomed, unlisted_missing, whole_bytes, Decision, FetchBudget,
    SearchStamp, Snapshot, BASE_PICK_SCORE, MANAGED_FLEET,
};
use crate::status::{DeleteStatus, DupeMode, MarkStatus};
use crate::xmlrpc_format::{
    NzbgetRsRpcFleetMember, NzbgetRsRpcFleetReqMember, RpcEmit,
};

pub const POST_PROCESS_RETRY_SEC: i64 = 30;
pub const RECENT_FAILURE_SEC: i64 = 3600;
pub const STATE_TTL_SEC: i64 = 30 * 24 * 3600;
pub const SEARCH_DEADLINE_SEC: i64 = 60;
pub const FETCH_PARALLEL: usize = 4;

pub const DONOR_PARAM: &[u8] = b"DupeSearch";
pub const ALIVE_PARAM: &[u8] = b"DupeAlive";
const ALIVE_PARAM_STR: &str = "DupeAlive";
pub const OWNED_KEY_PARAM: &[u8] = b"DupeSearchOwnedKey";
pub const FLEET_PARAM: &[u8] = b"DupeFleet";
/// Fleet intake verdict on a copy's video: `same` when its sampled payload
/// matches the fleet's, `unverified` when it could not be read or compared,
/// `different` when a later re-check found another video in a copy it could
/// not drop. A copy found carrying another video at intake is dropped.
pub const VIDEO_PARAM: &[u8] = b"DupeVideo";
const VIDEO_DIFFERENT: &[u8] = b"different";

fn video_param_value(same: bool) -> &'static [u8] {
    if same { b"same" } else { b"unverified" }
}

/// The kind stored on each copy of the key already queued (`id` 0 or a copy
/// not found gives `None`): only a copy that is queued, or parked in history
/// as a backup, and whose health does not say it is dead (`DupeAlive=0`).
fn fleet_queued_kinds(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    ids: &[i32],
) -> Vec<Option<crate::donorkind::DonorKind>> {
    if ids.iter().all(|&id| id == 0) {
        return vec![None; ids.len()];
    }
    with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
        let Some(qh) = crate::ffi::queue_host() else {
            return vec![None; ids.len()];
        };
        ids.iter()
            .map(|&id| {
                if id == 0 {
                    return None;
                }
                let (ptr_, in_history) = dq_find_nzb(host_ctx, host_cb, dq_ptr, id)?;
                if in_history {
                    let item = inspect_nzb(host_ctx, host_cb, ptr_, false);
                    if item.delete_status != DeleteStatus::Dupe {
                        return None;
                    }
                }
                if qh
                    .nzb_get_param(ptr_, ALIVE_PARAM)
                    .is_some_and(|v| v.trim_ascii() == b"0")
                {
                    return None;
                }
                Some(crate::donorkind::DonorKind::from_param(
                    qh.nzb_get_param(ptr_, crate::donorkind::KIND_PARAM)
                        .as_deref(),
                ))
            })
            .collect()
    })
}

/// Records the kind a probe found for a copy queued from an earlier
/// submission. It only ever gets better informed: `unknown` writes nothing,
/// and a kind already stored is replaced only by `twin` (a twin turned up), or
/// when the stored one is `unknown`.
fn fleet_upgrade_kind(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    nzb_id: i32,
    kind: crate::donorkind::DonorKind,
) {
    use crate::donorkind::{DonorKind, KIND_PARAM};
    if kind == DonorKind::Unknown {
        return;
    }
    with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
        let Some(qh) = crate::ffi::queue_host() else {
            return;
        };
        let Some((nzb_ptr, in_history)) = dq_find_nzb(host_ctx, host_cb, dq_ptr, nzb_id) else {
            return;
        };
        // it may have failed or died while the fleet was checked
        if in_history
            && inspect_nzb(host_ctx, host_cb, nzb_ptr, false).delete_status != DeleteStatus::Dupe
        {
            return;
        }
        if qh
            .nzb_get_param(nzb_ptr, ALIVE_PARAM)
            .is_some_and(|v| v.trim_ascii() == b"0")
        {
            return;
        }
        let stored = DonorKind::from_param(qh.nzb_get_param(nzb_ptr, KIND_PARAM).as_deref());
        if !kind_upgrades(stored, kind) {
            return;
        }
        qh.nzb_set_param(nzb_ptr, KIND_PARAM, Some(kind.param()));
        qh.nzb_set_int(nzb_ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
        if in_history {
            qh.dq_history_changed(dq_ptr);
            qh.dq_save(dq_ptr);
        } else {
            qh.dq_save_changed(dq_ptr);
        }
    });
}

/// Whether a probed kind replaces the stored one (see [`fleet_upgrade_kind`]).
/// An encrypted copy stored unborrowable (read before its password was at
/// hand, or by a build that did not try it) moves up once its password opens
/// it.
fn kind_upgrades(stored: crate::donorkind::DonorKind, found: crate::donorkind::DonorKind) -> bool {
    use crate::donorkind::DonorKind;
    found != DonorKind::Unknown
        && found != stored
        && (stored == DonorKind::Unknown
            || found == DonorKind::Twin
            || (stored == DonorKind::Unborrowable && found == DonorKind::EncryptedBorrowable))
}

/// Records on a copy already queued, or parked in history as a backup, that its
/// archive headers are encrypted ([`crate::donorkind::LOCKED_PARAM`]), so the
/// failover and rescue picks rank it last while no password is at hand.
fn fleet_mark_encrypted(host_ctx: *mut c_void, host_cb: DupeHostFn, nzb_id: i32) {
    use crate::donorkind::LOCKED_PARAM;
    if nzb_id <= 0 {
        return;
    }
    with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
        let Some(qh) = crate::ffi::queue_host() else {
            return;
        };
        let Some((nzb_ptr, in_history)) = dq_find_nzb(host_ctx, host_cb, dq_ptr, nzb_id) else {
            return;
        };
        if in_history
            && inspect_nzb(host_ctx, host_cb, nzb_ptr, false).delete_status != DeleteStatus::Dupe
        {
            return;
        }
        if crate::donorkind::encrypted_param(qh.nzb_get_param(nzb_ptr, LOCKED_PARAM).as_deref()) {
            return;
        }
        qh.nzb_set_param(nzb_ptr, LOCKED_PARAM, Some(b"yes"));
        qh.nzb_set_int(nzb_ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
        if in_history {
            qh.dq_history_changed(dq_ptr);
            qh.dq_save(dq_ptr);
        } else {
            qh.dq_save_changed(dq_ptr);
        }
    });
}

/// For each copy already queued or in history (`id` 0 or not found: `None`):
/// whether its headers are stored as encrypted, and whether it has a password
/// now (`*Unpack:Password`, which the user may have set since).
fn fleet_stored_locks(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    ids: &[i32],
) -> Vec<Option<(bool, bool)>> {
    if ids.iter().all(|&id| id == 0) {
        return vec![None; ids.len()];
    }
    with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
        let Some(qh) = crate::ffi::queue_host() else {
            return vec![None; ids.len()];
        };
        ids.iter()
            .map(|&id| {
                if id == 0 {
                    return None;
                }
                let (ptr_, _) = dq_find_nzb(host_ctx, host_cb, dq_ptr, id)?;
                Some((
                    crate::donorkind::encrypted_param(
                        qh.nzb_get_param(ptr_, crate::donorkind::LOCKED_PARAM)
                            .as_deref(),
                    ),
                    qh.nzb_get_param(ptr_, crate::donorkind::PASSWORD_PARAM)
                        .is_some_and(|p| !p.trim_ascii().is_empty()),
                ))
            })
            .collect()
    })
}

/// The archive password each queued or parked copy has now
/// (`*Unpack:Password`: from its nzb, its name, or set by the user since).
fn fleet_stored_passwords(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    ids: &[i32],
) -> Vec<Option<Vec<u8>>> {
    if ids.is_empty() {
        return Vec::new();
    }
    with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
        let Some(qh) = crate::ffi::queue_host() else {
            return vec![None; ids.len()];
        };
        ids.iter()
            .map(|&id| {
                let (ptr_, _) = dq_find_nzb(host_ctx, host_cb, dq_ptr, id)?;
                qh.nzb_get_param(ptr_, crate::donorkind::PASSWORD_PARAM)
                    .filter(|p| !p.trim_ascii().is_empty())
            })
            .collect()
    })
}

/// A `{{password}}` in a fleet member's name is its password, as the scanner
/// reads it into `*Unpack:Password` when the copy is added; a password in the
/// nzb's meta wins. Kept in the summary's meta so the probe, the intake and
/// the ranking all see it.
fn summary_take_name_password(info: &mut NzbSummaryData, name: &[u8]) {
    if summary_password_known(info) {
        return;
    }
    if let Some(pw) = crate::nzbfile::extract_password_from_filename(name) {
        if !pw.trim_ascii().is_empty() {
            info.meta.insert(b"password".to_vec(), pw.to_vec());
        }
    }
}

/// Records the intake verdict on a copy already in the queue or history.
fn fleet_mark_video(host_ctx: *mut c_void, host_cb: DupeHostFn, nzb_id: i32, same: bool) {
    fleet_set_video(host_ctx, host_cb, nzb_id, video_param_value(same));
}

/// Writes a video verdict. `unverified` never replaces an earlier verdict;
/// `same` and `different` are reads that succeeded, so they always win.
fn fleet_set_video(host_ctx: *mut c_void, host_cb: DupeHostFn, nzb_id: i32, value: &[u8]) {
    let decisive = value != b"unverified";
    with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
        let Some(qh) = crate::ffi::queue_host() else {
            return;
        };
        let Some((nzb_ptr, in_history)) = dq_find_nzb(host_ctx, host_cb, dq_ptr, nzb_id) else {
            return;
        };
        // a read that failed this time never takes back an earlier match
        if !decisive && qh.nzb_get_param(nzb_ptr, VIDEO_PARAM).is_some() {
            return;
        }
        qh.nzb_set_param(nzb_ptr, VIDEO_PARAM, Some(value));
        qh.nzb_set_int(nzb_ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
        if in_history {
            qh.dq_history_changed(dq_ptr);
            qh.dq_save(dq_ptr);
        } else {
            qh.dq_save_changed(dq_ptr);
        }
    });
}

use crate::log::{MK_INFO, MK_WARNING, MK_DEBUG, MK_DETAIL};

pub type DupeHostFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
    bytes: *const u8,
    len: usize,
) -> usize;

#[inline]
fn c_bytes(s: &[u8]) -> Vec<u8> {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    let mut out = Vec::with_capacity(end + 1);
    out.extend_from_slice(&s[..end]);
    out.push(0);
    out
}

#[inline]
fn push_c_slice(out: &mut Vec<u8>, s: &[u8]) {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    out.extend_from_slice(&s[..end]);
}

#[inline]
fn truncate_note(mut msg: Vec<u8>) -> Vec<u8> {
    if msg.len() > 1023 {
        msg.truncate(1023);
    }
    c_bytes(&msg)
}

#[inline]
fn path_join(dir: &[u8], sep: u8, leaf: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dir.len() + 1 + leaf.len());
    out.extend_from_slice(dir);
    out.push(sep);
    out.extend_from_slice(leaf);
    out
}

#[inline]
fn base_file_name(path: &[u8]) -> Vec<u8> {
    match path.iter().rposition(|&b| b == b'/' || b == b'\\') {
        Some(pos) => path[pos + 1..].to_vec(),
        None => path.to_vec(),
    }
}

#[inline]
fn make_dupe_key(name: &[u8]) -> Vec<u8> {
    let norm = crate::releasename::normalize(name);
    let mut out = Vec::with_capacity(6 + norm.len());
    out.extend_from_slice(b"dupes:");
    out.extend_from_slice(&norm);
    out
}

#[inline]
fn effective_key(dupe_key: &[u8], name: &[u8]) -> Vec<u8> {
    if !dupe_key.is_empty() {
        dupe_key.to_vec()
    } else {
        make_dupe_key(name)
    }
}

#[inline]
fn summary_whole_bytes_i64(info: &NzbSummaryData) -> i64 {
    let wb = if info.total_bytes <= 0 {
        0
    } else {
        whole_bytes(
            info.total_bytes as u64,
            info.listed_parts,
            info.declared_parts,
        )
    };
    wb.min(i64::MAX as u64) as i64
}

#[inline]
fn summary_ids_strings(ids: &[Vec<u8>]) -> Vec<String> {
    ids.iter()
        .map(|b| std::str::from_utf8(b).unwrap_or("").to_string())
        .collect()
}

#[inline]
fn summary_groups_arc(info: &NzbSummaryData) -> Arc<Vec<String>> {
    Arc::new(
        info.groups
            .iter()
            .map(|b| std::str::from_utf8(b).unwrap_or("").to_string())
            .collect(),
    )
}

#[inline]
fn same_posting_vecs(a: &[Vec<u8>], b: &[Vec<u8>]) -> bool {
    let ar: Vec<&[u8]> = a.iter().map(|v| v.as_slice()).collect();
    let br: Vec<&[u8]> = b.iter().map(|v| v.as_slice()).collect();
    crate::posting::same_posting(&ar, &br)
}

#[inline]
fn same_posting_str_vec(a: &[Vec<u8>], b: &[String]) -> bool {
    let ar: Vec<&[u8]> = a.iter().map(|v| v.as_slice()).collect();
    let br: Vec<&[u8]> = b.iter().map(|s| s.as_bytes()).collect();
    crate::posting::same_posting(&ar, &br)
}

#[inline]
fn outer_compatible(title: &[u8], name: &[u8]) -> bool {
    !crate::dupeidentity::conflicts(title, name)
        && !crate::dupeidentity::other_title(title, name)
        && (!crate::releasename::readable(name) || crate::dupeidentity::consistent(title, name))
}

#[inline]
fn identity_present(
    id: &Option<crate::dupeidentity::Identity>,
) -> Option<&crate::dupeidentity::Identity> {
    match id.as_ref() {
        Some(v) if v.bytes != 0 && !v.extension.is_empty() => Some(v),
        _ => None,
    }
}

#[derive(Clone)]
struct HostOpts {
    dupe_search: bool,
    dupe_watch: bool,
    dupe_check: bool,
    dry_run: bool,
    delay_sec: i32,
    max_donors: i32,
    fast_donors: i32,
    min_alive: i32,
    health_percent: i32,
    health_min: i32,
    health_max: i32,
    body_checks: i32,
    health_budget_sec: i32,
    health_check_mode: i32,
    primary_score: i32,
    size_tolerance: i32,
    body_percent: i32,
    check_concurrency: i32,
    queue_loaded: bool,
    stopped: bool,
    sep: u8,
    now: i64,
    queue_dir: Vec<u8>,
    temp_dir: Vec<u8>,
    search_url: Vec<u8>,
    api_key: Vec<u8>,
    ext_cleanup_disk: Vec<u8>,
}

fn now_unix_sec() -> i64 {
    #[cfg(unix)]
    unsafe {
        libc::time(ptr::null_mut()) as i64
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn query_host_stopped(host_ctx: *mut c_void, _host_cb: DupeHostFn) -> bool {
    if host_ctx.is_null() {
        return false;
    }
    unsafe { (*host_ctx.cast::<AtomicBool>()).load(Ordering::SeqCst) }
}

pub(crate) unsafe extern "C" fn native_dupe_host(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    _ints: *mut c_int,
    _i64s: *mut i64,
    strs: *mut *const c_char,
    _bytes: *const u8,
    _len: usize,
) -> usize {
    match op {
        0 => {
            if ctx.is_null() {
                0
            } else {
                usize::from(unsafe { (*ctx.cast::<AtomicBool>()).load(Ordering::SeqCst) })
            }
        }
        6 => {
            let Some(qh) = crate::ffi::queue_host() else {
                return 0;
            };
            let nzb = NzbRef::from_addr(p2);
            if a1 == 0 {
                let s1_b = unsafe { cstr::owned(s1) };
                qh.nzb_set_str(nzb, crate::queuesubsys_host::NZB_STR_DUPE_KEY, &s1_b);
                let s2_b = if s2.is_null() {
                    None
                } else {
                    Some(unsafe { CStr::from_ptr(s2).to_bytes() })
                };
                qh.nzb_set_param(nzb, OWNED_KEY_PARAM, s2_b);
                let primary = crate::dupewatch::primary_score();
                if (a2 & 2) != 0 && qh.nzb_snap(nzb, false).dupe_score < primary {
                    qh.nzb_set_int(nzb, crate::queuesubsys_host::NZB_INT_DUPE_SCORE, primary);
                }
                qh.nzb_set_int(nzb, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
                if (a2 & 1) != 0 {
                    qh.dq_history_changed(p1);
                    qh.dq_save(p1);
                } else {
                    qh.dq_save_changed(p1);
                }
                return 0;
            }
            if a1 == 1 {
                let s1_b = unsafe { cstr::owned(s1) };
                qh.nzb_add_message(nzb, a2, &s1_b);
                return 0;
            }
            if a1 == 2 {
                let queued = qh.queue_find_by_id(p1, a2).is_some();
                let action = if queued {
                    crate::queuesubsys_host::EA_GROUP_FINAL_DELETE
                } else {
                    crate::queuesubsys_host::EA_HISTORY_FINAL_DELETE
                };
                qh.dq_edit_list_single_id(p1, a2, action, None);
                return 0;
            }
            let h_len = qh.history_len(p1);
            for idx in 0..h_len {
                let h = qh.history_at(p1, idx);
                if h.id == a2 && h.kind == crate::queuesubsys_host::HK_DUP {
                    return 0;
                }
            }
            let score_arg = if s1.is_null() {
                None
            } else {
                Some(unsafe { CStr::from_ptr(s1).to_bytes() })
            };
            let kinds = [
                (
                    crate::queuesubsys_host::EA_HISTORY_SET_DUPE_SCORE,
                    crate::queuesubsys_host::EA_HISTORY_SET_PARAMETER,
                ),
                (
                    crate::queuesubsys_host::EA_GROUP_SET_DUPE_SCORE,
                    crate::queuesubsys_host::EA_GROUP_SET_PARAMETER,
                ),
            ];
            for (k_score, k_param) in kinds {
                if qh.dq_edit_list_single_id(p1, a2, k_score, score_arg) {
                    if !strs.is_null() {
                        for i in 0..p2 {
                            let param_ptr = unsafe { *strs.add(i) };
                            let param_arg = if param_ptr.is_null() {
                                None
                            } else {
                                Some(unsafe { CStr::from_ptr(param_ptr).to_bytes() })
                            };
                            qh.dq_edit_list_single_id(p1, a2, k_param, param_arg);
                        }
                    }
                    return 1;
                }
            }
            0
        }
        _ => 0,
    }
}

fn read_host_opts(host_ctx: *mut c_void, host_cb: DupeHostFn) -> HostOpts {
    let opts = crate::options::active_options();
    let sc = opts.as_deref().map(|o| *o.scalars());
    HostOpts {
        dupe_search: sc.is_some_and(|s| s.dupe_search != 0),
        dupe_check: sc.is_some_and(|s| s.dupe_check != 0),
        dupe_watch: crate::options::dupe_watch(),
        // DupeSearchDryRun only covers DupeSearch itself.
        dry_run: sc.is_some_and(|s| s.dupe_search != 0 && s.dupe_search_dry_run != 0),
        delay_sec: sc.map_or(0, |s| s.dupe_search_delay),
        max_donors: sc.map_or(0, |s| s.dupe_search_max_donors),
        fast_donors: sc.map_or(0, |s| s.dupe_fast_donors),
        min_alive: sc.map_or(0, |s| s.dupe_min_alive),
        health_percent: sc.map_or(0, |s| s.dupe_health_percent),
        health_min: sc.map_or(0, |s| s.dupe_health_min),
        health_max: sc.map_or(0, |s| s.dupe_health_max),
        body_checks: sc.map_or(0, |s| s.dupe_body_checks),
        health_budget_sec: sc.map_or(0, |s| s.dupe_health_budget),
        health_check_mode: sc.map_or(0, |s| s.health_check),
        primary_score: sc.map_or(BASE_PICK_SCORE, |s| s.dupe_primary_score),
        size_tolerance: sc.map_or(0, |s| s.dupe_size_tolerance),
        body_percent: sc.map_or(20, |s| s.dupe_body_percent),
        check_concurrency: sc.map_or(10, |s| s.dupe_check_concurrency),
        queue_loaded: crate::ffi::is_queue_loaded(),
        stopped: query_host_stopped(host_ctx, host_cb),
        sep: crate::paths::PATH_SEPARATOR as u8,
        now: now_unix_sec(),
        queue_dir: opts
            .as_deref()
            .map_or_else(Vec::new, |o| o.str_bytes(OptStr::QueueDir).to_vec()),
        temp_dir: opts
            .as_deref()
            .map_or_else(Vec::new, |o| o.str_bytes(OptStr::TempDir).to_vec()),
        search_url: opts
            .as_deref()
            .map_or_else(Vec::new, |o| o.str_bytes(OptStr::DupeSearchUrl).to_vec()),
        api_key: opts
            .as_deref()
            .map_or_else(Vec::new, |o| o.str_bytes(OptStr::DupeSearchApiKey).to_vec()),
        ext_cleanup_disk: opts
            .as_deref()
            .map_or_else(Vec::new, |o| o.str_bytes(OptStr::ExtCleanupDisk).to_vec()),
    }
}

/// The log prefix of the duplicate upkeep: `DupeSearch: ` when option
/// DupeSearch is on, `Dupes: ` when only the queued backups are looked after.
fn log_tag() -> &'static [u8] {
    if crate::options::active_options().is_some_and(|o| o.scalars().dupe_search != 0) {
        b"DupeSearch: "
    } else {
        b"Dupes: "
    }
}

#[inline]
fn host_now(_host_ctx: *mut c_void, _host_cb: DupeHostFn) -> i64 {
    now_unix_sec()
}

#[inline]
fn host_stopped(host_ctx: *mut c_void, host_cb: DupeHostFn) -> bool {
    query_host_stopped(host_ctx, host_cb)
}

fn host_log(_host_ctx: *mut c_void, _host_cb: DupeHostFn, kind: c_int, msg: &[u8]) {
    #[cfg(unix)]
    crate::log::log_active_bytes(kind, msg);
    #[cfg(not(unix))]
    let _ = (kind, msg);
}

fn with_guarded_queue<R, F: FnOnce(usize) -> R>(
    _host_ctx: *mut c_void,
    _host_cb: DupeHostFn,
    f: F,
) -> R {
    let qh = crate::ffi::queue_host().expect("queue_host not initialized");
    let mut out = None;
    qh.with_locked_queue(|dq_ptr| {
        out = Some(f(dq_ptr));
    });
    out.expect("guarded queue callback did not run")
}

#[derive(Clone, Default)]
struct NzbSnap {
    ptr: NzbRef,
    id: i32,
    kind: i32,
    deleting: bool,
    delete_status: DeleteStatus,
    mark_status: MarkStatus,
    dupe_mode: DupeMode,
    dupe_hint: i32,
    dupe_score: i32,
    has_post_info: bool,
    dupe_success: bool,
    full_hash: u32,
    is_donor: bool,
    has_dupe_fleet: bool,
    health: i32,
    critical_health: i32,
    name: Vec<u8>,
    dupe_key: Vec<u8>,
    category: Vec<u8>,
    queued_filename: Vec<u8>,
    final_dir: Vec<u8>,
    dest_dir: Vec<u8>,
    marked_alive: Option<Vec<u8>>,
    owned_key_param: Option<Vec<u8>>,
}

fn dq_counts(_host_ctx: *mut c_void, _host_cb: DupeHostFn, dq_ptr: usize) -> (usize, usize) {
    let Some(qh) = crate::ffi::queue_host() else {
        return (0, 0);
    };
    (qh.queue_len(dq_ptr), qh.history_len(dq_ptr))
}

fn dq_queue_at(_host_ctx: *mut c_void, _host_cb: DupeHostFn, dq_ptr: usize, idx: usize) -> NzbRef {
    let Some(qh) = crate::ffi::queue_host() else {
        return NzbRef::NONE;
    };
    qh.queue_at(dq_ptr, idx)
}

fn dq_history_at(
    _host_ctx: *mut c_void,
    _host_cb: DupeHostFn,
    dq_ptr: usize,
    idx: usize,
) -> (i32, i32, i64, NzbRef) {
    let Some(qh) = crate::ffi::queue_host() else {
        return (0, 0, 0, NzbRef::NONE);
    };
    let h = qh.history_at(dq_ptr, idx);
    let nzb_ptr = if h.kind == crate::queuesubsys_host::HK_NZB {
        h.nzb()
    } else {
        NzbRef::NONE
    };
    (h.id, h.kind, h.time, nzb_ptr)
}

fn dq_find_nzb(
    _host_ctx: *mut c_void,
    _host_cb: DupeHostFn,
    dq_ptr: usize,
    nzb_id: i32,
) -> Option<(NzbRef, bool)> {
    let qh = crate::ffi::queue_host()?;
    let q_ptr = qh.queue_find_by_id(dq_ptr, nzb_id);
    if q_ptr.is_some() {
        return Some((q_ptr, false));
    }
    let h_len = qh.history_len(dq_ptr);
    for idx in 0..h_len {
        let h = qh.history_at(dq_ptr, idx);
        if h.kind == crate::queuesubsys_host::HK_NZB
            && h.nzb().is_some()
            && qh.nzb_snap(h.nzb(), false).id == nzb_id
        {
            return Some((h.nzb(), true));
        }
    }
    None
}

fn inspect_nzb(
    _host_ctx: *mut c_void,
    _host_cb: DupeHostFn,
    nzb_ptr: NzbRef,
    with_health: bool,
) -> NzbSnap {
    let Some(qh) = crate::ffi::queue_host() else {
        return NzbSnap::default();
    };
    let s = qh.nzb_snap(nzb_ptr, false);
    let marked_alive = qh.nzb_get_param(nzb_ptr, ALIVE_PARAM);
    let owned_key_param = qh.nzb_get_param(nzb_ptr, OWNED_KEY_PARAM);
    let is_donor = marked_alive.is_some() || qh.nzb_get_param(nzb_ptr, DONOR_PARAM).is_some();
    let has_dupe_fleet = qh.nzb_get_param(nzb_ptr, FLEET_PARAM).is_some();
    NzbSnap {
        ptr: nzb_ptr,
        id: s.id,
        kind: s.kind,
        deleting: s.deleting,
        delete_status: s.delete_status,
        mark_status: s.mark_status,
        dupe_mode: s.dupe_mode,
        dupe_hint: s.dupe_hint,
        dupe_score: s.dupe_score,
        has_post_info: s.has_post_info,
        dupe_success: s.is_dupe_success,
        full_hash: s.full_content_hash,
        is_donor,
        has_dupe_fleet,
        health: if with_health { s.health } else { 0 },
        critical_health: if with_health {
            s.critical_health_opt
        } else {
            0
        },
        name: s.name,
        dupe_key: s.dupe_key,
        category: s.category,
        queued_filename: s.queued_filename,
        final_dir: s.final_dir,
        dest_dir: s.dest_dir,
        marked_alive,
        owned_key_param,
    }
}

fn check_placement_available(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    dq_ptr: usize,
    pick_snap: Option<&NzbSnap>,
    backup_id: i32,
    whole_backup: bool,
    hc_dupe: bool,
) -> bool {
    let Some((b_ptr, in_history)) = dq_find_nzb(host_ctx, host_cb, dq_ptr, backup_id) else {
        return false;
    };
    let backup = inspect_nzb(host_ctx, host_cb, b_ptr, whole_backup);
    let qf_exists = !backup.queued_filename.is_empty()
        && crate::fsops::file_exists(&backup.queued_filename, &crate::fsops::posix_stat);
    let has_marked_alive = backup.marked_alive.is_some();
    let marked_alive_val = backup
        .marked_alive
        .as_deref()
        .map(parse_atoi_bytes)
        .unwrap_or(0);
    placement_available(
        true,
        backup.kind == 0,
        backup.deleting,
        backup.delete_status,
        backup.mark_status,
        backup.dupe_mode,
        in_history,
        whole_backup,
        backup.has_post_info,
        backup.dupe_success,
        pick_snap.is_some(),
        pick_snap.map(|p| p.full_hash).unwrap_or(0),
        backup.full_hash,
        qf_exists,
        backup.health,
        backup.critical_health,
        hc_dupe,
        has_marked_alive,
        marked_alive_val,
    )
}

fn scanner_add_external(
    _host_ctx: *mut c_void,
    _host_cb: DupeHostFn,
    name: &[u8],
    category: &[u8],
    priority: i32,
    dupe_key: &[u8],
    score: i32,
    params: &[(&[u8], &[u8])],
    data: &[u8],
) -> i32 {
    let Some(sc) = crate::scanner::active_scanner() else {
        return 0;
    };
    let (st, out_id) = sc.add_external_bytes_active(
        name, category, false, priority, dupe_key, score, 0, params, false, false, data,
    );
    if st == crate::scanner::AS_SUCCESS && out_id != 0 {
        out_id
    } else {
        0
    }
}

// ============================================================================
// TimedMutex (Fleet per-key and global check locks)
// ============================================================================

struct TimedMutex {
    locked: Mutex<bool>,
    cond: Condvar,
}

struct TimedMutexGuard<'a>(&'a TimedMutex);

impl Drop for TimedMutexGuard<'_> {
    fn drop(&mut self) {
        let mut g = self.0.locked.lock().unwrap_or_else(|e| e.into_inner());
        *g = false;
        self.0.cond.notify_one();
    }
}

impl TimedMutex {
    fn new() -> Self {
        Self {
            locked: Mutex::new(false),
            cond: Condvar::new(),
        }
    }

    fn try_lock_for(&self, timeout_ms: i64) -> Option<TimedMutexGuard<'_>> {
        let mut g = self.locked.lock().unwrap_or_else(|e| e.into_inner());
        if !*g {
            *g = true;
            return Some(TimedMutexGuard(self));
        }
        if timeout_ms <= 0 {
            return None;
        }
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        while *g {
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let wait = deadline.saturating_duration_since(now);
            let (ng, _) = self
                .cond
                .wait_timeout(g, wait)
                .unwrap_or_else(|e| e.into_inner());
            g = ng;
        }
        *g = true;
        Some(TimedMutexGuard(self))
    }
}

static FLEET_CHECK_MUTEX: OnceLock<TimedMutex> = OnceLock::new();
static FLEET_KEY_LOCKS: OnceLock<Mutex<BTreeMap<Vec<u8>, Arc<TimedMutex>>>> = OnceLock::new();

fn fleet_check_mutex() -> &'static TimedMutex {
    FLEET_CHECK_MUTEX.get_or_init(TimedMutex::new)
}

fn fleet_key_lock(key: &[u8]) -> Arc<TimedMutex> {
    let map_mutex = FLEET_KEY_LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut map = map_mutex.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(dupe_util_lower(key))
        .or_insert_with(|| Arc::new(TimedMutex::new()))
        .clone()
}

// ============================================================================
// HttpGet (`HttpGet::Fetch`, `StopAll`, `Reset`, `Stopped`)
// ============================================================================

static HTTP_STOPPED: AtomicBool = AtomicBool::new(false);
static HTTP_COUNTER: AtomicI32 = AtomicI32::new(0);
#[cfg(unix)]
static HTTP_ACTIVE: OnceLock<Mutex<BTreeMap<usize, Arc<crate::net::NzbgetRsWebDl>>>> =
    OnceLock::new();

#[cfg(unix)]
fn http_active_map() -> &'static Mutex<BTreeMap<usize, Arc<crate::net::NzbgetRsWebDl>>> {
    HTTP_ACTIVE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn http_reset() {
    HTTP_STOPPED.store(false, Ordering::SeqCst);
}

pub fn http_stopped() -> bool {
    HTTP_STOPPED.load(Ordering::SeqCst)
}

pub fn http_stop_all() {
    #[cfg(unix)]
    {
        let map = http_active_map().lock().unwrap_or_else(|e| e.into_inner());
        HTTP_STOPPED.store(true, Ordering::SeqCst);
        for dl in map.values() {
            dl.stop();
        }
    }
    #[cfg(not(unix))]
    HTTP_STOPPED.store(true, Ordering::SeqCst);
}

pub fn http_fetch(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    url: &[u8],
    info_name: &[u8],
    max_bytes: usize,
    deadline_ms: i64,
) -> (bool, i32, Vec<u8>) {
    #[cfg(not(unix))]
    {
        let _ = (host_ctx, host_cb, url, info_name, max_bytes, deadline_ms);
        return (false, 0, Vec::new());
    }
    #[cfg(unix)]
    {
        if http_stopped() {
            return (false, 0, Vec::new());
        }
        let opts = read_host_opts(host_ctx, host_cb);
        let counter = HTTP_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
        let path = http_get_format_temp_path(&opts.temp_dir, opts.sep, opts.now as i32, counter);
        let dl = Arc::new(crate::net::NzbgetRsWebDl::new_configured(
            url, info_name, &path, true, false, 2, None, None,
        ));
        let dl_key = Arc::as_ptr(&dl) as usize;
        {
            let mut active = http_active_map().lock().unwrap_or_else(|e| e.into_inner());
            if http_stopped() {
                return (false, 0, Vec::new());
            }
            active.insert(dl_key, Arc::clone(&dl));
        }

        let done_mutex = Mutex::new(false);
        let done_cond = Condvar::new();
        let mut finished = false;
        let mut http_status = 0i32;

        std::thread::scope(|s| {
            let dm = &done_mutex;
            let dc = &done_cond;
            let path_ref = path.as_slice();
            let dl_watch = Arc::clone(&dl);
            s.spawn(move || {
                let mut done = dm.lock().unwrap_or_else(|e| e.into_inner());
                while !*done {
                    let now_ms = donor_now_ms();
                    let file_size = std::fs::metadata(dupe_bytes_to_path(path_ref))
                        .map(|m| m.len())
                        .unwrap_or(0);
                    if (deadline_ms > 0 && now_ms >= deadline_ms)
                        || (file_size > 0 && (file_size as usize) > max_bytes)
                    {
                        dl_watch.stop();
                        return;
                    }
                    let wait_ms = if deadline_ms > 0 {
                        (deadline_ms - now_ms).clamp(1, 250) as u64
                    } else {
                        250
                    };
                    let (nd, _) = dc
                        .wait_timeout(done, Duration::from_millis(wait_ms))
                        .unwrap_or_else(|e| e.into_inner());
                    done = nd;
                }
            });

            finished = dl.download_sync(5) == crate::webdownload::WebDlStatus::AdFinished as c_int;
            http_status = dl.http_status();
            {
                let mut done = dm.lock().unwrap_or_else(|e| e.into_inner());
                *done = true;
            }
            dc.notify_all();
        });

        {
            let mut active = http_active_map().lock().unwrap_or_else(|e| e.into_inner());
            active.remove(&dl_key);
        }

        match http_get_read_and_delete(&path, max_bytes, finished) {
            Some(body) => (true, http_status, body),
            None => (false, http_status, Vec::new()),
        }
    }
}

// ============================================================================
// NntpHealthServer (`NntpHealthTransportShared` & server pool cache)
// ============================================================================

type KnownHealthServer = (Arc<DonorServer>, Arc<AtomicUsize>, Arc<Mutex<Vec<i32>>>);

static NNTP_KNOWN_SERVERS: OnceLock<Mutex<BTreeMap<(i32, usize), KnownHealthServer>>> =
    OnceLock::new();

pub fn nntp_health_servers(host_ctx: *mut c_void, host_cb: DupeHostFn) -> Vec<Arc<DonorServer>> {
    let map_mutex = NNTP_KNOWN_SERVERS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut known = map_mutex.lock().unwrap_or_else(|e| e.into_inner());
    let mut raw_servers: Vec<(i32, i32, i32, i32)> = Vec::new();
    let mut ranks: Vec<HealthServerRank> = Vec::new();
    #[cfg(unix)]
    if let Some(pool) = crate::net::active_server_pool() {
        for s in pool.servers_snapshot() {
            let active = s.active.load(Ordering::SeqCst);
            raw_servers.push((s.id, c_int::from(active), s.max_connections, s.group));
            ranks.push(HealthServerRank {
                id: s.id,
                usable: active && s.max_connections != 0,
                group: s.group,
                retention: s.retention,
                level: s.norm_level.load(Ordering::SeqCst),
            });
        }
    }
    let order = backbone_order(&ranks);
    let check_connections =
        crate::options::active_options().map_or(0, |o| o.scalars().dupe_check_connections);
    let mut list = Vec::new();
    for (server_id, active, max_conns, group) in raw_servers {
        if active == 0 || max_conns == 0 {
            continue;
        }
        // one health server per backbone: its best server, then the siblings in order
        let servers = if group > 0 {
            match order.get(&server_id) {
                Some(ids) => ids.clone(),
                None => continue,
            }
        } else {
            vec![server_id]
        };
        let wanted_cap = dupe_check_server_cap(max_conns, check_connections);
        let key = (server_id, host_cb as usize);
        let recreate = match known.get(&key) {
            Some((srv, ctx_cell, ids)) => {
                ctx_cell.store(host_ctx as usize, Ordering::SeqCst);
                *ids.lock().unwrap_or_else(|e| e.into_inner()) = servers.clone();
                srv.cap() != wanted_cap
            }
            None => true,
        };
        if recreate {
            let ctx_cell = Arc::new(AtomicUsize::new(host_ctx as usize));
            let ids = Arc::new(Mutex::new(servers));
            let transport = NntpHealthTransportShared {
                server_ids: ids.clone(),
                host_ctx: ctx_cell.clone(),
                host_cb,
            };
            let srv = Arc::new(DonorServer::new(wanted_cap, transport));
            known.insert(key, (srv.clone(), ctx_cell, ids));
            list.push(srv);
        } else if let Some((srv, _, _)) = known.get(&key) {
            list.push(srv.clone());
        }
    }
    list
}

/// A news server as [`backbone_order`] ranks it.
#[derive(Debug, Clone, Copy)]
struct HealthServerRank {
    id: i32,
    usable: bool,
    group: i32,
    retention: i32,
    level: i32,
}

/// The servers per backbone the health checks ask, keyed by the first one:
/// servers on a backbone read from one article store, so one answer is
/// enough, and the others are only asked when the first has no connection
/// free (busy, blocked, login refused). The longest retention comes first
/// (0 = not set counts as the longest), then the lowest level, then the
/// lowest id. Independent servers (group 0) are not listed.
fn backbone_order(servers: &[HealthServerRank]) -> BTreeMap<i32, Vec<i32>> {
    let keep = |r: i32| if r <= 0 { i32::MAX } else { r };
    let mut by_group: BTreeMap<i32, Vec<HealthServerRank>> = BTreeMap::new();
    for s in servers.iter().filter(|s| s.usable && s.group > 0) {
        by_group.entry(s.group).or_default().push(*s);
    }
    by_group
        .into_values()
        .map(|mut members| {
            members.sort_by_key(|s| (std::cmp::Reverse(keep(s.retention)), s.level, s.id));
            (members[0].id, members.iter().map(|s| s.id).collect())
        })
        .collect()
}

/// Check connections per news server: all of its connections when
/// `DupeCheckConnections` is 0, else at most that many. Downloads keep first
/// claim through `NzbgetRsServerPool::get_check_connection`.
fn dupe_check_server_cap(server_connections: i32, check_connections: i32) -> i32 {
    if check_connections <= 0 {
        server_connections.max(1)
    } else {
        server_connections.min(check_connections).max(1)
    }
}

struct NntpHealthTransportShared {
    /// the backbone's servers in the order to ask (one id for an independent server)
    server_ids: Arc<Mutex<Vec<i32>>>,
    host_ctx: Arc<AtomicUsize>,
    host_cb: DupeHostFn,
}

impl DonorServerExchange for NntpHealthTransportShared {
    fn exchange(&self, batch: &[DonorRequest]) -> Result<Vec<DonorAnswer>, DonorExchangeError> {
        if batch.is_empty() {
            return Ok(Vec::new());
        }
        let ctx = self.host_ctx.load(Ordering::SeqCst) as *mut c_void;
        let host_cb = self.host_cb;

        #[cfg(unix)]
        if let Some(pool) = crate::net::active_server_pool() {
            struct NativeConnRelease {
                pool: Arc<crate::net::NzbgetRsServerPool>,
                conn_ptr: usize,
                broken: bool,
            }
            impl Drop for NativeConnRelease {
                fn drop(&mut self) {
                    let raw = self.conn_ptr as *mut crate::net::PooledNntpConn;
                    if self.broken {
                        unsafe {
                            (*raw).disconnect();
                        }
                    }
                    self.pool.free_connection(raw, true);
                }
            }

            let mut commands = Vec::with_capacity(batch.len() * 48);
            for req in batch {
                commands.extend_from_slice(b"STAT ");
                commands.extend_from_slice(&crate::nntp::nntp_health_format_bracketed_id(
                    req.message_id.as_bytes(),
                ));
                commands.extend_from_slice(b"\r\n");
            }

            let mut wait_start = host_now(ctx, host_cb);
            let ids = self.server_ids.lock().unwrap_or_else(|e| e.into_inner()).clone();
            // servers of this backbone whose connect failed during this exchange
            let mut failed: Vec<i32> = Vec::new();
            let (mut release, need_join) = loop {
                let mut conn_ptr = 0usize;
                let mut need_join = false;
                while conn_ptr == 0 && !http_stopped() && host_now(ctx, host_cb) - wait_start <= 10 {
                    let mut known_any = false;
                    for &id in ids.iter().filter(|id| !failed.contains(id)) {
                        let Some(srv) = pool.server_by_id(id) else {
                            continue;
                        };
                        known_any = true;
                        let raw = pool.get_check_connection(&srv);
                        if !raw.is_null() {
                            need_join = srv.join_group;
                            conn_ptr = raw as usize;
                            break;
                        }
                    }
                    if !known_any {
                        return Err(DonorExchangeError::default());
                    }
                    if conn_ptr == 0 {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                if conn_ptr == 0 {
                    return Err(DonorExchangeError {
                        answers: Vec::new(),
                        busy: true,
                    });
                }
                let mut release = NativeConnRelease {
                    pool: Arc::clone(&pool),
                    conn_ptr,
                    broken: false,
                };
                let conn = unsafe { &*(conn_ptr as *const crate::net::PooledNntpConn) };
                unsafe {
                    conn.prepare_for_use();
                }
                if unsafe { conn.connect() } {
                    break (release, need_join);
                }
                release.broken = true;
                if ids.len() < 2 || http_stopped() {
                    return Err(DonorExchangeError::default());
                }
                // a backbone sibling can answer this batch: block this server for the
                // retry interval (as a download's connect failure does) and ask the next
                let failed_id = conn.server.id;
                pool.block_server(&conn.server);
                drop(release);
                failed.push(failed_id);
                if ids.iter().all(|id| failed.contains(id)) {
                    return Err(DonorExchangeError::default());
                }
                // the failed connect may have taken the whole wait (a server that never
                // answers): the sibling gets a wait of its own
                wait_start = host_now(ctx, host_cb);
            };
            let conn_ptr = release.conn_ptr;
            let conn = unsafe { &*(conn_ptr as *const crate::net::PooledNntpConn) };
            if need_join && !batch[0].groups.is_empty() {
                let mut joined_ptr: *const c_char = ptr::null();
                for grp in batch[0].groups.iter() {
                    let g_c = c_bytes(grp.as_bytes());
                    joined_ptr = unsafe { conn.join_group(g_c.as_ptr() as *const c_char) };
                    if !joined_ptr.is_null() && unsafe { *joined_ptr as u8 } == b'2' {
                        break;
                    }
                }
                let ok_join = !joined_ptr.is_null() && unsafe { *joined_ptr as u8 } == b'2';
                if !ok_join {
                    release.broken = joined_ptr.is_null();
                    return Err(DonorExchangeError::default());
                }
            }
            if !unsafe { conn.send_bytes(&commands) } {
                release.broken = true;
                return Err(DonorExchangeError::default());
            }

            let mut answers = Vec::with_capacity(batch.len());
            let mut line = [0u8; 1024];
            for _ in 0..batch.len() {
                if http_stopped() || donor_stopping() {
                    release.broken = true;
                    return Err(DonorExchangeError {
                        answers,
                        busy: false,
                    });
                }
                let Some(n) = (unsafe { conn.read_line(&mut line) }) else {
                    release.broken = true;
                    return Err(DonorExchangeError {
                        answers,
                        busy: false,
                    });
                };
                answers.push(if n >= 3 && &line[..3] == b"223" {
                    DonorAnswer::Present
                } else if n >= 3 && &line[..3] == b"430" {
                    DonorAnswer::Missing
                } else {
                    DonorAnswer::Error
                });
            }

            for i in 0..batch.len() {
                if http_stopped() || donor_stopping() {
                    release.broken = true;
                    for j in i..batch.len() {
                        if batch[j].gate.is_some() && answers[j] == DonorAnswer::Present {
                            answers[j] = DonorAnswer::Error;
                        }
                    }
                    return Err(DonorExchangeError {
                        answers,
                        busy: false,
                    });
                }
                let Some(gate) = batch[i].gate.as_ref() else {
                    continue;
                };
                if answers[i] != DonorAnswer::Present {
                    continue;
                }
                let res = gate.body_once_fallible(|| {
                    let bracketed = crate::nntp::nntp_health_format_bracketed_id(
                        batch[i].message_id.as_bytes(),
                    );
                    let bg_vecs: Vec<Vec<u8>> = batch[i]
                        .groups
                        .iter()
                        .map(|g| g.as_bytes().to_vec())
                        .collect();
                    let art = crate::articlefetcher::ArticleFetcher::new(
                        &crate::articlefetcher::NativeNntpHost,
                    )
                    // SAFETY: `conn_ptr` came from the pool and is freed only when
                    // `release` drops, after this loop.
                    .fetch_from_connection(
                        unsafe {
                            crate::articlefetcher::ConnHandle::from_pooled(
                                conn_ptr as *mut crate::net::PooledNntpConn,
                            )
                        },
                        &bracketed,
                        &bg_vecs,
                    );
                    if art.success {
                        Ok(DonorAnswer::Present)
                    } else if art.transient {
                        Err(())
                    } else {
                        Ok(DonorAnswer::BodyBad)
                    }
                });
                match res {
                    Ok(ans) => answers[i] = ans,
                    Err(()) => {
                        release.broken = true;
                        answers[i] = DonorAnswer::Error;
                        for j in (i + 1)..batch.len() {
                            if batch[j].gate.is_some() && answers[j] == DonorAnswer::Present {
                                answers[j] = DonorAnswer::Error;
                            }
                        }
                        return Err(DonorExchangeError {
                            answers,
                            busy: false,
                        });
                    }
                }
            }
            return Ok(answers);
        }

        #[cfg(not(unix))]
        let _ = (ctx, host_cb);
        Err(DonorExchangeError::default())
    }
}

// ============================================================================
// DupeIdentity::FetchArticle / Read / ParDoomed via host callback
// ============================================================================

fn get_nntp_fetch_host(
    _host_ctx: *mut c_void,
    _host_cb: DupeHostFn,
) -> Option<&'static dyn crate::articlefetcher::NntpHost> {
    #[cfg(unix)]
    if crate::net::active_server_pool().is_some() {
        return Some(&crate::articlefetcher::NativeNntpHost);
    }
    None
}

/// The first article of a file (any other answers unsuccessful).
fn fetch_article_via_host(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    message_id: &[u8],
    groups: &[String],
    deadline: i64,
) -> FetchedArticleResult {
    let out = fetch_any_article_via_host(host_ctx, host_cb, message_id, groups, deadline);
    if out.offset != 0 {
        return FetchedArticleResult::default();
    }
    out
}

/// Any article of a file, wherever its bytes sit.
fn fetch_any_article_via_host(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    message_id: &[u8],
    groups: &[String],
    deadline: i64,
) -> FetchedArticleResult {
    let Some(nntp_host) = get_nntp_fetch_host(host_ctx, host_cb) else {
        return FetchedArticleResult::default();
    };
    // fleet checks read a bounded handful of articles: never throttled
    let fetcher = Arc::new(crate::articlefetcher::ArticleFetcher::unthrottled(nntp_host));
    let fetcher_stop = Arc::clone(&fetcher);
    let ctx_addr = host_ctx as usize;
    crate::dupesearch_nzb::fetch_article_until(
        message_id,
        groups,
        deadline,
        || host_now(ctx_addr as *mut c_void, host_cb),
        http_stopped,
        move |bracketed_id, grp_slice| {
            let g_vecs: Vec<Vec<u8>> = grp_slice.iter().map(|g| g.as_bytes().to_vec()).collect();
            let art = fetcher.fetch(bracketed_id, &g_vecs);
            Ok(FetchedArticleResult {
                success: art.success,
                offset: art.offset,
                file_size: art.file_size,
                data: art.data,
            })
        },
        move || fetcher_stop.stop(),
    )
}

fn read_identity_via_host(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    info: &NzbSummaryData,
    deadline: i64,
) -> Option<crate::dupeidentity::Identity> {
    let heads: Vec<(Vec<u8>, Vec<u8>, i64)> = info
        .heads
        .iter()
        .map(|h| (h.name.clone(), h.id.clone(), h.bytes))
        .collect();
    let groups = summary_groups_arc(info);
    dupe_identity_read(
        info.data_bytes,
        &heads,
        &groups,
        deadline,
        || host_now(host_ctx, host_cb),
        http_stopped,
        |msg_id, grps, dl| fetch_article_via_host(host_ctx, host_cb, msg_id, grps, dl),
    )
}

fn par_doomed_via_host(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    info: &NzbSummaryData,
    alive: f64,
    deadline: i64,
) -> bool {
    if info.par_index.id.is_empty()
        || info.recovery_bytes.is_empty()
        || info.data_bytes <= 0
        || info.article_bytes == 0
    {
        return false;
    }
    let groups = summary_groups_arc(info);
    let article = fetch_article_via_host(host_ctx, host_cb, &info.par_index.id, &groups, deadline);
    if !article.success {
        return false;
    }
    let block = crate::twin::block_size(&article.data);
    par_doomed(
        info.data_bytes as u64,
        info.article_bytes,
        &info.recovery_bytes,
        alive,
        block,
    )
}

/// Longest a fleet spends reading par2 lists and archive headers to tell its
/// TWINs and borrowable ALTs apart.
const FLEET_KIND_PROBE_MAX_SEC: i64 = 20;
const FLEET_KIND_PROBE_PARALLEL: usize = 16;
/// Copies queued from an earlier submission probed with a new one, at most.
const FLEET_KIND_EARLIER_MAX: usize = 8;

/// Seconds the copy-kind probe may use out of `left_ms`, the fleet's time
/// left: a third of it, at most [`FLEET_KIND_PROBE_MAX_SEC`], so the intake
/// and health check after it keep most of the budget.
fn fleet_kind_probe_secs(left_ms: i64) -> i64 {
    fleet_phase_secs(left_ms, 3, FLEET_KIND_PROBE_MAX_SEC)
}

/// Seconds a fleet phase that runs before the health check may use: one
/// `share`-th of `left_ms`, at most `max_sec`. The health check spends
/// whatever budget it is given, so the phases before it are bounded here.
fn fleet_phase_secs(left_ms: i64, share: i64, max_sec: i64) -> i64 {
    (left_ms.max(0) / (1000 * share.max(1))).min(max_sec)
}

/// Whether a fleet copy ranks as locked: headers encrypted and no password,
/// neither in its nzb (meta or `{{password}}` name) nor set on the copy
/// already queued, nor `UnpackPassFile` (see [`crate::donorkind::locked`]).
fn fleet_candidate_locked(encrypted: bool, nzb_password: bool, stored_password: bool) -> bool {
    crate::donorkind::locked(encrypted, nzb_password || stored_password)
}

fn summary_password_known(info: &NzbSummaryData) -> bool {
    summary_password(info).is_some()
}

/// The archive password a fleet member's nzb carries (its meta, or a
/// `{{password}}` in its name, see [`summary_take_name_password`]).
fn summary_password(info: &NzbSummaryData) -> Option<&[u8]> {
    info.meta
        .iter()
        .find(|(k, v)| k.eq_ignore_ascii_case(b"password") && !v.trim_ascii().is_empty())
        .map(|(_, v)| v.as_slice())
}

/// Reads each copy's par2 index and the first article of its largest file
/// (in parallel chunks, until `deadline`): its par2 file list for the twin
/// test and its packing (see [`crate::donorkind`]).
fn probe_copies(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    summaries: &[&NzbSummaryData],
    deadline: i64,
) -> Vec<(crate::twin::Listed, crate::donorkind::Packing)> {
    use crate::donorkind::Packing;
    let n = summaries.len();
    let slots: Vec<Mutex<(crate::twin::Listed, Packing)>> = (0..n)
        .map(|_| Mutex::new((crate::twin::Listed::default(), Packing::Unknown)))
        .collect();
    let ctx_addr = host_ctx as usize;
    for chunk in (0..n).collect::<Vec<_>>().chunks(FLEET_KIND_PROBE_PARALLEL) {
        if now_unix_sec() >= deadline || donor_stopping() || http_stopped() {
            break;
        }
        std::thread::scope(|s| {
            for &i in chunk {
                let info = summaries[i];
                let slot = &slots[i];
                s.spawn(move || {
                    let ctx = ctx_addr as *mut c_void;
                    let groups = summary_groups_arc(info);
                    let mut listed = crate::twin::Listed::default();
                    if !info.par_list.id.is_empty() {
                        let art = fetch_article_via_host(
                            ctx,
                            host_cb,
                            &info.par_list.id,
                            &groups,
                            deadline,
                        );
                        if art.success {
                            let (sigs, complete) = crate::twin::par2_set(&art.data);
                            listed.sigs = sigs;
                            listed.complete = complete;
                        }
                        // a volume read for want of an index file carries its
                        // file list after the recovery data, at its end
                        let tail = &info.par_list.tail_ids;
                        if !listed.complete
                            && info.par_list.id != info.par_index.id
                            && !tail.is_empty()
                            && tail.last() != Some(&info.par_list.id)
                        {
                            let mut data = Vec::new();
                            let mut all = true;
                            for id in tail.iter().filter(|id| **id != info.par_list.id) {
                                let art =
                                    fetch_article_via_host(ctx, host_cb, id, &groups, deadline);
                                if !art.success {
                                    all = false;
                                    break;
                                }
                                data.extend_from_slice(&art.data);
                            }
                            if all {
                                let (sigs, complete) = crate::twin::par2_set(&data);
                                if complete {
                                    listed.sigs = sigs;
                                    listed.complete = true;
                                } else if !sigs.is_empty() {
                                    listed.sigs = crate::twin::merge(&listed.sigs, &sigs);
                                }
                            }
                        }
                    }
                    let mut packing = Packing::Unknown;
                    if let Some(head) = info.heads.iter().max_by_key(|h| h.bytes) {
                        let art = fetch_article_via_host(ctx, host_cb, &head.id, &groups, deadline);
                        if art.success {
                            packing = crate::donorkind::packing_of_head(
                                &art.data,
                                &head.name,
                                summary_password(info),
                            );
                        } else if summary_password_known(info)
                            && crate::donorkind::is_7z_name(&head.name)
                        {
                            // a 7-Zip whose nzb carries a password is
                            // unborrowable whatever its head holds (as
                            // packing_of_head rules for a read head), so an
                            // unread head (missing, or no time left) gives the
                            // same answer instead of "unknown", which ranked
                            // such a copy ahead of whole encrypted copies
                            packing = Packing::Unborrowable;
                        }
                    }
                    *slot.lock().unwrap_or_else(|e| e.into_inner()) = (listed, packing);
                });
            }
        });
    }
    slots
        .into_iter()
        .map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner()))
        .collect()
}

/// Longest a fleet spends opening its copies down to the video at intake.
const FLEET_INTAKE_MAX_SEC: i64 = 30;
/// Time the background re-check of copies the intake did not reach may take.
const FLEET_INTAKE_LATER_SEC: i64 = 180;
/// Why the intake left a copy unverified when it had no time for it.
const INTAKE_TIME_RAN_OUT: &str = "time ran out";

/// Articles of one copy, read from the news servers for the intake.
struct HostArticles {
    ctx: usize,
    cb: DupeHostFn,
    groups: Arc<Vec<String>>,
    deadline: i64,
}

impl crate::dupeintake::ArticleFetch for HostArticles {
    fn fetch(&self, message_id: &[u8]) -> Option<crate::dupeintake::Fetched> {
        let art = fetch_any_article_via_host(
            self.ctx as *mut c_void,
            self.cb,
            message_id,
            &self.groups,
            self.deadline,
        );
        art.success.then_some(crate::dupeintake::Fetched {
            offset: art.offset,
            file_size: art.file_size,
            data: art.data,
        })
    }

    fn expired(&self) -> bool {
        now_unix_sec() >= self.deadline || donor_stopping() || http_stopped()
    }
}

/// A copy's print, and the size and CRC32 of the video its archive headers
/// record (see [`crate::dupeintake::inner_key_copy`]): a RAR set's last
/// volume, or a 7z header, which a 7z with a password but without `-mhe`
/// gives even when it cannot be printed.
type KeyedPrint = (
    Result<crate::dupeintake::Print, String>,
    Option<crate::twinfields::InnerKey>,
);

/// Opens each copy down to its video and prints it (see
/// [`crate::dupeintake::print_copy`]), in parallel until `deadline` (unix
/// seconds), with each copy's [`KeyedPrint`] key; a copy not reached says
/// "time ran out".
fn fleet_prints_keyed(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    summaries: &[&NzbSummaryData],
    samples: usize,
    seed: u64,
    deadline: i64,
) -> Vec<KeyedPrint> {
    use crate::dupeintake::{inner_key_copy, print_copy};
    let n = summaries.len();
    let slots: Vec<Mutex<KeyedPrint>> = (0..n)
        .map(|_| Mutex::new((Err(INTAKE_TIME_RAN_OUT.to_string()), None)))
        .collect();
    let ctx_addr = host_ctx as usize;
    for chunk in (0..n).collect::<Vec<_>>().chunks(FLEET_KIND_PROBE_PARALLEL) {
        if now_unix_sec() >= deadline || donor_stopping() || http_stopped() {
            break;
        }
        std::thread::scope(|s| {
            for &i in chunk {
                let info = summaries[i];
                let slot = &slots[i];
                s.spawn(move || {
                    let fetch = HostArticles {
                        ctx: ctx_addr,
                        cb: host_cb,
                        groups: summary_groups_arc(info),
                        deadline,
                    };
                    let password = info
                        .meta
                        .iter()
                        .find(|(k, v)| {
                            k.eq_ignore_ascii_case(b"password") && !v.trim_ascii().is_empty()
                        })
                        .map(|(_, v)| v.trim_ascii().to_vec());
                    let got = print_copy(
                        &info.data_segments,
                        &fetch,
                        password.as_deref(),
                        samples,
                        seed,
                    );
                    // a key costs article reads: a 7z header several, a RAR
                    // set's last volume two (its first article and the first
                    // volume's); only for a copy that names 7z or RAR volumes,
                    // or could not be printed
                    let named_archive = info.data_segments.iter().any(|f| {
                        let n = f.name.to_ascii_lowercase();
                        n.ends_with(b".7z")
                            || n.windows(4).any(|w| w == b".7z.")
                            || crate::filetypes::is_rar_file(&n)
                    });
                    let key = if named_archive || got.is_err() {
                        inner_key_copy(&info.data_segments, &fetch)
                    } else {
                        None
                    };
                    *slot.lock().unwrap_or_else(|e| e.into_inner()) = (got, key);
                });
            }
        });
    }
    slots
        .into_iter()
        .map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner()))
        .collect()
}

/// The intake verdict of each copy and why it is unverified, the fleet's video
/// (the print most copies share) and each copy's print, for a later check.
fn fleet_intake(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    summaries: &[&NzbSummaryData],
    samples: usize,
    seed: u64,
    deadline: i64,
) -> (
    Vec<(crate::dupeintake::Verdict, String)>,
    Option<crate::dupeintake::Print>,
    Vec<Option<crate::dupeintake::Print>>,
    Vec<Option<crate::twinfields::InnerKey>>,
) {
    use crate::dupeintake::{fleet_print, judge_against};
    let (got, keys): (Vec<_>, Vec<_>) =
        fleet_prints_keyed(host_ctx, host_cb, summaries, samples, seed, deadline)
            .into_iter()
            .unzip();
    let prints: Vec<Option<crate::dupeintake::Print>> =
        got.iter().map(|r| r.as_ref().ok().cloned()).collect();
    let fleet = fleet_print(&prints);
    let verdicts = judge_against(&prints, fleet.as_ref())
        .into_iter()
        .zip(got)
        .map(|(v, r)| (v, r.err().unwrap_or_default()))
        .collect();
    (verdicts, fleet, prints, keys)
}

/// What the intake could not verify when a fleet was added (no time left, or a
/// slow or failed read), read again
/// after the reply: each copy in `pending` (nzb id, name, summary) is printed
/// within [`FLEET_INTAKE_LATER_SEC`] and compared with `fleet` (the video two
/// or more copies shared at submission; else the queued copy's). A copy that
/// carries another video is removed only while it is still a parked backup
/// in history; anything queued or downloading is only reported.
struct IntakeLater {
    dupe_key: Vec<u8>,
    /// the fleet's video when two or more copies shared it at submission
    fleet: Option<crate::dupeintake::Print>,
    /// the queued copy, and its print from submission when it had one
    primary: Option<(i32, NzbSummaryData, Option<crate::dupeintake::Print>)>,
    pending: Vec<(i32, Vec<u8>, NzbSummaryData)>,
    samples: usize,
    seed: u64,
}

/// What a failover waits on while the fleet intake reads a backup: the nzb ids
/// the background check is reading, and the dupe keys (lowercase) whose lone
/// backup is being added, from before it is queued until its id is listed.
/// One lock, so a reader never sees the handoff between the two half done.
struct IntakeHolds {
    ids: Vec<i32>,
    keys: Vec<Vec<u8>>,
}

static INTAKE_PENDING: Mutex<IntakeHolds> = Mutex::new(IntakeHolds {
    ids: Vec::new(),
    keys: Vec::new(),
});

fn intake_holds() -> std::sync::MutexGuard<'static, IntakeHolds> {
    INTAKE_PENDING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether the background intake is still reading the copy `id`, or a lone
/// backup of `dupe_key` is being added to be read.
pub(crate) fn intake_pending(id: i32, dupe_key: &[u8]) -> bool {
    let holds = intake_holds();
    holds.ids.contains(&id)
        || (!dupe_key.is_empty() && holds.keys.iter().any(|k| k.eq_ignore_ascii_case(dupe_key)))
}

/// Holds failovers for a dupe key while its lone backup is added, until it is
/// dropped.
struct IntakeKeyGuard(Vec<u8>);

impl IntakeKeyGuard {
    fn enter(dupe_key: &[u8]) -> Self {
        let key = dupe_key.to_ascii_lowercase();
        intake_holds().keys.push(key.clone());
        Self(key)
    }
}

impl Drop for IntakeKeyGuard {
    fn drop(&mut self) {
        let mut holds = intake_holds();
        if let Some(at) = holds.keys.iter().position(|k| *k == self.0) {
            holds.keys.swap_remove(at);
        }
    }
}

/// Marks copies as being read in the background until it is dropped.
struct IntakePendingGuard(Vec<i32>);

impl IntakePendingGuard {
    fn enter(ids: Vec<i32>) -> Self {
        intake_holds().ids.extend_from_slice(&ids);
        Self(ids)
    }
}

impl Drop for IntakePendingGuard {
    fn drop(&mut self) {
        let mut holds = intake_holds();
        for id in &self.0 {
            if let Some(at) = holds.ids.iter().position(|p| p == id) {
                holds.ids.swap_remove(at);
            }
        }
    }
}

fn spawn_fleet_intake_later(host_cb: DupeHostFn, job: IntakeLater) {
    // counted like the donor walkers, so shutdown waits for it
    let guard = crate::dupesearch_nzb::DonorWalkerGuard::enter();
    // set before the thread starts, so a failover check right after the reply
    // already waits for these copies
    let pending = IntakePendingGuard::enter(job.pending.iter().map(|(id, _, _)| *id).collect());
    let run = move || {
        let _guard = guard;
        let _pending = pending;
        fleet_intake_later(ptr::null_mut(), host_cb, job)
    };
    let _ = std::thread::Builder::new()
        .name("fleet-intake-later".into())
        .spawn(run);
}

fn fleet_intake_later(host_ctx: *mut c_void, host_cb: DupeHostFn, job: IntakeLater) {
    use crate::dupeintake::Verdict;
    let deadline = now_unix_sec() + FLEET_INTAKE_LATER_SEC;
    let primary_id = job.primary.as_ref().map_or(0, |(id, _, _)| *id);
    let known = job
        .fleet
        .clone()
        .or_else(|| job.primary.as_ref().and_then(|(_, _, p)| p.clone()));
    // the queued copy's own print is recorded too when it is read here; it is
    // read beside the backups, not before them, so they share the whole time
    let primary_summary = match (&known, job.primary.as_ref()) {
        (Some(_), _) => None,
        (None, Some((_, summary, _))) => Some(summary),
        (None, None) => return,
    };
    let primary_read = primary_summary.is_some();
    let summaries: Vec<&NzbSummaryData> = primary_summary
        .into_iter()
        .chain(job.pending.iter().map(|(_, _, s)| s))
        .collect();
    let mut results = fleet_prints_keyed(
        host_ctx,
        host_cb,
        &summaries,
        job.samples,
        job.seed,
        deadline,
    );
    let fleet = match known {
        Some(p) => p,
        None => match results.remove(0).0 {
            Ok(p) => p,
            Err(why) => {
                let mut msg = b"Fleet intake later: the queued copy of ".to_vec();
                push_c_slice(&mut msg, &job.dupe_key);
                msg.extend_from_slice(b" could not be read, its backups stay unverified: ");
                msg.extend_from_slice(why.as_bytes());
                host_log(host_ctx, host_cb, MK_INFO, &msg);
                return;
            }
        },
    };
    let (got, keys): (Vec<_>, Vec<_>) = results.into_iter().unzip();
    #[cfg(unix)]
    crate::twin::set_inner_keys(
        &job.pending
            .iter()
            .zip(&keys)
            .filter_map(|((id, _, _), k)| k.map(|k| (*id, k.param())))
            .collect::<Vec<_>>(),
    );
    let prints: Vec<Option<crate::dupeintake::Print>> =
        got.iter().map(|r| r.as_ref().ok().cloned()).collect();
    let verdicts = crate::dupeintake::judge_against(&prints, Some(&fleet));
    let mut digests: Vec<(i32, String)> = Vec::new();
    if primary_read && primary_id > 0 {
        digests.push((
            primary_id,
            crate::dupeintake::inner_param_value(&fleet, job.samples, job.seed),
        ));
    }
    for ((id, _, _), p) in job.pending.iter().zip(&prints) {
        if let Some(p) = p {
            digests.push((
                *id,
                crate::dupeintake::inner_param_value(p, job.samples, job.seed),
            ));
        }
    }
    #[cfg(unix)]
    crate::twin::set_inner_digests(&digests);
    for (((id, name, _), verdict), r) in job.pending.iter().zip(verdicts).zip(got) {
        let mut msg = b"Fleet intake later: ".to_vec();
        msg.extend_from_slice(name);
        match verdict {
            Verdict::Same => {
                fleet_mark_video(host_ctx, host_cb, *id, true);
                msg.extend_from_slice(b" carries the fleet's video")
            }
            Verdict::Different if *id == primary_id => msg.extend_from_slice(
                b" carries another video; it is the copy being downloaded, left as is",
            ),
            Verdict::Different => {
                let id = *id;
                let key = &job.dupe_key;
                let removed = with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
                    if donor_stopping() {
                        return false;
                    }
                    // only a backup still parked in history: never anything
                    // queued, downloading, done, or no longer of this fleet
                    let Some((ptr_, true)) = dq_find_nzb(host_ctx, host_cb, dq_ptr, id) else {
                        return false;
                    };
                    let item = inspect_nzb(host_ctx, host_cb, ptr_, false);
                    if item.delete_status != DeleteStatus::Dupe
                        || item.dupe_success
                        || !item.dupe_key.eq_ignore_ascii_case(key)
                    {
                        return false;
                    }
                    unsafe {
                        host_cb(
                            host_ctx,
                            6,
                            2,
                            id,
                            dq_ptr,
                            0,
                            ptr::null(),
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null(),
                            0,
                        );
                    }
                    true
                });
                if !removed {
                    // kept, so the panel must not offer it as a donor
                    fleet_set_video(host_ctx, host_cb, id, VIDEO_DIFFERENT);
                }
                msg.extend_from_slice(if removed {
                    b" carries another video, removed from the fleet".as_slice()
                } else {
                    b" carries another video; no longer a parked backup, left as is".as_slice()
                });
            }
            Verdict::Unverified => {
                msg.extend_from_slice(b" still unverified: ");
                msg.extend_from_slice(r.err().unwrap_or_default().as_bytes());
            }
        }
        host_log(host_ctx, host_cb, MK_INFO, &msg);
    }
}

// ============================================================================
// Newznab::Search (`Newznab::Search` in `Util.h`)
// ============================================================================

#[derive(Default, Clone, Copy)]
pub struct NewznabSearchStats {
    pub queries: i32,
    pub failed: i32,
    pub pages: i32,
}

pub fn newznab_search(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    base: &[u8],
    api_key: &[u8],
    queries: &[Vec<(Vec<u8>, Vec<u8>)>],
    deadline: i64,
) -> (Vec<NewznabResult>, NewznabSearchStats) {
    const PAGE_SIZE: i32 = 100;
    const MAX_PAGES: i32 = 5;
    const PARALLEL: usize = 4;
    const MAX_XML_BYTES: usize = 16 * 1024 * 1024;

    let found: Vec<Mutex<Vec<NewznabResult>>> =
        (0..queries.len()).map(|_| Mutex::new(Vec::new())).collect();
    let next = AtomicUsize::new(0);
    let failed = AtomicI32::new(0);
    let pages = AtomicI32::new(0);
    let ctx_addr = host_ctx as usize;

    let thread_count = queries.len().min(PARALLEL);
    std::thread::scope(|s| {
        let found_ref = &found;
        let next_ref = &next;
        let failed_ref = &failed;
        let pages_ref = &pages;
        for _ in 0..thread_count {
            s.spawn(move || {
                let ctx = ctx_addr as *mut c_void;
                loop {
                    let index = next_ref.fetch_add(1, Ordering::SeqCst);
                    if index >= queries.len() {
                        break;
                    }
                    let q_keys: Vec<&[u8]> = queries[index].iter().map(|p| p.0.as_slice()).collect();
                    let q_vals: Vec<&[u8]> = queries[index].iter().map(|p| p.1.as_slice()).collect();
                    let label = newznab_format_query_label(&q_keys, &q_vals);

                    for page in 0..MAX_PAGES {
                        if host_now(ctx, host_cb) >= deadline {
                            let mut msg = b"DupeSearch: no time left for the search ".to_vec();
                            push_c_slice(&mut msg, &label);
                            host_log(ctx, host_cb, MK_DETAIL, &msg);
                            break;
                        }
                        let limit_str = PAGE_SIZE.to_string();
                        let offset_str = (page * PAGE_SIZE).to_string();
                        let mut p_keys = q_keys.clone();
                        let mut p_vals = q_vals.clone();
                        p_keys.push(b"limit");
                        p_vals.push(limit_str.as_bytes());
                        p_keys.push(b"offset");
                        p_vals.push(offset_str.as_bytes());

                        let url = build_newznab_url_percent(base, &p_keys, &p_vals, api_key);
                        let info_name = newznab_format_search_info_name(&label, page + 1);
                        let page_deadline_ms =
                            1i64.max(donor_now_ms() + (deadline - host_now(ctx, host_cb)) * 1000);

                        let mut reply = http_fetch(
                            ctx,
                            host_cb,
                            &url,
                            &info_name,
                            MAX_XML_BYTES,
                            page_deadline_ms,
                        );
                        if !reply.0
                            && reply.1 == 0
                            && !http_stopped()
                            && host_now(ctx, host_cb) < deadline
                        {
                            reply = http_fetch(
                                ctx,
                                host_cb,
                                &url,
                                &info_name,
                                MAX_XML_BYTES,
                                page_deadline_ms,
                            );
                        }
                        pages_ref.fetch_add(1, Ordering::SeqCst);

                        if !reply.0 {
                            let mut msg = b"DupeSearch: the search ".to_vec();
                            push_c_slice(&mut msg, &label);
                            msg.extend_from_slice(format!(" failed (HTTP {})", reply.1).as_bytes());
                            host_log(ctx, host_cb, MK_WARNING, &msg);
                            failed_ref.fetch_add(1, Ordering::SeqCst);
                            break;
                        }
                        let Some(parsed) = parse_newznab_response(&reply.2) else {
                            let mut msg = b"DupeSearch: the answer to the search ".to_vec();
                            push_c_slice(&mut msg, &label);
                            msg.extend_from_slice(
                                b" isn't XML (is option DupeSearchUrl the indexer's api address, like http://host:5076/api?)",
                            );
                            host_log(ctx, host_cb, MK_WARNING, &msg);
                            failed_ref.fetch_add(1, Ordering::SeqCst);
                            break;
                        };
                        if parsed.error {
                            let masked = mask_newznab_text(&parsed.error_text);
                            let mut msg = b"DupeSearch: the indexer answered the search ".to_vec();
                            push_c_slice(&mut msg, &label);
                            msg.extend_from_slice(
                                format!(" with error {}: ", parsed.error_code).as_bytes(),
                            );
                            push_c_slice(&mut msg, &masked);
                            host_log(ctx, host_cb, MK_WARNING, &msg);
                            failed_ref.fetch_add(1, Ordering::SeqCst);
                            break;
                        }
                        let item_count = parsed.items.len() as i32;
                        found_ref[index]
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .extend(parsed.items);
                        if item_count < PAGE_SIZE {
                            break;
                        }
                    }
                }
            });
        }
    });

    let mut merged = Vec::new();
    let mut links = BTreeSet::new();
    for slot in found {
        let items = slot.into_inner().unwrap_or_else(|e| e.into_inner());
        for item in items {
            if !item.link.is_empty() && links.insert(item.link.clone()) {
                merged.push(item);
            }
        }
    }

    let stats = NewznabSearchStats {
        queries: queries.len() as i32,
        failed: failed.load(Ordering::SeqCst),
        pages: pages.load(Ordering::SeqCst),
    };
    (merged, stats)
}

// ============================================================================
// DupeSearch Coordinator (`NzbgetRsDupeSearch`)
// ============================================================================

#[derive(Clone, Default)]
struct SearchedStamp {
    score: i32,
    time: i64,
    title: Vec<u8>,
    retryable: bool,
    raw_name: Vec<u8>,
}

#[derive(Clone, Default)]
struct SearchJob {
    nzb_id: i32,
    failed: bool,
    rank_only: bool,
    name: Vec<u8>,
    dupe_key: Vec<u8>,
    category: Vec<u8>,
    queued_file: Vec<u8>,
    known_files: Vec<Vec<u8>>,
    members: Vec<(i32, Vec<u8>)>,
    member_scores: Vec<i32>,
    /// whether each member is locked now (see [`crate::dupecoord::locked_host`])
    member_locked: Vec<bool>,
    /// whether each member has a password now (`*Unpack:Password`)
    member_password: Vec<bool>,
    score: i32,
}

#[derive(Default)]
struct SearchInner {
    rank_pending: BTreeSet<i32>,
    pending: BTreeMap<i32, i64>,
    searched: BTreeMap<Vec<u8>, SearchedStamp>,
    title_searched: BTreeMap<(Vec<u8>, Vec<u8>), SearchedStamp>,
    sent: BTreeMap<i32, BTreeSet<Vec<u8>>>,
}

impl SearchInner {
    fn find_search_mut(&mut self, lower_key: &[u8], title: &[u8]) -> Option<&mut SearchedStamp> {
        let pair = (lower_key.to_vec(), title.to_vec());
        if self.title_searched.contains_key(&pair) {
            return self.title_searched.get_mut(&pair);
        }
        let matches = self
            .searched
            .get(lower_key)
            .is_some_and(|s| s.title.is_empty() || s.title == title);
        if matches {
            self.searched.get_mut(lower_key)
        } else {
            None
        }
    }

    fn find_search_cloned(&self, lower_key: &[u8], title: &[u8]) -> Option<SearchedStamp> {
        let pair = (lower_key.to_vec(), title.to_vec());
        if let Some(exact) = self.title_searched.get(&pair) {
            return Some(exact.clone());
        }
        let latest = self.searched.get(lower_key)?;
        if latest.title.is_empty() || latest.title == title {
            Some(latest.clone())
        } else {
            None
        }
    }

    fn record_search(&mut self, lower_key: &[u8], searched: SearchedStamp) {
        if !searched.title.is_empty() {
            self.title_searched.insert(
                (lower_key.to_vec(), searched.title.clone()),
                searched.clone(),
            );
        }
        self.searched.insert(lower_key.to_vec(), searched);
    }

    fn set_retryable(&mut self, lower_key: &[u8], title: &[u8], retryable: bool) {
        if let Some(found) = self.find_search_mut(lower_key, title) {
            found.retryable = retryable;
        }
        if let Some(latest) = self.searched.get_mut(lower_key) {
            if latest.title.is_empty() || latest.title == title {
                latest.retryable = retryable;
            }
        }
    }
}

pub struct NzbgetRsDupeSearch {
    fetcher: NzbFetcherCore,
    dead: DeadPostingsStore,
    inner: Mutex<SearchInner>,
    cond: Condvar,
}

impl Default for NzbgetRsDupeSearch {
    fn default() -> Self {
        Self::new()
    }
}

impl NzbgetRsDupeSearch {
    pub fn new() -> Self {
        http_reset();
        donor_reset();
        Self {
            fetcher: NzbFetcherCore::new(),
            dead: DeadPostingsStore::new(),
            inner: Mutex::new(SearchInner::default()),
            cond: Condvar::new(),
        }
    }

    pub fn stop(&self) {
        http_stop_all();
        donor_stop_all();
        let _g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.cond.notify_all();
    }

    pub fn schedule(&self, nzb_id: i32, due: i64) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.pending.insert(nzb_id, due);
        self.cond.notify_all();
    }

    pub fn notify_name_changed(&self, nzb_id: i32, host_ctx: *mut c_void, host_cb: DupeHostFn) {
        let opts = read_host_opts(host_ctx, host_cb);
        if opts.dupe_search && !opts.stopped {
            self.schedule(nzb_id, opts.now + i64::from(opts.delay_sec));
        }
    }

    pub(crate) fn download_queue_update(
        &self,
        action_kind: i32,
        dq_ptr: usize,
        added_ptr: NzbRef,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
    ) {
        // action_kind: 1 = eaNzbAdded, 2 = eaNzbNamed
        if (action_kind != 1 && action_kind != 2) || added_ptr.is_none() || dq_ptr == 0 {
            return;
        }
        let opts = read_host_opts(host_ctx, host_cb);
        // Option DupeWatch: without DupeSearch the queued backups of a key
        // are still looked after (FORCE to SCORE, dead pick, swap): see
        // `prepare_native`.
        if !(opts.dupe_search || (opts.dupe_check && opts.dupe_watch)) || opts.stopped {
            return;
        }
        let added = inspect_nzb(host_ctx, host_cb, added_ptr, false);
        let due = opts.now + i64::from(opts.delay_sec);
        self.schedule(added.id, due);

        // A fleet backup added after the first upkeep pass also wakes the
        // queued primary, so it checks the new backup (and can swap to it);
        // fleet_append has already scored it, so it needs no rerank.
        let fleet_backup = added.has_dupe_fleet;
        if action_kind == 1
            && added.dupe_mode == DupeMode::Score
            && (fleet_backup || !added.is_donor)
        {
            let added_key = lower_key(&effective_key(&added.dupe_key, &added.name));
            let (q_len, _) = dq_counts(host_ctx, host_cb, dq_ptr);
            let mut parent: Option<(i32, i32)> = None;
            for i in 0..q_len {
                let item_ptr = dq_queue_at(host_ctx, host_cb, dq_ptr, i);
                if item_ptr.is_none() {
                    continue;
                }
                let item = inspect_nzb(host_ctx, host_cb, item_ptr, false);
                let item_key = lower_key(&effective_key(&item.dupe_key, &item.name));
                if dupesearch_is_better_parent_candidate(
                    item_ptr == added_ptr,
                    item.kind,
                    0,
                    item.deleting,
                    item.delete_status,
                    item.has_post_info,
                    item_key == added_key,
                    item.dupe_score,
                    added.dupe_score,
                    parent.is_some(),
                    parent.map(|p| p.1).unwrap_or(0),
                ) {
                    parent = Some((item.id, item.dupe_score));
                }
            }
            if let Some((parent_id, _)) = parent {
                let fresh_now = host_now(host_ctx, host_cb);
                let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                if !fleet_backup {
                    g.rank_pending.insert(parent_id);
                }
                g.pending
                    .insert(parent_id, fresh_now + i64::from(opts.delay_sec));
                self.cond.notify_all();
            }
        }
    }

    fn scan_queue(&self, host_ctx: *mut c_void, host_cb: DupeHostFn) {
        let ids = with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
            let opts = read_host_opts(host_ctx, host_cb);
            let (q_len, h_len) = dq_counts(host_ctx, host_cb, dq_ptr);
            let mut found_ids = Vec::new();
            for i in 0..q_len {
                let p = dq_queue_at(host_ctx, host_cb, dq_ptr, i);
                if p.is_some() {
                    let s = inspect_nzb(host_ctx, host_cb, p, false);
                    if s.kind == 0 {
                        found_ids.push(s.id);
                    }
                }
            }
            let recent = opts.now - RECENT_FAILURE_SEC;
            for i in 0..h_len {
                let (_, h_kind, h_time, p) = dq_history_at(host_ctx, host_cb, dq_ptr, i);
                if h_kind == 1 && h_time >= recent && p.is_some() {
                    let s = inspect_nzb(host_ctx, host_cb, p, false);
                    if !s.dupe_success {
                        found_ids.push(s.id);
                    }
                }
            }
            found_ids
        });
        for id in ids {
            let opts = read_host_opts(host_ctx, host_cb);
            self.schedule(id, opts.now + i64::from(opts.delay_sec));
        }
    }

    fn collect(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        dq_ptr: usize,
        pick: &NzbSnap,
        job: &mut SearchJob,
    ) {
        let key = effective_key(&pick.dupe_key, &pick.name);
        let low_key = lower_key(&key);
        job.nzb_id = pick.id;
        job.name = pick.name.clone();
        job.dupe_key = key;
        job.category = pick.category.clone();
        job.queued_file = pick.queued_filename.clone();

        let mut consider_item = |item: &NzbSnap, in_history: bool| {
            let item_low = lower_key(&effective_key(&item.dupe_key, &item.name));
            let same_key = item_low == low_key;
            // Members of one fleet (appendfleet) hold the same content under
            // differing release names: the client matched them.
            let same_rel = crate::releasename::same_release(&pick.name, &item.name)
                || (same_key && pick.has_dupe_fleet && item.has_dupe_fleet);
            let qf = &item.queued_filename;
            let add_known = item.kind == 0
                && !qf.is_empty()
                && item.delete_status != DeleteStatus::Manual
                && (same_key || same_rel);
            let add_member = if !in_history {
                dupesearch_is_rank_only_queued_member(
                    job.rank_only,
                    item.ptr == pick.ptr,
                    item.kind,
                    0,
                    item.dupe_mode,
                    item.has_post_info,
                    item.delete_status,
                    item.dupe_score,
                    pick.dupe_score,
                ) && same_key
                    && same_rel
                    && !qf.is_empty()
                    && !qf.contains(&b'|')
            } else {
                item.ptr != pick.ptr
                    && saved_history_backup(
                        true,
                        pick.full_hash,
                        item.delete_status,
                        item.full_hash,
                    )
                    && same_key
                    && same_rel
                    && !qf.is_empty()
                    && !qf.contains(&b'|')
            };
            if add_known {
                fleet_visit_pipe_names(qf, |part| {
                    job.known_files.push(part.to_vec());
                });
            }
            if add_member {
                job.members.push((item.id, qf.clone()));
                job.member_scores.push(item.dupe_score);
                let qh = crate::ffi::queue_host();
                job.member_locked.push(
                    qh.as_ref()
                        .is_some_and(|qh| crate::dupecoord::locked_host(qh, item.ptr)),
                );
                job.member_password.push(qh.as_ref().is_some_and(|qh| {
                    qh.nzb_get_param(item.ptr, crate::donorkind::PASSWORD_PARAM)
                        .is_some_and(|p| !p.trim_ascii().is_empty())
                }));
            }
        };

        let (q_len, h_len) = dq_counts(host_ctx, host_cb, dq_ptr);
        for i in 0..q_len {
            let p = dq_queue_at(host_ctx, host_cb, dq_ptr, i);
            if p.is_some() {
                let item = inspect_nzb(host_ctx, host_cb, p, false);
                consider_item(&item, false);
            }
        }
        for i in 0..h_len {
            let (_, h_kind, _, p) = dq_history_at(host_ctx, host_cb, dq_ptr, i);
            if h_kind == 1 && p.is_some() {
                let item = inspect_nzb(host_ctx, host_cb, p, false);
                consider_item(&item, true);
            }
        }
    }

    fn prepare_native(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        dq_ptr: usize,
        nzb_id: i32,
        job: &mut SearchJob,
    ) -> bool {
        let opts = read_host_opts(host_ctx, host_cb);
        if !opts.dupe_check || !(opts.dupe_search || opts.dupe_watch) {
            return false;
        }
        // Option DupeWatch without DupeSearch: nothing is searched, a pick is only ranked
        // against the backups already queued or in history under its own
        // duplicate key, which needs a key.
        let upkeep_only = !opts.dupe_search;
        let Some((pick_ptr, history)) = dq_find_nzb(host_ctx, host_cb, dq_ptr, nzb_id) else {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.rank_pending.remove(&nzb_id);
            return false;
        };
        let mut pick = inspect_nzb(host_ctx, host_cb, pick_ptr, false);
        if upkeep_only && pick.dupe_key.is_empty() {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.rank_pending.remove(&nzb_id);
            return false;
        }
        let mut key = effective_key(&pick.dupe_key, &pick.name);
        let mut low_key = lower_key(&key);
        let mut owner_prefix = nzb_id.to_string().into_bytes();
        owner_prefix.push(b':');
        let mut expected_owned = owner_prefix.clone();
        expected_owned.extend_from_slice(&key);
        let generated_key = pick.dupe_key.is_empty()
            || (key.starts_with(b"dupes:")
                && pick.owned_key_param.as_deref() == Some(expected_owned.as_slice()));

        let mut previous = SearchedStamp::default();
        let mut have_previous = false;
        let mut cached_name = false;
        let rank_requested;
        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(found) = g.searched.get(&low_key) {
                previous = found.clone();
                have_previous = true;
                if previous.title.is_empty() && generated_key && key.starts_with(b"dupes:") {
                    previous.title = key[6..].to_vec();
                }
                cached_name = !previous.title.is_empty() && previous.raw_name == pick.name;
            }
            rank_requested = g.rank_pending.remove(&nzb_id);
        }

        let title = if cached_name {
            previous.title.clone()
        } else if pick.dupe_key.is_empty() {
            key[6..].to_vec()
        } else {
            crate::releasename::normalize(&pick.name)
        };
        let mut changed_identity =
            have_previous && !previous.title.is_empty() && previous.title != title;
        let mut owned_key_changed = false;
        let mut wanted_owned_key = b"dupes:".to_vec();
        wanted_owned_key.extend_from_slice(&title);
        if generated_key
            && !pick.dupe_key.is_empty()
            && key.starts_with(b"dupes:")
            && key != wanted_owned_key
        {
            key = wanted_owned_key;
            low_key = lower_key(&key);
            owned_key_changed = true;
        }

        let mut rank_only;
        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(found_copy) = g.find_search_cloned(&low_key, &title) {
                previous = found_copy;
                have_previous = true;
                changed_identity = false;
                if !previous.title.is_empty() {
                    previous.raw_name = pick.name.clone();
                }
                cached_name = !previous.title.is_empty();
                if let Some(slot) = g.find_search_mut(&low_key, &title) {
                    *slot = previous.clone();
                }
                g.searched.insert(low_key.clone(), previous.clone());
                rank_only = rank_requested && previous.title == title;
            } else {
                have_previous = false;
                cached_name = false;
                rank_only = false;
            }
        }
        if upkeep_only {
            rank_only = true;
        }

        if owned_key_changed {
            rank_only = upkeep_only;
            if !opts.dry_run {
                let mut owned_val = owner_prefix.clone();
                owned_val.extend_from_slice(&key);
                let key_c = c_bytes(&key);
                let owned_c = c_bytes(&owned_val);
                unsafe {
                    host_cb(
                        host_ctx,
                        6,
                        0,
                        c_int::from(history),
                        dq_ptr,
                        pick.ptr.addr(),
                        key_c.as_ptr() as *const c_char,
                        owned_c.as_ptr() as *const c_char,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null(),
                        0,
                    );
                }
                pick.dupe_key = key.clone();
                pick.owned_key_param = Some(owned_val);
            }
        }

        let flags_of = |item: &NzbSnap, in_hist: bool| -> u32 {
            build_item_flags(
                item.kind,
                in_hist,
                item.dupe_success,
                item.deleting,
                item.delete_status,
                item.has_post_info,
                item.dupe_mode,
                item.is_donor,
                item.dupe_hint,
                item.has_dupe_fleet,
            )
        };

        let mut pick_flags = flags_of(&pick, history);
        // A fleet (appendfleet) is left out of the duplicate search, as the
        // client searched, and its members carry DupeAlive like donors. With
        // DupeSearch off its queued copy still gets the upkeep (dead pick,
        // swap); history members stay skipped as filed duplicates.
        let fleet_upkeep = upkeep_only && pick.has_dupe_fleet;
        if fleet_upkeep {
            pick_flags &= !(MANAGED_FLEET | crate::dupewatch::DONOR);
        }
        // DupeMode FORCE or ALL turns off failover: a pick DupeSearch looks
        // after is switched to SCORE once it is searched.
        let switch_to_score = crate::dupewatch::wants_score_mode(pick_flags);
        if switch_to_score {
            pick_flags |= crate::dupewatch::SCORE_MODE;
        }
        let now = host_now(host_ctx, host_cb);
        let preliminary_flags = pick_flags;
        let mut preliminary: Option<Decision> = None;
        if cached_name && have_previous && !previous.retryable && !rank_only {
            let dec = decide(
                pick_flags,
                pick.dupe_score,
                None,
                now,
                Some(SearchStamp {
                    score: previous.score,
                    time: previous.time,
                }),
                false,
            );
            if dec == Decision::Skip {
                return false;
            }
            if dec == Decision::RetryPostProcess {
                let retry_delay = POST_PROCESS_RETRY_SEC.max(i64::from(opts.delay_sec));
                self.schedule(nzb_id, now + retry_delay);
                return false;
            }
            preliminary = Some(dec);
        }

        let mut others = Vec::new();
        let (q_len, h_len) = dq_counts(host_ctx, host_cb, dq_ptr);
        for i in 0..q_len {
            let p = dq_queue_at(host_ctx, host_cb, dq_ptr, i);
            if p.is_some() && p != pick.ptr {
                let s = inspect_nzb(host_ctx, host_cb, p, false);
                if s.dupe_score > pick.dupe_score || s.has_dupe_fleet {
                    others.push((s, false));
                }
            }
        }
        for i in 0..h_len {
            let (_, h_kind, _, p) = dq_history_at(host_ctx, host_cb, dq_ptr, i);
            if h_kind == 1 && p.is_some() && p != pick.ptr {
                let s = inspect_nzb(host_ctx, host_cb, p, false);
                if s.dupe_score > pick.dupe_score || s.has_dupe_fleet {
                    others.push((s, true));
                }
            }
        }

        let peer_keys: Vec<Vec<u8>> = others
            .iter()
            .map(|(s, _)| lower_key(&effective_key(&s.dupe_key, &s.name)))
            .collect();
        let mut peer_flags_vec = Vec::with_capacity(others.len());
        for (idx, (other, in_hist)) in others.iter().enumerate() {
            let f = flags_of(other, *in_hist);
            if is_live_fleet_peer(&peer_keys[idx], &low_key, f) {
                pick_flags |= MANAGED_FLEET;
            }
            peer_flags_vec.push(f);
        }
        if fleet_upkeep {
            pick_flags &= !MANAGED_FLEET;
        }

        let decision = if preliminary == Some(Decision::Search)
            && others.is_empty()
            && pick_flags == preliminary_flags
        {
            Decision::Search
        } else {
            let prev_stamp =
                if have_previous && !previous.retryable && !rank_only && !changed_identity {
                    Some(SearchStamp {
                        score: previous.score,
                        time: previous.time,
                    })
                } else {
                    None
                };
            let prelim = decide(
                pick_flags,
                pick.dupe_score,
                None,
                now,
                prev_stamp,
                changed_identity,
            );
            if prelim != Decision::Search || others.is_empty() {
                prelim
            } else {
                let mine = Snapshot {
                    id: nzb_id,
                    score: pick.dupe_score,
                    flags: pick_flags,
                    key: &low_key,
                };
                let has_higher = others.iter().enumerate().any(|(idx, (other, _))| {
                    other.id != nzb_id
                        && other.dupe_score > pick.dupe_score
                        && higher_peer(
                            mine,
                            Snapshot {
                                id: other.id,
                                score: other.dupe_score,
                                flags: peer_flags_vec[idx],
                                key: &peer_keys[idx],
                            },
                            true,
                        )
                });
                if has_higher {
                    Decision::Skip
                } else {
                    Decision::Search
                }
            }
        };

        if decision == Decision::RetryPostProcess {
            if rank_only {
                let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                g.rank_pending.insert(nzb_id);
            }
            let retry_delay = POST_PROCESS_RETRY_SEC.max(i64::from(opts.delay_sec));
            self.schedule(nzb_id, now + retry_delay);
            return false;
        }
        if decision != Decision::Search {
            return false;
        }

        let score = pick.dupe_score.max(opts.primary_score);
        if !rank_only {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.record_search(
                &low_key,
                SearchedStamp {
                    score,
                    time: now,
                    title,
                    retryable: false,
                    raw_name: pick.name.clone(),
                },
            );
        }
        if !opts.dry_run && !rank_only {
            let _ = self.save_state(host_ctx, host_cb);
        }
        if switch_to_score {
            let mode = crate::dupewatch::dupe_mode_name(pick.dupe_mode);
            let mut msg = log_tag().to_vec();
            push_c_slice(&mut msg, &pick.name);
            if opts.dry_run {
                msg.extend_from_slice(b": dry run, would set DupeMode ");
                msg.extend_from_slice(mode);
                msg.extend_from_slice(b" to SCORE");
            } else {
                msg.extend_from_slice(b": DupeMode ");
                msg.extend_from_slice(mode);
                msg.extend_from_slice(b" set to SCORE, so it can fail over to a backup");
                if let Some(qh) = crate::ffi::queue_host() {
                    qh.nzb_set_dupe_mode(pick.ptr, DupeMode::Score);
                    qh.nzb_set_int(pick.ptr, crate::queuesubsys_host::NZB_INT_CHANGED, 1);
                    if history {
                        qh.dq_history_changed(dq_ptr);
                        qh.dq_save(dq_ptr);
                    } else {
                        qh.dq_save_changed(dq_ptr);
                    }
                }
                pick.dupe_mode = DupeMode::Score;
            }
            host_log(host_ctx, host_cb, MK_INFO, &msg);
        }
        if !opts.dry_run && !history {
            let key_c = c_bytes(&key);
            let owned_c = if generated_key {
                let mut ov = owner_prefix;
                ov.extend_from_slice(&key);
                Some(c_bytes(&ov))
            } else {
                None
            };
            unsafe {
                host_cb(
                    host_ctx,
                    6,
                    0,
                    2, // !history (bit 0 = 0), bump_base_score (bit 1 = 2)
                    dq_ptr,
                    pick.ptr.addr(),
                    key_c.as_ptr() as *const c_char,
                    owned_c
                        .as_ref()
                        .map_or(ptr::null(), |v| v.as_ptr() as *const c_char),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                    0,
                );
            }
            pick.dupe_key = key.clone();
            if pick.dupe_score < opts.primary_score {
                pick.dupe_score = opts.primary_score;
            }
        }

        job.rank_only = rank_only;
        self.collect(host_ctx, host_cb, dq_ptr, &pick, job);
        job.dupe_key = key.clone();
        job.score = score;
        job.failed = history;
        job.rank_only = rank_only;

        let mut msg = if upkeep_only {
            b"Dupes: checking the backups of ".to_vec()
        } else {
            b"DupeSearch: searching duplicates of ".to_vec()
        };
        push_c_slice(&mut msg, &pick.name);
        msg.extend_from_slice(b" (key ");
        push_c_slice(&mut msg, &key);
        msg.push(b')');
        if upkeep_only {
            // nothing searched: the backups are only ranked
        } else if rank_only {
            msg.extend_from_slice(b": ranking new backups");
        } else if history {
            msg.extend_from_slice(b": it failed before its search was due");
        }
        let msg_c = truncate_note(msg);
        unsafe {
            host_cb(
                host_ctx,
                6,
                1,
                MK_INFO,
                dq_ptr,
                pick.ptr.addr(),
                msg_c.as_ptr() as *const c_char,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
                0,
            );
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn expand(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        job: &SearchJob,
        pick: &NzbSummaryData,
        prior: &[NewznabResult],
        known: &[Vec<u32>],
        deadline: i64,
        budget: &FetchBudget,
        fetched: &mut Vec<NzbFetched>,
        attempted: &mut BTreeSet<Vec<u8>>,
        stats: &mut NzbFetcherStats,
        query_stats: &mut NewznabSearchStats,
        results_count: &mut i32,
        candidates_count: &mut i32,
        postings_count: &mut i32,
    ) {
        let opts = read_host_opts(host_ctx, host_cb);
        if opts.stopped || opts.now >= deadline || budget.spent() || pick.message_ids.is_empty() {
            return;
        }
        let servers = nntp_health_servers(host_ctx, host_cb);
        if servers.is_empty() {
            return;
        }
        let make_health_options = |ctx: *mut c_void, cb: DupeHostFn| -> DonorOptions {
            let cur = read_host_opts(ctx, cb);
            DonorOptions {
                sample: DonorSampleOptions {
                    percent: cur.health_percent,
                    minimum: cur.health_min,
                    maximum: cur.health_max,
                    body_percent: cur.body_percent,
                    max_body: cur.body_checks,
                    seed: 0,
                },
                probe: 10,
                budget_ms: dupesearch_calc_expand_budget_ms(
                    cur.health_budget_sec,
                    deadline,
                    cur.now,
                ),
                deadline_ms: 0,
            }
        };
        let check_summary = |info: &NzbSummaryData| -> DonorHealthResult {
            let ids = summary_ids_strings(&info.message_ids);
            let groups = summary_groups_arc(info);
            donor_check_posting(
                &servers,
                &ids,
                &groups,
                &make_health_options(host_ctx, host_cb),
                true,
            )
        };

        let mut alive = -1.0;
        let mut doomed = false;
        let mut missing = 0i32;
        if !job.failed {
            let health = check_summary(pick);
            alive = effective_alive(
                health.present,
                health.missing,
                health.error,
                pick.listed_parts,
                pick.declared_parts,
            );
            let combined = i64::from(health.missing)
                + i64::from(unlisted_missing(
                    health.answered(),
                    pick.listed_parts,
                    pick.declared_parts,
                ));
            missing = combined.min(i64::from(i32::MAX)) as i32;
            if alive >= 0.85
                && !pick.data_message_ids.is_empty()
                && !pick.par_index.id.is_empty()
                && !pick.recovery_bytes.is_empty()
            {
                let mut data = pick.clone();
                data.message_ids = pick.data_message_ids.clone();
                data.listed_parts = pick.data_listed_parts;
                data.declared_parts = pick.data_declared_parts;
                let data_health = check_summary(&data);
                let data_alive = effective_alive(
                    data_health.present,
                    data_health.missing,
                    data_health.error,
                    data.listed_parts,
                    data.declared_parts,
                );
                let data_missing = i64::from(data_health.missing)
                    + i64::from(unlisted_missing(
                        data_health.answered(),
                        data.listed_parts,
                        data.declared_parts,
                    ));
                if data_alive >= 0.0 && data_missing >= i64::from(DONOR_MIN_KNOWN) {
                    doomed = par_doomed_via_host(host_ctx, host_cb, pick, data_alive, deadline);
                }
            }
        }
        if !(job.failed
            || doomed
            || (alive.is_finite() && (0.0..0.85).contains(&alive) && missing >= 5))
        {
            return;
        }

        let mut probed: BTreeSet<Vec<u8>> = BTreeSet::new();
        let pick_fp = pick.fingerprint();
        let is_whole = |info: &NzbSummaryData, probed_set: &mut BTreeSet<Vec<u8>>| -> bool {
            let fp = info.fingerprint();
            if fp == pick_fp || same_posting_vecs(&pick.message_ids, &info.message_ids) {
                return false;
            }
            let cur = read_host_opts(host_ctx, host_cb);
            if cur.now >= deadline || cur.stopped || !probed_set.insert(fp) {
                return false;
            }
            let h = check_summary(info);
            effective_alive(
                h.present,
                h.missing,
                h.error,
                info.listed_parts,
                info.declared_parts,
            ) >= 0.999
        };

        for posting in fetched.iter() {
            let sketch =
                crate::posting::make_sketch(posting.info.message_ids.iter().map(|v| v.as_slice()));
            let now_s = host_now(host_ctx, host_cb);
            if self.dead.is_dead(&sketch, now_s)
                || known
                    .iter()
                    .any(|k| crate::posting::same_sketch(&sketch, k))
            {
                continue;
            }
            if (!crate::releasename::readable(&posting.info.main_name)
                || crate::releasename::same_release(&job.name, &posting.info.main_name))
                && is_whole(&posting.info, &mut probed)
            {
                return;
            }
        }

        let member_available = |member_id: i32| -> bool {
            with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
                let cur = read_host_opts(host_ctx, host_cb);
                let Some((p_ptr, _)) = dq_find_nzb(host_ctx, host_cb, dq_ptr, job.nzb_id) else {
                    return false;
                };
                let cur_pick = inspect_nzb(host_ctx, host_cb, p_ptr, false);
                check_placement_available(
                    host_ctx,
                    host_cb,
                    dq_ptr,
                    Some(&cur_pick),
                    member_id,
                    true,
                    cur.health_check_mode == 4,
                )
            })
        };

        for (member_id, member_path) in &job.members {
            if !member_available(*member_id) {
                continue;
            }
            if let Some(info) = parse_nzb_summary(&dupe_read_all_bytes(member_path)) {
                let fp = info.fingerprint();
                if is_whole(&info, &mut probed) {
                    if member_available(*member_id) {
                        return;
                    }
                    probed.remove(&fp);
                }
            }
        }

        let cur = read_host_opts(host_ctx, host_cb);
        if cur.stopped || budget.spent() || cur.now >= deadline {
            return;
        }

        let imdb = pick
            .meta
            .get(b"imdb".as_slice())
            .map(|v| v.as_slice())
            .unwrap_or(b"");
        let tvdb = pick
            .meta
            .get(b"tvdb".as_slice())
            .map(|v| v.as_slice())
            .unwrap_or(b"");
        let strict = build_newznab_queries(&job.name, imdb, tvdb);
        let mut queries: Vec<Vec<(Vec<u8>, Vec<u8>)>> = Vec::new();
        crate::dupesearch_nzb::dupesearch_visit_expand_queries(&job.name, |q| {
            let duplicate = strict.iter().any(|params| {
                params
                    .iter()
                    .any(|(k, v)| k.as_slice() == b"q" && v.as_slice() == q)
            });
            if !duplicate {
                queries.push(vec![
                    (b"t".to_vec(), b"search".to_vec()),
                    (b"q".to_vec(), q.to_vec()),
                ]);
            }
        });

        let (wider, s_stats) = newznab_search(
            host_ctx,
            host_cb,
            &cur.search_url,
            &cur.api_key,
            &queries,
            deadline,
        );
        query_stats.queries += s_stats.queries;
        query_stats.failed += s_stats.failed;
        query_stats.pages += s_stats.pages;

        let mut all = prior.to_vec();
        let mut seen: BTreeSet<Vec<u8>> = prior.iter().map(|r| r.link.clone()).collect();
        for r in wider {
            if seen.insert(r.link.clone()) {
                *results_count += 1;
                all.push(r);
            }
        }

        let bytes = summary_whole_bytes_i64(pick);
        let accepted: Vec<NewznabResult> = all
            .into_iter()
            .filter(|r| {
                !attempted.contains(&r.link)
                    && crate::dupeidentity::candidate_ok(
                        &job.name,
                        bytes.max(0) as u64,
                        &r.title,
                        r.size.max(0) as u64,
                        f64::from(opts.size_tolerance) / 100.0,
                        true,
                    )
            })
            .collect();
        *candidates_count += accepted.len() as i32;

        let listings_view: Vec<crate::posting::Listing<'_>> = accepted
            .iter()
            .map(|r| crate::posting::Listing {
                link: &r.link,
                size: r.size,
                grabs: r.grabs,
                date: r.date,
            })
            .collect();
        let order_idx = crate::posting::order_postings(&listings_view, bytes, cur.max_donors);
        *postings_count += order_idx.len() as i32;

        let mut msg = log_tag().to_vec();
        push_c_slice(&mut msg, &job.name);
        msg.extend_from_slice(
            format!(
                ": failing pick has no whole backup; broader discovery ({} candidate(s))",
                accepted.len()
            )
            .as_bytes(),
        );
        self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);

        let mut mine: Option<crate::dupeidentity::Identity> = None;
        let mut read_mine = false;
        for group_indices in order_idx {
            let c_opts = read_host_opts(host_ctx, host_cb);
            if c_opts.stopped || budget.spent() || c_opts.now >= deadline {
                break;
            }
            let group: Vec<NewznabResult> = group_indices
                .into_iter()
                .map(|i| accepted[i].clone())
                .collect();
            for r in &group {
                attempted.insert(r.link.clone());
            }
            let mut counts = NzbFetcherStats::default();
            let mut posting = self.fetch_posting_group(
                host_ctx,
                host_cb,
                &group,
                deadline,
                &mut counts,
                Some(budget),
            );
            stats.add(&counts);
            if posting.reason != NzbFetchReason::Ok
                || !outer_compatible(&job.name, &posting.info.main_name)
            {
                continue;
            }
            if crate::releasename::same_release(&job.name, &posting.listing.title)
                && (!crate::releasename::readable(&posting.info.main_name)
                    || crate::releasename::same_release(&job.name, &posting.info.main_name))
            {
                fetched.push(posting);
                continue;
            }
            if !read_mine {
                mine = read_identity_via_host(host_ctx, host_cb, pick, deadline);
                read_mine = true;
            }
            let theirs = read_identity_via_host(host_ctx, host_cb, &posting.info, deadline);
            let mine_ref = identity_present(&mine);
            let theirs_ref = identity_present(&theirs);
            if !crate::dupeidentity::lenient_ok(
                &job.name,
                &posting.info.main_name,
                mine_ref,
                theirs_ref,
            ) {
                continue;
            }
            posting.lenient = true;
            posting.same_inner = match (mine_ref, theirs_ref) {
                (Some(m), Some(t)) => m.bytes == t.bytes && m.extension == t.extension,
                _ => false,
            };
            fetched.push(posting);
        }
    }

    fn fetch_posting_group(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        listings: &[NewznabResult],
        deadline: i64,
        stats: &mut NzbFetcherStats,
        budget: Option<&FetchBudget>,
    ) -> NzbFetched {
        let ctx_addr = host_ctx as usize;
        self.fetcher.fetch_posting_with(
            listings,
            deadline,
            stats,
            budget,
            move || host_now(ctx_addr as *mut c_void, host_cb),
            donor_now_ms,
            http_stopped,
            || std::thread::sleep(Duration::from_millis(100)),
            move |url, label, max_bytes, dl_ms| {
                http_fetch(
                    ctx_addr as *mut c_void,
                    host_cb,
                    url,
                    label,
                    max_bytes,
                    dl_ms,
                )
            },
            move |msg| host_log(ctx_addr as *mut c_void, host_cb, MK_INFO, msg),
            move |msg| host_log(ctx_addr as *mut c_void, host_cb, MK_WARNING, msg),
        )
    }

    fn search(&self, host_ctx: *mut c_void, host_cb: DupeHostFn, job: &SearchJob) {
        let opts = read_host_opts(host_ctx, host_cb);
        let deadline = opts.now + SEARCH_DEADLINE_SEC;

        let mut pick = NzbSummaryData::default();
        if !job.queued_file.is_empty() {
            match parse_nzb_summary(&dupe_read_all_bytes(&job.queued_file)) {
                Some(parsed) => pick = parsed,
                None => {
                    let mut msg = b"DupeSearch: could not read the nzb-file of ".to_vec();
                    push_c_slice(&mut msg, &job.name);
                    self.note(host_ctx, host_cb, job.nzb_id, MK_DETAIL, &msg);
                }
            }
        }
        if job.rank_only {
            let mut verified = Vec::new();
            let mut rejected = BTreeMap::new();
            self.place(
                host_ctx,
                host_cb,
                job,
                &pick,
                &mut verified,
                &mut rejected,
                0,
                0,
                0,
            );
            return;
        }

        let budget = FetchBudget::from_max_donors(opts.max_donors.max(0) as usize);
        let mut attempted: BTreeSet<Vec<u8>> = BTreeSet::new();
        let imdb = pick
            .meta
            .get(b"imdb".as_slice())
            .map(|v| v.as_slice())
            .unwrap_or(b"");
        let tvdb = pick
            .meta
            .get(b"tvdb".as_slice())
            .map(|v| v.as_slice())
            .unwrap_or(b"");

        let queries = build_newznab_queries(&job.name, imdb, tvdb);
        let (results, mut stats) = newznab_search(
            host_ctx,
            host_cb,
            &opts.search_url,
            &opts.api_key,
            &queries,
            deadline,
        );

        let pick_bytes = summary_whole_bytes_i64(&pick).max(0) as u64;
        let tolerance = f64::from(opts.size_tolerance) / 100.0;
        let candidates: Vec<NewznabResult> = results
            .iter()
            .filter(|r| {
                crate::dupeidentity::candidate_ok(
                    &job.name,
                    pick_bytes,
                    &r.title,
                    r.size.max(0) as u64,
                    tolerance,
                    false,
                )
            })
            .cloned()
            .collect();

        let mut info_msg = log_tag().to_vec();
        push_c_slice(&mut info_msg, &job.name);
        info_msg.extend_from_slice(
            format!(
                ": {} result(s) from {} search(es) ({} failed, {} page(s)), {} of them the same release",
                results.len(),
                stats.queries,
                stats.failed,
                stats.pages,
                candidates.len()
            )
            .as_bytes(),
        );
        self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &info_msg);

        let order_bytes = summary_whole_bytes_i64(&pick);
        let listings_view: Vec<crate::posting::Listing<'_>> = candidates
            .iter()
            .map(|r| crate::posting::Listing {
                link: &r.link,
                size: r.size,
                grabs: r.grabs,
                date: r.date,
            })
            .collect();
        let order_indices =
            crate::posting::order_postings(&listings_view, order_bytes, opts.max_donors);
        let order: Vec<Vec<NewznabResult>> = order_indices
            .into_iter()
            .map(|grp| grp.into_iter().map(|i| candidates[i].clone()).collect())
            .collect();

        let mut known: Vec<Vec<u32>> = Vec::new();
        let mut fetch_stats = NzbFetcherStats::default();
        let mut fetched: Vec<NzbFetched> = Vec::new();
        let mut not_tried = 0i32;
        let parallel = if opts.max_donors > 0 {
            1
        } else {
            FETCH_PARALLEL
        };
        let ctx_addr = host_ctx as usize;

        std::thread::scope(|outer_scope| {
            let known_files = &job.known_files;
            let known_handle = outer_scope.spawn(move || {
                let mut sketches = Vec::new();
                for path in known_files {
                    if let Some(sk) = crate::posting::sketch_of_file(dupe_bytes_to_path(path)) {
                        sketches.push(sk);
                    }
                }
                sketches
            });

            let mut from = 0usize;
            while from < order.len() {
                let cur = read_host_opts(host_ctx, host_cb);
                if cur.stopped || cur.now >= deadline || budget.spent() {
                    not_tried = (order.len() - from) as i32;
                    break;
                }
                let count = parallel.min(order.len() - from);
                for i in 0..count {
                    for listing in &order[from + i] {
                        attempted.insert(listing.link.clone());
                    }
                }
                let chunk_results: Vec<Mutex<(NzbFetched, NzbFetcherStats)>> = (0..count)
                    .map(|_| Mutex::new((NzbFetched::default(), NzbFetcherStats::default())))
                    .collect();
                std::thread::scope(|chunk_scope| {
                    let cr_ref = &chunk_results;
                    let order_ref = &order;
                    let budget_ref = &budget;
                    for i in 0..count {
                        chunk_scope.spawn(move || {
                            let mut st = NzbFetcherStats::default();
                            let f = self.fetch_posting_group(
                                ctx_addr as *mut c_void,
                                host_cb,
                                &order_ref[from + i],
                                deadline,
                                &mut st,
                                Some(budget_ref),
                            );
                            *cr_ref[i].lock().unwrap_or_else(|e| e.into_inner()) = (f, st);
                        });
                    }
                });
                for slot in chunk_results {
                    let (f, st) = slot.into_inner().unwrap_or_else(|e| e.into_inner());
                    fetch_stats.add(&st);
                    if f.reason == NzbFetchReason::Ok {
                        fetched.push(f);
                    }
                }
                from += parallel;
            }

            if let Ok(sketches) = known_handle.join() {
                known = sketches;
            }
        });

        let mut result_count = results.len() as i32;
        let mut candidate_count = candidates.len() as i32;
        let mut posting_count = order.len() as i32;
        self.expand(
            host_ctx,
            host_cb,
            job,
            &pick,
            &results,
            &known,
            deadline,
            &budget,
            &mut fetched,
            &mut attempted,
            &mut fetch_stats,
            &mut stats,
            &mut result_count,
            &mut candidate_count,
            &mut posting_count,
        );

        let mut accepted_postings: Vec<Vec<Vec<u8>>> = vec![pick.message_ids.clone()];
        let mut verified: Vec<NzbFetched> = Vec::new();
        let mut rejected: BTreeMap<Vec<u8>, i32> = BTreeMap::new();
        for posting in fetched.iter() {
            let sketch =
                crate::posting::make_sketch(posting.info.message_ids.iter().map(|v| v.as_slice()));
            let ids = &posting.info.message_ids;
            let main_readable =
                !posting.lenient && crate::releasename::readable(&posting.info.main_name);
            let same_rel = main_readable
                && crate::releasename::same_release(&job.name, &posting.info.main_name);
            if main_readable && !same_rel {
                *rejected.entry(b"other-release".to_vec()).or_insert(0) += 1;
            } else if known
                .iter()
                .any(|k| crate::posting::same_sketch(&sketch, k))
            {
                *rejected.entry(b"in-nzbget".to_vec()).or_insert(0) += 1;
            } else if self.dead.is_dead(&sketch, host_now(host_ctx, host_cb)) {
                *rejected.entry(b"known-dead".to_vec()).or_insert(0) += 1;
            } else if accepted_postings.iter().any(|p| same_posting_vecs(ids, p)) {
                *rejected.entry(b"same-posting".to_vec()).or_insert(0) += 1;
            } else {
                accepted_postings.push(ids.clone());
                verified.push(posting.clone());
            }
        }
        *rejected.entry(b"listing-mismatch".to_vec()).or_insert(0) += fetch_stats.listing_mismatch;
        *rejected.entry(b"refused".to_vec()).or_insert(0) += fetch_stats.refused;
        *rejected.entry(b"relisted".to_vec()).or_insert(0) += fetch_stats.relisted;
        *rejected.entry(b"fetch".to_vec()).or_insert(0) += fetch_stats.fetch;
        *rejected.entry(b"parse".to_vec()).or_insert(0) += fetch_stats.parse;
        *rejected.entry(b"deadline".to_vec()).or_insert(0) += not_tried;
        *rejected.entry(b"budget".to_vec()).or_insert(0) += fetch_stats.budget;

        let retry_reason = dupesearch_classify_search_retry_reason(
            stats.queries,
            stats.failed,
            verified.is_empty(),
            fetched.is_empty(),
            fetch_stats.refused,
            fetch_stats.fetch,
            not_tried,
        );
        if retry_reason != 0 {
            let reason_text: &[u8] = match retry_reason {
                1 => b"no indexer answered",
                2 => b"an indexer query failed and nothing came back",
                _ => b"no posting could be fetched",
            };
            let norm_name = crate::releasename::normalize(&job.name);
            let _ = self.forget_search(host_ctx, host_cb, &job.dupe_key, &norm_name);
            let mut msg = log_tag().to_vec();
            push_c_slice(&mut msg, &job.name);
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(reason_text);
            msg.extend_from_slice(b", the key may be searched again");
            self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
        }

        self.save_pending(host_ctx, host_cb, job, &pick, &verified);
        self.place(
            host_ctx,
            host_cb,
            job,
            &pick,
            &mut verified,
            &mut rejected,
            result_count,
            candidate_count,
            posting_count,
        );
    }

    fn record_dead(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        sketch: Vec<u32>,
        save: bool,
    ) {
        let opts = read_host_opts(host_ctx, host_cb);
        if !self.dead.add(sketch, opts.now, save) {
            let state_path = path_join(&opts.queue_dir, opts.sep, b"dupesearch-dead");
            let mut msg = b"Could not save the DupeSearch dead postings to ".to_vec();
            push_c_slice(&mut msg, &state_path);
            host_log(host_ctx, host_cb, MK_WARNING, &msg);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn place(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        job: &SearchJob,
        pick: &NzbSummaryData,
        verified: &mut Vec<NzbFetched>,
        rejected: &mut BTreeMap<Vec<u8>, i32>,
        results: i32,
        candidates: i32,
        postings: i32,
    ) {
        let pick_bytes = summary_whole_bytes_i64(pick);
        if verified.len() > 1 {
            let tb: Vec<i64> = verified.iter().map(|v| v.info.total_bytes).collect();
            let gr: Vec<i32> = verified.iter().map(|v| v.listing.grabs).collect();
            let dt: Vec<i64> = verified.iter().map(|v| v.listing.date).collect();
            let mut ord = vec![0usize; verified.len()];
            sort_verified_postings(&tb, &gr, &dt, pick_bytes, &mut ord);
            let old = std::mem::take(verified);
            let mut slots: Vec<Option<NzbFetched>> = old.into_iter().map(Some).collect();
            verified.reserve(ord.len());
            for idx in ord {
                if let Some(item) = slots[idx].take() {
                    verified.push(item);
                }
            }
        }

        let servers = if !verified.is_empty() || !job.members.is_empty() {
            let s = nntp_health_servers(host_ctx, host_cb);
            if s.is_empty() {
                let mut msg = log_tag().to_vec();
                push_c_slice(&mut msg, &job.name);
                msg.extend_from_slice(
                    b": no active news servers, the postings are added unchecked",
                );
                self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
            }
            s
        } else {
            Vec::new()
        };

        let init_opts = read_host_opts(host_ctx, host_cb);
        let health_options = DonorOptions {
            sample: DonorSampleOptions {
                percent: init_opts.health_percent,
                minimum: init_opts.health_min,
                maximum: init_opts.health_max,
                body_percent: init_opts.body_percent,
                max_body: init_opts.body_checks,
                seed: 0,
            },
            probe: 10,
            budget_ms: init_opts.health_budget_sec * 1000,
            deadline_ms: 0,
        };

        // TWIN (the same post: matching par2 hashes and article counts) and
        // DonorKind of a copy against the pick, read from the news servers
        // once per copy. The probes share FLEET_KIND_PROBE_MAX_SEC of their
        // own time: the health checks between them do not use it up.
        let kind_budget = std::sync::atomic::AtomicI64::new(FLEET_KIND_PROBE_MAX_SEC);
        let kind_deadline = || {
            let start = now_unix_sec();
            (
                start,
                start
                    + kind_budget
                        .load(std::sync::atomic::Ordering::Relaxed)
                        .max(0),
            )
        };
        let kind_spent = |start: i64| {
            kind_budget.fetch_sub(
                (now_unix_sec() - start).max(0),
                std::sync::atomic::Ordering::Relaxed,
            );
        };
        let kind_ctx = host_ctx as usize;
        let pick_listed =
            if !servers.is_empty() && (!verified.is_empty() || !job.members.is_empty()) {
                let (start, deadline) = kind_deadline();
                let mut probed = probe_copies(host_ctx, host_cb, &[pick], deadline);
                kind_spent(start);
                probed.pop().map(|(listed, _)| listed)
            } else {
                None
            };
        // (twin, kind, archive headers encrypted)
        let copy_kind = |info: &NzbSummaryData| -> (bool, crate::donorkind::DonorKind, bool) {
            use crate::donorkind::{DonorKind, Packing};
            let Some(pick_listed) = pick_listed.as_ref() else {
                return (false, DonorKind::Unknown, false);
            };
            let (start, deadline) = kind_deadline();
            let (listed, packing) =
                probe_copies(kind_ctx as *mut c_void, host_cb, &[info], deadline)
                    .pop()
                    .unwrap_or((crate::twin::Listed::default(), Packing::Unknown));
            kind_spent(start);
            let twin =
                crate::donorkind::twins(pick_listed, &listed, &pick.data_layout, &info.data_layout);
            (
                twin,
                DonorKind::of(twin, packing),
                packing.headers_encrypted(),
            )
        };
        let alive_of = |h: &DonorHealthResult, s: &NzbSummaryData| -> f64 {
            effective_alive(
                h.present,
                h.missing,
                h.error,
                s.listed_parts,
                s.declared_parts,
            )
        };
        let is_dead_summary = |h: &DonorHealthResult, s: &NzbSummaryData, min_alive: i32| -> bool {
            dead_donor(
                h.present,
                h.missing,
                h.error,
                s.listed_parts,
                s.declared_parts,
                min_alive,
            )
        };
        let cacheable_loss = |h: &DonorHealthResult, min_alive: i32| -> bool {
            dead_donor(h.present, h.missing, 0, 0, 0, min_alive)
        };

        #[derive(Clone)]
        struct PlacedItem {
            id: i32,
            entry: crate::donorscore::Entry,
            // a member whose probe found its archive headers encrypted: the
            // rescore records it (DupeLocked) for failover and the rescue
            encrypted: bool,
            title: Vec<u8>,
            member: bool,
            valid: bool,
            old_score: i32,
        }

        struct PlaceMutable {
            dead: i32,
            rescore_failed: i32,
            added: i32,
            placed: BTreeMap<usize, PlacedItem>,
            deferred: BTreeMap<usize, f64>,
            todo: BTreeSet<usize>,
            ranks_used: BTreeSet<i32>,
        }

        let take_rank_set = |used: &mut BTreeSet<i32>, alive: f64, twin: bool| -> i32 {
            let v: Vec<i32> = used.iter().copied().collect();
            let sc = crate::donorscore::take_rank(alive, twin, &v);
            used.insert(sc);
            sc
        };

        let ctx_addr = host_ctx as usize;
        let slots_used = |placed_map: &BTreeMap<usize, PlacedItem>| -> i32 {
            let ctx = ctx_addr as *mut c_void;
            with_guarded_queue(ctx, host_cb, |dq_ptr| {
                let cur = read_host_opts(ctx, host_cb);
                let cur_pick = dq_find_nzb(ctx, host_cb, dq_ptr, job.nzb_id)
                    .map(|(p, _)| inspect_nzb(ctx, host_cb, p, false));
                let mut cnt = 0;
                for item in placed_map.values() {
                    if !item.member && item.valid && item.entry.score > crate::donorscore::DEAD {
                        let avail = if item.id < 0 {
                            cur.dry_run
                        } else {
                            check_placement_available(
                                ctx,
                                host_cb,
                                dq_ptr,
                                cur_pick.as_ref(),
                                item.id,
                                false,
                                cur.health_check_mode == 4,
                            )
                        };
                        if avail {
                            cnt += 1;
                        }
                    }
                }
                cnt
            })
        };

        let cap = init_opts.max_donors;
        let base = crate::donorscore::base(job.score);

        let mut interrupted_early = false;
        let mut final_dead = 0i32;
        let mut final_over_cap = 0i32;
        let mut final_rescore_failed = 0i32;
        let mut final_added = 0i32;
        let mut final_placed: BTreeMap<usize, PlacedItem> = BTreeMap::new();
        let mut unchecked_members: BTreeSet<usize> = BTreeSet::new();
        let mut final_pick_sample: Option<DonorHealthResult> = None;

        std::thread::scope(|place_scope| {
            let servers_ref = &servers;
            let mut pick_check = if !servers.is_empty() && !pick.message_ids.is_empty() {
                let pick_ids = summary_ids_strings(&pick.message_ids);
                let pick_groups = summary_groups_arc(pick);
                Some(place_scope.spawn(move || {
                    let h = donor_check_posting(
                        servers_ref,
                        &pick_ids,
                        &pick_groups,
                        &health_options,
                        false,
                    );
                    if donor_dead_probe(&h) {
                        let dry = read_host_opts(ctx_addr as *mut c_void, host_cb).dry_run;
                        let sk = crate::posting::make_sketch(
                            pick.message_ids.iter().map(|v| v.as_slice()),
                        );
                        self.record_dead(ctx_addr as *mut c_void, host_cb, sk, !dry);
                        let mut msg = log_tag().to_vec();
                        push_c_slice(&mut msg, &job.name);
                        msg.extend_from_slice(
                            format!(
                                ": the pick is dead ({} of {} probe articles on no server)",
                                h.missing,
                                h.answered()
                            )
                            .as_bytes(),
                        );
                        self.note(ctx_addr as *mut c_void, host_cb, job.nzb_id, MK_INFO, &msg);
                    }
                    Some(h)
                }))
            } else {
                None
            };
            // `pick_check` holds the pick's probe until the first donors are
            // placed, then its full sample, which decides the swap.
            let mut pick_sampled = false;

            let probe_mutex = Mutex::new(BTreeMap::<usize, DonorHealthResult>::new());
            if !servers.is_empty() && !verified.is_empty() {
                let list: Vec<DonorPosting> = verified
                    .iter()
                    .enumerate()
                    .map(|(i, v)| DonorPosting {
                        key: i.to_string(),
                        ids: summary_ids_strings(&v.info.message_ids),
                        groups: summary_groups_arc(&v.info),
                    })
                    .collect();
                donor_check_postings(
                    &servers,
                    &list,
                    &health_options,
                    false,
                    init_opts.check_concurrency,
                    |key, h| {
                        if let Ok(idx) = key.parse::<usize>() {
                            probe_mutex
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(idx, *h);
                        }
                    },
                );
            }
            let probe = probe_mutex.into_inner().unwrap_or_else(|e| e.into_inner());

            let mut state = PlaceMutable {
                dead: 0,
                rescore_failed: 0,
                added: 0,
                placed: BTreeMap::new(),
                deferred: BTreeMap::new(),
                todo: BTreeSet::new(),
                ranks_used: BTreeSet::new(),
            };

            let mut live: Vec<usize> = Vec::new();
            for (i, v) in verified.iter().enumerate() {
                if let Some(ph) = probe.get(&i) {
                    if donor_dead_probe(ph) {
                        let dry = read_host_opts(host_ctx, host_cb).dry_run;
                        let sk = crate::posting::make_sketch(
                            v.info.message_ids.iter().map(|m| m.as_slice()),
                        );
                        self.record_dead(host_ctx, host_cb, sk, !dry);
                        state.dead += 1;
                        let mut msg = log_tag().to_vec();
                        push_c_slice(&mut msg, &job.name);
                        msg.extend_from_slice(b": dropping dead posting ");
                        push_c_slice(&mut msg, &v.listing.title);
                        msg.extend_from_slice(b" [");
                        push_c_slice(&mut msg, &v.listing.indexer);
                        msg.extend_from_slice(
                            format!(
                                "] after the probe: {} of {} probe articles on no server",
                                ph.missing,
                                ph.answered()
                            )
                            .as_bytes(),
                        );
                        self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
                        continue;
                    }
                }
                live.push(i);
            }

            let place_one = |st: &mut PlaceMutable, i: usize, alive: f64, how: &[u8]| {
                let ctx = ctx_addr as *mut c_void;
                let (twin, kind, encrypted) = copy_kind(&verified[i].info);
                let locked =
                    crate::donorkind::locked(encrypted, summary_password_known(&verified[i].info));
                let score = take_rank_set(&mut st.ranks_used, alive, twin);
                let id = self.add_donor(
                    ctx,
                    host_cb,
                    job,
                    pick,
                    &verified[i],
                    base + score,
                    alive,
                    kind,
                );
                if id == 0 {
                    st.ranks_used.remove(&score);
                    return;
                }
                st.added += 1;
                st.placed.insert(
                    i,
                    PlacedItem {
                        id,
                        encrypted: false,
                        entry: crate::donorscore::Entry {
                            score,
                            alive,
                            twin,
                            bytes: verified[i].info.total_bytes,
                            grabs: verified[i].listing.grabs,
                            kind: kind as u8,
                            cap: 0,
                            locked,
                        },
                        title: verified[i].listing.title.clone(),
                        member: false,
                        valid: true,
                        old_score: 0,
                    },
                );
                let pct = if alive < 0.0 {
                    -1
                } else {
                    (100.0 * alive).round() as i32
                };
                let mut msg = log_tag().to_vec();
                push_c_slice(&mut msg, &job.name);
                msg.extend_from_slice(b": added ");
                push_c_slice(&mut msg, &verified[i].listing.title);
                msg.extend_from_slice(b" [");
                push_c_slice(&mut msg, &verified[i].listing.indexer);
                msg.extend_from_slice(
                    format!("] score={} alive={}% (", base + score, pct).as_bytes(),
                );
                msg.extend_from_slice(how);
                msg.push(b')');
                self.note(ctx, host_cb, job.nzb_id, MK_INFO, &msg);
            };

            if let Some(probe_handle) = pick_check.take() {
                match probe_handle.join().ok().flatten() {
                    Some(h) if donor_dead_probe(&h) => {
                        self.demote_dead_pick(host_ctx, host_cb, job);
                        // A dead pick fails for sure: a whole backup already
                        // held for it takes over through the swap.
                        final_pick_sample = Some(h);
                    }
                    Some(_) if !job.failed && !host_stopped(host_ctx, host_cb) => {
                        let pick_ids = summary_ids_strings(&pick.message_ids);
                        let pick_groups = summary_groups_arc(pick);
                        pick_sampled = true;
                        pick_check = Some(place_scope.spawn(move || {
                            Some(donor_check_posting(
                                servers_ref,
                                &pick_ids,
                                &pick_groups,
                                &health_options,
                                true,
                            ))
                        }));
                    }
                    _ => {}
                }
            }

            let cur_fast_opt = read_host_opts(host_ctx, host_cb).fast_donors;
            let mut fast = (cur_fast_opt.max(0) as usize).min(live.len());
            if cap > 0 {
                fast = fast.min(cap as usize);
            }
            for &idx in live.iter().take(fast) {
                let a = match probe.get(&idx) {
                    Some(ph) => alive_of(ph, &verified[idx].info),
                    None => -1.0,
                };
                place_one(&mut state, idx, a, b"fast");
            }

            state.todo = live.iter().copied().collect();
            let state_mutex = Mutex::new(state);
            if !servers.is_empty() {
                let list: Vec<DonorPosting> = live
                    .iter()
                    .map(|&i| DonorPosting {
                        key: i.to_string(),
                        ids: summary_ids_strings(&verified[i].info.message_ids),
                        groups: summary_groups_arc(&verified[i].info),
                    })
                    .collect();
                if !list.is_empty() {
                    donor_check_postings(
                        &servers,
                        &list,
                        &health_options,
                        true,
                        read_host_opts(host_ctx, host_cb).check_concurrency,
                        |key, health| {
                            let Ok(i) = key.parse::<usize>() else {
                                return;
                            };
                            let ctx = ctx_addr as *mut c_void;
                            let mut st = state_mutex.lock().unwrap_or_else(|e| e.into_inner());
                            let cur = read_host_opts(ctx, host_cb);
                            if cur.stopped || !st.todo.remove(&i) {
                                return;
                            }
                            let alive = alive_of(health, &verified[i].info);
                            let is_dead = is_dead_summary(health, &verified[i].info, cur.min_alive);
                            let pct = if alive < 0.0 {
                                -1
                            } else {
                                (100.0 * alive).round() as i32
                            };
                            let mut msg = log_tag().to_vec();
                            push_c_slice(&mut msg, &job.name);
                            msg.extend_from_slice(b": ");
                            push_c_slice(&mut msg, &verified[i].listing.title);
                            msg.extend_from_slice(b" [");
                            push_c_slice(&mut msg, &verified[i].listing.indexer);
                            msg.extend_from_slice(
                                format!(
                                    "]: alive={}% ({} of {} articles)",
                                    pct,
                                    health.present,
                                    health.answered()
                                )
                                .as_bytes(),
                            );
                            self.note(ctx, host_cb, job.nzb_id, MK_INFO, &msg);

                            if st.placed.contains_key(&i) {
                                let (donor_id, old_entry_score, donor_twin) = {
                                    let d = &st.placed[&i];
                                    (d.id, d.entry.score, d.entry.twin)
                                };
                                st.ranks_used.remove(&old_entry_score);
                                let new_score = if is_dead {
                                    crate::donorscore::DEAD
                                } else {
                                    take_rank_set(&mut st.ranks_used, alive, donor_twin)
                                };
                                let alive_val = if is_dead {
                                    0
                                } else if alive < 0.0 {
                                    100
                                } else {
                                    (100.0 * alive).round() as i32
                                };
                                let param = format!("DupeAlive={alive_val}").into_bytes();
                                let params_vec = vec![param];
                                if alive < 0.0
                                    || self.set_score(
                                        ctx,
                                        host_cb,
                                        donor_id,
                                        base + new_score,
                                        &params_vec,
                                    )
                                {
                                    if alive < 0.0 {
                                        st.ranks_used.remove(&new_score);
                                        st.ranks_used.insert(old_entry_score);
                                    } else if let Some(d) = st.placed.get_mut(&i) {
                                        d.entry.score = new_score;
                                    }
                                    if let Some(d) = st.placed.get_mut(&i) {
                                        d.entry.alive = alive;
                                    }
                                    if is_dead {
                                        if cacheable_loss(health, cur.min_alive) {
                                            let sk = crate::posting::make_sketch(
                                                verified[i]
                                                    .info
                                                    .message_ids
                                                    .iter()
                                                    .map(|m| m.as_slice()),
                                            );
                                            self.record_dead(ctx, host_cb, sk, !cur.dry_run);
                                        }
                                        st.dead += 1;
                                    }
                                } else {
                                    st.ranks_used.remove(&new_score);
                                    st.ranks_used.insert(old_entry_score);
                                    st.rescore_failed += 1;
                                    if let Some(d) = st.placed.get_mut(&i) {
                                        d.valid = false;
                                    }
                                    let mut wmsg = b"DupeSearch: could not rescore ".to_vec();
                                    push_c_slice(&mut wmsg, &verified[i].listing.title);
                                    wmsg.extend_from_slice(
                                        format!(": keeps score {}", base + old_entry_score)
                                            .as_bytes(),
                                    );
                                    self.note(ctx, host_cb, job.nzb_id, MK_WARNING, &wmsg);
                                }
                            } else if is_dead {
                                if cacheable_loss(health, cur.min_alive) {
                                    let sk = crate::posting::make_sketch(
                                        verified[i].info.message_ids.iter().map(|m| m.as_slice()),
                                    );
                                    self.record_dead(ctx, host_cb, sk, !cur.dry_run);
                                }
                                st.dead += 1;
                                let mut dmsg = log_tag().to_vec();
                                push_c_slice(&mut dmsg, &job.name);
                                dmsg.extend_from_slice(b": dropping dead posting ");
                                push_c_slice(&mut dmsg, &verified[i].listing.title);
                                dmsg.extend_from_slice(
                                    format!(": alive={}%", (100.0 * alive).round() as i32)
                                        .as_bytes(),
                                );
                                self.note(ctx, host_cb, job.nzb_id, MK_INFO, &dmsg);
                            } else if cap > 0 && slots_used(&st.placed) >= cap {
                                st.deferred.insert(i, alive);
                            } else {
                                place_one(&mut st, i, alive, b"checked");
                            }
                        },
                    );
                }
            }

            let mut state = state_mutex.into_inner().unwrap_or_else(|e| e.into_inner());

            let check_interrupted = |pc: &mut Option<
                std::thread::ScopedJoinHandle<'_, Option<DonorHealthResult>>,
            >|
             -> bool {
                if !host_stopped(host_ctx, host_cb) {
                    return false;
                }
                if let Some(h) = pc.take() {
                    let _ = h.join();
                }
                let mut msg = log_tag().to_vec();
                push_c_slice(&mut msg, &job.name);
                msg.extend_from_slice(
                    b": RustyGet shuts down, the search resumes after the restart",
                );
                self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
                true
            };

            if check_interrupted(&mut pick_check) {
                interrupted_early = true;
                return;
            }

            let deferred_keys: Vec<usize> = state.deferred.keys().copied().collect();
            for k in deferred_keys {
                if cap > 0 && slots_used(&state.placed) >= cap {
                    continue;
                }
                if let Some(alive) = state.deferred.remove(&k) {
                    place_one(&mut state, k, alive, b"checked");
                }
            }

            let todo_copy: Vec<usize> = state.todo.iter().copied().collect();
            for i in todo_copy {
                state.todo.remove(&i);
                if state.placed.contains_key(&i) {
                    continue;
                }
                if cap > 0 && slots_used(&state.placed) >= cap {
                    final_over_cap += 1;
                    continue;
                }
                place_one(&mut state, i, -1.0, b"unchecked");
            }

            if !job.members.is_empty() && (!servers.is_empty() || job.rank_only) {
                let mut summaries: Vec<Option<NzbSummaryData>> = vec![None; job.members.len()];
                let mut list: Vec<DonorPosting> = Vec::new();
                let mut same_as: BTreeMap<usize, usize> = BTreeMap::new();
                for m in 0..job.members.len() {
                    unchecked_members.insert(m);
                    let Some(parsed) = parse_nzb_summary(&dupe_read_all_bytes(&job.members[m].1))
                    else {
                        continue;
                    };
                    if same_posting_vecs(&parsed.message_ids, &pick.message_ids) {
                        unchecked_members.remove(&m);
                        summaries[m] = Some(parsed);
                        continue;
                    }
                    if let Some(first) = list
                        .iter()
                        .find(|p| same_posting_str_vec(&parsed.message_ids, &p.ids))
                    {
                        if let Ok(first_idx) = first.key.parse::<usize>() {
                            same_as.insert(m, first_idx);
                        }
                        summaries[m] = Some(parsed);
                        continue;
                    }
                    list.push(DonorPosting {
                        key: m.to_string(),
                        ids: summary_ids_strings(&parsed.message_ids),
                        groups: summary_groups_arc(&parsed),
                    });
                    summaries[m] = Some(parsed);
                }

                let measured_mutex = Mutex::new(BTreeMap::<usize, DonorHealthResult>::new());
                if !servers.is_empty() && !list.is_empty() {
                    donor_check_postings(
                        &servers,
                        &list,
                        &health_options,
                        true,
                        read_host_opts(host_ctx, host_cb).check_concurrency,
                        |key, h| {
                            if let Ok(m_idx) = key.parse::<usize>() {
                                measured_mutex
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .insert(m_idx, *h);
                            }
                        },
                    );
                }
                let mut measured = measured_mutex
                    .into_inner()
                    .unwrap_or_else(|e| e.into_inner());
                for (&alias_idx, &orig_idx) in &same_as {
                    if let Some(&h) = measured.get(&orig_idx) {
                        measured.insert(alias_idx, h);
                    }
                }
                for (m_idx, h) in measured {
                    let Some(summary) = summaries[m_idx].as_ref() else {
                        continue;
                    };
                    let alive = alive_of(&h, summary);
                    if alive < 0.0 {
                        continue;
                    }
                    let cur = read_host_opts(host_ctx, host_cb);
                    let is_dead = is_dead_summary(&h, summary, cur.min_alive);
                    if is_dead && cacheable_loss(&h, cur.min_alive) {
                        let sk = crate::posting::make_sketch(
                            summary.message_ids.iter().map(|v| v.as_slice()),
                        );
                        self.record_dead(host_ctx, host_cb, sk, !cur.dry_run);
                    }
                    let member_title = base_file_name(&job.members[m_idx].1);
                    let old_sc = job.member_scores.get(m_idx).copied().unwrap_or(0);
                    let (member_twin, member_kind, member_encrypted) = copy_kind(summary);
                    let member_password = summary_password_known(summary)
                        || job.member_password.get(m_idx).copied().unwrap_or(false);
                    let member_locked = crate::donorkind::locked(member_encrypted, member_password)
                        || job.member_locked.get(m_idx).copied().unwrap_or(false);
                    let item = PlacedItem {
                        id: job.members[m_idx].0,
                        encrypted: member_encrypted,
                        entry: crate::donorscore::Entry {
                            score: if is_dead {
                                crate::donorscore::DEAD
                            } else {
                                crate::donorscore::DEAD + 1
                            },
                            alive,
                            twin: member_twin,
                            bytes: summary.total_bytes,
                            grabs: 0,
                            kind: member_kind as u8,
                            cap: if (100.0 * alive).round() < 100.0 {
                                old_sc.saturating_sub(base).max(1)
                            } else {
                                0
                            },
                            locked: member_locked,
                        },
                        title: member_title.clone(),
                        member: true,
                        valid: true,
                        old_score: old_sc,
                    };
                    state.placed.insert(verified.len() + m_idx, item);
                    let mut mmsg = log_tag().to_vec();
                    push_c_slice(&mut mmsg, &job.name);
                    mmsg.extend_from_slice(b": duplicate ");
                    push_c_slice(&mut mmsg, &member_title);
                    mmsg.extend_from_slice(
                        format!(
                            " in history: alive={}% ({} of {} articles){}",
                            (100.0 * alive).round() as i32,
                            h.present,
                            h.answered(),
                            if is_dead { ", dead" } else { "" }
                        )
                        .as_bytes(),
                    );
                    self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &mmsg);
                }
            }

            if check_interrupted(&mut pick_check) {
                interrupted_early = true;
                return;
            }

            let mut do_rerank = |st: &mut PlaceMutable| {
                let mut order2 = Vec::new();
                let mut entries = Vec::new();
                for (&k, v) in &st.placed {
                    if !v.valid {
                        continue;
                    }
                    order2.push(k);
                    entries.push(v.entry);
                }
                let wanted = crate::donorscore::rerank(&entries, pick_bytes);
                let all_dead = wanted.iter().all(|&sc| sc == crate::donorscore::DEAD);
                for (n, &slot_key) in order2.iter().enumerate() {
                    let (
                        donor_id,
                        donor_member,
                        donor_old_score,
                        donor_entry_score,
                        donor_alive,
                        donor_title,
                        donor_kind,
                        donor_encrypted,
                    ) = {
                        let d = &st.placed[&slot_key];
                        (
                            d.id,
                            d.member,
                            d.old_score,
                            d.entry.score,
                            d.entry.alive,
                            d.title.clone(),
                            d.entry.kind,
                            d.encrypted,
                        )
                    };
                    if wanted[n] == donor_entry_score && !donor_member {
                        continue;
                    }
                    let mut params: Vec<Vec<u8>> = Vec::new();
                    let mut score = base + wanted[n];
                    if donor_member {
                        let (sc, pct, add_zero) = eval_member_rerank(
                            base,
                            wanted[n],
                            crate::donorscore::DEAD,
                            donor_old_score,
                            donor_alive,
                            all_dead,
                        );
                        score = sc;
                        params.push(format!("DupeHealth={pct}").into_bytes());
                        if donor_kind != crate::donorkind::DonorKind::Unknown as u8 {
                            let mut kp = crate::donorkind::KIND_PARAM.to_vec();
                            kp.push(b'=');
                            kp.extend_from_slice(
                                crate::donorkind::DonorKind::from_u8(donor_kind).param(),
                            );
                            params.push(kp);
                        }
                        if add_zero {
                            params.push(b"DupeAlive=0".to_vec());
                        }
                        if donor_encrypted {
                            let mut lp = crate::donorkind::LOCKED_PARAM.to_vec();
                            lp.extend_from_slice(b"=yes");
                            params.push(lp);
                        }
                    }
                    if self.set_score(host_ctx, host_cb, donor_id, score, &params) {
                        if donor_member {
                            unchecked_members.remove(&(slot_key - verified.len()));
                        }
                        let mut rmsg = log_tag().to_vec();
                        push_c_slice(&mut rmsg, &job.name);
                        rmsg.extend_from_slice(b": reranked ");
                        push_c_slice(&mut rmsg, &donor_title);
                        rmsg.extend_from_slice(format!(": score {score}").as_bytes());
                        self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &rmsg);
                        if let Some(d) = st.placed.get_mut(&slot_key) {
                            d.entry.score = wanted[n];
                        }
                    } else {
                        if let Some(d) = st.placed.get_mut(&slot_key) {
                            d.valid = false;
                        }
                        st.rescore_failed += 1;
                    }
                }
            };

            do_rerank(&mut state);
            while !state.deferred.is_empty() && (cap <= 0 || slots_used(&state.placed) < cap) {
                let first_key = *state.deferred.keys().next().unwrap();
                if let Some(alive) = state.deferred.remove(&first_key) {
                    place_one(&mut state, first_key, alive, b"checked");
                    do_rerank(&mut state);
                }
            }
            final_over_cap += state.deferred.len() as i32;

            if let Some(h) = pick_check.take() {
                let sample = h.join().ok().flatten();
                if pick_sampled {
                    final_pick_sample = sample;
                }
            }

            final_dead = state.dead;
            final_rescore_failed = state.rescore_failed;
            final_added = state.added;
            final_placed = state.placed;
        });

        if interrupted_early {
            return;
        }

        if let Some(h) = final_pick_sample {
            let candidates: Vec<(i32, f64, u8)> = final_placed
                .values()
                .filter(|item| {
                    item.valid && item.id > 0 && item.entry.score > crate::donorscore::DEAD
                })
                .map(|item| {
                    let kind = if item.entry.twin {
                        crate::donorkind::DonorKind::Twin as u8
                    } else {
                        item.entry
                            .kind
                            .max(crate::donorkind::DonorKind::Borrowable as u8)
                    };
                    (item.id, item.entry.alive, kind)
                })
                .collect();
            self.swap_if_failing(
                host_ctx,
                host_cb,
                job,
                pick,
                &h,
                &candidates,
                &servers,
                &health_options,
            );
        }

        *rejected.entry(b"dead".to_vec()).or_insert(0) += final_dead;
        *rejected.entry(b"over-cap".to_vec()).or_insert(0) += final_over_cap;
        *rejected.entry(b"rescore".to_vec()).or_insert(0) += final_rescore_failed;

        let rej_keys: Vec<&[u8]> = rejected.keys().map(|k| k.as_slice()).collect();
        let rej_counts: Vec<i32> = rejected.values().copied().collect();
        let rej_str = dupesearch_format_rejected_outcome(&rej_keys, &rej_counts);
        let mut out_msg = log_tag().to_vec();
        push_c_slice(&mut out_msg, &job.name);
        out_msg.extend_from_slice(
            format!(
                ": results={results} candidates={candidates} postings={postings} verified={} added={final_added} rejected={{",
                verified.len()
            )
            .as_bytes(),
        );
        push_c_slice(&mut out_msg, &rej_str);
        out_msg.push(b'}');
        self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &out_msg);

        self.remove_pending(host_ctx, host_cb, job.nzb_id);
        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.sent.remove(&job.nzb_id);
        }

        let usable_backup = with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
            let cur = read_host_opts(host_ctx, host_cb);
            let cur_pick = dq_find_nzb(host_ctx, host_cb, dq_ptr, job.nzb_id)
                .map(|(p, _)| inspect_nzb(host_ctx, host_cb, p, false));
            final_placed.values().any(|item| {
                item.valid
                    && item.entry.score > crate::donorscore::DEAD
                    && item.entry.alive >= 0.999
                    && if item.id < 0 {
                        cur.dry_run
                    } else {
                        check_placement_available(
                            host_ctx,
                            host_cb,
                            dq_ptr,
                            cur_pick.as_ref(),
                            item.id,
                            true,
                            cur.health_check_mode == 4,
                        )
                    }
            })
        });

        let low_job_key = lower_key(&job.dupe_key);
        let norm_job_name = crate::releasename::normalize(&job.name);
        let state_retryable = if job.rank_only {
            let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.find_search_cloned(&low_job_key, &norm_job_name)
                .is_some_and(|s| s.retryable)
        } else {
            false
        };

        if usable_backup {
            {
                let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                g.set_retryable(&low_job_key, &norm_job_name, false);
            }
            if !read_host_opts(host_ctx, host_cb).dry_run {
                let _ = self.save_state(host_ctx, host_cb);
            }
        }

        let (_, retry_discovery, rank_incomplete) = dupesearch_eval_post_place_retry(
            job.rank_only,
            state_retryable,
            usable_backup,
            unchecked_members.len(),
        );

        if retry_discovery && !host_stopped(host_ctx, host_cb) {
            let _ = self.forget_search(host_ctx, host_cb, &job.dupe_key, &norm_job_name);
        }
        if (retry_discovery || rank_incomplete) && !host_stopped(host_ctx, host_cb) {
            with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
                let Some((cur_ptr, in_history)) =
                    dq_find_nzb(host_ctx, host_cb, dq_ptr, job.nzb_id)
                else {
                    return;
                };
                if in_history {
                    return;
                }
                let current = inspect_nzb(host_ctx, host_cb, cur_ptr, false);
                if !current.deleting
                    && current.delete_status == DeleteStatus::None
                    && !current.has_post_info
                    && lower_key(&effective_key(&current.dupe_key, &current.name)) == low_job_key
                    && crate::releasename::normalize(&current.name) == norm_job_name
                {
                    if retry_discovery {
                        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                        g.rank_pending.remove(&job.nzb_id);
                    } else {
                        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                        g.rank_pending.insert(job.nzb_id);
                    }
                    if !retry_discovery {
                        self.mark_searching(host_ctx, host_cb, job);
                    }
                    self.schedule(job.nzb_id, host_now(host_ctx, host_cb) + 900);
                    if !retry_discovery {
                        let mut rmsg = log_tag().to_vec();
                        push_c_slice(&mut rmsg, &job.name);
                        rmsg.extend_from_slice(b": retrying backup health in 15 minutes");
                        let rmsg_c = truncate_note(rmsg);
                        unsafe {
                            host_cb(
                                host_ctx,
                                6,
                                1,
                                MK_INFO,
                                dq_ptr,
                                cur_ptr.addr(),
                                rmsg_c.as_ptr() as *const c_char,
                                ptr::null(),
                                ptr::null_mut(),
                                ptr::null_mut(),
                                ptr::null_mut(),
                                ptr::null(),
                                0,
                            );
                        }
                    }
                }
            });
        }
    }

    /// A pick with nothing left on any server gets the dead score, below
    /// every live backup, so the first live backup placed takes its place.
    fn demote_dead_pick(&self, host_ctx: *mut c_void, host_cb: DupeHostFn, job: &SearchJob) {
        if job.failed || job.nzb_id <= 0 {
            return;
        }
        let score = crate::donorscore::base(job.score) + crate::donorscore::DEAD;
        let mut msg = log_tag().to_vec();
        push_c_slice(&mut msg, &job.name);
        if read_host_opts(host_ctx, host_cb).dry_run {
            msg.extend_from_slice(
                format!(": dry run, would demote the dead pick to score {score}").as_bytes(),
            );
            self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
            return;
        }
        let demoted = with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
            let Some(qh) = crate::ffi::queue_host() else {
                return false;
            };
            let ptr = qh.queue_find_by_id(dq_ptr, job.nzb_id);
            if ptr.is_none() {
                return false;
            }
            let snap = qh.nzb_snap(ptr, false);
            if snap.deleting
                || snap.delete_status != DeleteStatus::None
                || snap.has_post_info
                || snap.dupe_score <= score
                || lower_key(&effective_key(&snap.dupe_key, &snap.name)) != lower_key(&job.dupe_key)
                || crate::releasename::normalize(&snap.name)
                    != crate::releasename::normalize(&job.name)
                || crate::dupewatch::swap_nearly_done(
                    snap.size,
                    snap.remaining_size,
                    snap.health,
                    snap.critical_health_opt,
                )
            {
                return false;
            }
            qh.dq_edit_list_single_id(
                dq_ptr,
                job.nzb_id,
                crate::queuesubsys_host::EA_GROUP_SET_DUPE_SCORE,
                Some(score.to_string().as_bytes()),
            )
        });
        if demoted {
            msg.extend_from_slice(
                format!(": dead pick demoted to score {score}, the healthiest backup takes over")
                    .as_bytes(),
            );
        } else {
            msg.extend_from_slice(
                b": dead pick not demoted, it left the queue, changed its key or name, is post-processing or is mostly downloaded",
            );
        }
        self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
    }

    /// Swap: a pick whose full sample shows it will fail (RustyGet's own
    /// health check only counts failures against the whole download, so it
    /// would crawl on for hours first) is swapped for the wholest backup, if
    /// that one is whole enough. The backup goes back to the queue and the
    /// pick is filed as a duplicate backup scored by its sample.
    fn swap_if_failing(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        job: &SearchJob,
        pick: &NzbSummaryData,
        sample: &DonorHealthResult,
        candidates: &[(i32, f64, u8)],
        servers: &[Arc<DonorServer>],
        health_options: &DonorOptions,
    ) {
        if job.failed || job.nzb_id <= 0 || host_stopped(host_ctx, host_cb) {
            return;
        }
        let alive = effective_alive(
            sample.present,
            sample.missing,
            sample.error,
            pick.listed_parts,
            pick.declared_parts,
        );
        // Structurally absent parts count as missing, as in `expand`.
        let missing = (i64::from(sample.missing)
            + i64::from(unlisted_missing(
                sample.answered(),
                pick.listed_parts,
                pick.declared_parts,
            )))
        .min(i64::from(i32::MAX)) as i32;
        let deadline = host_now(host_ctx, host_cb) + 30;
        // par2 feasibility is judged on the data files alone: missing
        // recovery articles do not damage the data.
        let par_doomed = || {
            if pick.data_message_ids.is_empty()
                || pick.par_index.id.is_empty()
                || pick.recovery_bytes.is_empty()
                || servers.is_empty()
            {
                return false;
            }
            let ids = summary_ids_strings(&pick.data_message_ids);
            let groups = summary_groups_arc(pick);
            let h = donor_check_posting(servers, &ids, &groups, health_options, true);
            let data_alive = effective_alive(
                h.present,
                h.missing,
                h.error,
                pick.data_listed_parts,
                pick.data_declared_parts,
            );
            let data_missing = i64::from(h.missing)
                + i64::from(unlisted_missing(
                    h.answered(),
                    pick.data_listed_parts,
                    pick.data_declared_parts,
                ));
            data_alive >= 0.0
                && data_missing >= i64::from(DONOR_MIN_KNOWN)
                && par_doomed_via_host(host_ctx, host_cb, pick, data_alive, deadline)
        };
        let Some(need) = crate::dupewatch::swap_need(alive, missing, par_doomed) else {
            return;
        };
        let pct = |a: f64| (100.0 * a).round() as i32;
        let mut msg = log_tag().to_vec();
        push_c_slice(&mut msg, &job.name);
        msg.extend_from_slice(format!(": the pick sampled {}% alive", pct(alive)).as_bytes());
        if alive >= crate::dupewatch::SWAP_BELOW {
            msg.extend_from_slice(b", but par2 cannot repair it");
        }
        // Only backups failover could use: not marked bad, not in DupeMode
        // FORCE, with their nzb-file and health intact.
        let hc_dupe = read_host_opts(host_ctx, host_cb).health_check_mode == 4;
        let eligible = |dq_ptr: usize, id: i32| -> bool {
            // still a backup of this key and release
            let same_group = dq_find_nzb(host_ctx, host_cb, dq_ptr, id).is_some_and(|(p, _)| {
                let b = inspect_nzb(host_ctx, host_cb, p, false);
                lower_key(&effective_key(&b.dupe_key, &b.name)) == lower_key(&job.dupe_key)
                    && (b.has_dupe_fleet || crate::releasename::same_release(&job.name, &b.name))
            });
            if !same_group {
                return false;
            }
            let pick_snap = dq_find_nzb(host_ctx, host_cb, dq_ptr, job.nzb_id)
                .map(|(p, _)| inspect_nzb(host_ctx, host_cb, p, false));
            check_placement_available(
                host_ctx,
                host_cb,
                dq_ptr,
                pick_snap.as_ref(),
                id,
                true,
                hc_dupe,
            )
        };
        // backups locked now (encrypted headers, no password) only last
        let (usable, locked): (Vec<(i32, f64, u8)>, Vec<i32>) =
            with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
                let usable: Vec<(i32, f64, u8)> = candidates
                    .iter()
                    .copied()
                    .filter(|&(id, _, _)| eligible(dq_ptr, id))
                    .collect();
                let qh = crate::ffi::queue_host();
                let locked = usable
                    .iter()
                    .map(|c| c.0)
                    .filter(|&id| {
                        qh.as_ref().is_some_and(|qh| {
                            dq_find_nzb(host_ctx, host_cb, dq_ptr, id)
                                .is_some_and(|(p, _)| crate::dupecoord::locked_host(qh, p))
                        })
                    })
                    .collect();
                (usable, locked)
            });
        // With DupeBorrowFirst and live article fallback, a backup the pick
        // can borrow from (a TWIN, a borrowable ALT, or one whose packing is
        // not known yet) fills the pick's holes while it downloads: the pick
        // keeps its downloaded data and its extraction, which a swap throws
        // away (the backup would start again from its first byte). The bar
        // is SWAP_BACKUP_ALIVE even where a swap would need the par2 bar:
        // a donor only fills holes, it need not be whole on its own.
        let live_fallback = crate::options::active_options().is_some_and(|o| {
            o.scalars().dupe_article_fallback == crate::queuesubsys_host::DAF_LIVE
        });
        if crate::options::dupe_borrow_first() && live_fallback {
            if let Some(&(donor_id, donor_alive, _)) = usable
                .iter()
                .filter(|&&(_, a, k)| {
                    a >= crate::dupewatch::SWAP_BACKUP_ALIVE
                        && k <= crate::donorkind::DonorKind::Unknown as u8
                })
                .max_by(|a, b| a.2.cmp(&b.2).reverse().then(a.1.total_cmp(&b.1)))
            {
                msg.extend_from_slice(
                    format!(
                        ": it borrows from backup {donor_id} ({}% alive) instead of swapping, keeping its download and extraction",
                        pct(donor_alive)
                    )
                    .as_bytes(),
                );
                self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
                return;
            }
        }
        let Some((backup_id, backup_alive)) =
            crate::dupewatch::swap_backup_locked_last(&usable, &locked, need)
        else {
            msg.extend_from_slice(
                b" and will likely fail, but no backup is whole enough to swap in",
            );
            self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
            return;
        };
        if read_host_opts(host_ctx, host_cb).dry_run {
            msg.extend_from_slice(
                format!(
                    ": dry run, would swap it for backup {backup_id} ({}% alive)",
                    pct(backup_alive)
                )
                .as_bytes(),
            );
            self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
            return;
        }
        let pick_score =
            crate::donorscore::base(job.score) + crate::donorscore::target(alive, false);
        let outcome = with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
            let Some(qh) = crate::ffi::queue_host() else {
                return Err(&b"the queue is not available"[..]);
            };
            let pick_ptr = qh.queue_find_by_id(dq_ptr, job.nzb_id);
            if pick_ptr.is_none() {
                return Err(&b"it already left the queue"[..]);
            }
            let snap = qh.nzb_snap(pick_ptr, false);
            if snap.deleting || snap.delete_status != DeleteStatus::None || snap.has_post_info {
                return Err(&b"it is already finishing"[..]);
            }
            // The key or name may have changed during the health checks.
            if lower_key(&effective_key(&snap.dupe_key, &snap.name)) != lower_key(&job.dupe_key)
                || crate::releasename::normalize(&snap.name)
                    != crate::releasename::normalize(&job.name)
            {
                return Err(&b"its duplicate key or name changed"[..]);
            }
            if crate::dupewatch::swap_nearly_done(
                snap.size,
                snap.remaining_size,
                snap.health,
                snap.critical_health_opt,
            ) {
                return Err(&b"it is mostly downloaded and above its critical health"[..]);
            }
            if !eligible(dq_ptr, backup_id) {
                return Err(&b"the backup is no longer usable"[..]);
            }
            let hi_ptr = qh.history_find_by_id(dq_ptr, backup_id);
            let backup = (0..qh.history_len(dq_ptr))
                .map(|i| qh.history_at(dq_ptr, i))
                .find(|h| h.hi_ptr == hi_ptr && h.inner_ptr != 0);
            let Some(backup) = backup.filter(|_| hi_ptr != 0) else {
                return Err(&b"the backup is not in the history"[..]);
            };
            qh.nzb_set_int(
                backup.nzb(),
                crate::queuesubsys_host::NZB_INT_DUPE_HINT,
                crate::queuesubsys_host::DH_REDOWNLOAD_AUTO,
            );
            qh.history_redownload(dq_ptr, hi_ptr);
            if qh.history_find_by_id(dq_ptr, backup_id) != 0 {
                return Err(&b"the backup could not be returned to the queue"[..]);
            }
            let alive_param = format!("{ALIVE_PARAM_STR}={}", pct(alive));
            qh.dq_edit_list_single_id(
                dq_ptr,
                job.nzb_id,
                crate::queuesubsys_host::EA_GROUP_SET_DUPE_SCORE,
                Some(pick_score.to_string().as_bytes()),
            );
            qh.dq_edit_list_single_id(
                dq_ptr,
                job.nzb_id,
                crate::queuesubsys_host::EA_GROUP_SET_PARAMETER,
                Some(alive_param.as_bytes()),
            );
            qh.dq_edit_list_single_id(
                dq_ptr,
                job.nzb_id,
                crate::queuesubsys_host::EA_GROUP_DUPE_DELETE,
                None,
            );
            Ok(())
        });
        match outcome {
            Ok(()) => msg.extend_from_slice(
                format!(
                    " and will fail: swapped for backup {backup_id} ({}% alive)",
                    pct(backup_alive)
                )
                .as_bytes(),
            ),
            Err(why) => {
                msg.extend_from_slice(b", no swap: ");
                msg.extend_from_slice(why);
            }
        }
        self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
    }

    fn pick_gone(&self, host_ctx: *mut c_void, host_cb: DupeHostFn, job: &SearchJob) -> Vec<u8> {
        with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
            let found = dq_find_nzb(host_ctx, host_cb, dq_ptr, job.nzb_id)
                .map(|(p, _)| inspect_nzb(host_ctx, host_cb, p, false));
            let eff_key = found
                .as_ref()
                .map(|p| effective_key(&p.dupe_key, &p.name))
                .unwrap_or_default();
            let pick_norm = found
                .as_ref()
                .map(|p| crate::releasename::normalize(&p.name))
                .unwrap_or_default();
            let job_norm = crate::releasename::normalize(&job.name);
            let code = eval_pick_gone(
                found.is_some(),
                found.as_ref().is_some_and(|p| p.deleting),
                found
                    .as_ref()
                    .map_or(DeleteStatus::None, |p| p.delete_status),
                &eff_key,
                &job.dupe_key,
                &pick_norm,
                &job_norm,
            );
            match code {
                0 => Vec::new(),
                1 => b"the pick was deleted".to_vec(),
                2 => b"the duplicate key of the pick changed".to_vec(),
                _ => b"the name of the pick changed".to_vec(),
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn add_donor(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        job: &SearchJob,
        pick: &NzbSummaryData,
        posting: &NzbFetched,
        score: i32,
        alive: f64,
        kind: crate::donorkind::DonorKind,
    ) -> i32 {
        let opts = read_host_opts(host_ctx, host_cb);
        if opts.stopped {
            return 0;
        }
        let mut gone = self.pick_gone(host_ctx, host_cb, job);
        if !gone.is_empty() {
            let mut msg = log_tag().to_vec();
            push_c_slice(&mut msg, &job.name);
            msg.extend_from_slice(b": not adding ");
            push_c_slice(&mut msg, &posting.listing.title);
            msg.extend_from_slice(b" [");
            push_c_slice(&mut msg, &posting.listing.indexer);
            msg.extend_from_slice(b"]: ");
            push_c_slice(&mut msg, &gone);
            self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
            return 0;
        }
        if opts.dry_run {
            let pct = if alive < 0.0 {
                -1
            } else {
                (100.0 * alive).round() as i32
            };
            let mut msg = log_tag().to_vec();
            push_c_slice(&mut msg, &job.name);
            msg.extend_from_slice(b": dry run, would add ");
            push_c_slice(&mut msg, &posting.listing.title);
            msg.extend_from_slice(b" [");
            push_c_slice(&mut msg, &posting.listing.indexer);
            msg.extend_from_slice(format!("] score={score} alive={pct}%").as_bytes());
            self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
            return -1;
        }

        let fingerprint = posting.info.fingerprint();
        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !g
                .sent
                .entry(job.nzb_id)
                .or_default()
                .insert(fingerprint.clone())
            {
                return 0;
            }
        }

        let same_size = pick.data_bytes > 0
            && posting.info.data_bytes > 0
            && pick.data_files > 0
            && posting.info.data_files > 0
            && pick.data_bytes == posting.info.data_bytes
            && pick.data_files == posting.info.data_files;
        let alive_str = if alive >= 0.0 {
            Some(((100.0 * alive).round() as i32).to_string().into_bytes())
        } else {
            None
        };
        let mut params: Vec<(&[u8], &[u8])> = vec![(DONOR_PARAM, b"yes")];
        if same_size {
            params.push((b"DupeSameSize", b"1"));
        }
        params.push((crate::donorkind::KIND_PARAM, kind.param()));
        if posting.same_inner {
            params.push((b"DupeSameInner", b"1"));
        }
        if let Some(av) = alive_str.as_deref() {
            params.push((ALIVE_PARAM, av));
        }

        let name = fleet_ensure_nzb_ext(&posting.listing.title);
        let nzb_id = scanner_add_external(
            host_ctx,
            host_cb,
            &name,
            &job.category,
            0,
            &job.dupe_key,
            score,
            &params,
            &posting.data,
        );
        if nzb_id == 0 {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(set) = g.sent.get_mut(&job.nzb_id) {
                set.remove(&fingerprint);
            }
            return 0;
        }

        gone = self.pick_gone(host_ctx, host_cb, job);
        if !gone.is_empty() {
            with_guarded_queue(host_ctx, host_cb, |dq_ptr| unsafe {
                host_cb(
                    host_ctx,
                    6,
                    2,
                    nzb_id,
                    dq_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                    0,
                );
            });
            let mut msg = log_tag().to_vec();
            push_c_slice(&mut msg, &job.name);
            msg.extend_from_slice(b": removed the duplicate ");
            push_c_slice(&mut msg, &posting.listing.title);
            msg.extend_from_slice(b" just added: ");
            push_c_slice(&mut msg, &gone);
            self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &msg);
            return 0;
        }

        let pending_dir = dupesearch_format_pending_dir(&opts.queue_dir, opts.sep, job.nzb_id);
        let pending_path = dupe_bytes_to_path(&pending_dir);
        if std::fs::metadata(pending_path).is_ok_and(|m| m.is_dir()) {
            let sent_path = pending_path.join("sent");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(sent_path)
            {
                use std::io::Write;
                let _ = f.write_all(&fingerprint);
                let _ = f.write_all(b"\n");
            }
        }
        nzb_id
    }

    fn set_score(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        id: i32,
        score: i32,
        params: &[Vec<u8>],
    ) -> bool {
        let opts = read_host_opts(host_ctx, host_cb);
        if opts.dry_run || id < 0 {
            let mut msg =
                format!("DupeSearch: dry run, would set the score of download {id} to {score}")
                    .into_bytes();
            for p in params {
                msg.push(b' ');
                push_c_slice(&mut msg, p);
            }
            host_log(host_ctx, host_cb, MK_INFO, &msg);
            return true;
        }
        with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
            let score_c = c_bytes(score.to_string().as_bytes());
            let c_params: Vec<Vec<u8>> = params.iter().map(|p| c_bytes(p)).collect();
            let mut ptrs: Vec<*const c_char> = c_params
                .iter()
                .map(|v| v.as_ptr() as *const c_char)
                .collect();
            let ok = unsafe {
                host_cb(
                    host_ctx,
                    6,
                    3,
                    id,
                    dq_ptr,
                    ptrs.len(),
                    score_c.as_ptr() as *const c_char,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    if ptrs.is_empty() {
                        ptr::null_mut()
                    } else {
                        ptrs.as_mut_ptr()
                    },
                    ptr::null(),
                    0,
                )
            };
            ok != 0
        })
    }

    fn note(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        nzb_id: i32,
        kind: c_int,
        msg: &[u8],
    ) {
        let msg_c = truncate_note(msg.to_vec());
        let added_to_nzb = with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
            let Some((nzb_ptr, _)) = dq_find_nzb(host_ctx, host_cb, dq_ptr, nzb_id) else {
                return false;
            };
            unsafe {
                host_cb(
                    host_ctx,
                    6,
                    1,
                    kind,
                    dq_ptr,
                    nzb_ptr.addr(),
                    msg_c.as_ptr() as *const c_char,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                    0,
                );
            }
            true
        });
        if !added_to_nzb {
            let g_kind = match kind {
                MK_WARNING => MK_WARNING,
                MK_DETAIL => MK_DETAIL,
                _ => MK_INFO,
            };
            host_log(
                host_ctx,
                host_cb,
                g_kind,
                &msg_c[..msg_c.len().saturating_sub(1)],
            );
        }
    }

    fn mark_searching(&self, host_ctx: *mut c_void, host_cb: DupeHostFn, job: &SearchJob) {
        let opts = read_host_opts(host_ctx, host_cb);
        if opts.dry_run {
            return;
        }
        let dir = dupesearch_format_pending_dir(&opts.queue_dir, opts.sep, job.nzb_id);
        if !mark_searching_pending(&dir, &job.name, &job.dupe_key, job.rank_only) {
            let mut msg = b"Could not save the DupeSearch state to ".to_vec();
            push_c_slice(&mut msg, &dir);
            host_log(host_ctx, host_cb, MK_WARNING, &msg);
        }
    }

    fn save_pending(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        job: &SearchJob,
        pick: &NzbSummaryData,
        verified: &[NzbFetched],
    ) {
        let opts = read_host_opts(host_ctx, host_cb);
        if opts.dry_run {
            return;
        }
        let dir = dupesearch_format_pending_dir(&opts.queue_dir, opts.sep, job.nzb_id);
        let fp = pick.fingerprint();
        let mut ok = save_pending_header(&dir, &job.name, &job.dupe_key, &fp);
        for (i, v) in verified.iter().enumerate() {
            if !ok {
                break;
            }
            ok = save_pending_item(
                &dir,
                i,
                &v.listing.title,
                &v.listing.indexer,
                v.listing.grabs,
                v.listing.size,
                v.listing.date,
                &v.data,
                v.same_inner,
            );
        }
        if !ok || !save_pending_finish(&dir) {
            let mut msg = b"Could not save the DupeSearch state to ".to_vec();
            push_c_slice(&mut msg, &dir);
            host_log(host_ctx, host_cb, MK_WARNING, &msg);
        }
    }

    fn remove_pending(&self, host_ctx: *mut c_void, host_cb: DupeHostFn, nzb_id: i32) {
        let opts = read_host_opts(host_ctx, host_cb);
        if opts.stopped {
            return;
        }
        let dir = dupesearch_format_pending_dir(&opts.queue_dir, opts.sep, nzb_id);
        let _ = std::fs::remove_dir_all(dupe_bytes_to_path(&dir));
    }

    fn resume_pending(&self, host_ctx: *mut c_void, host_cb: DupeHostFn) {
        let init_opts = read_host_opts(host_ctx, host_cb);
        let root_dir = dupesearch_format_pending_dir(&init_opts.queue_dir, init_opts.sep, 0);
        let mut ids = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dupe_bytes_to_path(&root_dir)) {
            for entry in entries.flatten() {
                let fname = entry.file_name();
                #[cfg(unix)]
                let name_bytes = {
                    use std::os::unix::ffi::OsStrExt;
                    fname.as_bytes()
                };
                #[cfg(not(unix))]
                let name_bytes = fname.to_str().unwrap_or("").as_bytes();
                let id = dupesearch_parse_pending_dir_id(name_bytes);
                if id != 0 {
                    ids.push(id);
                }
            }
        }

        for nzb_id in ids {
            let opts = read_host_opts(host_ctx, host_cb);
            if opts.stopped {
                return;
            }
            let dir = dupesearch_format_pending_dir(&opts.queue_dir, opts.sep, nzb_id);
            let phase = dupe_read_all_bytes(&path_join(&dir, opts.sep, b"phase"));
            let (fetched, ranking) =
                crate::dupesearch_nzb::dupesearch_classify_pending_phase(&phase);

            let mut job = SearchJob::default();
            let mut state_erased = false;
            with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
                let Some((nzb_ptr, _)) = dq_find_nzb(host_ctx, host_cb, dq_ptr, nzb_id) else {
                    return;
                };
                let snap = inspect_nzb(host_ctx, host_cb, nzb_ptr, false);
                if fetched {
                    self.collect(host_ctx, host_cb, dq_ptr, &snap, &mut job);
                    job.score = snap
                        .dupe_score
                        .max(read_host_opts(host_ctx, host_cb).primary_score);
                } else if ranking {
                    let key = dupe_read_all_bytes(&path_join(&dir, opts.sep, b"pick-key"));
                    let title = dupe_read_all_bytes(&path_join(&dir, opts.sep, b"pick-title"));
                    let cur = read_host_opts(host_ctx, host_cb);
                    let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                    if key == lower_key(&effective_key(&snap.dupe_key, &snap.name))
                        && title == crate::releasename::normalize(&snap.name)
                        && g.find_search_cloned(&key, &title)
                            .is_some_and(|st| st.title == title)
                    {
                        g.rank_pending.insert(nzb_id);
                    }
                    g.pending.insert(nzb_id, cur.now + i64::from(cur.delay_sec));
                    self.cond.notify_all();
                } else {
                    let key = dupe_read_all_bytes(&path_join(&dir, opts.sep, b"pick-key"));
                    let title = dupe_read_all_bytes(&path_join(&dir, opts.sep, b"pick-title"));
                    let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                    if !key.is_empty() && !title.is_empty() {
                        state_erased = g
                            .title_searched
                            .remove(&(key.clone(), title.clone()))
                            .is_some();
                    }
                    let cache_key = if key.is_empty() {
                        lower_key(&effective_key(&snap.dupe_key, &snap.name))
                    } else {
                        key.clone()
                    };
                    let erase_latest = g.searched.get(&cache_key).is_some_and(|l| {
                        l.title.is_empty()
                            || (!key.is_empty() && !title.is_empty() && l.title == title)
                    });
                    if erase_latest {
                        g.searched.remove(&cache_key);
                        state_erased = true;
                    }
                }
            });

            if state_erased
                && !read_host_opts(host_ctx, host_cb).dry_run
                && !self.save_state(host_ctx, host_cb)
            {
                continue;
            }
            if !fetched || job.nzb_id == 0 {
                let _ = std::fs::remove_dir_all(dupe_bytes_to_path(&dir));
                continue;
            }

            let mut resume_msg = b"DupeSearch: resuming the search of ".to_vec();
            push_c_slice(&mut resume_msg, &job.name);
            resume_msg.extend_from_slice(b" after a restart");
            self.note(host_ctx, host_cb, job.nzb_id, MK_INFO, &resume_msg);

            let pick =
                parse_nzb_summary(&dupe_read_all_bytes(&job.queued_file)).unwrap_or_default();
            let pick_fp = pick.fingerprint();
            if !check_pending_identity(&dir, &job.name, &job.dupe_key, &pick_fp) {
                self.note(
                    host_ctx,
                    host_cb,
                    job.nzb_id,
                    MK_INFO,
                    b"DupeSearch: saved search identity changed; searching the current pick again",
                );
                let old_key = dupe_read_all_bytes(&path_join(&dir, opts.sep, b"pick-key"));
                let old_title = dupe_read_all_bytes(&path_join(&dir, opts.sep, b"pick-title"));
                if !old_key.is_empty() && !old_title.is_empty() {
                    if !self.forget_search(host_ctx, host_cb, &old_key, &old_title) {
                        continue;
                    }
                } else {
                    let mut erased = false;
                    {
                        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                        let low_k = lower_key(&job.dupe_key);
                        if g.searched.get(&low_k).is_some_and(|l| l.title.is_empty()) {
                            g.searched.remove(&low_k);
                            erased = true;
                        }
                    }
                    if erased
                        && !read_host_opts(host_ctx, host_cb).dry_run
                        && !self.save_state(host_ctx, host_cb)
                    {
                        continue;
                    }
                }
                let _ = std::fs::remove_dir_all(dupe_bytes_to_path(&dir));
                let cur = read_host_opts(host_ctx, host_cb);
                self.schedule(job.nzb_id, cur.now + i64::from(cur.delay_sec));
                continue;
            }

            let mut known = Vec::new();
            for path in &job.known_files {
                if let Some(sk) = crate::posting::sketch_of_file(dupe_bytes_to_path(path)) {
                    known.push(sk);
                }
            }

            let sent_raw = dupe_read_all_bytes(&path_join(&dir, opts.sep, b"sent"));
            let mut sent: BTreeSet<Vec<u8>> = BTreeSet::new();
            let mut rem = sent_raw.as_slice();
            while !rem.is_empty() {
                match rem.iter().position(|&b| b == b'\n') {
                    Some(pos) => {
                        sent.insert(rem[..pos].to_vec());
                        rem = &rem[pos + 1..];
                    }
                    None => {
                        sent.insert(rem.to_vec());
                        break;
                    }
                }
            }

            let mut verified = Vec::new();
            let mut rejected: BTreeMap<Vec<u8>, i32> = BTreeMap::new();
            for i in 0usize.. {
                let base_leaf = i.to_string().into_bytes();
                let base_path = path_join(&dir, opts.sep, &base_leaf);
                let mut meta_path = base_path.clone();
                meta_path.extend_from_slice(b".meta");
                if !std::fs::metadata(dupe_bytes_to_path(&meta_path)).is_ok_and(|m| !m.is_dir()) {
                    break;
                }
                let meta_raw = dupe_read_all_bytes(&meta_path);
                let view = parse_pending_meta(&meta_raw);
                let mut nzb_path = base_path.clone();
                nzb_path.extend_from_slice(b".nzb");
                let data = dupe_read_all_bytes(&nzb_path);
                let Some(info) = parse_nzb_summary(&data) else {
                    continue;
                };
                let mut inner_path = base_path;
                inner_path.extend_from_slice(b".inner");
                let same_inner = dupe_read_all_bytes(&inner_path) == b"1\n";
                let sketch =
                    crate::posting::make_sketch(info.message_ids.iter().map(|v| v.as_slice()));
                if sent.contains(&info.fingerprint()) {
                    *rejected.entry(b"already-sent".to_vec()).or_insert(0) += 1;
                    continue;
                }
                if known
                    .iter()
                    .any(|k| crate::posting::same_sketch(&sketch, k))
                {
                    *rejected.entry(b"in-nzbget".to_vec()).or_insert(0) += 1;
                    continue;
                }
                let listing = NewznabResult {
                    title: meta_raw[view.title_offset..view.title_offset + view.title_len].to_vec(),
                    link: Vec::new(),
                    size: view.size,
                    grabs: view.grabs,
                    date: view.date,
                    indexer: meta_raw[view.indexer_offset..view.indexer_offset + view.indexer_len]
                        .to_vec(),
                };
                verified.push(NzbFetched {
                    listing,
                    reason: NzbFetchReason::Ok,
                    data,
                    info,
                    same_inner,
                    lenient: false,
                });
            }
            self.place(
                host_ctx,
                host_cb,
                &job,
                &pick,
                &mut verified,
                &mut rejected,
                0,
                0,
                0,
            );
        }
    }

    fn load_state(&self, host_ctx: *mut c_void, host_cb: DupeHostFn) {
        let opts = read_host_opts(host_ctx, host_cb);
        let state_path = path_join(&opts.queue_dir, opts.sep, b"dupesearch");
        let raw = dupe_read_all_bytes(&state_path);
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut rem = raw.as_slice();
        while !rem.is_empty() {
            let line = match rem.iter().position(|&b| b == b'\n') {
                Some(pos) => {
                    let l = &rem[..pos];
                    rem = &rem[pos + 1..];
                    l
                }
                None => {
                    let l = rem;
                    rem = &[];
                    l
                }
            };
            let parsed = parse_dupesearch_state_line(line, opts.now, STATE_TTL_SEC);
            if parsed.valid != 0 && parsed.within_ttl != 0 {
                let key = line[parsed.key_offset..parsed.key_offset + parsed.key_len].to_vec();
                let stamp = SearchedStamp {
                    score: parsed.score,
                    time: parsed.time,
                    title: line[parsed.title_offset..parsed.title_offset + parsed.title_len]
                        .to_vec(),
                    retryable: parsed.retryable != 0,
                    raw_name: Vec::new(),
                };
                g.record_search(&key, stamp);
            }
        }
    }

    fn forget_search(
        &self,
        host_ctx: *mut c_void,
        host_cb: DupeHostFn,
        dupe_key: &[u8],
        title: &[u8],
    ) -> bool {
        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let low_key = lower_key(dupe_key);
            let effective_title = if title.is_empty() {
                g.searched
                    .get(&low_key)
                    .map(|s| s.title.clone())
                    .unwrap_or_default()
            } else {
                title.to_vec()
            };
            g.set_retryable(&low_key, &effective_title, true);
        }
        if !read_host_opts(host_ctx, host_cb).dry_run {
            return self.save_state(host_ctx, host_cb);
        }
        true
    }

    fn save_state(&self, host_ctx: *mut c_void, host_cb: DupeHostFn) -> bool {
        let opts = read_host_opts(host_ctx, host_cb);
        let state_path = path_join(&opts.queue_dir, opts.sep, b"dupesearch");
        let mut text = Vec::new();
        {
            let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            for ((key, _), searched) in &g.title_searched {
                text.extend_from_slice(&format_searched_state_line(
                    opts.now,
                    STATE_TTL_SEC,
                    key,
                    searched.score,
                    searched.time,
                    &searched.title,
                    searched.retryable,
                ));
            }
            for (key, searched) in &g.searched {
                if !g
                    .title_searched
                    .contains_key(&(key.clone(), searched.title.clone()))
                {
                    text.extend_from_slice(&format_searched_state_line(
                        opts.now,
                        STATE_TTL_SEC,
                        key,
                        searched.score,
                        searched.time,
                        &searched.title,
                        searched.retryable,
                    ));
                }
            }
        }
        if !dupe_write_atomic_bytes(&state_path, &text) {
            let mut msg = b"Could not save the DupeSearch state to ".to_vec();
            push_c_slice(&mut msg, &state_path);
            host_log(host_ctx, host_cb, MK_WARNING, &msg);
            return false;
        }
        true
    }

    pub fn run(&self, host_ctx: *mut c_void, host_cb: DupeHostFn) {
        host_log(host_ctx, host_cb, MK_DEBUG, b"Entering DupeSearch-loop");
        loop {
            let opts = read_host_opts(host_ctx, host_cb);
            if opts.queue_loaded || opts.stopped {
                if opts.stopped {
                    return;
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        self.load_state(host_ctx, host_cb);
        let init_opts = read_host_opts(host_ctx, host_cb);
        self.fetcher.set_state_path_bytes(&path_join(
            &init_opts.queue_dir,
            init_opts.sep,
            b"dupesearch-indexers",
        ));
        self.fetcher.load(init_opts.now);
        self.dead.set_state_path(&path_join(
            &init_opts.queue_dir,
            init_opts.sep,
            b"dupesearch-dead",
        ));
        self.dead.load(init_opts.now);
        if init_opts.dupe_search {
            self.resume_pending(host_ctx, host_cb);
            self.scan_queue(host_ctx, host_cb);
        } else if init_opts.dupe_check && init_opts.dupe_watch {
            self.scan_queue(host_ctx, host_cb);
        }

        while !host_stopped(host_ctx, host_cb) {
            let mut due = Vec::new();
            {
                let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                let (mut g, _) = self
                    .cond
                    .wait_timeout(g, Duration::from_millis(500))
                    .unwrap_or_else(|e| e.into_inner());
                drop(g);
                let now = host_now(host_ctx, host_cb);
                g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                let due_ids: Vec<i32> = g
                    .pending
                    .iter()
                    .filter_map(|(&id, &when)| if when <= now { Some(id) } else { None })
                    .collect();
                for id in due_ids {
                    g.pending.remove(&id);
                    due.push(id);
                }
            }

            for nzb_id in due {
                if host_stopped(host_ctx, host_cb) {
                    break;
                }
                let mut job = SearchJob::default();
                let do_search = with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
                    self.prepare_native(host_ctx, host_cb, dq_ptr, nzb_id, &mut job)
                });
                if do_search {
                    // Only a search is resumed after a restart.
                    if read_host_opts(host_ctx, host_cb).dupe_search {
                        self.mark_searching(host_ctx, host_cb, &job);
                    }
                    self.search(host_ctx, host_cb, &job);
                }
            }
        }

        donor_wait_all();
        host_log(host_ctx, host_cb, MK_DEBUG, b"Exiting DupeSearch-loop");
    }
}

struct DaemonDupeSearch {
    ds: Arc<NzbgetRsDupeSearch>,
    thread: Option<std::thread::JoinHandle<()>>,
}

static DAEMON_DS_RUNNING: AtomicBool = AtomicBool::new(false);
static DAEMON_DS_STOPPED: AtomicBool = AtomicBool::new(false);
static DAEMON_DUPESEARCH: Mutex<Option<DaemonDupeSearch>> = Mutex::new(None);

pub(crate) fn create_daemon_dupesearch() {
    DAEMON_DS_RUNNING.store(false, Ordering::SeqCst);
    DAEMON_DS_STOPPED.store(false, Ordering::SeqCst);
    let mut g = DAEMON_DUPESEARCH.lock().unwrap_or_else(|e| e.into_inner());
    *g = Some(DaemonDupeSearch {
        ds: Arc::new(NzbgetRsDupeSearch::new()),
        thread: None,
    });
}

pub(crate) fn start_daemon_dupesearch() {
    let mut g = DAEMON_DUPESEARCH.lock().unwrap_or_else(|e| e.into_inner());
    let Some(st) = g.as_mut() else {
        return;
    };
    if st.thread.is_some() {
        return;
    }
    DAEMON_DS_STOPPED.store(false, Ordering::SeqCst);
    DAEMON_DS_RUNNING.store(true, Ordering::SeqCst);
    let ds = st.ds.clone();
    if let Ok(h) = std::thread::Builder::new()
        .name("dupe-search".into())
        .spawn(move || {
            struct RunningReset;
            impl Drop for RunningReset {
                fn drop(&mut self) {
                    DAEMON_DS_RUNNING.store(false, Ordering::SeqCst);
                }
            }
            let _reset = RunningReset;
            let _tg = crate::ffi::RustThreadGuard::enter(&DAEMON_DS_STOPPED);
            ds.run(
                (&raw const DAEMON_DS_STOPPED) as *mut c_void,
                native_dupe_host,
            );
        })
    {
        st.thread = Some(h);
    } else {
        DAEMON_DS_RUNNING.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn stop_daemon_dupesearch() {
    DAEMON_DS_STOPPED.store(true, Ordering::SeqCst);
    let ds = DAEMON_DUPESEARCH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|st| st.ds.clone());
    if let Some(ds) = ds {
        ds.stop();
    }
}

pub(crate) fn is_daemon_dupesearch_running() -> bool {
    DAEMON_DS_RUNNING.load(Ordering::SeqCst)
}

pub(crate) fn shutdown_daemon_dupesearch() {
    stop_daemon_dupesearch();
    let st = DAEMON_DUPESEARCH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(mut st) = st {
        if let Some(h) = st.thread.take() {
            let _ = h.join();
        }
        #[cfg(unix)]
        crate::log::log_active_bytes(MK_DEBUG, b"Destroying DupeSearch");
        drop(st.ds);
    }
    DAEMON_DS_RUNNING.store(false, Ordering::SeqCst);
}

pub(crate) fn notify_active_dupesearch_name_changed(nzb_id: i32) {
    let ds = DAEMON_DUPESEARCH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|st| st.ds.clone());
    if let Some(ds) = ds {
        ds.notify_name_changed(
            nzb_id,
            (&raw const DAEMON_DS_STOPPED) as *mut c_void,
            native_dupe_host,
        );
    }
}

pub(crate) fn notify_active_dupesearch_queue_update(
    action_kind: i32,
    dq_ptr: usize,
    added_ptr: NzbRef,
) {
    let ds = DAEMON_DUPESEARCH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|st| st.ds.clone());
    if let Some(ds) = ds {
        ds.download_queue_update(
            action_kind,
            dq_ptr,
            added_ptr,
            (&raw const DAEMON_DS_STOPPED) as *mut c_void,
            native_dupe_host,
        );
    }
}

// ============================================================================
// Fleet (`Fleet::Append` in `Util.h`)
// ============================================================================

struct FleetOutEntry {
    nzb_id: i32,
    name: Vec<u8>,
    rank: i32,
    alive: i32,
    status: Vec<u8>,
    same_as: i32,
    same_as_rank: i32,
    member_idx: i32,
    reason: Vec<u8>,
}

fn emit_fleet_result(
    out: &mut dyn RpcEmit,
    chosen: i32,
    complete: bool,
    reason: &[u8],
    entries: &[FleetOutEntry],
) -> c_int {
    let reason_c = crate::cstr::to_cstring(reason);
    let owned: Vec<_> = entries
        .iter()
        .map(|e| {
            (
                crate::cstr::to_cstring(&e.name),
                crate::cstr::to_cstring(&e.status),
                crate::cstr::to_cstring(&e.reason),
            )
        })
        .collect();
    let ffi_entries: Vec<_> = entries
        .iter()
        .zip(&owned)
        .map(|(e, (name, status, reason))| NzbgetRsRpcFleetMember {
            nzb_id: e.nzb_id,
            name,
            rank: e.rank,
            alive: e.alive,
            status,
            same_as: e.same_as,
            same_as_rank: e.same_as_rank,
            reason,
        })
        .collect();
    out.fleet_result(chosen, complete, &reason_c, &ffi_entries);
    1
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn fleet_append(
    host_ctx: *mut c_void,
    host_cb: DupeHostFn,
    dupe_key_ptr: *const c_char,
    category_ptr: *const c_char,
    priority: c_int,
    timeout_sec_in: c_int,
    req_members: *const NzbgetRsRpcFleetReqMember,
    req_members_len: usize,
    out: &mut dyn RpcEmit,
) -> c_int {
    const MAX_NZB_BYTES: usize = 64 * 1024 * 1024;
    const MAX_PARALLEL: i32 = 16;
    const FLEET_SAMPLE_MAX: i32 = 200;
    const FINISH_RESERVE_MS: i64 = 2000;
    const MAX_TIMEOUT_SEC: i32 = 120;

    let timeout_sec = fleet_clamp_timeout_sec(timeout_sec_in, MAX_TIMEOUT_SEC);
    if req_members.is_null() || req_members_len == 0 {
        return emit_fleet_result(out, 0, true, b"NO_MEMBERS", &[]);
    }

    struct ReqMember {
        name: Vec<u8>,
        data: Vec<u8>,
        url: Vec<u8>,
    }

    let raw_slice = unsafe { std::slice::from_raw_parts(req_members, req_members_len) };
    let mut members = Vec::with_capacity(req_members_len);
    for rm in raw_slice {
        let name = unsafe { cstr::owned(rm.name) };
        let payload = if rm.data.is_null() || rm.data_len == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(rm.data as *const u8, rm.data_len).to_vec() }
        };
        if rm.is_url != 0 {
            members.push(ReqMember {
                name,
                data: Vec::new(),
                url: payload,
            });
        } else {
            members.push(ReqMember {
                name,
                data: payload,
                url: Vec::new(),
            });
        }
    }

    let mut dupe_key = unsafe { cstr::owned(dupe_key_ptr) };
    let category = unsafe { cstr::owned(category_ptr) };
    if dupe_key.is_empty() {
        dupe_key = make_dupe_key(&members[0].name);
    }

    let deadline_ms = donor_now_ms() + i64::from(timeout_sec) * 1000;
    let key_lock = fleet_key_lock(&dupe_key);
    let key_wait_ms = (deadline_ms - donor_now_ms() - FINISH_RESERVE_MS).max(0);
    let Some(_key_guard) = key_lock.try_lock_for(key_wait_ms) else {
        return emit_fleet_result(out, 0, false, b"KEY_BUSY", &[]);
    };

    let fetched_slots: Vec<Mutex<(bool, i32, Vec<u8>)>> = (0..members.len())
        .map(|_| Mutex::new((false, 0, Vec::new())))
        .collect();
    let ctx_addr = host_ctx as usize;
    std::thread::scope(|s| {
        let slots_ref = &fetched_slots;
        let members_ref = &members;
        for i in 0..members.len() {
            if !members[i].url.is_empty() {
                s.spawn(move || {
                    let mut info_name = b"fleet member ".to_vec();
                    info_name.extend_from_slice(&members_ref[i].name);
                    let reply = http_fetch(
                        ctx_addr as *mut c_void,
                        host_cb,
                        &members_ref[i].url,
                        &info_name,
                        MAX_NZB_BYTES,
                        deadline_ms - FINISH_RESERVE_MS,
                    );
                    *slots_ref[i].lock().unwrap_or_else(|e| e.into_inner()) = reply;
                });
            }
        }
    });

    struct FleetCandidate {
        index: usize,
        summary: NzbSummaryData,
        health: DonorHealthResult,
        checked: bool,
        alive: i32,
        dead: bool,
        same_as: i32,
        existing_id: i32,
        error: Vec<u8>,
        kind: crate::donorkind::DonorKind,
        // intake verdict: Some(true) same video, Some(false) unverified
        video: Option<bool>,
        // intake found another video in it
        video_different: bool,
        // archive headers encrypted (stored as DupeLocked); without a
        // password it is locked: never extractable, ranked after every copy
        // that is
        encrypted: bool,
        // the copy already queued has a password now (*Unpack:Password)
        stored_password: bool,
    }

    let mut candidates: Vec<FleetCandidate> = Vec::with_capacity(members.len());
    for (i, slot) in fetched_slots.into_iter().enumerate() {
        let mut cand = FleetCandidate {
            index: i,
            summary: NzbSummaryData::default(),
            health: DonorHealthResult::default(),
            checked: false,
            alive: -1,
            dead: false,
            same_as: -1,
            existing_id: 0,
            error: Vec::new(),
            kind: crate::donorkind::DonorKind::Unknown,
            video: None,
            video_different: false,
            encrypted: false,
            stored_password: false,
        };
        if !members[i].url.is_empty() {
            let (ok, status, body) = slot.into_inner().unwrap_or_else(|e| e.into_inner());
            if !ok {
                cand.error = fleet_format_fetch_error(status);
                candidates.push(cand);
                continue;
            }
            members[i].data = body;
        }
        match parse_nzb_summary(&members[i].data) {
            Some(parsed) if !parsed.message_ids.is_empty() => {
                cand.summary = parsed;
                summary_take_name_password(&mut cand.summary, &members[i].name);
            }
            _ => {
                cand.error = b"the nzb-file is unreadable".to_vec();
                candidates.push(cand);
                continue;
            }
        }
        for (j, prev) in candidates.iter().enumerate() {
            if prev.error.is_empty()
                && prev.same_as < 0
                && same_posting_vecs(&prev.summary.message_ids, &cand.summary.message_ids)
            {
                cand.same_as = j as i32;
                break;
            }
        }
        candidates.push(cand);
    }

    let (top, running_id, lowest, existing) = with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
        let mut top = i64::from(crate::dupewatch::primary_score());
        let mut running_id = 0i32;
        let mut running_score = 0i32;
        let mut lowest = 0i32;
        let mut any_seen = false;
        let mut existing: Vec<(i32, Vec<u8>)> = Vec::new();

        let mut consider = |snap: &NzbSnap, queued: bool| {
            if snap.kind != 0 || !snap.dupe_key.eq_ignore_ascii_case(&dupe_key) {
                return;
            }
            let (nt, nl, nrid, nrsc) = fleet_update_key_score_bounds(
                top,
                lowest,
                any_seen,
                queued,
                snap.deleting,
                snap.delete_status,
                running_id,
                running_score,
                snap.id,
                snap.dupe_score,
            );
            top = nt;
            lowest = nl;
            running_id = nrid;
            running_score = nrsc;
            any_seen = true;
            fleet_visit_pipe_names(&snap.queued_filename, |part| {
                existing.push((snap.id, part.to_vec()));
            });
        };

        let (q_len, h_len) = dq_counts(host_ctx, host_cb, dq_ptr);
        for i in 0..q_len {
            let p = dq_queue_at(host_ctx, host_cb, dq_ptr, i);
            if p.is_some() {
                let s = inspect_nzb(host_ctx, host_cb, p, false);
                consider(&s, true);
            }
        }
        for i in 0..h_len {
            let (_, h_kind, _, p) = dq_history_at(host_ctx, host_cb, dq_ptr, i);
            if h_kind == 1 && p.is_some() {
                let s = inspect_nzb(host_ctx, host_cb, p, false);
                consider(&s, false);
            }
        }
        (top, running_id, lowest, existing)
    });

    // copies of this release already queued or parked from an earlier
    // submission: each one posted as one nzb (not a merge of several) is probed
    // with the new members, so a backup added later is classed now against
    // them, before it downloads, and they get a kind they lacked
    let mut earlier: Vec<(i32, NzbSummaryData)> = Vec::new();
    // queued copies merged from several nzbs: one member's probe never speaks for one
    let merged: Vec<i32> = existing
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| existing.iter().filter(|(x, _)| x == id).count() > 1)
        .collect();
    for (ex_id, ex_path) in &existing {
        let Some(summary) = parse_nzb_summary(&dupe_read_all_bytes(ex_path)) else {
            continue;
        };
        for cand in &mut candidates {
            if cand.error.is_empty()
                && cand.same_as < 0
                && cand.existing_id == 0
                && same_posting_vecs(&summary.message_ids, &cand.summary.message_ids)
            {
                cand.existing_id = *ex_id;
            }
        }
        let parts = existing.iter().filter(|(id, _)| id == ex_id).count();
        // the same posting as a member (even queued twice) is that member
        let a_member = candidates.iter().any(|c| {
            c.error.is_empty() && same_posting_vecs(&summary.message_ids, &c.summary.message_ids)
        });
        if parts == 1 && !summary.message_ids.is_empty() && !a_member {
            earlier.push((*ex_id, summary));
        }
    }
    // a queued copy a member is the same posting as is probed as that member
    earlier.retain(|(id, _)| !candidates.iter().any(|c| c.existing_id == *id));
    // only copies queued or parked as backups, and not dead, are partners;
    // the kind each one has stored is kept unless the probe knows better
    let earlier_stored = fleet_queued_kinds(
        host_ctx,
        host_cb,
        &earlier.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
    );
    let mut earlier_kept: Vec<(i32, NzbSummaryData)> = Vec::new();
    for ((id, summary), stored) in earlier.into_iter().zip(&earlier_stored) {
        if stored.is_some() && earlier_kept.len() < FLEET_KIND_EARLIER_MAX {
            earlier_kept.push((id, summary));
        }
    }
    let mut earlier = earlier_kept;
    // a queued copy is read with the password it has now
    let stored_pw = fleet_stored_passwords(
        host_ctx,
        host_cb,
        &earlier.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
    );
    for ((_, summary), pw) in earlier.iter_mut().zip(stored_pw) {
        if let Some(pw) = pw {
            // the password it has now wins over its nzb's, which the user may
            // have corrected since
            summary
                .meta
                .retain(|k, _| !k.eq_ignore_ascii_case(b"password"));
            summary.meta.insert(b"password".to_vec(), pw);
        }
    }
    let earlier = earlier;
    // the stored kind of each queued copy a member is the same posting as
    let member_stored = fleet_queued_kinds(
        host_ctx,
        host_cb,
        &candidates.iter().map(|c| c.existing_id).collect::<Vec<_>>(),
    );
    // what is stored of its headers, and whether it has a password now: a
    // probe that runs out of time must not forget the one, and a password
    // set since unlocks it
    let member_locks = fleet_stored_locks(
        host_ctx,
        host_cb,
        &candidates.iter().map(|c| c.existing_id).collect::<Vec<_>>(),
    );
    for (cand, stored) in candidates.iter_mut().zip(&member_locks) {
        if let Some((encrypted, password)) = *stored {
            cand.encrypted |= encrypted;
            cand.stored_password = password;
        }
    }

    let already_downloaded_name: Option<Vec<u8>> =
        with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
            let opts = read_host_opts(host_ctx, host_cb);
            let (_, h_len) = dq_counts(host_ctx, host_cb, dq_ptr);
            for i in 0..h_len {
                let (_, h_kind, _, p) = dq_history_at(host_ctx, host_cb, dq_ptr, i);
                if h_kind != 1 || p.is_none() {
                    continue;
                }
                let item = inspect_nzb(host_ctx, host_cb, p, false);
                if item.delete_status != DeleteStatus::None
                    || !item.dupe_success
                    || item.mark_status == MarkStatus::Bad
                    || !crate::releasename::readable(&item.name)
                {
                    continue;
                }
                let same = candidates.iter().any(|c| {
                    let m_name = &members[c.index].name;
                    c.error.is_empty()
                        && crate::releasename::readable(m_name)
                        && crate::releasename::same_release(m_name, &item.name)
                });
                if same
                    && crate::dupecoord::files_on_disk(
                        &item.final_dir,
                        &item.dest_dir,
                        &opts.ext_cleanup_disk,
                    )
                {
                    return Some(item.name);
                }
            }
            None
        });

    if let Some(dl_name) = already_downloaded_name {
        let mut out_entries = Vec::with_capacity(candidates.len());
        for (rank_zero, cand) in candidates.iter().enumerate() {
            let reason = if cand.error.is_empty() {
                let mut r = b"downloaded as ".to_vec();
                push_c_slice(&mut r, &dl_name);
                r
            } else {
                cand.error.clone()
            };
            out_entries.push(FleetOutEntry {
                nzb_id: 0,
                name: members[cand.index].name.clone(),
                rank: (rank_zero + 1) as i32,
                alive: cand.alive,
                status: if cand.error.is_empty() {
                    b"SKIPPED".to_vec()
                } else {
                    b"ERROR".to_vec()
                },
                same_as: 0,
                same_as_rank: 0,
                member_idx: cand.index as i32,
                reason,
            });
        }
        let mut msg = b"Fleet for ".to_vec();
        push_c_slice(&mut msg, &dupe_key);
        msg.extend_from_slice(b": ");
        push_c_slice(&mut msg, &dl_name);
        msg.extend_from_slice(b" is downloaded already");
        host_log(host_ctx, host_cb, MK_INFO, &msg);
        return emit_fleet_result(
            out,
            0,
            true,
            b"ALREADY_DOWNLOADED",
            &out_entries,
        );
    }

    // TWINs first, then ALTs by how well stream repair can borrow from them.
    // The copies are probed before the health check, with their own share of
    // the time: the health check spends whatever budget it is given, so a
    // probe run after it got no time and left every member "unknown". The
    // kinds are told after it, so a dead copy never makes another a TWIN.
    // A single copy is probed too: its packing alone gives its kind. Copies
    // queued from an earlier submission are probed with the members and count
    // as twin partners.
    let mut probed: Vec<Option<(crate::twin::Listed, crate::donorkind::Packing)>> =
        vec![None; candidates.len()];
    let mut earlier_probed: Vec<(crate::twin::Listed, crate::donorkind::Packing)> = Vec::new();
    {
        let usable: Vec<usize> = (0..candidates.len())
            .filter(|&i| {
                let c = &candidates[i];
                c.error.is_empty() && c.same_as < 0
            })
            .collect();
        if !usable.is_empty() {
            let probe_deadline = now_unix_sec()
                + fleet_kind_probe_secs(deadline_ms - donor_now_ms() - FINISH_RESERVE_MS);
            let mut summaries: Vec<&NzbSummaryData> =
                usable.iter().map(|&i| &candidates[i].summary).collect();
            summaries.extend(earlier.iter().map(|(_, s)| s));
            let mut got = probe_copies(host_ctx, host_cb, &summaries, probe_deadline);
            earlier_probed = got.split_off(usable.len().min(got.len()));
            for (&i, p) in usable.iter().zip(got) {
                candidates[i].encrypted |= p.1.headers_encrypted();
                probed[i] = Some(p);
            }
        }
    }

    let mut intake_later: Vec<usize> = Vec::new();
    // held from before a lone backup is queued until its background check owns
    // it (dropped when this call returns, after that check is spawned)
    let mut _lone_key_guard: Option<IntakeKeyGuard> = None;
    let mut intake_print: Option<crate::dupeintake::Print> = None;
    let mut intake_prints: Vec<Option<crate::dupeintake::Print>> = vec![None; candidates.len()];
    let mut intake_keys: Vec<Option<crate::twinfields::InnerKey>> = vec![None; candidates.len()];
    let mut intake_samples = 0usize;
    let mut intake_seed = 0u64;
    // Intake: every copy opened down to its video; a copy carrying another
    // video is not part of this fleet. Like the kinds, it runs before the
    // health check with its own share of the time (half of what is left).
    {
        let usable: Vec<usize> = (0..candidates.len())
            .filter(|&i| {
                let c = &candidates[i];
                c.error.is_empty() && c.same_as < 0
            })
            .collect();
        // A backup sent on its own after the fleet's first submission is opened
        // against the copy already queued for its key (the client's pick, so it
        // goes first and wins a tie), so a nested alt is told the same video
        // before the queued copy dies, not only once a later pass samples it.
        // Only the running copy is authoritative: never a parked backup, and not
        // when the one member sent is the running copy itself. It is read in
        // the background, after the backup is added: reading two copies down to
        // their video here would hold the reply, and the backup, while the
        // queued copy may already be failing.
        let lone_backup = earlier.iter().any(|(id, _)| running_id != 0 && *id == running_id)
            && usable.len() == 1
            && candidates[usable[0]].existing_id != running_id;
        let samples = crate::dupeintake::samples_for_body_checks(
            read_host_opts(host_ctx, host_cb).body_checks,
        );
        // TwinCheck groups dupe keys case-blind; the samples must land alike
        let seed = u64::from(crate::dupefallback::crc32_ieee(&dupe_key.to_ascii_lowercase())) | 1;
        if lone_backup {
            _lone_key_guard = Some(IntakeKeyGuard::enter(&dupe_key));
            intake_samples = samples;
            intake_seed = seed;
            intake_later.push(usable[0]);
            let mut msg = b"Fleet intake: ".to_vec();
            msg.extend_from_slice(&members[candidates[usable[0]].index].name);
            msg.extend_from_slice(b" is checked against the queued copy in the background");
            host_log(host_ctx, host_cb, MK_INFO, &msg);
        } else if usable.len() >= 2 && !donor_stopping() && !http_stopped() {
            let intake_deadline = now_unix_sec()
                + fleet_phase_secs(
                    deadline_ms - donor_now_ms() - FINISH_RESERVE_MS,
                    2,
                    FLEET_INTAKE_MAX_SEC,
                );
            let summaries: Vec<&NzbSummaryData> =
                usable.iter().map(|&i| &candidates[i].summary).collect();
            let (verdicts, print, prints, keys) = fleet_intake(
                host_ctx,
                host_cb,
                &summaries,
                samples,
                seed,
                intake_deadline,
            );
            intake_print = print;
            for (&i, p) in usable.iter().zip(prints) {
                intake_prints[i] = p;
            }
            for (&i, k) in usable.iter().zip(keys) {
                intake_keys[i] = k;
            }
            intake_samples = samples;
            intake_seed = seed;
            for (&i, (verdict, why)) in usable.iter().zip(verdicts) {
                // unverified for want of time or a slow read: tried again later
                if verdict == crate::dupeintake::Verdict::Unverified {
                    intake_later.push(i);
                }
                let name = &members[candidates[i].index].name;
                let mut msg = b"Fleet intake: ".to_vec();
                msg.extend_from_slice(name);
                match verdict {
                    crate::dupeintake::Verdict::Same => {
                        candidates[i].video = Some(true);
                        msg.extend_from_slice(b" carries the fleet's video")
                    }
                    crate::dupeintake::Verdict::Different => {
                        msg.extend_from_slice(b" carries another video, dropped");
                        candidates[i].error = b"not the same video".to_vec();
                        candidates[i].video_different = true;
                    }
                    crate::dupeintake::Verdict::Unverified => {
                        candidates[i].video = Some(false);
                        msg.extend_from_slice(b" unverified: ");
                        msg.extend_from_slice(why.as_bytes());
                        // read with the nzb's password, if any: a wrong one
                        // leaves the headers encrypted too, but whether that
                        // locks the copy is told when it is ranked
                        if why == crate::streamrepair::ContentMapper::SKIP_ENCRYPTED_HEADERS {
                            candidates[i].encrypted = true;
                        }
                    }
                }
                host_log(host_ctx, host_cb, MK_INFO, &msg);
            }
        }
    }

    let mut postings = Vec::new();
    for cand in &candidates {
        if cand.error.is_empty() && cand.same_as < 0 && cand.existing_id == 0 {
            postings.push(DonorPosting {
                key: cand.index.to_string(),
                ids: summary_ids_strings(&cand.summary.message_ids),
                groups: summary_groups_arc(&cand.summary),
            });
        }
    }

    let servers = nntp_health_servers(host_ctx, host_cb);
    let mut complete = true;
    let check_guard = if !servers.is_empty() && !postings.is_empty() {
        let wait_ms = (deadline_ms - donor_now_ms() - FINISH_RESERVE_MS - 1000).max(0);
        fleet_check_mutex().try_lock_for(wait_ms)
    } else {
        None
    };
    if !servers.is_empty() && !postings.is_empty() && check_guard.is_none() {
        complete = false;
    }
    if let Some(_cg) = check_guard {
        let cur = read_host_opts(host_ctx, host_cb);
        let check_start = donor_now_ms();
        let budget_ms = 1000i64.max(deadline_ms - check_start - FINISH_RESERVE_MS) as i32;
        let options = DonorOptions {
            sample: DonorSampleOptions {
                percent: cur.health_percent,
                minimum: cur.health_min,
                maximum: cur.health_max.min(FLEET_SAMPLE_MAX),
                body_percent: cur.body_percent,
                max_body: cur.body_checks,
                seed: 0,
            },
            probe: 10,
            budget_ms,
            deadline_ms: check_start + i64::from(budget_ms),
        };
        let checked_map = Mutex::new(BTreeMap::<usize, DonorHealthResult>::new());
        {
            struct FleetHoldGuard;
            impl Drop for FleetHoldGuard {
                fn drop(&mut self) {
                    if let Some(ws) = crate::ffi::active_workstate() {
                        ws.hold_download_for_fleet(false);
                    }
                }
            }
            if let Some(ws) = crate::ffi::active_workstate() {
                ws.hold_download_for_fleet(true);
            }
            let _hold = FleetHoldGuard;
            donor_check_postings(
                &servers,
                &postings,
                &options,
                true,
                (postings.len() as i32).min(MAX_PARALLEL),
                |key, h| {
                    if let Ok(idx) = key.parse::<usize>() {
                        checked_map
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(idx, *h);
                    }
                },
            );
        }
        for (idx, h) in checked_map.into_inner().unwrap_or_else(|e| e.into_inner()) {
            if idx < candidates.len() {
                candidates[idx].health = h;
                candidates[idx].checked = true;
            }
        }
        complete = donor_now_ms() - check_start < i64::from(options.budget_ms);
    }

    let min_alive_opt = read_host_opts(host_ctx, host_cb).min_alive;
    for cand in &mut candidates {
        if cand.error.is_empty() && cand.same_as < 0 {
            let (alive, dead, m_comp) = eval_fleet_member_health(
                cand.checked,
                complete,
                cand.health.present,
                cand.health.missing,
                cand.health.error,
                DONOR_MIN_KNOWN,
                min_alive_opt,
            );
            cand.alive = alive;
            cand.dead = dead;
            complete &= m_comp;
        }
    }

    // kinds found for copies already queued, written once the members are
    // added: a twin only counts when its partner is queued by then
    struct KindWrite {
        nzb_id: i32,
        packing: crate::donorkind::Packing,
        twin_queued: bool,
        partners: Vec<usize>,
    }
    let mut kind_writes: Vec<KindWrite> = Vec::new();
    {
        let live: Vec<usize> = (0..candidates.len())
            .filter(|&i| {
                let c = &candidates[i];
                c.error.is_empty() && c.same_as < 0 && !c.dead && probed[i].is_some()
            })
            .collect();
        let mut earlier_partners: Vec<Vec<usize>> = vec![Vec::new(); earlier_probed.len()];
        for &i in &live {
            let (listed, packing) = probed[i].as_ref().expect("filtered on is_some");
            let partners: Vec<usize> = live
                .iter()
                .copied()
                .filter(|&j| {
                    // a member that is a dead or failed queued copy is no partner
                    j != i
                        && (candidates[j].existing_id == 0 || member_stored[j].is_some())
                        && crate::donorkind::twins(
                            listed,
                            &probed[j].as_ref().expect("filtered on is_some").0,
                            &candidates[i].summary.data_layout,
                            &candidates[j].summary.data_layout,
                        )
                })
                .collect();
            let mut twin_queued = false;
            for (e, (e_listed, _)) in earlier_probed.iter().enumerate() {
                if crate::donorkind::twins(
                    listed,
                    e_listed,
                    &candidates[i].summary.data_layout,
                    &earlier[e].1.data_layout,
                ) {
                    twin_queued = true;
                    earlier_partners[e].push(i);
                }
            }
            let found =
                crate::donorkind::DonorKind::of(twin_queued || !partners.is_empty(), *packing);
            candidates[i].kind = found;
            let id = candidates[i].existing_id;
            // only a copy queued or parked as a backup (not failed, deleted or
            // dead) is ranked by, or written, its stored kind
            if id != 0 && member_stored[i].is_some() && !merged.contains(&id) {
                // a member already queued: rank and report what is known of it
                if let Some(stored) = member_stored[i] {
                    if !kind_upgrades(stored, found) {
                        candidates[i].kind = stored;
                    }
                }
                kind_writes.push(KindWrite {
                    nzb_id: id,
                    packing: *packing,
                    twin_queued,
                    partners,
                });
            }
        }
        for (e, (_, packing)) in earlier_probed.iter().enumerate() {
            kind_writes.push(KindWrite {
                nzb_id: earlier[e].0,
                packing: *packing,
                twin_queued: false,
                partners: std::mem::take(&mut earlier_partners[e]),
            });
        }
    }

    let mut cand_err = vec![0u8; candidates.len()];
    let mut cand_dead = vec![0u8; candidates.len()];
    let mut cand_same_as = vec![0i32; candidates.len()];
    let mut cand_alive = vec![0i32; candidates.len()];
    let mut cand_bytes = vec![0i64; candidates.len()];
    let mut cand_kind = vec![0u8; candidates.len()];
    let mut cand_locked = vec![0u8; candidates.len()];
    for i in 0..candidates.len() {
        if candidates[i].same_as >= 0 {
            let orig = candidates[i].same_as as usize;
            candidates[i].alive = candidates[orig].alive;
            candidates[i].dead = candidates[orig].dead;
            candidates[i].kind = candidates[orig].kind;
            candidates[i].encrypted = candidates[orig].encrypted;
        }
        cand_kind[i] = candidates[i].kind as u8;
        cand_locked[i] = u8::from(fleet_candidate_locked(
            candidates[i].encrypted,
            summary_password_known(&candidates[i].summary),
            candidates[i].stored_password,
        ));
        cand_err[i] = u8::from(!candidates[i].error.is_empty());
        cand_same_as[i] = candidates[i].same_as;
        cand_dead[i] = u8::from(candidates[i].dead);
        cand_alive[i] = candidates[i].alive;
        cand_bytes[i] = candidates[i].summary.total_bytes;
    }

    let mut ranked_idx = vec![0usize; candidates.len()];
    let ranked_count = rank_fleet_candidates(
        &cand_err,
        &cand_same_as,
        &cand_dead,
        &cand_alive,
        &cand_bytes,
        &cand_kind,
        &cand_locked,
        &mut ranked_idx,
    );
    ranked_idx.truncate(ranked_count);

    let mut any_alive = false;
    let mut any_usable = false;
    let mut any_unknown = false;
    for &idx in &ranked_idx {
        let c = &candidates[idx];
        if c.error.is_empty() {
            any_usable = true;
        }
        if c.same_as < 0 && c.error.is_empty() && !c.dead {
            any_alive = true;
        }
        if c.same_as < 0 && c.error.is_empty() && c.alive < 0 {
            any_unknown = true;
        }
    }

    if donor_stopping() || http_stopped() {
        return emit_fleet_result(out, 0, false, b"SHUTDOWN", &[]);
    }

    let mut added = vec![0i32; candidates.len()];
    let mut out_entries: Vec<FleetOutEntry> = Vec::with_capacity(ranked_idx.len());
    // Without DupeCheck nothing parks the backups (see fleet_member_skip_reason).
    let dupe_check = read_host_opts(host_ctx, host_cb).dupe_check;
    // running_id was read before the health checks: a download of the key that
    // has left the queue since must not block the whole fleet.
    let running_now = if !dupe_check && running_id != 0 {
        let still_queued = with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
            dq_find_nzb(host_ctx, host_cb, dq_ptr, running_id).is_some_and(|(_, in_hist)| !in_hist)
        });
        if still_queued {
            running_id
        } else {
            0
        }
    } else {
        running_id
    };
    let mut any_added = false;
    for (rank_zero, &idx) in ranked_idx.iter().enumerate() {
        let cand = &candidates[idx];
        let member = &members[cand.index];
        let rank = (rank_zero + 1) as i32;
        let mut entry = FleetOutEntry {
            nzb_id: 0,
            name: member.name.clone(),
            rank,
            alive: cand.alive,
            status: Vec::new(),
            same_as: 0,
            same_as_rank: 0,
            member_idx: cand.index as i32,
            reason: Vec::new(),
        };
        if !cand.error.is_empty() {
            entry.status = b"ERROR".to_vec();
            entry.reason = cand.error.clone();
            // a copy already queued stays where it is, but is no donor
            if cand.existing_id != 0 && cand.video_different {
                fleet_set_video(host_ctx, host_cb, cand.existing_id, VIDEO_DIFFERENT);
            }
        } else if cand.same_as >= 0 || cand.existing_id != 0 {
            entry.status = b"SAME_POSTING".to_vec();
            // the copy already queued is the one the intake read
            if let (true, Some(same)) = (cand.existing_id != 0, cand.video) {
                fleet_mark_video(host_ctx, host_cb, cand.existing_id, same);
            }
            if cand.existing_id != 0 && cand.encrypted {
                fleet_mark_encrypted(host_ctx, host_cb, cand.existing_id);
            }
            entry.same_as = if cand.existing_id != 0 {
                cand.existing_id
            } else {
                added[cand.same_as as usize]
            };
            if cand.existing_id == 0 {
                for other in &out_entries {
                    if other.member_idx == cand.same_as {
                        entry.same_as_rank = other.rank;
                    }
                }
            }
        } else if !any_alive {
            entry.status = b"DEAD".to_vec();
        } else if let Some(why) = fleet_member_skip_reason(dupe_check, running_now, any_added) {
            entry.status = b"SKIPPED".to_vec();
            entry.reason = why.to_vec();
        } else {
            let rank_str = rank.to_string().into_bytes();
            let alive_str = if cand.alive >= 0 {
                Some(
                    (if cand.dead { 0 } else { cand.alive })
                        .to_string()
                        .into_bytes(),
                )
            } else {
                None
            };
            let mut params: Vec<(&[u8], &[u8])> = vec![
                (FLEET_PARAM, &rank_str),
                (crate::donorkind::KIND_PARAM, cand.kind.param()),
            ];
            if let Some(av) = alive_str.as_deref() {
                params.push((ALIVE_PARAM, av));
            }
            if let Some(same) = cand.video {
                params.push((VIDEO_PARAM, video_param_value(same)));
            }
            if cand.encrypted {
                params.push((crate::donorkind::LOCKED_PARAM, b"yes"));
            }
            // the video inside, for TwinCheck to tell an alt from a different video
            let inner = intake_prints[idx]
                .as_ref()
                .map(|p| crate::dupeintake::inner_param_value(p, intake_samples, intake_seed));
            if let Some(d) = inner.as_deref() {
                params.push((crate::dupeintake::INNER_PARAM, d.as_bytes()));
            }
            // the video's size and CRC32 from a 7z header, for a copy the
            // intake could not open (a password without -mhe)
            let inner_key = intake_keys[idx].map(|k| k.param());
            if let Some(k) = inner_key.as_deref() {
                params.push((crate::twinfields::INNER_KEY_PARAM, k.as_bytes()));
            }
            let add_name = fleet_ensure_nzb_ext(&member.name);
            let member_score = fleet_calc_member_add_score(running_id, lowest, top, rank);
            entry.nzb_id = scanner_add_external(
                host_ctx,
                host_cb,
                &add_name,
                &category,
                priority,
                &dupe_key,
                member_score,
                &params,
                &member.data,
            );
            added[cand.index] = entry.nzb_id;
            any_added |= entry.nzb_id != 0;
            if entry.nzb_id == 0 {
                entry.status = b"ERROR".to_vec();
                entry.reason = b"RustyGet could not add the nzb-file".to_vec();
            } else if cand.dead {
                entry.status = b"DEAD".to_vec();
            } else {
                entry.status = b"BACKUP".to_vec();
            }
        }
        out_entries.push(entry);
    }

    for w in &kind_writes {
        let partner_queued = w.twin_queued
            || w.partners
                .iter()
                .any(|&p| added[p] != 0 || member_stored[p].is_some());
        let kind = crate::donorkind::DonorKind::of(partner_queued, w.packing);
        fleet_upgrade_kind(host_ctx, host_cb, w.nzb_id, kind);
        if w.packing.headers_encrypted() {
            fleet_mark_encrypted(host_ctx, host_cb, w.nzb_id);
        }
    }

    let mut chosen = 0i32;
    let mut reason: Vec<u8> = Vec::new();
    let on_disk = with_guarded_queue(host_ctx, host_cb, |dq_ptr| {
        let cur = read_host_opts(host_ctx, host_cb);
        if running_id != 0
            && dq_find_nzb(host_ctx, host_cb, dq_ptr, running_id)
                .is_some_and(|(_, in_hist)| !in_hist)
        {
            chosen = running_id;
            reason = b"ALREADY_QUEUED".to_vec();
        }
        for entry in &mut out_entries {
            if running_id == 0
                && chosen == 0
                && entry.nzb_id > 0
                && dq_find_nzb(host_ctx, host_cb, dq_ptr, entry.nzb_id)
                    .is_some_and(|(_, in_hist)| !in_hist)
            {
                entry.status = b"QUEUED".to_vec();
                chosen = entry.nzb_id;
            }
        }
        let check_disk =
            (cur.health_check_mode == 4 || cur.health_check_mode == 2) && !dupe_key.is_empty();
        let mut disk_found = false;
        let (_, h_len) = dq_counts(host_ctx, host_cb, dq_ptr);
        for i in 0..h_len {
            let (_, h_kind, _, p) = dq_history_at(host_ctx, host_cb, dq_ptr, i);
            if h_kind != 1 || p.is_none() {
                continue;
            }
            let item = inspect_nzb(host_ctx, host_cb, p, false);
            if item.delete_status == DeleteStatus::Copy {
                for entry in &mut out_entries {
                    if entry.nzb_id == item.id {
                        entry.status = b"COPY".to_vec();
                        entry.reason =
                            b"the same nzb-file as an item RustyGet has already".to_vec();
                    }
                }
            }
            if !disk_found
                && check_disk
                && item.delete_status == DeleteStatus::None
                && item.dupe_success
                && item.mark_status != MarkStatus::Bad
                && item.dupe_key.eq_ignore_ascii_case(&dupe_key)
                && crate::dupecoord::files_on_disk(
                    &item.final_dir,
                    &item.dest_dir,
                    &cur.ext_cleanup_disk,
                )
            {
                disk_found = true;
            }
        }
        disk_found
    });

    if chosen == 0 && reason.is_empty() {
        let code = eval_fleet_outcome_reason(any_usable, any_alive, any_unknown, on_disk);
        reason = match code {
            0 => b"NO_USABLE_MEMBERS".to_vec(),
            1 => b"INCOMPLETE".to_vec(),
            2 => b"ALL_DEAD".to_vec(),
            3 => b"ALREADY_DOWNLOADED".to_vec(),
            _ => b"NOT_QUEUED".to_vec(),
        };
    }

    // copies the intake could not verify are read again after the reply
    let pending: Vec<(i32, Vec<u8>, NzbSummaryData)> = intake_later
        .iter()
        .filter_map(|&i| {
            let c = &candidates[i];
            // a member already parked from an earlier submission keeps its id
            let id = if c.existing_id > 0 { c.existing_id } else { added[c.index] };
            (id > 0 && !c.dead).then(|| (id, members[c.index].name.clone(), c.summary.clone()))
        })
        .collect();
    if chosen > 0 && !pending.is_empty() {
        let primary = candidates
            .iter()
            .enumerate()
            .find(|(_, c)| added[c.index] == chosen)
            .map(|(i, c)| (chosen, c.summary.clone(), intake_prints[i].clone()))
            .or_else(|| {
                // a backup sent alone: the copy queued from the first submission
                earlier
                    .iter()
                    .find(|(id, _)| *id == chosen)
                    .map(|(id, s)| (*id, s.clone(), None))
            });
        let mut msg = b"Fleet intake: ".to_vec();
        msg.extend_from_slice(pending.len().to_string().as_bytes());
        msg.extend_from_slice(b" unverified copies are checked again in the background");
        host_log(host_ctx, host_cb, MK_INFO, &msg);
        spawn_fleet_intake_later(
            host_cb,
            IntakeLater {
                dupe_key: dupe_key.clone(),
                fleet: intake_print.clone().filter(|p| {
                    intake_prints
                        .iter()
                        .filter(|q| q.as_ref() == Some(p))
                        .count()
                        >= 2
                }),
                primary,
                pending,
                samples: intake_samples,
                seed: intake_seed,
            },
        );
    }

    let summary_log =
        fleet_format_summary_log(members.len() as i32, &dupe_key, chosen, &reason, complete);
    host_log(host_ctx, host_cb, MK_INFO, &summary_log);
    emit_fleet_result(out, chosen, complete, &reason, &out_entries)
}

#[cfg(test)]
mod tests {

    #[test]
    fn health_checks_ask_the_longest_retention_server_of_a_backbone() {
        let r = |id, group, retention, level| HealthServerRank {
            id,
            usable: true,
            group,
            retention,
            level,
        };
        let mut servers = vec![r(1, 5, 1000, 0), r(2, 5, 3000, 1), r(3, 0, 0, 0), r(4, 6, 0, 1), r(5, 6, 900, 0)];
        let order = backbone_order(&servers);
        assert_eq!(order.keys().copied().collect::<Vec<_>>(), vec![2, 4]);
        // the siblings follow, to be asked when the first has no connection free
        assert_eq!(order[&2], vec![2, 1]);
        assert_eq!(order[&4], vec![4, 5]);
        // an unusable server is never asked
        servers[1].usable = false;
        assert_eq!(backbone_order(&servers)[&1], vec![1]);
    }
    #[test]
    fn kind_probe_keeps_a_third_of_the_fleet_time() {
        // the add-on's 45 s minus the 2 s reserve: 14 s for the probe
        assert_eq!(super::fleet_kind_probe_secs(43_000), 14);
        assert_eq!(
            super::fleet_kind_probe_secs(118_000),
            super::FLEET_KIND_PROBE_MAX_SEC
        );
        assert_eq!(super::fleet_kind_probe_secs(2_000), 0);
        assert_eq!(super::fleet_kind_probe_secs(-5), 0);
    }

    use super::*;

    #[test]
    fn earlier_copy_kind_only_gets_better_informed() {
        use crate::donorkind::DonorKind::*;
        // nothing learned writes nothing
        assert!(!kind_upgrades(Unknown, Unknown));
        assert!(!kind_upgrades(Borrowable, Unknown));
        // a copy without a kind gets one
        assert!(kind_upgrades(Unknown, Borrowable));
        assert!(kind_upgrades(Unknown, Unborrowable));
        assert!(kind_upgrades(Unknown, Twin));
        // a twin that turned up wins over packing
        assert!(kind_upgrades(Borrowable, Twin));
        assert!(kind_upgrades(Unborrowable, Twin));
        // packing never takes back a stored twin or another packing
        assert!(!kind_upgrades(Twin, Borrowable));
        assert!(!kind_upgrades(Borrowable, Unborrowable));
        assert!(!kind_upgrades(Twin, Twin));
        // an encrypted copy stored unborrowable, opened by its password now
        assert!(kind_upgrades(Unborrowable, EncryptedBorrowable));
        assert!(!kind_upgrades(EncryptedBorrowable, Unborrowable));
        assert!(!kind_upgrades(Borrowable, EncryptedBorrowable));
    }

    #[test]
    fn check_server_cap_all_connections_or_explicit_limit() {
        // 0: every connection of the server; downloads keep priority in the pool
        assert_eq!(dupe_check_server_cap(60, 0), 60);
        assert_eq!(dupe_check_server_cap(1, 0), 1);
        // positive: an explicit limit, never above the server's connections
        assert_eq!(dupe_check_server_cap(50, 20), 20);
        assert_eq!(dupe_check_server_cap(8, 20), 8);
        assert_eq!(dupe_check_server_cap(1, 20), 1);
        assert_eq!(dupe_check_server_cap(100, 5), 5);
    }

    #[test]
    fn timed_mutex_locks_and_times_out() {
        let tm = Arc::new(TimedMutex::new());
        let g1 = tm.try_lock_for(50);
        assert!(g1.is_some());
        let tm2 = tm.clone();
        let blocked = std::thread::spawn(move || tm2.try_lock_for(20).is_some())
            .join()
            .unwrap();
        assert!(!blocked);
        drop(g1);
        let g2 = tm.try_lock_for(50);
        assert!(g2.is_some());
    }

    #[test]
    fn effective_key_and_search_inner_stamps() {
        assert_eq!(
            effective_key(b"", b"My.Release.2026.1080p.WEB-GRP"),
            b"dupes:my.release.2026.1080p.web.grp"
        );
        assert_eq!(effective_key(b"custom:123", b"Ignored"), b"custom:123");

        let mut inner = SearchInner::default();
        inner.record_search(
            b"dupes:alpha",
            SearchedStamp {
                score: 1_000_000,
                time: 500,
                title: b"alpha".to_vec(),
                retryable: false,
                raw_name: b"Alpha".to_vec(),
            },
        );
        assert!(inner.find_search_cloned(b"dupes:alpha", b"alpha").is_some());
        assert!(inner.find_search_cloned(b"dupes:alpha", b"beta").is_none());
        inner.set_retryable(b"dupes:alpha", b"alpha", true);
        assert!(
            inner
                .find_search_cloned(b"dupes:alpha", b"alpha")
                .unwrap()
                .retryable
        );
    }
}

#[cfg(test)]
mod locked_copy_tests {
    use super::*;

    #[test]
    fn a_name_password_is_the_copys_password() {
        let mut info = NzbSummaryData::default();
        summary_take_name_password(&mut info, b"Show.S01E01.{{secret}}.nzb");
        assert!(summary_password_known(&info));
        assert_eq!(info.meta.get(b"password".as_slice()).unwrap(), b"secret");
        // the nzb's own password wins, as in the scanner
        let mut info = NzbSummaryData::default();
        info.meta.insert(b"Password".to_vec(), b"meta".to_vec());
        summary_take_name_password(&mut info, b"Show.S01E01.{{other}}");
        assert!(!info.meta.contains_key(b"password".as_slice()));
        // no braces, no password
        let mut info = NzbSummaryData::default();
        summary_take_name_password(&mut info, b"Show.S01E01");
        assert!(!summary_password_known(&info));
    }

    #[test]
    fn an_encrypted_copy_is_locked_only_without_a_password() {
        // (no test sets UnpackPassFile)
        assert!(fleet_candidate_locked(true, false, false));
        // in its nzb (meta or name)
        assert!(!fleet_candidate_locked(true, true, false));
        // set on the queued copy after intake
        assert!(!fleet_candidate_locked(true, false, true));
        assert!(!fleet_candidate_locked(false, false, false));
    }
}
