//! `HistoryCoordinator` (`daemon/queue/HistoryCoordinator.cpp`) end-to-end Rust implementation.

use crate::queuehotpath::{
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
    filename.len() > 8 && filename.ends_with(b".out.tmp")
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
}
