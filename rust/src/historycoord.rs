//! `HistoryCoordinator` (`daemon/queue/HistoryCoordinator.cpp`) end-to-end Rust implementation.

use crate::queuecoord::{
    ARTICLE_FAILED, ARTICLE_FINISHED, ARTICLE_UNDEFINED, DELETE_COPY, DELETE_DUPE, DELETE_GOOD,
    DELETE_MANUAL, DELETE_NONE, DUP_STATUS_BAD, DUP_STATUS_DELETED, DUP_STATUS_DUPE,
    DUP_STATUS_FAILED, DUP_STATUS_GOOD, DUP_STATUS_SUCCESS, MARK_BAD, MARK_GOOD, MARK_SUCCESS,
    PARTIAL_STATE_NONE, PAR_FAILURE, PAR_REPAIR_POSSIBLE, PAR_SUCCESS, UNPACK_FAILURE,
    UNPACK_PASSWORD, UNPACK_SKIPPED, UNPACK_SPACE, UNPACK_SUCCESS,
};

/// `HistoryCoordinator::ServiceWork` minimum history retention timestamp (`cur_time - keep_days * 86400`).
pub fn calc_min_time(cur_time: i64, keep_history_days: i32) -> i64 {
    cur_time.wrapping_sub((keep_history_days as i64).wrapping_mul(60 * 60 * 24))
}

/// Classify action for an item in `HistoryCoordinator::ServiceWork`:
/// - `0`: keep item (`++it; index++;`)
/// - `1`: hide item (`HistoryHide`, `index++`)
/// - `2`: delete disk files & erase from history
/// - `3`: erase from history without deleting disk files
pub fn classify_service_work_item(
    kind: i32,
    hk_dup: i32,
    hk_nzb: i32,
    item_time: i64,
    min_time: i64,
    dupe_check: bool,
) -> i32 {
    if kind != hk_dup && item_time < min_time {
        if dupe_check && kind == hk_nzb {
            1
        } else if kind == hk_nzb {
            2
        } else {
            3
        }
    } else {
        0
    }
}

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

/// Decides whether `HistoryCoordinator::RecheckFailedArticles` passes its initial eligibility gate.
pub fn should_recheck_failed_articles(
    daf_none: bool,
    hc_dupe: bool,
    kind_nzb: bool,
    failed_articles: i32,
    text_status: &[u8],
    has_dupe_recheck_param: bool,
) -> bool {
    !((daf_none && !hc_dupe)
        || !kind_nzb
        || failed_articles <= 0
        || !text_status.starts_with(b"FAILURE")
        || text_status == b"FAILURE/BAD"
        || has_dupe_recheck_param)
}

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

/// Decides whether `HistoryCoordinator::HistoryDelete` should delete the destination directory.
#[allow(clippy::too_many_arguments)]
pub fn should_delete_dest_dir(
    is_nzb: bool,
    delete_status: i32,
    ds_none: i32,
    par_status: i32,
    ps_failure: i32,
    unpack_status: i32,
    us_failure: i32,
    us_password: i32,
    dir_exists: bool,
) -> bool {
    is_nzb
        && (delete_status != ds_none
            || par_status == ps_failure
            || unpack_status == us_failure
            || unpack_status == us_password)
        && dir_exists
}

/// Calculate reset `PostTotalSec` and normalized `MoveStatus` in `HistoryCoordinator::MoveToQueue`.
#[allow(clippy::too_many_arguments)]
pub fn calc_move_to_queue_reset(
    unpack_cleaned_up_disk: bool,
    post_total_sec: i32,
    unpack_sec: i32,
    has_main_pars: bool,
    par_sec: i32,
    move_status: i32,
    ms_failure: i32,
    ms_none: i32,
) -> (i32, i32) {
    let mut new_post_total_sec = post_total_sec;
    if !unpack_cleaned_up_disk {
        new_post_total_sec -= unpack_sec;
        if has_main_pars {
            new_post_total_sec -= par_sec;
        }
    }
    let new_move_status = if move_status == ms_failure {
        ms_none
    } else {
        move_status
    };
    (new_post_total_sec, new_move_status)
}

/// `HistoryCoordinator::HistoryRetry` log action labels.
pub fn retry_action_labels(
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

/// Evaluate whether a loaded file in `HistoryCoordinator::HistoryRetry` should be requeued.
pub fn eval_requeue_loaded_file(
    status: i32,
    cf_none: i32,
    cf_failure: i32,
    cf_partial: i32,
    reset_failed: bool,
    state_loaded: bool,
    remaining_size: i64,
) -> bool {
    status == cf_none
        || (status == cf_failure && reset_failed)
        || (status == cf_partial && state_loaded && (reset_failed || remaining_size > 0))
}

/// Checks whether a recorded completed filename is a `.out.tmp` temporary output name.
pub fn is_temporary_output_name(filename: &[u8]) -> bool {
    filename.len() >= 8 && filename.ends_with(b".out.tmp")
}

/// `HistoryCoordinator::AddToHistory` (`HistoryCoordinator.cpp`):
/// Determines whether a completed file entry should be counted as parked.
pub fn should_park_completed_file(
    status: i32,
    cf_none: i32,
    cf_partial: i32,
    is_last: bool,
) -> bool {
    status == cf_none || (status == cf_partial && is_last)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequeueFilePrepOutcome {
    pub set_recorded_filename: bool,
    pub new_partial_state: i32,
    pub warn_missing_on_disk: bool,
    pub no_retryable_articles: bool,
}

/// `HistoryCoordinator::HistoryRetry` (`HistoryCoordinator.cpp`):
/// Resolves recorded filename, cleans up 0-byte or 0-success output files, updates
/// `partial_state` (`psCompleted` vs `psNone`), and checks whether any article is retryable.
#[allow(clippy::too_many_arguments)]
pub fn prepare_requeued_loaded_file(
    dest_dir: &[u8],
    path_sep: u8,
    recorded_name: &[u8],
    current_file_filename: &[u8],
    success_articles: i32,
    current_partial_state: i32,
    ps_none: i32,
    ps_completed: i32,
    reprocess: bool,
    reset_failed: bool,
    article_statuses: &[i32],
    ai_undefined: i32,
    ai_failed: i32,
    file_size_fn: impl FnOnce(&[u8]) -> i64,
    delete_file_fn: impl FnOnce(&[u8]),
    file_exists_fn: impl FnOnce(&[u8]) -> bool,
) -> RequeueFilePrepOutcome {
    let set_recorded_filename =
        !is_temporary_output_name(recorded_name) || current_file_filename.is_empty();
    let mut output_filename = crate::artwritercore::article_writer_format_dest_file_path(
        dest_dir,
        path_sep,
        recorded_name,
    );
    if output_filename.len() > 1023 {
        output_filename.truncate(1023);
    }

    let out_file_size = if success_articles == 0 {
        0
    } else {
        file_size_fn(&output_filename)
    };
    if success_articles == 0 || out_file_size == 0 {
        delete_file_fn(&output_filename);
    }

    let mut new_partial_state = current_partial_state;
    let mut warn_missing_on_disk = false;
    if success_articles > 0 {
        if file_exists_fn(&output_filename) {
            new_partial_state = ps_completed;
        } else if !reprocess {
            warn_missing_on_disk = true;
            new_partial_state = ps_none;
        }
    }

    let no_retryable_articles = new_partial_state == ps_completed
        && !has_retryable_article(article_statuses, reset_failed, ai_undefined, ai_failed);

    RequeueFilePrepOutcome {
        set_recorded_filename,
        new_partial_state,
        warn_missing_on_disk,
        no_retryable_articles,
    }
}

/// Check if a completed partial file has at least one retryable article in `HistoryCoordinator::HistoryRetry`.
pub fn has_retryable_article(
    statuses: &[i32],
    reset_failed: bool,
    ai_undefined: i32,
    ai_failed: i32,
) -> bool {
    statuses
        .iter()
        .any(|&st| st == ai_undefined || (reset_failed && st == ai_failed))
}

/// Apply initial header & NzbInfo counter adjustments in `HistoryCoordinator::ResetArticles`.
#[allow(clippy::too_many_arguments)]
pub fn reset_articles_header(
    all_failed: bool,
    is_par_file: bool,
    file_size: i64,
    file_missed_size: i64,
    file_total_articles: i32,
    file_missed_articles: i32,
    cur_file_failed_size: i64,
    cur_file_failed_articles: i32,
    cur_file_remaining_size: i64,
    cur_file_completed_articles: i32,
    file_success_size: i64,
    file_success_articles: i32,
    cur_nzb_failed_size: i64,
    cur_nzb_success_size: i64,
    cur_nzb_failed_articles: i32,
    cur_nzb_success_articles: i32,
    cur_nzb_par_failed_size: i64,
    cur_nzb_par_success_size: i64,
) -> (i64, i32, i64, i32, i64, i64, i32, i32, i64, i64) {
    let (file_failed_size, file_failed_articles, file_remaining_size, file_completed_articles) =
        if all_failed {
            let fs = file_size - file_missed_size;
            let fa = file_total_articles - file_missed_articles;
            (fs, fa, 0, fa)
        } else {
            (
                cur_file_failed_size,
                cur_file_failed_articles,
                cur_file_remaining_size,
                cur_file_completed_articles,
            )
        };

    let nzb_failed_size = cur_nzb_failed_size - file_failed_size;
    let nzb_success_size = cur_nzb_success_size - file_success_size;
    let nzb_failed_articles = cur_nzb_failed_articles - file_failed_articles;
    let nzb_success_articles = cur_nzb_success_articles - file_success_articles;
    let (nzb_par_failed_size, nzb_par_success_size) = if is_par_file {
        (
            cur_nzb_par_failed_size - file_failed_size,
            cur_nzb_par_success_size - file_success_size,
        )
    } else {
        (cur_nzb_par_failed_size, cur_nzb_par_success_size)
    };

    (
        file_failed_size,
        file_failed_articles,
        file_remaining_size,
        file_completed_articles,
        nzb_failed_size,
        nzb_success_size,
        nzb_failed_articles,
        nzb_success_articles,
        nzb_par_failed_size,
        nzb_par_success_size,
    )
}

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

/// Apply per-article counter delta when resetting an article in `HistoryCoordinator::ResetArticles`.
#[allow(clippy::too_many_arguments)]
pub fn apply_article_reset_delta(
    pa_status: i32,
    ai_failed: i32,
    ai_undefined: i32,
    ai_finished: i32,
    pa_size: i32,
    cur_completed_articles: i32,
    cur_remaining_size: i64,
    cur_failed_articles: i32,
    cur_failed_size: i64,
    cur_success_articles: i32,
    cur_success_size: i64,
) -> (i32, i64, i32, i64, i32, i64) {
    let completed_articles = cur_completed_articles - 1;
    let remaining_size = cur_remaining_size + pa_size as i64;
    let mut failed_articles = cur_failed_articles;
    let mut failed_size = cur_failed_size;
    let mut success_articles = cur_success_articles;
    let mut success_size = cur_success_size;

    if pa_status == ai_failed || pa_status == ai_undefined {
        failed_articles -= 1;
        failed_size -= pa_size as i64;
    } else if pa_status == ai_finished {
        success_articles -= 1;
        success_size -= pa_size as i64;
    }

    (
        completed_articles,
        remaining_size,
        failed_articles,
        failed_size,
        success_articles,
        success_size,
    )
}

/// `HistoryCoordinator::ResetArticles` (`HistoryCoordinator.cpp`):
/// Resets retryable articles in `article_statuses` to `ARTICLE_UNDEFINED`, populates
/// `out_reset_flags` (`1` if reset, `0` otherwise), and updates all `FileInfo` and `NzbInfo`
/// counters in place.
#[allow(clippy::too_many_arguments)]
pub fn reset_articles_batch(
    all_failed: bool,
    reset_failed: bool,
    is_par_file: bool,
    partial_state: i32,
    file_size: i64,
    file_missed_size: i64,
    file_total_articles: i32,
    file_missed_articles: i32,
    file_failed_size: &mut i64,
    file_failed_articles: &mut i32,
    file_remaining_size: &mut i64,
    file_completed_articles: &mut i32,
    file_success_size: &mut i64,
    file_success_articles: &mut i32,
    nzb_failed_size: &mut i64,
    nzb_success_size: &mut i64,
    nzb_failed_articles: &mut i32,
    nzb_success_articles: &mut i32,
    nzb_par_failed_size: &mut i64,
    nzb_par_success_size: &mut i64,
    article_statuses: &mut [i32],
    article_sizes: &[i32],
    out_reset_flags: &mut [u8],
) {
    let (ffs, ffa, frs, fca, nfs, nss, nfa, nsa, npfs, npss) = reset_articles_header(
        all_failed,
        is_par_file,
        file_size,
        file_missed_size,
        file_total_articles,
        file_missed_articles,
        *file_failed_size,
        *file_failed_articles,
        *file_remaining_size,
        *file_completed_articles,
        *file_success_size,
        *file_success_articles,
        *nzb_failed_size,
        *nzb_success_size,
        *nzb_failed_articles,
        *nzb_success_articles,
        *nzb_par_failed_size,
        *nzb_par_success_size,
    );
    *file_failed_size = ffs;
    *file_failed_articles = ffa;
    *file_remaining_size = frs;
    *file_completed_articles = fca;
    *nzb_failed_size = nfs;
    *nzb_success_size = nss;
    *nzb_failed_articles = nfa;
    *nzb_success_articles = nsa;
    *nzb_par_failed_size = npfs;
    *nzb_par_success_size = npss;

    let count = article_statuses
        .len()
        .min(article_sizes.len())
        .min(out_reset_flags.len());
    for i in 0..count {
        let st = article_statuses[i];
        if should_reset_article(st, partial_state, all_failed, reset_failed) {
            let (ca, rs, fa, fs, sa, ss) = apply_article_reset_delta(
                st,
                ARTICLE_FAILED,
                ARTICLE_UNDEFINED,
                ARTICLE_FINISHED,
                article_sizes[i],
                *file_completed_articles,
                *file_remaining_size,
                *file_failed_articles,
                *file_failed_size,
                *file_success_articles,
                *file_success_size,
            );
            *file_completed_articles = ca;
            *file_remaining_size = rs;
            *file_failed_articles = fa;
            *file_failed_size = fs;
            *file_success_articles = sa;
            *file_success_size = ss;
            article_statuses[i] = ARTICLE_UNDEFINED;
            out_reset_flags[i] = 1;
        } else {
            out_reset_flags[i] = 0;
        }
    }
}

/// Parses the `eaHistorySetDupeBackup` argument into `NzbInfo::dsDupe` or `NzbInfo::dsManual`.
pub fn parse_dupe_backup_flag(text: &[u8], ds_dupe: i32, ds_manual: i32) -> i32 {
    if text.eq_ignore_ascii_case(b"YES") || text.eq_ignore_ascii_case(b"TRUE") || text == b"1" {
        ds_dupe
    } else {
        ds_manual
    }
}

/// Splits a `"name=value"` parameter assignment at the first `'='`.
/// Returns `Some((name, value))` if `'='` is present, else `None`
/// (`HistoryCoordinator::HistorySetParameter`, `QueueEditor::SetNzbParameter`).
pub fn split_param_assignment(text: &[u8]) -> Option<(&[u8], &[u8])> {
    text.iter()
        .position(|&b| b == b'=')
        .map(|eq| (&text[..eq], &text[eq + 1..]))
}

// ============================================================================
// End-to-end `HistoryCoordinator` FFI coordinator (`daemon/queue/HistoryCoordinator.cpp`)
// ============================================================================

use std::ffi::{c_char, c_int, c_void, CStr};

#[repr(C)]
pub struct NzbgetRsHistoryCoordEnv {
    pub ctx: *mut c_void,
    pub p1: usize,
    pub p2: usize,
    pub a1: c_int,
    pub a2: c_int,
    pub a3: c_int,
    pub a4: c_int,
    pub a5: c_int,
    pub a6: c_int,
    pub t1: i64,
    pub s1: *const c_char,
    pub int_arr: *const c_int,
    pub int_len: usize,
    pub log_cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>,
    pub action_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            c_int,
            c_int,
            usize,
            usize,
            *const c_char,
            *const c_char,
            *mut c_int,
            *mut i64,
            *mut *const c_char,
            *mut u8,
            usize,
        ) -> usize,
    >,
}

fn hc_cbuf(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let mut v = Vec::with_capacity(end + 1);
    v.extend_from_slice(&bytes[..end]);
    v.push(0);
    v
}

unsafe fn hc_cstr_bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        CStr::from_ptr(p).to_bytes()
    }
}

