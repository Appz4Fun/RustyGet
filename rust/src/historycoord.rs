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
    let mut output_filename =
        crate::artwritercore::article_writer_format_dest_file_path(dest_dir, path_sep, recorded_name);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diskstate_format::{
        calc_staged_fallback, diskstate_format_file_state_name, diskstate_format_staged_part_path,
        diskstate_select_original_or_current_msg_id, eval_article_load_state,
        need_staged_file_check, nzbget_rs_diskstate_format_file_info,
        nzbget_rs_diskstate_format_file_state, parse_ids_file, save_id_floor_file,
        NzbgetRsDiskArticleInfo, NzbgetRsDiskArticleState, NzbgetRsDiskFileInfoInput,
        NzbgetRsDiskFileStateInput,
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
            let finfo_buf = unsafe {
                nzbget_rs_diskstate_format_file_info(
                    &finfo_in,
                    std::ptr::null(),
                    0,
                    1,
                    disk_articles.as_ptr(),
                    disk_articles.len(),
                )
            };
            let finfo_text = unsafe {
                String::from_utf8_lossy(std::slice::from_raw_parts(
                    finfo_buf.data as *const u8,
                    finfo_buf.len,
                ))
                .into_owned()
            };
            unsafe { crate::ffi::nzbget_rs_free(finfo_buf) };
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
            let state_buf = unsafe {
                nzbget_rs_diskstate_format_file_state(
                    &fstate_in,
                    std::ptr::null(),
                    0,
                    art_states.as_ptr(),
                    art_states.len(),
                )
            };
            let state_text = unsafe {
                String::from_utf8_lossy(std::slice::from_raw_parts(
                    state_buf.data as *const u8,
                    state_buf.len,
                ))
                .into_owned()
            };
            unsafe { crate::ffi::nzbget_rs_free(state_buf) };
            let state_fname = String::from_utf8(diskstate_format_file_state_name(file_id, false)).unwrap();
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
}