fn hc_bytes_as_cstr(buf: &[u8]) -> &CStr {
    CStr::from_bytes_with_nul(buf).unwrap_or(c"")
}

unsafe fn hc_log(env: &NzbgetRsHistoryCoordEnv, level: c_int, msg: &[u8]) {
    if let Some(cb) = env.log_cb {
        let c = hc_cbuf(msg);
        cb(env.ctx, level, c.as_ptr() as *const c_char);
    }
}

unsafe fn hc_nzb_msg(env: &NzbgetRsHistoryCoordEnv, nzb_ptr: usize, kind: c_int, msg: &[u8]) {
    if let Some(cb) = env.action_cb {
        let c = hc_cbuf(msg);
        cb(
            env.ctx,
            4,
            kind,
            0,
            0,
            nzb_ptr,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        );
    }
}

unsafe fn hc_delete_disk_files(
    env: &NzbgetRsHistoryCoordEnv,
    nzb_ptr: usize,
    server_mode: bool,
    nzb_cleanup_disk: bool,
) {
    let Some(cb) = env.action_cb else {
        return;
    };
    if nzb_ptr == 0 {
        return;
    }
    if server_mode {
        cb(
            env.ctx,
            0,
            1,
            0,
            0,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        );
    }
    let mut strs: [*const c_char; 1] = [std::ptr::null()];
    cb(
        env.ctx,
        1,
        0,
        0,
        0,
        nzb_ptr,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        strs.as_mut_ptr(),
        std::ptr::null_mut(),
        0,
    );
    if !nzb_cleanup_disk {
        return;
    }
    let queued_fn = hc_cstr_bytes(strs[0]).to_vec();
    let mut pos = 0usize;
    while pos <= queued_fn.len() {
        let Some((ts, te, next_pos)) =
            crate::util::tokenizer_find_next_token(&queued_fn[pos..], b"|")
        else {
            break;
        };
        let name1 = &queued_fn[pos + ts..pos + te];
        pos += next_pos;
        if crate::fsops::file_exists(name1, &crate::fsops::posix_stat) {
            let name_c = hc_cbuf(name1);
            let cstr = hc_bytes_as_cstr(&name_c);
            let mut msg = b"Deleting file ".to_vec();
            msg.extend_from_slice(name1);
            hc_log(env, 2, &msg);
            crate::fsops::delete_file(cstr);
        }
    }
}

unsafe fn hc_delete_dest_dir_if_present(
    env: &NzbgetRsHistoryCoordEnv,
    dest_dir: &[u8],
    sep: u8,
    log_level: c_int,
) {
    if crate::fsops::directory_exists(dest_dir, &crate::fsops::posix_stat) {
        let mut msg = b"Deleting ".to_vec();
        msg.extend_from_slice(dest_dir);
        hc_log(env, log_level, &msg);
        let mut err_text = Vec::new();
        let done = crate::fsdelete::delete_directory_with_content(
            &mut crate::fsops::PosixFsOps,
            dest_dir,
            sep,
            &mut err_text,
        )
        .unwrap_or(false);
        if !done {
            let mut em = b"Could not delete directory ".to_vec();
            em.extend_from_slice(dest_dir);
            em.extend_from_slice(b": ");
            let end = err_text
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(err_text.len());
            em.extend_from_slice(&err_text[..end]);
            hc_log(env, 4, &em);
        }
    }
}

unsafe extern "C" fn hc_emit_failed_sample(
    ctx: *mut c_void,
    msg_id: *const c_char,
    groups_ptr: *const *const c_char,
    groups_len: usize,
) {
    if ctx.is_null() || msg_id.is_null() {
        return;
    }
    let out = &mut *(ctx as *mut Vec<crate::dupeprobe::ProbeSample>);
    let mid = hc_cstr_bytes(msg_id).to_vec();
    if mid.is_empty() {
        return;
    }
    let mut groups = Vec::with_capacity(groups_len);
    if !groups_ptr.is_null() && groups_len > 0 {
        let sl = std::slice::from_raw_parts(groups_ptr, groups_len);
        for &gp in sl {
            if !gp.is_null() {
                groups.push(hc_cstr_bytes(gp).to_vec());
            }
        }
    }
    out.push(crate::dupeprobe::ProbeSample {
        message_id: mid,
        groups,
    });
}

unsafe fn hc_recheck_failed_articles(env: &NzbgetRsHistoryCoordEnv, dq_ptr: usize, nzb_ptr: usize) {
    let Some(cb) = env.action_cb else {
        return;
    };
    #[cfg(unix)]
    if let Some(qh) = crate::ffi::queue_host() {
        let opts = qh.opts();
        let nzb = qh.nzb_snap(nzb_ptr, true);
        let has_recheck = qh.nzb_get_param(nzb_ptr, b"*DupeRecheck").is_some();
        if !should_recheck_failed_articles(
            opts.dupe_article_fallback == 0,
            opts.health_check == 4,
            nzb.kind == 0,
            nzb.failed_articles,
            &nzb.text_status,
            has_recheck,
        ) {
            return;
        }
        let q_len = qh.queue_len(dq_ptr);
        for qidx in 0..q_len {
            let q = qh.nzb_snap(qh.queue_at(dq_ptr, qidx), false);
            if crate::dupecoord::same_name_or_key(&q.name, &q.dupe_key, &nzb.name, &nzb.dupe_key) {
                return;
            }
        }
        if opts.dupe_check
            && nzb.dupe_mode == crate::queuesubsys_host::DM_SCORE
            && crate::dupecoord::find_dupe_backup_host(
                &qh,
                &opts,
                dq_ptr,
                nzb_ptr,
                &nzb.name,
                &nzb.dupe_key,
            )
            .is_some()
        {
            return;
        }
        let mut failed: Vec<crate::dupeprobe::ProbeSample> = Vec::new();
        cb(
            env.ctx,
            10,
            0,
            0,
            std::ptr::addr_of_mut!(failed) as usize,
            nzb_ptr,
            hc_emit_failed_sample as *const c_void as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        );
        if failed.len() < 2 {
            return;
        }
        let idxs = crate::dupeprobe::sample_indexes(failed.len(), 10);
        let samples: Vec<_> = idxs.into_iter().map(|i| failed[i].clone()).collect();
        crate::dupeprobe::start_recheck_rs(nzb.id, samples);
    }
    #[cfg(not(unix))]
    let _ = (cb, dq_ptr, nzb_ptr);
}

unsafe fn hc_history_hide(
    env: &NzbgetRsHistoryCoordEnv,
    dq_ptr: usize,
    hist_ptr: usize,
    rindex: c_int,
    server_mode: bool,
    nzb_cleanup_disk: bool,
) {
    let Some(cb) = env.action_cb else {
        return;
    };
    let mut ints = [0i32; 3];
    let mut strs: [*const c_char; 1] = [std::ptr::null()];
    let nzb_ptr = cb(
        env.ctx,
        12,
        0,
        0,
        dq_ptr,
        hist_ptr,
        std::ptr::null(),
        std::ptr::null(),
        ints.as_mut_ptr(),
        std::ptr::null_mut(),
        strs.as_mut_ptr(),
        std::ptr::null_mut(),
        0,
    );
    let hist_name = hc_cstr_bytes(strs[0]).to_vec();
    let dup_status = classify_dup_status(ints[0], ints[1], ints[2] != 0);

    hc_delete_disk_files(env, nzb_ptr, server_mode, nzb_cleanup_disk);

    let mut msg = b"Collection ".to_vec();
    msg.extend_from_slice(&hist_name);
    msg.extend_from_slice(b" removed from history");
    hc_log(env, 2, &msg);

    cb(
        env.ctx,
        13,
        dup_status,
        rindex,
        dq_ptr,
        hist_ptr,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        0,
    );
}

unsafe fn hc_move_to_queue(
    env: &NzbgetRsHistoryCoordEnv,
    dq_ptr: usize,
    hist_ptr: usize,
    hidx: c_int,
    nzb_ptr: usize,
    hist_name: &[u8],
    reprocess: bool,
) {
    let Some(cb) = env.action_cb else {
        return;
    };
    let mut dbg_msg = b"Returning ".to_vec();
    dbg_msg.extend_from_slice(hist_name);
    dbg_msg.extend_from_slice(b" from history back to download queue");
    hc_log(env, 0, &dbg_msg);

    let nicename = hist_name.to_vec();

    let mut ints = [0i32; 6];
    let mut strs: [*const c_char; 1] = [std::ptr::null()];
    cb(
        env.ctx,
        21,
        -1,
        0,
        0,
        nzb_ptr,
        std::ptr::null(),
        std::ptr::null(),
        ints.as_mut_ptr(),
        std::ptr::null_mut(),
        strs.as_mut_ptr(),
        std::ptr::null_mut(),
        0,
    );
    let unpack_cleaned_up_disk = ints[0] != 0;
    let post_total_sec = ints[1];
    let unpack_sec = ints[2];
    let par_sec = ints[3];
    let move_status = ints[4];
    let file_count = ints[5];
    let dest_dir = hc_cstr_bytes(strs[0]).to_vec();

    for fidx in 0..file_count {
        let mut fstrs: [*const c_char; 1] = [std::ptr::null()];
        cb(
            env.ctx,
            21,
            fidx,
            0,
            0,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fstrs.as_mut_ptr(),
            std::ptr::null_mut(),
            0,
        );
        let mut unpark = b"Unparking file ".to_vec();
        unpark.extend_from_slice(hc_cstr_bytes(fstrs[0]));
        hc_nzb_msg(env, nzb_ptr, 1, &unpark);
    }

    let has_main_pars = if !unpack_cleaned_up_disk {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&dest_dir));
            crate::parparser::find_main_pars(p, false, |_| {})
        }
        #[cfg(not(unix))]
        {
            false
        }
    } else {
        false
    };

    let (new_post_total_sec, new_move_status) = calc_move_to_queue_reset(
        unpack_cleaned_up_disk,
        post_total_sec,
        unpack_sec,
        has_main_pars,
        par_sec,
        move_status,
        1,
        0,
    );
    let mut apply_ints = [
        c_int::from(has_main_pars),
        new_post_total_sec,
        new_move_status,
    ];
    cb(
        env.ctx,
        22,
        hidx,
        c_int::from(reprocess),
        dq_ptr,
        hist_ptr,
        std::ptr::null(),
        std::ptr::null(),
        apply_ints.as_mut_ptr(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        0,
    );

    let mut ret_msg = nicename.clone();
    ret_msg.extend_from_slice(b" returned from history back to download queue");
    hc_nzb_msg(env, nzb_ptr, 2, &ret_msg);

    if reprocess {
        let mut rst_msg = b"Restarting postprocessing for ".to_vec();
        rst_msg.extend_from_slice(&nicename);
        hc_log(env, 0, &rst_msg);
        cb(
            env.ctx,
            23,
            0,
            0,
            dq_ptr,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        );
        #[cfg(unix)]
        {
            crate::feed::notify_active_feed_queue_aspect(4, None);
            crate::urlcoord::notify_active_urlcoord_queue_update(4);
        }
    }
}

unsafe fn hc_history_redownload(
    env: &NzbgetRsHistoryCoordEnv,
    dq_ptr: usize,
    hist_ptr: usize,
    hidx: c_int,
    kind: c_int,
    nzb_ptr: usize,
    hist_name: &[u8],
    restore_pause_state: bool,
) {
    let Some(cb) = env.action_cb else {
        return;
    };
    if kind == 2 {
        cb(
            env.ctx,
            25,
            hidx,
            0,
            dq_ptr,
            hist_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        );
        #[cfg(unix)]
        {
            crate::feed::notify_active_feed_queue_aspect(12, None);
            crate::urlcoord::notify_active_urlcoord_queue_update(12);
        }
        return;
    }
    if kind != 1 {
        let mut em = b"Could not download again ".to_vec();
        em.extend_from_slice(hist_name);
        em.extend_from_slice(b": history item has wrong type");
        hc_log(env, 4, &em);
        return;
    }

    let mut ints = [0i32; 2];
    let mut strs: [*const c_char; 3] = [std::ptr::null(); 3];
    cb(
        env.ctx,
        26,
        0,
        0,
        0,
        nzb_ptr,
        std::ptr::null(),
        std::ptr::null(),
        ints.as_mut_ptr(),
        std::ptr::null_mut(),
        strs.as_mut_ptr(),
        std::ptr::null_mut(),
        0,
    );
    let paused = restore_pause_state && ints[0] != 0;
    let nzb_name = hc_cstr_bytes(strs[0]).to_vec();
    let queued_fn = hc_cstr_bytes(strs[1]).to_vec();
    let old_dest_dir = hc_cstr_bytes(strs[2]).to_vec();

    let qf_c = hc_cbuf(&queued_fn);
    if !crate::fsops::file_exists(&queued_fn, &crate::fsops::posix_stat) {
        let mut em = b"Could not download again ".to_vec();
        em.extend_from_slice(&nzb_name);
        em.extend_from_slice(b": could not find source nzb-file ");
        em.extend_from_slice(&queued_fn);
        hc_log(env, 4, &em);
        return;
    }

    let new_nzb_ptr = cb(
        env.ctx,
        27,
        0,
        0,
        0,
        0,
        qf_c.as_ptr() as *const c_char,
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        0,
    );
    if new_nzb_ptr == 0 {
        let mut em = b"Could not download again ".to_vec();
        em.extend_from_slice(&nzb_name);
        em.extend_from_slice(b": could not parse nzb-file");
        hc_log(env, 4, &em);
        return;
    }

    let mut info_m = b"Downloading again ".to_vec();
    info_m.extend_from_slice(&nzb_name);
    hc_log(env, 2, &info_m);

    let sep = env.a5 as u8;
    hc_delete_dest_dir_if_present(env, &old_dest_dir, sep, 1);

    let mut dstrs: [*const c_char; 1] = [std::ptr::null()];
    cb(
        env.ctx,
        24,
        0,
        0,
        0,
        nzb_ptr,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        dstrs.as_mut_ptr(),
        std::ptr::null_mut(),
        0,
    );
    let new_dest_dir = hc_cstr_bytes(dstrs[0]).to_vec();
    hc_delete_dest_dir_if_present(env, &new_dest_dir, sep, 1);

    cb(
        env.ctx,
        28,
        c_int::from(paused),
        0,
        new_nzb_ptr,
        nzb_ptr,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        0,
    );

    hc_move_to_queue(env, dq_ptr, hist_ptr, hidx, nzb_ptr, hist_name, false);

    cb(
        env.ctx,
        23,
        1,
        0,
        dq_ptr,
        nzb_ptr,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        0,
    );
    #[cfg(unix)]
    {
        crate::feed::notify_active_feed_queue_aspect(4, None);
        crate::urlcoord::notify_active_urlcoord_queue_update(4);
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn hc_history_retry(
    env: &NzbgetRsHistoryCoordEnv,
    dq_ptr: usize,
    hist_ptr: usize,
    hidx: c_int,
    kind: c_int,
    nzb_ptr: usize,
    hist_name: &[u8],
    reset_failed: bool,
    reprocess: bool,
) {
    let Some(cb) = env.action_cb else {
        return;
    };
    let (wrong_type, missing_dir, start_msg) = retry_action_labels(reset_failed, reprocess);
    if kind != 1 {
        let mut em = b"Could not ".to_vec();
        em.extend_from_slice(&wrong_type[..wrong_type.len() - 1]);
        em.extend_from_slice(b" for ");
        em.extend_from_slice(hist_name);
        em.extend_from_slice(b": history item has wrong type");
        hc_log(env, 4, &em);
        return;
    }

    let mut ints = [0i32; 2];
    let mut strs: [*const c_char; 3] = [std::ptr::null(); 3];
    cb(
        env.ctx,
        26,
        0,
        0,
        0,
        nzb_ptr,
        std::ptr::null(),
        std::ptr::null(),
        ints.as_mut_ptr(),
        std::ptr::null_mut(),
        strs.as_mut_ptr(),
        std::ptr::null_mut(),
        0,
    );
    let nzb_name = hc_cstr_bytes(strs[0]).to_vec();
    let dest_dir = hc_cstr_bytes(strs[2]).to_vec();

    if !crate::fsops::directory_exists(&dest_dir, &crate::fsops::posix_stat) {
        let mut em = b"Could not ".to_vec();
        em.extend_from_slice(&missing_dir[..missing_dir.len() - 1]);
        em.push(b' ');
        em.extend_from_slice(hist_name);
        em.extend_from_slice(b": destination directory ");
        em.extend_from_slice(&dest_dir);
        em.extend_from_slice(b" doesn't exist");
        hc_log(env, 4, &em);
        return;
    }

    let mut sm = start_msg[..start_msg.len() - 1].to_vec();
    sm.push(b' ');
    sm.extend_from_slice(&nzb_name);
    hc_nzb_msg(env, nzb_ptr, 2, &sm);

    let sep = env.a5 as u8;
    let mut requeued: Vec<c_int> = Vec::new();
    let mut cidx = 0i32;

    loop {
        let mut cf_ints = [0i32; 2];
        let mut cf_strs: [*const c_char; 1] = [std::ptr::null()];
        let exists = cb(
            env.ctx,
            3,
            cidx,
            0,
            0,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            cf_ints.as_mut_ptr(),
            std::ptr::null_mut(),
            cf_strs.as_mut_ptr(),
            std::ptr::null_mut(),
            0,
        );
        if exists == 0 {
            break;
        }
        let cf_status = cf_ints[0];
        let cf_id = cf_ints[1];
        let recorded_name = hc_cstr_bytes(cf_strs[0]).to_vec();

        if cf_status != 1 && (cf_status != 3 || reset_failed) && cf_id > 0 {
            let mut f_ints = [0i32; 12];
            let mut f_i64s = [0i64; 9];
            let mut f_strs: [*const c_char; 1] = [std::ptr::null()];
            let file_ptr = cb(
                env.ctx,
                30,
                cf_id,
                cf_status,
                0,
                nzb_ptr,
                std::ptr::null(),
                std::ptr::null(),
                f_ints.as_mut_ptr(),
                f_i64s.as_mut_ptr(),
                f_strs.as_mut_ptr(),
                std::ptr::null_mut(),
                0,
            );
            if file_ptr != 0 {
                let state_loaded = f_ints[0] != 0;
                let success_articles = f_ints[1];
                let cur_partial_state = f_ints[2];
                let article_count = f_ints[3].max(0) as usize;
                let is_par_file = f_ints[4] != 0;
                let total_articles = f_ints[5];
                let missed_articles = f_ints[6];
                let mut file_failed_articles = f_ints[7];
                let mut file_completed_articles = f_ints[8];
                let mut file_success_articles = f_ints[9];
                let mut nzb_failed_articles = f_ints[10];
                let mut nzb_success_articles = f_ints[11];

                let file_size = f_i64s[0];
                let file_missed_size = f_i64s[1];
                let mut file_failed_size = f_i64s[2];
                let mut file_remaining_size = f_i64s[3];
                let mut file_success_size = f_i64s[4];
                let mut nzb_failed_size = f_i64s[5];
                let mut nzb_success_size = f_i64s[6];
                let mut nzb_par_failed_size = f_i64s[7];
                let mut nzb_par_success_size = f_i64s[8];

                let cur_filename = hc_cstr_bytes(f_strs[0]).to_vec();

                if eval_requeue_loaded_file(
                    cf_status,
                    0,
                    3,
                    2,
                    reset_failed,
                    state_loaded,
                    file_remaining_size,
                ) {
                    let mut st_sz = vec![0i32; article_count * 2];
                    if article_count > 0 {
                        cb(
                            env.ctx,
                            32,
                            0,
                            0,
                            0,
                            file_ptr,
                            std::ptr::null(),
                            std::ptr::null(),
                            st_sz.as_mut_ptr(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            article_count,
                        );
                    }
                    let (statuses_slice, sizes_slice) = st_sz.split_at_mut(article_count);

                    let prep = prepare_requeued_loaded_file(
                        &dest_dir,
                        sep,
                        &recorded_name,
                        &cur_filename,
                        success_articles,
                        cur_partial_state,
                        0,
                        2,
                        reprocess,
                        reset_failed,
                        statuses_slice,
                        ARTICLE_UNDEFINED,
                        ARTICLE_FAILED,
                        |p| {
                            let pc = hc_cbuf(p);
                            crate::fsops::file_size(hc_bytes_as_cstr(&pc))
                        },
                        |p| {
                            let pc = hc_cbuf(p);
                            crate::fsops::delete_file(hc_bytes_as_cstr(&pc));
                        },
                        |p| crate::fsops::file_exists(p, &crate::fsops::posix_stat),
                    );

                    let effective_filename = if prep.set_recorded_filename {
                        let rec_c = hc_cbuf(&recorded_name);
                        cb(
                            env.ctx,
                            31,
                            1,
                            0,
                            0,
                            file_ptr,
                            rec_c.as_ptr() as *const c_char,
                            std::ptr::null(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            0,
                        );
                        recorded_name.clone()
                    } else {
                        cur_filename
                    };

                    if prep.warn_missing_on_disk {
                        let mut wm = b"File ".to_vec();
                        wm.extend_from_slice(&effective_filename);
                        wm.extend_from_slice(b" could not be found on disk, downloading again");
                        hc_nzb_msg(env, nzb_ptr, 3, &wm);
                    }

                    if prep.no_retryable_articles {
                        cb(
                            env.ctx,
                            31,
                            0,
                            0,
                            0,
                            file_ptr,
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            0,
                        );
                        cidx += 1;
                        continue;
                    }

                    let mut reset_flags = vec![0u8; article_count];
                    reset_articles_batch(
                        cf_status == 3,
                        reset_failed,
                        is_par_file,
                        prep.new_partial_state,
                        file_size,
                        file_missed_size,
                        total_articles,
                        missed_articles,
                        &mut file_failed_size,
                        &mut file_failed_articles,
                        &mut file_remaining_size,
                        &mut file_completed_articles,
                        &mut file_success_size,
                        &mut file_success_articles,
                        &mut nzb_failed_size,
                        &mut nzb_success_size,
                        &mut nzb_failed_articles,
                        &mut nzb_success_articles,
                        &mut nzb_par_failed_size,
                        &mut nzb_par_success_size,
                        statuses_slice,
                        sizes_slice,
                        &mut reset_flags,
                    );

                    let mut out_ints = [
                        file_failed_articles,
                        file_completed_articles,
                        file_success_articles,
                        nzb_failed_articles,
                        nzb_success_articles,
                    ];
                    let mut out_i64s = [
                        file_failed_size,
                        file_remaining_size,
                        file_success_size,
                        nzb_failed_size,
                        nzb_success_size,
                        nzb_par_failed_size,
                        nzb_par_success_size,
                    ];
                    let req_id = cb(
                        env.ctx,
                        33,
                        cidx,
                        prep.new_partial_state,
                        file_ptr,
                        nzb_ptr,
                        std::ptr::null(),
                        std::ptr::null(),
                        out_ints.as_mut_ptr(),
                        out_i64s.as_mut_ptr(),
                        std::ptr::null_mut(),
                        reset_flags.as_mut_ptr(),
                        article_count,
                    ) as c_int;
                    requeued.push(req_id);
                    continue;
                }

                cb(
                    env.ctx,
                    31,
                    0,
                    0,
                    0,
                    file_ptr,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
        }
        cidx += 1;
    }

    let mut fin_i64s = [0i64; 2];
    cb(
        env.ctx,
        34,
        c_int::from(!reset_failed && !reprocess),
        0,
        0,
        nzb_ptr,
        std::ptr::null(),
        std::ptr::null(),
        requeued.as_mut_ptr(),
        fin_i64s.as_mut_ptr(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        requeued.len(),
    );
    let is_completed = fin_i64s[0] != 0;
    let nzb_id = fin_i64s[1] as c_int;

    hc_move_to_queue(
        env,
        dq_ptr,
        hist_ptr,
        hidx,
        nzb_ptr,
        hist_name,
        reprocess || is_completed,
    );

    if env.a6 != 0 {
        cb(
            env.ctx,
            35,
            nzb_id,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        );
    }
}

unsafe fn hc_set_parameter(
    env: &NzbgetRsHistoryCoordEnv,
    hist_ptr: usize,
    kind: c_int,
    nzb_ptr: usize,
    hist_name: &[u8],
    text_ptr: *const c_char,
) -> bool {
    let Some(cb) = env.action_cb else {
        return false;
    };
    let text = hc_cstr_bytes(text_ptr);
    let mut dm = b"Setting post-process-parameter '".to_vec();
    dm.extend_from_slice(text);
    dm.extend_from_slice(b"' for '");
    dm.extend_from_slice(hist_name);
    dm.push(b'\'');
    hc_log(env, 0, &dm);

    if kind != 1 && kind != 2 {
        let mut em = b"Could not set post-process-parameter for ".to_vec();
        em.extend_from_slice(hist_name);
        em.extend_from_slice(b": history item has wrong type");
        hc_log(env, 4, &em);
        return false;
    }

    if !text_ptr.is_null() {
        if let Some((name, val)) = split_param_assignment(text) {
            let k_c = hc_cbuf(name);
            let v_c = hc_cbuf(val);
            cb(
                env.ctx,
                36,
                0,
                0,
                0,
                hist_ptr,
                k_c.as_ptr() as *const c_char,
                v_c.as_ptr() as *const c_char,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            );
            return true;
        }
    }

    let mut strs: [*const c_char; 3] = [std::ptr::null(); 3];
    let mut ints = [0i32; 2];
    cb(
        env.ctx,
        26,
        0,
        0,
        0,
        nzb_ptr,
        std::ptr::null(),
        std::ptr::null(),
        ints.as_mut_ptr(),
        std::ptr::null_mut(),
        strs.as_mut_ptr(),
        std::ptr::null_mut(),
        0,
    );
    let mut em = b"Could not set post-process-parameter for ".to_vec();
    em.extend_from_slice(hc_cstr_bytes(strs[0]));
    em.extend_from_slice(b": invalid argument: ");
    em.extend_from_slice(text);
    hc_log(env, 4, &em);
    true
}

unsafe fn hc_set_category(
    env: &NzbgetRsHistoryCoordEnv,
    hist_ptr: usize,
    kind: c_int,
    hist_name: &[u8],
    text_ptr: *const c_char,
) -> bool {
    let Some(cb) = env.action_cb else {
        return false;
    };
    let text = hc_cstr_bytes(text_ptr);
    let mut dm = b"Setting category '".to_vec();
    dm.extend_from_slice(text);
    dm.extend_from_slice(b"' for '");
    dm.extend_from_slice(hist_name);
    dm.push(b'\'');
    hc_log(env, 0, &dm);

    if kind != 1 && kind != 2 {
        let mut em = b"Could not set category for ".to_vec();
        em.extend_from_slice(hist_name);
        em.extend_from_slice(b": history item has wrong type");
        hc_log(env, 4, &em);
        return false;
    }

    let t_c = hc_cbuf(text);
    cb(
        env.ctx,
        36,
        1,
        0,
        0,
        hist_ptr,
        t_c.as_ptr() as *const c_char,
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        0,
    );
    true
}

unsafe fn hc_set_name(
    env: &NzbgetRsHistoryCoordEnv,
    hist_ptr: usize,
    hist_name: &[u8],
    text_ptr: *const c_char,
) -> bool {
    let Some(cb) = env.action_cb else {
        return false;
    };
    let text = hc_cstr_bytes(text_ptr);
    let mut dm = b"Setting name '".to_vec();
    dm.extend_from_slice(text);
    dm.extend_from_slice(b"' for '");
    dm.extend_from_slice(hist_name);
    dm.push(b'\'');
    hc_log(env, 0, &dm);

    if text.is_empty() {
        let mut em = b"Could not rename ".to_vec();
        em.extend_from_slice(hist_name);
        em.extend_from_slice(b". The new name cannot be empty");
        hc_log(env, 4, &em);
        return false;
    }

    let t_c = hc_cbuf(text);
    let changed_id = cb(
        env.ctx,
        36,
        2,
        0,
        0,
        hist_ptr,
        t_c.as_ptr() as *const c_char,
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        0,
    );
    #[cfg(unix)]
    if changed_id != 0 {
        crate::dupesearch_coord::notify_active_dupesearch_name_changed(changed_id as i32);
    }
    true
}

unsafe fn hc_set_dupe_param(
    env: &NzbgetRsHistoryCoordEnv,
    hist_ptr: usize,
    kind: c_int,
    delete_status: c_int,
    hist_name: &[u8],
    action: c_int,
    text_ptr: *const c_char,
) {
    let Some(cb) = env.action_cb else {
        return;
    };
    let text = hc_cstr_bytes(text_ptr);
    let mut dm = format!("Setting dupe-parameter '{action}'='").into_bytes();
    dm.extend_from_slice(text);
    dm.extend_from_slice(b"' for '");
    dm.extend_from_slice(hist_name);
    dm.push(b'\'');
    hc_log(env, 0, &dm);

    let mut mode = 0i32;
    if action == 45 {
        let Some(parsed) = crate::queueeditor::parse_dupe_mode(text) else {
            let mut em = b"Could not set duplicate mode for ".to_vec();
            em.extend_from_slice(hist_name);
            em.extend_from_slice(b": incorrect mode (");
            em.extend_from_slice(text);
            em.push(b')');
            hc_log(env, 4, &em);
            return;
        };
        mode = parsed;
    }

    let t_c = hc_cbuf(text);
    let score = if text_ptr.is_null() {
        0
    } else {
        libc::atoi(t_c.as_ptr() as *const c_char)
    };

    if kind == 1 || kind == 2 {
        match action {
            43 => {
                cb(
                    env.ctx,
                    36,
                    3,
                    0,
                    0,
                    hist_ptr,
                    t_c.as_ptr() as *const c_char,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
            44 => {
                cb(
                    env.ctx,
                    36,
                    4,
                    score,
                    0,
                    hist_ptr,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
            45 => {
                cb(
                    env.ctx,
                    36,
                    5,
                    mode,
                    0,
                    hist_ptr,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
            46 => {
                if kind == 2 {
                    let mut em = b"Could not set duplicate parameter for ".to_vec();
                    em.extend_from_slice(hist_name);
                    em.extend_from_slice(b": history item has wrong type");
                    hc_log(env, 4, &em);
                    return;
                }
                if delete_status != 3 && delete_status != 1 {
                    let mut em = b"Could not set duplicate parameter for ".to_vec();
                    em.extend_from_slice(hist_name);
                    em.extend_from_slice(b": history item has wrong delete status");
                    hc_log(env, 4, &em);
                    return;
                }
                let new_ds = parse_dupe_backup_flag(text, 3, 1);
                cb(
                    env.ctx,
                    36,
                    6,
                    new_ds,
                    0,
                    hist_ptr,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
            _ => {}
        }
    } else if kind == 3 {
        match action {
            43 => {
                cb(
                    env.ctx,
                    36,
                    7,
                    0,
                    0,
                    hist_ptr,
                    t_c.as_ptr() as *const c_char,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
            44 => {
                cb(
                    env.ctx,
                    36,
                    8,
                    score,
                    0,
                    hist_ptr,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
            45 => {
                cb(
                    env.ctx,
                    36,
                    9,
                    mode,
                    0,
                    hist_ptr,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
            46 => {
                let mut em = b"Could not set duplicate parameter for ".to_vec();
                em.extend_from_slice(hist_name);
                em.extend_from_slice(b": history item has wrong type");
                hc_log(env, 4, &em);
            }
            _ => {}
        }
    }
}

/// Executes `HistoryCoordinator` operations (`daemon/queue/HistoryCoordinator.cpp`):
/// - `0`: `DeleteDiskFiles`
/// - `1`: `AddToHistory` (+ `RecheckFailedArticles`)
/// - `2`: `HistoryHide`
/// - `3`: `ServiceWork`
/// - `4`: `Redownload`
/// - `5`: `EditList`
pub unsafe fn history_coord_exec(op: c_int, env: *const NzbgetRsHistoryCoordEnv) -> c_int {
    let Some(e) = env.as_ref() else {
        return 0;
    };
    let Some(cb) = e.action_cb else {
        return 0;
    };
    match op {
        0 => {
            hc_delete_disk_files(e, e.p2, e.a1 != 0, e.a2 != 0);
            1
        }
        1 => {
            let mut ints = [0i32; 6];
            let mut i64s = [0i64; 2];
            let mut strs: [*const c_char; 1] = [std::ptr::null()];
            cb(
                e.ctx,
                2,
                0,
                0,
                e.p1,
                e.p2,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                strs.as_mut_ptr(),
                std::ptr::null_mut(),
                0,
            );
            let par_status = ints[0];
            let unpack_status = ints[1];
            let delete_status = ints[2];
            let parking = ints[3];
            let unpack_cleaned_up = ints[4];
            let mut completed_count = ints[5];
            let failed_size = i64s[0];
            let par_failed_size = i64s[1];
            let nzb_name = hc_cstr_bytes(strs[0]).to_vec();

            let cleanup = par_status == 3
                || unpack_status == 3
                || (par_status == 0 && unpack_status == 0 && (failed_size - par_failed_size) == 0)
                || (delete_status != 0 && parking == 0)
                || unpack_cleaned_up != 0;
            if cleanup {
                cb(
                    e.ctx,
                    0,
                    0,
                    0,
                    0,
                    e.p2,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
                completed_count = 0;
            }

            let mut parked_count = 0i32;
            for cidx in 0..completed_count {
                let mut cf_ints = [0i32; 2];
                let mut cf_strs: [*const c_char; 1] = [std::ptr::null()];
                cb(
                    e.ctx,
                    3,
                    cidx,
                    0,
                    0,
                    e.p2,
                    std::ptr::null(),
                    std::ptr::null(),
                    cf_ints.as_mut_ptr(),
                    std::ptr::null_mut(),
                    cf_strs.as_mut_ptr(),
                    std::ptr::null_mut(),
                    0,
                );
                let is_last = cidx + 1 == completed_count;
                if should_park_completed_file(cf_ints[0], 0, 2, is_last) {
                    let mut pm = b"Parking file ".to_vec();
                    pm.extend_from_slice(hc_cstr_bytes(cf_strs[0]));
                    hc_nzb_msg(e, e.p2, 1, &pm);
                    parked_count += 1;
                }
            }

            cb(
                e.ctx,
                5,
                parked_count,
                0,
                0,
                e.p2,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            );

            let mut added = b"Collection ".to_vec();
            added.extend_from_slice(&nzb_name);
            added.extend_from_slice(b" added to history");
            hc_nzb_msg(e, e.p2, 2, &added);

            hc_recheck_failed_articles(e, e.p1, e.p2);

            cb(
                e.ctx,
                6,
                0,
                0,
                0,
                e.p2,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            );
            1
        }
        2 => {
            hc_history_hide(e, e.p1, e.p2, e.a1, e.a2 != 0, e.a3 != 0);
            1
        }
        3 => {
            let min_time = calc_min_time(e.t1, e.a1);
            let dupe_check = e.a2 != 0;
            let server_mode = e.a3 != 0;
            let nzb_cleanup_disk = e.a4 != 0;
            let mut changed = false;
            let mut rindex = 0i32;

            loop {
                let mut ints = [0i32; 1];
                let mut i64s = [0i64; 2];
                let mut strs: [*const c_char; 1] = [std::ptr::null()];
                let hist_ptr = cb(
                    e.ctx,
                    14,
                    rindex,
                    0,
                    e.p1,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    ints.as_mut_ptr(),
                    i64s.as_mut_ptr(),
                    strs.as_mut_ptr(),
                    std::ptr::null_mut(),
                    0,
                );
                if hist_ptr == 0 {
                    break;
                }
                let act = classify_service_work_item(ints[0], 3, 1, i64s[0], min_time, dupe_check);
                if act != 0 {
                    if act == 1 {
                        hc_history_hide(e, e.p1, hist_ptr, rindex, server_mode, nzb_cleanup_disk);
                        rindex += 1;
                    } else {
                        let hist_name = hc_cstr_bytes(strs[0]).to_vec();
                        if act == 2 {
                            hc_delete_disk_files(
                                e,
                                i64s[1] as usize,
                                server_mode,
                                nzb_cleanup_disk,
                            );
                        }
                        let mut rm = b"Collection ".to_vec();
                        rm.extend_from_slice(&hist_name);
                        rm.extend_from_slice(b" removed from history");
                        hc_log(e, 2, &rm);
                        cb(
                            e.ctx,
                            15,
                            rindex,
                            1,
                            e.p1,
                            0,
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            0,
                        );
                    }
                    changed = true;
                } else {
                    rindex += 1;
                }
            }

            if changed {
                cb(
                    e.ctx,
                    16,
                    0,
                    0,
                    e.p1,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
            1
        }
        4 => {
            let mut ints = [0i32; 7];
            let mut i64s = [0i64; 1];
            let mut strs: [*const c_char; 2] = [std::ptr::null(); 2];
            let found = cb(
                e.ctx,
                18,
                0,
                1,
                e.p1,
                e.p2,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                strs.as_mut_ptr(),
                std::ptr::null_mut(),
                0,
            );
            if found != 0 {
                let hist_name = hc_cstr_bytes(strs[0]).to_vec();
                hc_history_redownload(
                    e,
                    e.p1,
                    found,
                    ints[0],
                    ints[2],
                    i64s[0] as usize,
                    &hist_name,
                    true,
                );
            }
            1
        }
        5 => {
            let ids = if e.int_arr.is_null() || e.int_len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(e.int_arr, e.int_len)
            };
            let action = e.a1;
            let dupe_check = e.a2 != 0;
            let server_mode = e.a3 != 0;
            let nzb_cleanup_disk = e.a4 != 0;
            let sep = e.a5 as u8;

            if action == 47 {
                for &id in ids {
                    cb(
                        e.ctx,
                        17,
                        id,
                        0,
                        e.p1,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        0,
                    );
                }
            }

            let mut ok = false;
            for &id in ids {
                let mut ints = [0i32; 7];
                let mut i64s = [0i64; 1];
                let mut strs: [*const c_char; 2] = [std::ptr::null(); 2];
                let hist_ptr = cb(
                    e.ctx,
                    18,
                    id,
                    0,
                    e.p1,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    ints.as_mut_ptr(),
                    i64s.as_mut_ptr(),
                    strs.as_mut_ptr(),
                    std::ptr::null_mut(),
                    0,
                );
                if hist_ptr == 0 {
                    continue;
                }
                let hidx = ints[0];
                let hist_len = ints[1];
                let kind = ints[2];
                let delete_status = ints[3];
                let par_status = ints[4];
                let unpack_status = ints[5];
                let parked_count = ints[6];
                let nzb_ptr = i64s[0] as usize;
                let hist_name = hc_cstr_bytes(strs[0]).to_vec();
                let dest_dir = hc_cstr_bytes(strs[1]).to_vec();

                let mut done = true;
                match action {
                    36 | 37 => {
                        let is_final = action == 37;
                        let mut dm = b"Deleting ".to_vec();
                        dm.extend_from_slice(&hist_name);
                        dm.extend_from_slice(b" from history");
                        hc_log(e, 2, &dm);

                        if kind == 1 {
                            hc_delete_disk_files(e, nzb_ptr, server_mode, nzb_cleanup_disk);
                            let dir_exists = crate::fsops::directory_exists(
                                &dest_dir,
                                &crate::fsops::posix_stat,
                            );
                            if should_delete_dest_dir(
                                true,
                                delete_status,
                                0,
                                par_status,
                                2,
                                unpack_status,
                                2,
                                5,
                                dir_exists,
                            ) {
                                hc_delete_dest_dir_if_present(e, &dest_dir, sep, 2);
                            }
                        }

                        if is_final || !dupe_check || kind == 2 {
                            cb(
                                e.ctx,
                                15,
                                hidx,
                                0,
                                e.p1,
                                0,
                                std::ptr::null(),
                                std::ptr::null(),
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                                0,
                            );
                        } else if kind == 1 {
                            let rindex = hist_len - 1 - hidx;
                            hc_history_hide(
                                e,
                                e.p1,
                                hist_ptr,
                                rindex,
                                server_mode,
                                nzb_cleanup_disk,
                            );
                        }
                    }
                    38 => {
                        if kind == 2 {
                            hc_history_redownload(
                                e, e.p1, hist_ptr, hidx, kind, nzb_ptr, &hist_name, false,
                            );
                        } else if kind == 1 && parked_count == 0 {
                            let mut wm = b"Could not download remaining files for ".to_vec();
                            wm.extend_from_slice(&hist_name);
                            wm.extend_from_slice(
                                b": history item does not have any files left for download",
                            );
                            hc_log(e, 3, &wm);
                        } else if kind == 1 {
                            hc_history_retry(
                                e, e.p1, hist_ptr, hidx, kind, nzb_ptr, &hist_name, false, false,
                            );
                        } else {
                            let mut em = b"Could not download remaining files for ".to_vec();
                            em.extend_from_slice(&hist_name);
                            em.extend_from_slice(b": history item has wrong type");
                            hc_log(e, 4, &em);
                        }
                    }
                    39 => {
                        if kind != 1 {
                            let mut em = b"Could not post-process again ".to_vec();
                            em.extend_from_slice(&hist_name);
                            em.extend_from_slice(b": history item has wrong type");
                            hc_log(e, 4, &em);
                        } else {
                            hc_history_retry(
                                e, e.p1, hist_ptr, hidx, kind, nzb_ptr, &hist_name, false, true,
                            );
                        }
                    }
                    40 => {
                        hc_history_redownload(
                            e, e.p1, hist_ptr, hidx, kind, nzb_ptr, &hist_name, false,
                        );
                    }
                    41 => {
                        hc_history_retry(
                            e, e.p1, hist_ptr, hidx, kind, nzb_ptr, &hist_name, true, false,
                        );
                    }
                    42 => {
                        done = hc_set_parameter(e, hist_ptr, kind, nzb_ptr, &hist_name, e.s1);
                    }
                    50 => {
                        done = hc_set_category(e, hist_ptr, kind, &hist_name, e.s1);
                    }
                    51 => {
                        done = hc_set_name(e, hist_ptr, &hist_name, e.s1);
                    }
                    43..=46 => {
                        hc_set_dupe_param(
                            e,
                            hist_ptr,
                            kind,
                            delete_status,
                            &hist_name,
                            action,
                            e.s1,
                        );
                    }
                    47..=49 =>
                    {
                        #[cfg(unix)]
                        if let Some(qh) = crate::ffi::queue_host() {
                            crate::dupecoord::history_mark_host(&qh, e.p1, hist_ptr, action - 46);
                        }
                    }
                    _ => {}
                }
                ok = ok || done;
            }

            if ok {
                cb(
                    e.ctx,
                    16,
                    0,
                    0,
                    e.p1,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
            c_int::from(ok)
        }
        _ => 0,
    }
}

#[cfg(unix)]
std::thread_local! {
    static HC_STR_ARENA: std::cell::RefCell<Vec<std::ffi::CString>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(unix)]
struct HcArenaGuard;

#[cfg(unix)]
impl Drop for HcArenaGuard {
    fn drop(&mut self) {
        HC_STR_ARENA.with(|a| a.borrow_mut().clear());
    }
}

#[cfg(unix)]
fn hc_stash_str(bytes: &[u8]) -> *const c_char {
    let cs = if let Some(pos) = bytes.iter().position(|&b| b == 0) {
        std::ffi::CString::new(&bytes[..pos]).unwrap_or_default()
    } else {
        std::ffi::CString::new(bytes).unwrap_or_default()
    };
    HC_STR_ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        arena.push(cs);
        arena.last().map_or(std::ptr::null(), |s| s.as_ptr())
    })
}

#[cfg(unix)]
unsafe extern "C" fn hc_native_log_cb(_ctx: *mut c_void, level: c_int, msg: *const c_char) {
    use crate::queuesubsys_host::{MK_DEBUG, MK_DETAIL, MK_ERROR, MK_INFO, MK_WARNING};
    let mk = match level {
        0 => MK_DEBUG,
        1 => MK_DETAIL,
        2 => MK_INFO,
        3 => MK_WARNING,
        4 => MK_ERROR,
        _ => return,
    };
    crate::log::log_active_bytes(mk, hc_cstr_bytes(msg));
}

#[cfg(unix)]
#[allow(clippy::too_many_lines)]
unsafe extern "C" fn hc_native_action_cb(
    ctx: *mut c_void,
    sub: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
    bytes: *mut u8,
    len: usize,
) -> usize {
    use crate::queuesubsys_host::{
        QueueSubsysHost, AI_FAILED, AI_UNDEFINED, ASPECT_NZB_RETURNED, ASPECT_URL_RETURNED,
        CF_FAILURE, CF_NONE, CF_PARTIAL, CS_NONE, DH_NONE, DH_REDOWNLOAD_MANUAL, DP_NONE, DS_NONE,
        DUP_INT_DUPE_MODE, DUP_INT_DUPE_SCORE, DUP_STR_DUPE_KEY, DUP_STR_NAME,
        EA_GROUP_PAUSE_EXTRA_PARS, FILE_ACT_CLEAR_ARTICLES, FILE_I64_FAILED_SIZE,
        FILE_I64_REMAINING_SIZE, FILE_I64_SUCCESS_SIZE, FILE_INT_COMPLETED_ARTICLES,
        FILE_INT_FAILED_ARTICLES, FILE_INT_PARTIAL_STATE, FILE_INT_PAUSED,
        FILE_INT_SUCCESS_ARTICLES, FILE_STR_FILENAME, HK_DUP, HK_NZB, HK_URL, KS_BAD, KS_NONE,
        LS_NONE, MK_DETAIL, MK_INFO, MK_WARNING, MS_NONE, NS_NONE, NS_RUNNING,
        NZB_ACT_BUILD_DEST_DIR, NZB_ACT_CLEAR_EXTRACTED_ARCHIVES, NZB_ACT_CLEAR_FILE_LIST,
        NZB_ACT_CLEAR_LIVE_REPAIR_THREAD, NZB_ACT_CLEAR_SCRIPT_STATUSES,
        NZB_ACT_UPDATE_COMPLETED_STATS, NZB_ACT_UPDATE_CURRENT_STATS, NZB_I64_DOWNLOADED_SIZE,
        NZB_I64_DUPE_RECOVERED_BYTES, NZB_I64_FAILED_SIZE, NZB_I64_PAR_FAILED_SIZE,
        NZB_I64_PAR_SUCCESS_SIZE, NZB_I64_SUCCESS_SIZE, NZB_INT_ALL_FIRST, NZB_INT_CLEANUP_STATUS,
        NZB_INT_DEAD_PICK_PROBED, NZB_INT_DELETE_PAUSED, NZB_INT_DELETE_STATUS,
        NZB_INT_DIRECT_RENAME_STATUS, NZB_INT_DIRECT_UNPACK_STATUS, NZB_INT_DOWNLOAD_SEC,
        NZB_INT_DUPE_ATTEMPTED_ARTICLES, NZB_INT_DUPE_FAILED_RUN, NZB_INT_DUPE_FAILOVER_CHECKED,
        NZB_INT_DUPE_HINT, NZB_INT_DUPE_MODE, NZB_INT_DUPE_NO_PAR_NOTED,
        NZB_INT_DUPE_PAR_DEFER_STATE, NZB_INT_DUPE_RECOVERED_ARTICLES,
        NZB_INT_DUPE_RECOVERED_HOLES, NZB_INT_DUPE_SCORE, NZB_INT_DUPE_UNSOURCED_ARTICLES,
        NZB_INT_EXTRA_PAR_BLOCKS, NZB_INT_FAILED_ARTICLES, NZB_INT_HEALTH_PAUSED,
        NZB_INT_LAST_UNPACK_STATUS, NZB_INT_LOADING_PAR, NZB_INT_MARK_STATUS, NZB_INT_MOVE_STATUS,
        NZB_INT_PARKED_FILE_COUNT, NZB_INT_PARKING, NZB_INT_PAR_FULL, NZB_INT_PAR_RENAME_STATUS,
        NZB_INT_PAR_REPAIRED, NZB_INT_PAR_SEC, NZB_INT_PAR_STATUS, NZB_INT_POST_TOTAL_SEC,
        NZB_INT_RAR_RENAME_STATUS, NZB_INT_REMAINING_PAR_COUNT, NZB_INT_REPAIR_SEC,
        NZB_INT_REPROCESS, NZB_INT_SUCCESS_ARTICLES, NZB_INT_UNPACK_CLEANED_UP_DISK,
        NZB_INT_UNPACK_SEC, NZB_INT_UNPACK_STATUS, NZB_INT_UNPACK_TRIED, NZB_INT_URL_STATUS,
        NZB_INT_WAITING_PAR, NZB_STR_CATEGORY, NZB_STR_DUPE_KEY, NZB_STR_FINAL_DIR, NZB_STR_NAME,
        PARTIAL_COMPLETED, PAR_STATUS_NONE, RS_NONE, TS_FAILURE, TS_NONE, TS_RUNNING, US_NONE,
    };

    HC_STR_ARENA.with(|a| a.borrow_mut().clear());
    if ctx.is_null() {
        return 0;
    }
    let host = &*(ctx as *const QueueSubsysHost);
    match sub {
        0 => {
            host.disk_discard_files_ex(p2, a1 != 0);
            0
        }
        1 => {
            let nzb = host.nzb_snap(p2, false);
            host.nzb_action(p2, NZB_ACT_CLEAR_FILE_LIST, 0);
            *strs.add(0) = hc_stash_str(&nzb.queued_filename);
            0
        }
        2 => {
            let nzb0 = host.nzb_snap(p2, false);
            if nzb0.has_live_repair_thread {
                let t = nzb0.live_repair_thread_ptr;
                host.nzb_action(p2, NZB_ACT_CLEAR_LIVE_REPAIR_THREAD, 0);
                host.thread_stop(t);
            }
            host.history_move_queue_item(p1, p2);
            host.dq_history_changed(p1);
            let fn_len = host.nzb_file_len(p2);
            for i in 0..fn_len {
                let fp = host.nzb_file_at(p2, i);
                let fs = host.file_snap(fp);
                host.nzb_action(p2, NZB_ACT_UPDATE_COMPLETED_STATS, fp);
                host.nzb_push_completed_file(p2, fp, &fs.filename, CF_NONE, 0);
            }
            let nzb = host.nzb_snap(p2, false);
            let cfs_len = host.nzb_completed_files_len(p2) as c_int;
            *ints.add(0) = nzb.par_status;
            *ints.add(1) = nzb.unpack_status;
            *ints.add(2) = nzb.delete_status;
            *ints.add(3) = c_int::from(nzb.parking);
            *ints.add(4) = c_int::from(nzb.unpack_cleaned_up_disk);
            *ints.add(5) = cfs_len;
            *i64s.add(0) = nzb.failed_size;
            *i64s.add(1) = nzb.par_failed_size;
            *strs.add(0) = hc_stash_str(&nzb.name);
            0
        }
        3 => {
            let cfs_len = host.nzb_completed_files_len(p2);
            if a1 < 0 || (a1 as usize) >= cfs_len {
                return 0;
            }
            let cf = host.nzb_completed_file_at(p2, a1 as usize);
            *ints.add(0) = cf.status;
            *ints.add(1) = cf.id;
            *strs.add(0) = hc_stash_str(&cf.filename);
            1
        }
        4 => {
            let mk = if a1 == 1 {
                MK_DETAIL
            } else if a1 == 3 {
                MK_WARNING
            } else {
                MK_INFO
            };
            host.nzb_print_message(p2, mk, hc_cstr_bytes(s1));
            0
        }
        5 => {
            let nzb = host.nzb_snap(p2, false);
            host.nzb_set_int(p2, NZB_INT_PARKED_FILE_COUNT, a1);
            host.nzb_action(p2, NZB_ACT_CLEAR_FILE_LIST, 0);
            host.nzb_set_int(p2, NZB_INT_REMAINING_PAR_COUNT, 0);
            host.nzb_set_int(p2, NZB_INT_PARKING, 0);
            if nzb.direct_rename_status == TS_RUNNING {
                host.nzb_set_int(p2, NZB_INT_DIRECT_RENAME_STATUS, TS_FAILURE);
            }
            if nzb.direct_unpack_status == NS_RUNNING {
                host.nzb_set_int(p2, NZB_INT_DIRECT_UNPACK_STATUS, NS_NONE);
            }
            host.nzb_set_int(p2, NZB_INT_DUPE_HINT, DH_NONE);
            0
        }
        6 => {
            let nzb = host.nzb_snap(p2, false);
            crate::dupefallback::par_damage_forget_all(nzb.id);
            0
        }
        10 => {
            host.nzb_set_param(p2, b"*DupeRecheck", Some(b"1"));
            let emit: unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const *const c_char,
                usize,
            ) = std::mem::transmute(s1 as usize);
            let cfs = host.nzb_completed_files(p2);
            for cf in cfs {
                if (cf.status != CF_PARTIAL && cf.status != CF_FAILURE) || cf.id <= 0 {
                    continue;
                }
                let partial = cf.status == CF_PARTIAL;
                let (fi_ptr, state_loaded) = host.disk_load_file_for_inspect(cf.id, false, partial);
                if fi_ptr == 0 {
                    continue;
                }
                if partial && !state_loaded {
                    host.delete_file_info(fi_ptr);
                    continue;
                }
                let groups_vec = host.file_groups(fi_ptr);
                let group_cstrs: Vec<std::ffi::CString> = groups_vec
                    .iter()
                    .map(|g| {
                        let end = g.iter().position(|&b| b == 0).unwrap_or(g.len());
                        std::ffi::CString::new(&g[..end]).unwrap_or_default()
                    })
                    .collect();
                let group_ptrs: Vec<*const c_char> =
                    group_cstrs.iter().map(|c| c.as_ptr()).collect();
                let fs = host.file_snap(fi_ptr);
                for aidx in 0..fs.articles_len {
                    let art = host.article_at(fi_ptr, aidx);
                    if (!partial || art.status == AI_FAILED) && !art.message_id.is_empty() {
                        let mid_c = hc_cbuf(&art.message_id);
                        emit(
                            p1 as *mut c_void,
                            mid_c.as_ptr() as *const c_char,
                            if group_ptrs.is_empty() {
                                std::ptr::null()
                            } else {
                                group_ptrs.as_ptr()
                            },
                            group_ptrs.len(),
                        );
                    }
                }
                host.delete_file_info(fi_ptr);
            }
            0
        }
        12 => {
            let hi = host.hist_item_snap(p2);
            let nzb_ptr = hi.inner_ptr;
            let nzb = host.nzb_snap(nzb_ptr, false);
            *ints.add(0) = nzb.mark_status;
            *ints.add(1) = nzb.delete_status;
            *ints.add(2) = c_int::from(nzb.is_dupe_success);
            *strs.add(0) = hc_stash_str(&hi.name);
            nzb_ptr
        }
        13 => {
            host.history_replace_with_dup(p1, p2, a2 as usize, a1);
            0
        }
        14 => {
            let hl_len = host.history_len(p1);
            if a1 < 0 || (a1 as usize) >= hl_len {
                return 0;
            }
            let pos = hl_len - 1 - (a1 as usize);
            let hi = host.history_at(p1, pos);
            *ints.add(0) = hi.kind;
            *i64s.add(0) = hi.time;
            *i64s.add(1) = if hi.kind == HK_NZB {
                hi.inner_ptr as i64
            } else {
                0
            };
            *strs.add(0) = hc_stash_str(&hi.name);
            hi.hi_ptr
        }
        15 => {
            let hl_len = host.history_len(p1);
            let pos = if a2 != 0 {
                hl_len - 1 - (a1 as usize)
            } else {
                a1 as usize
            };
            host.history_erase_at(p1, pos);
            0
        }
        16 => {
            host.dq_history_changed(p1);
            host.dq_save(p1);
            0
        }
        17 => {
            let hi_ptr = host.history_find_by_id(p1, a1);
            if hi_ptr != 0 {
                let hi = host.hist_item_snap(hi_ptr);
                if hi.kind == HK_NZB && hi.inner_ptr != 0 {
                    host.nzb_set_int(hi.inner_ptr, NZB_INT_MARK_STATUS, KS_BAD);
                }
            }
            0
        }
        18 => {
            let hl_len = host.history_len(p1);
            let mut found_idx: Option<usize> = None;
            let mut found_hi = crate::queuesubsys_host::HistEntrySnap::default();
            for idx in 0..hl_len {
                let hi = host.history_at(p1, idx);
                let m = if a2 != 0 {
                    hi.hi_ptr == p2
                } else {
                    hi.id == a1
                };
                if m {
                    found_idx = Some(idx);
                    found_hi = hi;
                    break;
                }
            }
            let Some(idx) = found_idx else {
                return 0;
            };
            let nzb_ptr = if found_hi.kind == HK_NZB || found_hi.kind == HK_URL {
                found_hi.inner_ptr
            } else {
                0
            };
            let nzb = if nzb_ptr != 0 {
                Some(host.nzb_snap(nzb_ptr, false))
            } else {
                None
            };
            *ints.add(0) = idx as c_int;
            *ints.add(1) = hl_len as c_int;
            *ints.add(2) = found_hi.kind;
            *ints.add(3) = nzb.as_ref().map_or(0, |n| n.delete_status);
            *ints.add(4) = nzb.as_ref().map_or(0, |n| n.par_status);
            *ints.add(5) = nzb.as_ref().map_or(0, |n| n.unpack_status);
            *ints.add(6) = nzb.as_ref().map_or(0, |n| n.parked_file_count);
            *i64s.add(0) = nzb_ptr as i64;
            *strs.add(0) = hc_stash_str(&found_hi.name);
            *strs.add(1) = hc_stash_str(nzb.as_ref().map_or(b"", |n| n.dest_dir.as_slice()));
            found_hi.hi_ptr
        }
        21 => {
            if a1 < 0 {
                let nzb = host.nzb_snap(p2, false);
                *ints.add(0) = c_int::from(nzb.unpack_cleaned_up_disk);
                *ints.add(1) = nzb.post_total_sec;
                *ints.add(2) = nzb.unpack_sec;
                *ints.add(3) = nzb.par_sec;
                *ints.add(4) = nzb.move_status;
                *ints.add(5) = host.nzb_file_len(p2) as c_int;
                *strs.add(0) = hc_stash_str(&nzb.dest_dir);
            } else {
                let fp = host.nzb_file_at(p2, a1 as usize);
                let fs = host.file_snap(fp);
                *strs.add(0) = hc_stash_str(&fs.filename);
            }
            0
        }
        22 => {
            let hi = host.hist_item_snap(p2);
            let nzb_ptr = hi.inner_ptr;
            let nzb = host.nzb_snap(nzb_ptr, false);
            if !nzb.unpack_cleaned_up_disk {
                host.nzb_set_int(nzb_ptr, NZB_INT_UNPACK_STATUS, US_NONE);
                host.nzb_set_int(nzb_ptr, NZB_INT_DIRECT_UNPACK_STATUS, NS_NONE);
                host.nzb_set_int(nzb_ptr, NZB_INT_PAR_REPAIRED, 0);
                host.nzb_set_int(nzb_ptr, NZB_INT_UNPACK_TRIED, 0);
                host.nzb_set_int(nzb_ptr, NZB_INT_LAST_UNPACK_STATUS, 0);
                host.nzb_action(nzb_ptr, NZB_ACT_CLEAR_EXTRACTED_ARCHIVES, 0);
                host.nzb_set_int(nzb_ptr, NZB_INT_CLEANUP_STATUS, CS_NONE);
                host.nzb_set_int(nzb_ptr, NZB_INT_PAR_RENAME_STATUS, RS_NONE);
                host.nzb_set_int(nzb_ptr, NZB_INT_RAR_RENAME_STATUS, RS_NONE);
                host.nzb_set_int(nzb_ptr, NZB_INT_POST_TOTAL_SEC, *ints.add(1));
                host.nzb_set_int(nzb_ptr, NZB_INT_UNPACK_SEC, 0);
                if *ints.add(0) != 0 {
                    host.nzb_set_int(nzb_ptr, NZB_INT_PAR_STATUS, PAR_STATUS_NONE);
                    host.nzb_set_int(nzb_ptr, NZB_INT_PAR_SEC, 0);
                    host.nzb_set_int(nzb_ptr, NZB_INT_REPAIR_SEC, 0);
                    host.nzb_set_int(nzb_ptr, NZB_INT_PAR_FULL, 0);
                }
            }
            host.nzb_set_int(nzb_ptr, NZB_INT_DELETE_STATUS, DS_NONE);
            host.nzb_set_int(nzb_ptr, NZB_INT_DELETE_PAUSED, 0);
            host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_FAILOVER_CHECKED, -1);
            host.nzb_set_int(nzb_ptr, NZB_INT_DEAD_PICK_PROBED, 0);
            host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_ATTEMPTED_ARTICLES, 0);
            host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_UNSOURCED_ARTICLES, 0);
            host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_FAILED_RUN, 0);
            host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_PAR_DEFER_STATE, DP_NONE);
            host.nzb_set_int(nzb_ptr, NZB_INT_MARK_STATUS, KS_NONE);
            host.nzb_action(nzb_ptr, NZB_ACT_CLEAR_SCRIPT_STATUSES, 0);
            host.nzb_set_int(nzb_ptr, NZB_INT_PARKED_FILE_COUNT, 0);
            host.nzb_set_int(nzb_ptr, NZB_INT_MOVE_STATUS, *ints.add(2));
            host.nzb_set_int(nzb_ptr, NZB_INT_REPROCESS, c_int::from(a2 != 0));
            host.nzb_set_str(nzb_ptr, NZB_STR_FINAL_DIR, b"");
            host.history_return_to_queue(p1, p2, a1 as usize);
            0
        }
        23 => {
            if let Some(pp) = crate::ffi::active_prepost() {
                if a1 != 0 {
                    pp.nzb_added(host, p1, p2);
                } else {
                    pp.nzb_downloaded(host, p1, p2);
                }
            }
            let mut notify_i64s = [0i64];
            host.raw(
                24,
                3,
                ASPECT_NZB_RETURNED,
                p1,
                p2,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                notify_i64s.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            0
        }
        24 => {
            host.nzb_action(p2, NZB_ACT_BUILD_DEST_DIR, 0);
            let nzb = host.nzb_snap(p2, false);
            *strs.add(0) = hc_stash_str(&nzb.dest_dir);
            0
        }
        25 => {
            let hi = host.hist_item_snap(p2);
            let nzb_ptr = hi.inner_ptr;
            let nzb = host.nzb_snap(nzb_ptr, false);
            host.nzb_set_int(nzb_ptr, NZB_INT_URL_STATUS, LS_NONE);
            host.nzb_set_int(nzb_ptr, NZB_INT_DELETE_STATUS, DS_NONE);
            if nzb.dupe_hint == DH_NONE {
                host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_HINT, DH_REDOWNLOAD_MANUAL);
            }
            host.history_return_to_queue(p1, p2, a1 as usize);
            let mut notify_i64s = [0i64];
            host.raw(
                24,
                3,
                ASPECT_URL_RETURNED,
                p1,
                nzb_ptr,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                notify_i64s.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            0
        }
        26 => {
            let nzb = host.nzb_snap(p2, false);
            *ints.add(0) = c_int::from(nzb.delete_paused);
            *strs.add(0) = hc_stash_str(&nzb.name);
            *strs.add(1) = hc_stash_str(&nzb.queued_filename);
            *strs.add(2) = hc_stash_str(&nzb.dest_dir);
            0
        }
        27 => {
            let parsed = crate::nzbfile::parse_and_materialize_nzb(host, hc_cstr_bytes(s1), b"");
            if !parsed.ok {
                host.destroy_nzb_uptr(parsed.uptr_addr);
                return 0;
            }
            parsed.uptr_addr
        }
        28 => {
            let src_nzb_ptr = if p1 != 0 { *(p1 as *const usize) } else { 0 };
            if src_nzb_ptr != 0 {
                let n = host.nzb_file_len(src_nzb_ptr);
                let paused_val = c_int::from(a1 != 0);
                for idx in 0..n {
                    let fp = host.nzb_file_at(src_nzb_ptr, idx);
                    host.file_set_int(fp, FILE_INT_PAUSED, paused_val);
                }
            }
            host.disk_discard_files(p2);
            host.nzb_set_int(p2, NZB_INT_MOVE_STATUS, MS_NONE);
            host.nzb_set_int(p2, NZB_INT_UNPACK_CLEANED_UP_DISK, 0);
            host.nzb_set_int(p2, NZB_INT_PAR_STATUS, PAR_STATUS_NONE);
            host.nzb_set_int(p2, NZB_INT_PAR_RENAME_STATUS, RS_NONE);
            host.nzb_set_int(p2, NZB_INT_RAR_RENAME_STATUS, RS_NONE);
            host.nzb_set_int(p2, NZB_INT_DIRECT_RENAME_STATUS, TS_NONE);
            host.nzb_set_int(p2, NZB_INT_DIRECT_UNPACK_STATUS, NS_NONE);
            host.nzb_set_int(p2, NZB_INT_PAR_REPAIRED, 0);
            host.nzb_set_int(p2, NZB_INT_UNPACK_TRIED, 0);
            host.nzb_set_int(p2, NZB_INT_LAST_UNPACK_STATUS, 0);
            host.nzb_action(p2, NZB_ACT_CLEAR_EXTRACTED_ARCHIVES, 0);
            host.nzb_set_int(p2, NZB_INT_HEALTH_PAUSED, 0);
            host.nzb_set_i64(p2, NZB_I64_DOWNLOADED_SIZE, 0);
            host.nzb_set_int(p2, NZB_INT_DOWNLOAD_SEC, 0);
            host.nzb_set_int(p2, NZB_INT_POST_TOTAL_SEC, 0);
            host.nzb_set_int(p2, NZB_INT_PAR_SEC, 0);
            host.nzb_set_int(p2, NZB_INT_REPAIR_SEC, 0);
            host.nzb_set_int(p2, NZB_INT_UNPACK_SEC, 0);
            host.nzb_set_int(p2, NZB_INT_EXTRA_PAR_BLOCKS, 0);
            host.nzb_set_int(p2, NZB_INT_ALL_FIRST, 0);
            host.nzb_set_int(p2, NZB_INT_WAITING_PAR, 0);
            host.nzb_set_int(p2, NZB_INT_LOADING_PAR, 0);
            host.nzb_set_int(p2, NZB_INT_DUPE_RECOVERED_ARTICLES, 0);
            host.nzb_set_i64(p2, NZB_I64_DUPE_RECOVERED_BYTES, 0);
            host.nzb_set_int(p2, NZB_INT_DUPE_RECOVERED_HOLES, 0);
            host.nzb_set_int(p2, NZB_INT_DUPE_PAR_DEFER_STATE, DP_NONE);
            host.nzb_set_int(p2, NZB_INT_DUPE_NO_PAR_NOTED, 0);
            host.nzb_redownload_replace_files(p2, src_nzb_ptr);
            host.destroy_nzb_uptr(p1);
            if let Some((qc, qh)) = crate::ffi::active_queue_coord_and_host() {
                qc.check_dupe_file_infos(&qh, p2);
            }
            0
        }
        30 => {
            let try_state = a2 == CF_PARTIAL;
            let (fi_ptr, state_loaded) = host.disk_load_file_for_inspect(a1, true, try_state);
            if fi_ptr == 0 {
                return 0;
            }
            host.file_set_nzb_info(fi_ptr, p2);
            let fs = host.file_snap(fi_ptr);
            let nzb = host.nzb_snap(p2, false);
            *ints.add(0) = c_int::from(state_loaded);
            *ints.add(1) = fs.success_articles;
            *ints.add(2) = fs.partial_state;
            *ints.add(3) = fs.articles_len as c_int;
            *ints.add(4) = c_int::from(fs.par_file);
            *ints.add(5) = fs.total_articles;
            *ints.add(6) = fs.missed_articles;
            *ints.add(7) = fs.failed_articles;
            *ints.add(8) = fs.completed_articles;
            *ints.add(9) = fs.success_articles;
            *ints.add(10) = nzb.failed_articles;
            *ints.add(11) = nzb.success_articles;
            *i64s.add(0) = fs.size;
            *i64s.add(1) = fs.missed_size;
            *i64s.add(2) = fs.failed_size;
            *i64s.add(3) = fs.remaining_size;
            *i64s.add(4) = fs.success_size;
            *i64s.add(5) = nzb.failed_size;
            *i64s.add(6) = nzb.success_size;
            *i64s.add(7) = nzb.par_failed_size;
            *i64s.add(8) = nzb.par_success_size;
            *strs.add(0) = hc_stash_str(&fs.filename);
            fi_ptr
        }
        31 => {
            if a1 == 0 {
                host.delete_file_info(p2);
            } else {
                host.file_set_str(p2, FILE_STR_FILENAME, Some(hc_cstr_bytes(s1)));
            }
            0
        }
        32 => {
            for idx in 0..len {
                let art = host.article_at(p2, idx);
                *ints.add(idx) = art.status;
                *ints.add(len + idx) = art.size;
            }
            0
        }
        33 => {
            host.file_set_int(p1, FILE_INT_PARTIAL_STATE, a2);
            host.file_subtract_server_stats_from_nzb_total(p1, p2);
            host.nzb_set_i64(p2, NZB_I64_FAILED_SIZE, *i64s.add(3));
            host.nzb_set_i64(p2, NZB_I64_SUCCESS_SIZE, *i64s.add(4));
            host.nzb_set_int(p2, NZB_INT_FAILED_ARTICLES, *ints.add(3));
            host.nzb_set_int(p2, NZB_INT_SUCCESS_ARTICLES, *ints.add(4));
            host.nzb_set_i64(p2, NZB_I64_PAR_FAILED_SIZE, *i64s.add(5));
            host.nzb_set_i64(p2, NZB_I64_PAR_SUCCESS_SIZE, *i64s.add(6));
            if !bytes.is_null() {
                for idx in 0..len {
                    if *bytes.add(idx) != 0 {
                        let art = host.article_at(p1, idx);
                        if art.ptr != 0 {
                            host.article_reset_for_retry(art.ptr);
                        }
                    }
                }
            }
            host.file_set_i64(p1, FILE_I64_FAILED_SIZE, *i64s.add(0));
            host.file_set_i64(p1, FILE_I64_REMAINING_SIZE, *i64s.add(1));
            host.file_set_i64(p1, FILE_I64_SUCCESS_SIZE, *i64s.add(2));
            host.file_set_int(p1, FILE_INT_FAILED_ARTICLES, *ints.add(0));
            host.file_set_int(p1, FILE_INT_COMPLETED_ARTICLES, *ints.add(1));
            host.file_set_int(p1, FILE_INT_SUCCESS_ARTICLES, *ints.add(2));
            let fs = host.file_snap(p1);
            host.disk_discard_file(fs.id, false, true, a2 != PARTIAL_COMPLETED);
            if a2 == PARTIAL_COMPLETED {
                host.disk_save_file_state(p1, true);
            }
            host.file_action(p1, FILE_ACT_CLEAR_ARTICLES, 0);
            let fid = fs.id;
            host.nzb_add_requeued_file(p2, p1);
            host.nzb_erase_completed_file(p2, a1 as usize);
            fid as usize
        }
        34 => {
            if len > 0 && !ints.is_null() {
                let req = std::slice::from_raw_parts(ints, len);
                let jobs = host.nzb_stream_repair_jobs(p2);
                for j in (0..jobs.len()).rev() {
                    if req.contains(&jobs[j].file_id) {
                        host.nzb_stream_repair_erase_job(p2, j, false);
                    }
                }
            }
            host.nzb_action(p2, NZB_ACT_UPDATE_CURRENT_STATS, 0);
            if a1 != 0 {
                host.nzb_set_int(p2, NZB_INT_HEALTH_PAUSED, 1);
            }
            let nzb = host.nzb_snap(p2, false);
            *i64s.add(0) = i64::from(nzb.is_download_completed);
            *i64s.add(1) = i64::from(nzb.id);
            let _ = AI_UNDEFINED;
            0
        }
        35 => {
            host.dq_edit_entry(p1, a1, EA_GROUP_PAUSE_EXTRA_PARS, None);
            0
        }
        36 => {
            let hi = host.hist_item_snap(p2);
            match a1 {
                0 => {
                    host.nzb_set_param(hi.inner_ptr, hc_cstr_bytes(s1), Some(hc_cstr_bytes(s2)));
                }
                1 => {
                    host.nzb_set_str(hi.inner_ptr, NZB_STR_CATEGORY, hc_cstr_bytes(s1));
                }
                2 => {
                    if hi.kind == HK_NZB || hi.kind == HK_URL {
                        let prev = host.nzb_snap(hi.inner_ptr, false).name;
                        host.nzb_set_str(hi.inner_ptr, NZB_STR_NAME, hc_cstr_bytes(s1));
                        let after = host.nzb_snap(hi.inner_ptr, false);
                        if hi.kind == HK_NZB && prev != after.name {
                            return after.id as usize;
                        }
                    } else if hi.kind == HK_DUP {
                        host.dup_set_str(hi.inner_ptr, DUP_STR_NAME, hc_cstr_bytes(s1));
                    }
                }
                3 => {
                    host.nzb_set_param(hi.inner_ptr, b"DupeSearchOwnedKey", None);
                    host.nzb_set_str(hi.inner_ptr, NZB_STR_DUPE_KEY, hc_cstr_bytes(s1));
                }
                4 => host.nzb_set_int(hi.inner_ptr, NZB_INT_DUPE_SCORE, a2),
                5 => host.nzb_set_int(hi.inner_ptr, NZB_INT_DUPE_MODE, a2),
                6 => host.nzb_set_int(hi.inner_ptr, NZB_INT_DELETE_STATUS, a2),
                7 => host.dup_set_str(hi.inner_ptr, DUP_STR_DUPE_KEY, hc_cstr_bytes(s1)),
                8 => host.dup_set_int(hi.inner_ptr, DUP_INT_DUPE_SCORE, a2),
                9 => host.dup_set_int(hi.inner_ptr, DUP_INT_DUPE_MODE, a2),
                _ => {}
            }
            0
        }
        _ => 0,
    }
}

#[cfg(unix)]
fn hc_make_native_env(host: &crate::queuesubsys_host::QueueSubsysHost) -> NzbgetRsHistoryCoordEnv {
    NzbgetRsHistoryCoordEnv {
        ctx: std::ptr::from_ref(host) as *mut c_void,
        p1: 0,
        p2: 0,
        a1: 0,
        a2: 0,
        a3: 0,
        a4: 0,
        a5: b'/' as c_int,
        a6: 0,
        t1: 0,
        s1: std::ptr::null(),
        int_arr: std::ptr::null(),
        int_len: 0,
        log_cb: Some(hc_native_log_cb),
        action_cb: Some(hc_native_action_cb),
    }
}

#[cfg(unix)]
fn hc_nzb_cleanup_disk() -> c_int {
    c_int::from(crate::options::active_options().is_some_and(|o| o.scalars().nzb_cleanup_disk != 0))
}

#[cfg(unix)]
pub(crate) fn delete_disk_files_active(nzb_ptr: usize) {
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    let _guard = HcArenaGuard;
    let opts = host.opts();
    let mut env = hc_make_native_env(&host);
    env.p2 = nzb_ptr;
    env.a1 = c_int::from(opts.server_mode);
    env.a2 = hc_nzb_cleanup_disk();
    unsafe {
        history_coord_exec(0, &env);
    }
}

#[cfg(unix)]
pub(crate) fn add_to_history_active(dq_ptr: usize, nzb_ptr: usize) {
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    let _guard = HcArenaGuard;
    let mut env = hc_make_native_env(&host);
    env.p1 = dq_ptr;
    env.p2 = nzb_ptr;
    unsafe {
        history_coord_exec(1, &env);
    }
}

#[cfg(unix)]
pub(crate) fn history_hide_active(dq_ptr: usize, hist_ptr: usize, rindex: i32) {
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    let _guard = HcArenaGuard;
    let opts = host.opts();
    let mut env = hc_make_native_env(&host);
    env.p1 = dq_ptr;
    env.p2 = hist_ptr;
    env.a1 = rindex;
    env.a2 = c_int::from(opts.server_mode);
    env.a3 = hc_nzb_cleanup_disk();
    unsafe {
        history_coord_exec(2, &env);
    }
}

#[cfg(unix)]
pub(crate) fn service_work_active() {
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    host.with_locked_queue(|dq_ptr| {
        let _guard = HcArenaGuard;
        let opts = host.opts();
        let mut env = hc_make_native_env(&host);
        env.p1 = dq_ptr;
        env.a1 = opts.keep_history;
        env.a2 = c_int::from(opts.dupe_check);
        env.a3 = c_int::from(opts.server_mode);
        env.a4 = hc_nzb_cleanup_disk();
        env.t1 = opts.cur_time;
        unsafe {
            history_coord_exec(3, &env);
        }
    });
}

#[cfg(unix)]
pub(crate) fn redownload_active(dq_ptr: usize, hist_ptr: usize) {
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    let _guard = HcArenaGuard;
    let mut env = hc_make_native_env(&host);
    env.p1 = dq_ptr;
    env.p2 = hist_ptr;
    unsafe {
        history_coord_exec(4, &env);
    }
}

#[cfg(unix)]
pub(crate) fn edit_list_active(
    dq_ptr: usize,
    ids: &[i32],
    action: i32,
    args: Option<&[u8]>,
) -> bool {
    let Some(host) = crate::ffi::queue_host() else {
        return false;
    };
    let _guard = HcArenaGuard;
    let opts = host.opts();
    let args_c = args.map(hc_cbuf);
    let mut env = hc_make_native_env(&host);
    env.p1 = dq_ptr;
    env.a1 = action;
    env.a2 = c_int::from(opts.dupe_check);
    env.a3 = c_int::from(opts.server_mode);
    env.a4 = hc_nzb_cleanup_disk();
    env.a6 = c_int::from(opts.par_check != crate::queuesubsys_host::PC_FORCE);
    env.s1 = args_c
        .as_ref()
        .map_or(std::ptr::null(), |v| v.as_ptr() as *const c_char);
    env.int_arr = if ids.is_empty() {
        std::ptr::null()
    } else {
        ids.as_ptr()
    };
    env.int_len = ids.len();
    unsafe { history_coord_exec(5, &env) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diskstate_format::{
        calc_staged_fallback, diskstate_format_file_state_name, diskstate_format_staged_part_path,
        diskstate_select_original_or_current_msg_id, eval_article_load_state,
        format_file_info_bytes, format_file_state_bytes, need_staged_file_check, parse_ids_file,
        save_id_floor_file, NzbgetRsDiskArticleInfo, NzbgetRsDiskArticleState,
        NzbgetRsDiskFileInfoInput, NzbgetRsDiskFileStateInput,
    };
    use crate::downloadinfo::is_download_completed;
    use crate::queuecoord::PARTIAL_STATE_COMPLETED;
    use std::ffi::CString;

    #[test]
    fn test_history_coord_reset_and_dup_status() {
        assert_eq!(calc_min_time(100_000, 1), 13_600);
        assert_eq!(
            classify_dup_status(MARK_GOOD, DELETE_NONE, false),
            DUP_STATUS_GOOD
        );
        assert_eq!(
            classify_dup_status(MARK_BAD, DELETE_NONE, true),
            DUP_STATUS_BAD
        );
        assert!(is_temporary_output_name(b"42.out.tmp"));
        assert!(!is_temporary_output_name(b"movie.mkv"));
        assert_eq!(
            split_param_assignment(b"*Unpack:Password=secret=123"),
            Some((&b"*Unpack:Password"[..], &b"secret=123"[..]))
        );
        assert_eq!(split_param_assignment(b"invalid_no_equals"), None);
    }

    #[test]
    fn history_coordinator_full_suite_port_from_cpp() {
        const CF_NONE: i32 = 0;
        const CF_PARTIAL: i32 = 1;
        const CF_FAILURE: i32 = 3;

        let base_dir = std::env::temp_dir().join(format!(
            "nzbget-histcoord-suite-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let queue_dir = base_dir.join("queue");
        let download_dir = base_dir.join("download");
        let tmp_dir = base_dir.join("tmp");
        std::fs::create_dir_all(&queue_dir).unwrap();
        std::fs::create_dir_all(&download_dir).unwrap();
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let file_size_fn = |p: &[u8]| -> i64 {
            let s = String::from_utf8_lossy(p);
            std::fs::metadata(s.as_ref())
                .map(|m| m.len() as i64)
                .unwrap_or(-1)
        };
        let delete_file_fn = |p: &[u8]| {
            let s = String::from_utf8_lossy(p);
            let _ = std::fs::remove_file(s.as_ref());
        };
        let file_exists_fn = |p: &[u8]| -> bool {
            let s = String::from_utf8_lossy(p);
            std::fs::metadata(s.as_ref())
                .map(|m| m.is_file())
                .unwrap_or(false)
        };

        // Helper matching HistoryRetryFixture::SavePartialFile + RetryFailed
        let run_partial_retry = |file_id: i32,
                                 failed_article: bool,
                                 output_exists: bool,
                                 borrowed_id: bool,
                                 has_paused_par: bool| {
            let out_file = download_dir.join("part152.rar");
            let _ = std::fs::remove_file(&out_file);
            if output_exists {
                std::fs::write(&out_file, b"AAAA........BBBB").unwrap();
            }

            let mut parts = vec![1, 4];
            if failed_article {
                parts.insert(1, 3);
            }
            let mut art_statuses: Vec<i32> = parts
                .iter()
                .map(|&p| {
                    if p == 3 {
                        ARTICLE_FAILED
                    } else {
                        ARTICLE_FINISHED
                    }
                })
                .collect();
            let art_sizes: Vec<i32> = vec![4; parts.len()];

            let c_subj = CString::new("partial archive").unwrap();
            let c_fname = CString::new("part152.rar").unwrap();
            let c_orig = CString::new("original-name").unwrap();
            let c_empty = CString::new("").unwrap();
            let mut c_msg_ids = Vec::new();
            for &part in &parts {
                let orig_id = format!("part-{part}@example.test");
                let cur_id = if borrowed_id && part == 3 {
                    "donor-3@example.test".to_string()
                } else {
                    orig_id.clone()
                };
                let orig_bytes = if borrowed_id && part == 3 {
                    orig_id.as_bytes()
                } else {
                    b""
                };
                let selected =
                    diskstate_select_original_or_current_msg_id(orig_bytes, cur_id.as_bytes());
                c_msg_ids.push(CString::new(selected).unwrap());
            }
            let disk_articles: Vec<NzbgetRsDiskArticleInfo> = parts
                .iter()
                .zip(c_msg_ids.iter())
                .map(|(&part, mid)| NzbgetRsDiskArticleInfo {
                    part_number: part,
                    size: 4,
                    message_id: mid.as_ptr(),
                })
                .collect();
            let finfo_in = NzbgetRsDiskFileInfoInput {
                subject: c_subj.as_ptr(),
                filename: c_fname.as_ptr(),
                origname: c_orig.as_ptr(),
                output_filename: c_empty.as_ptr(),
                filename_confirmed: 1,
                time: 0,
                output_initialized: 0,
                size: 16,
                missed_size: if failed_article { 4 } else { 8 },
                par_file: 0,
                total_articles: 4,
                missed_articles: if failed_article { 1 } else { 2 },
            };
            let finfo_bytes = unsafe {
                format_file_info_bytes(
                    &finfo_in,
                    std::ptr::null(),
                    0,
                    1,
                    disk_articles.as_ptr(),
                    disk_articles.len(),
                )
            };
            let finfo_text = String::from_utf8(finfo_bytes).unwrap();
            std::fs::write(queue_dir.join(file_id.to_string()), &finfo_text).unwrap();

            assert!(eval_requeue_loaded_file(
                CF_PARTIAL, CF_NONE, CF_FAILURE, CF_PARTIAL, true, true, 0
            ));

            let prep = prepare_requeued_loaded_file(
                download_dir.to_string_lossy().as_bytes(),
                b'/',
                b"part152.rar",
                b"part152.rar",
                2,
                PARTIAL_STATE_NONE,
                PARTIAL_STATE_NONE,
                PARTIAL_STATE_COMPLETED,
                false,
                true,
                &art_statuses,
                ARTICLE_UNDEFINED,
                ARTICLE_FAILED,
                file_size_fn,
                delete_file_fn,
                file_exists_fn,
            );

            let mut file_failed_size = if failed_article { 4 } else { 0 };
            let mut file_failed_articles = if failed_article { 1 } else { 0 };
            let mut file_remaining_size = 0i64;
            let mut file_completed_articles = if failed_article { 3 } else { 2 };
            let mut file_success_size = 8i64;
            let mut file_success_articles = 2i32;
            let mut nzb_failed_size = 8i64;
            let mut nzb_success_size = 8i64;
            let mut nzb_failed_articles = 2i32;
            let mut nzb_success_articles = 2i32;
            let mut nzb_par_failed_size = 0i64;
            let mut nzb_par_success_size = 0i64;
            let mut reset_flags = vec![0u8; art_statuses.len()];

            let mut queued_entries = Vec::new();
            if !prep.no_retryable_articles {
                reset_articles_batch(
                    false,
                    true,
                    false,
                    prep.new_partial_state,
                    16,
                    if failed_article { 4 } else { 8 },
                    4,
                    if failed_article { 1 } else { 2 },
                    &mut file_failed_size,
                    &mut file_failed_articles,
                    &mut file_remaining_size,
                    &mut file_completed_articles,
                    &mut file_success_size,
                    &mut file_success_articles,
                    &mut nzb_failed_size,
                    &mut nzb_success_size,
                    &mut nzb_failed_articles,
                    &mut nzb_success_articles,
                    &mut nzb_par_failed_size,
                    &mut nzb_par_success_size,
                    &mut art_statuses,
                    &art_sizes,
                    &mut reset_flags,
                );
                queued_entries.push((false, false, false));
            }
            if has_paused_par {
                queued_entries.push((true, true, false));
            }
            let download_completed = is_download_completed(0, true, &queued_entries);
            (
                finfo_text,
                prep,
                art_statuses,
                file_remaining_size,
                file_completed_articles,
                file_success_articles,
                file_failed_articles,
                nzb_success_articles,
                nzb_failed_articles,
                nzb_success_size,
                nzb_failed_size,
                download_completed,
            )
        };

        // 1. HistoryRetryMissingNzbArticlesProceedsToPostprocess
        {
            let (
                _,
                prep,
                _,
                _,
                _,
                _,
                _,
                nzb_succ_art,
                nzb_fail_art,
                nzb_succ_sz,
                nzb_fail_sz,
                completed,
            ) = run_partial_retry(101, false, true, false, false);
            assert!(prep.no_retryable_articles);
            assert_eq!(nzb_succ_art, 2);
            assert_eq!(nzb_fail_art, 2);
            assert_eq!(nzb_succ_sz, 8);
            assert_eq!(nzb_fail_sz, 8);
            assert!(completed);
        }

        // 2. HistoryRetryFailedArticleStillDownloads
        {
            let (
                _,
                prep,
                art_statuses,
                file_rem_sz,
                file_comp_art,
                file_succ_art,
                file_fail_art,
                nzb_succ_art,
                nzb_fail_art,
                _,
                _,
                completed,
            ) = run_partial_retry(102, true, true, false, false);
            assert!(!prep.no_retryable_articles);
            assert_eq!(file_rem_sz, 4);
            assert!(!completed);
            assert_eq!(
                art_statuses,
                vec![ARTICLE_FINISHED, ARTICLE_UNDEFINED, ARTICLE_FINISHED]
            );
            assert_eq!(file_comp_art, 2);
            assert_eq!(nzb_succ_art + file_succ_art, 2);
            assert_eq!(nzb_fail_art + file_fail_art, 1);
        }

        // 3. StagedArticleSavedForUpstream
        {
            let file_id = 103;
            let c_fname = CString::new("staged.bin").unwrap();
            let c_empty = CString::new("").unwrap();
            let mut art_states = Vec::new();
            for part in [1, 2, 3] {
                let status = if part == 3 {
                    ARTICLE_UNDEFINED
                } else {
                    ARTICLE_FINISHED
                };
                let (round, has_res) = if part == 2 { (1, true) } else { (0, false) };
                let (staged_fallback, saved_status) = calc_staged_fallback(
                    false,
                    status,
                    ARTICLE_FINISHED,
                    round,
                    has_res,
                    ARTICLE_UNDEFINED,
                );
                art_states.push(NzbgetRsDiskArticleState {
                    saved_status,
                    segment_offset: ((part - 1) * 4) as i64,
                    segment_size: 4,
                    crc: 0,
                    staged_fallback,
                });
            }
            let fstate_in = NzbgetRsDiskFileStateInput {
                success_articles: 2,
                failed_articles: 0,
                remaining_size: 4,
                success_size: 8,
                failed_size: 0,
                filename: c_fname.as_ptr(),
                hash16k: c_empty.as_ptr(),
                par_set_id: c_empty.as_ptr(),
                par_file: 0,
                decoded_file_size: 12,
                output_initialized: 0,
            };
            let state_bytes = unsafe {
                format_file_state_bytes(
                    &fstate_in,
                    std::ptr::null(),
                    0,
                    art_states.as_ptr(),
                    art_states.len(),
                )
            };
            let state_text = String::from_utf8(state_bytes).unwrap();
            let state_fname =
                String::from_utf8(diskstate_format_file_state_name(file_id, false)).unwrap();
            std::fs::write(queue_dir.join(&state_fname), &state_text).unwrap();

            let lines: Vec<&str> = state_text.lines().collect();
            let count_idx = lines.iter().position(|&l| l == "3").unwrap();
            assert!(lines[count_idx + 1].starts_with("2,"));
            assert!(lines[count_idx + 2].starts_with("0,"));

            let staged_path_bytes = diskstate_format_staged_part_path(
                tmp_dir.to_string_lossy().as_bytes(),
                b'/',
                file_id,
                2,
            );
            let staged_path = String::from_utf8(staged_path_bytes).unwrap();
            std::fs::write(&staged_path, b"abcd").unwrap();
            assert!(need_staged_file_check(
                false,
                7,
                ARTICLE_UNDEFINED,
                ARTICLE_FINISHED,
                4,
                4,
                1,
                2,
                true,
                false
            ));
            let eval_with_staged = eval_article_load_state(
                7,
                false,
                5,
                ARTICLE_UNDEFINED,
                4,
                4,
                1,
                2,
                std::path::Path::new(&staged_path).exists(),
                true,
                false,
                false,
                1,
                3,
            );
            assert_eq!(eval_with_staged.status, ARTICLE_FINISHED);
            assert_eq!(eval_with_staged.dupe_fallback_round, 1);

            std::fs::remove_file(&staged_path).unwrap();
            let eval_without_staged = eval_article_load_state(
                7,
                false,
                5,
                ARTICLE_UNDEFINED,
                4,
                4,
                1,
                2,
                std::path::Path::new(&staged_path).exists(),
                true,
                false,
                false,
                1,
                3,
            );
            assert_eq!(eval_without_staged.status, ARTICLE_UNDEFINED);
            assert_eq!(eval_without_staged.dupe_fallback_round, 0);
        }

        // 4. HistoryRetryKeepsOriginalMessageId
        {
            let (finfo_text, ..) = run_partial_retry(104, true, true, true, false);
            assert!(finfo_text.contains("part-3@example.test"));
            assert!(!finfo_text.contains("donor-3@example.test"));
        }

        // 5. HistoryRetryMissingNzbArticlesIgnoresPausedRecoveryFiles
        {
            let (_, prep, .., completed) = run_partial_retry(105, false, true, false, true);
            assert!(prep.no_retryable_articles);
            assert!(completed);
        }

        // 6. HistoryRetryMissingOutputStillDownloads
        {
            let (_, prep, art_statuses, file_rem_sz, .., completed) =
                run_partial_retry(106, false, false, false, false);
            assert!(prep.warn_missing_on_disk);
            assert_eq!(prep.new_partial_state, PARTIAL_STATE_NONE);
            assert!(!prep.no_retryable_articles);
            assert_eq!(file_rem_sz, 8);
            assert_eq!(art_statuses, vec![ARTICLE_UNDEFINED, ARTICLE_UNDEFINED]);
            assert!(!completed);
        }

        // 7. IdFloorReplacesExistingFile
        {
            assert!(save_id_floor_file(
                queue_dir.to_string_lossy().as_bytes(),
                b'/',
                10,
                20
            ));
            assert!(save_id_floor_file(
                queue_dir.to_string_lossy().as_bytes(),
                b'/',
                15,
                25
            ));
            let saved_bytes = std::fs::read(queue_dir.join("ids")).unwrap();
            assert_eq!(parse_ids_file(&saved_bytes), Some((15, 25)));
            assert!(!queue_dir.join("ids.new").exists());
        }

        let _ = std::fs::remove_dir_all(&base_dir);
    }

    #[test]
    fn history_coord_exec_service_work_hides_and_purges_expired() {
        struct MockHist {
            items: Vec<(i32, i64, &'static [u8])>,
            saved: bool,
        }
        unsafe extern "C" fn mock_hc_action(
            raw: *mut c_void,
            sub: c_int,
            a1: c_int,
            a2: c_int,
            _p1: usize,
            p2: usize,
            _s1: *const c_char,
            _s2: *const c_char,
            ints: *mut c_int,
            i64s: *mut i64,
            strs: *mut *const c_char,
            _u8s: *mut u8,
            _len: usize,
        ) -> usize {
            let h = &mut *(raw as *mut MockHist);
            match sub {
                12 => {
                    let it = &h.items[p2 - 1];
                    let is = std::slice::from_raw_parts_mut(ints, 3);
                    is[0] = 0;
                    is[1] = 0;
                    is[2] = 1;
                    *strs = it.2.as_ptr() as *const c_char;
                    0
                }
                13 => {
                    h.items[a2 as usize].0 = 3;
                    1
                }
                14 => {
                    if (a1 as usize) >= h.items.len() {
                        return 0;
                    }
                    let it = &h.items[a1 as usize];
                    *ints = it.0;
                    let ts = std::slice::from_raw_parts_mut(i64s, 2);
                    ts[0] = it.1;
                    ts[1] = 0;
                    *strs = it.2.as_ptr() as *const c_char;
                    (a1 as usize) + 1
                }
                15 => {
                    h.items.remove(a1 as usize);
                    1
                }
                16 => {
                    h.saved = true;
                    1
                }
                _ => 0,
            }
        }

        unsafe {
            let mut mock = MockHist {
                items: vec![
                    (1, 1_000_000 - 10 * 86_400, b"expired-nzb\0"),
                    (2, 1_000_000 - 10 * 86_400, b"expired-url\0"),
                    (1, 1_000_000 - 86_400, b"recent-nzb\0"),
                ],
                saved: false,
            };
            let env = NzbgetRsHistoryCoordEnv {
                ctx: (&mut mock as *mut MockHist).cast(),
                p1: 1,
                p2: 0,
                a1: 7,
                a2: 1,
                a3: 0,
                a4: 0,
                a5: b'/' as c_int,
                a6: 0,
                t1: 1_000_000,
                s1: std::ptr::null(),
                int_arr: std::ptr::null(),
                int_len: 0,
                log_cb: None,
                action_cb: Some(mock_hc_action),
            };
            assert_eq!(history_coord_exec(3, &env), 1);
            assert!(mock.saved);
            assert_eq!(mock.items.len(), 2);
            assert_eq!(mock.items[0].0, 3);
            assert_eq!(mock.items[1].0, 1);
        }
    }
}
