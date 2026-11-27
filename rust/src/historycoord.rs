//! `HistoryCoordinator` (`daemon/queue/HistoryCoordinator.cpp`) end-to-end Rust implementation.

use crate::queuesubsys_host::NzbRef;
use crate::queuecoord::{ARTICLE_FAILED, ARTICLE_FINISHED, ARTICLE_UNDEFINED, PARTIAL_STATE_NONE};

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
pub fn classify_dup_status(
    mark_status: MarkStatus,
    delete_status: DeleteStatus,
    is_dupe_success: bool,
) -> DupStatus {
    if mark_status == MarkStatus::Good {
        DupStatus::Good
    } else if mark_status == MarkStatus::Bad {
        DupStatus::Bad
    } else if mark_status == MarkStatus::Success {
        DupStatus::Success
    } else if delete_status == DeleteStatus::Dupe {
        DupStatus::Dupe
    } else if delete_status == DeleteStatus::Manual
        || delete_status == DeleteStatus::Good
        || delete_status == DeleteStatus::Copy
    {
        DupStatus::Deleted
    } else if is_dupe_success {
        DupStatus::Success
    } else {
        DupStatus::Failed
    }
}

/// Decides whether `HistoryCoordinator::HistoryDelete` should delete the destination directory.
#[allow(clippy::too_many_arguments)]
pub fn should_delete_dest_dir(
    is_nzb: bool,
    delete_status: DeleteStatus,
    par_status: NzbParStatus,
    unpack_status: UnpackStatus,
    dir_exists: bool,
) -> bool {
    is_nzb
        && (delete_status != DeleteStatus::None
            || par_status == NzbParStatus::Failure
            || unpack_status == UnpackStatus::Failure
            || unpack_status == UnpackStatus::Password)
        && dir_exists
}

/// Calculate reset `PostTotalSec` and normalized `MoveStatus` in `HistoryCoordinator::MoveToQueue`.
///
/// `live_unpack_sec` and `live_par_sec` are the parts of `unpack_sec` and `par_sec`
/// spent while downloading (direct unpack, live PAR): `PostTotalSec` never held them,
/// so only the rest is taken from it.
#[allow(clippy::too_many_arguments)]
pub fn calc_move_to_queue_reset(
    unpack_cleaned_up_disk: bool,
    post_total_sec: i32,
    unpack_sec: i32,
    live_unpack_sec: i32,
    has_main_pars: bool,
    par_sec: i32,
    live_par_sec: i32,
    move_status: MoveStatus,
) -> (i32, MoveStatus) {
    let mut new_post_total_sec = post_total_sec;
    if !unpack_cleaned_up_disk {
        new_post_total_sec -= unpack_sec - live_unpack_sec;
        if has_main_pars {
            new_post_total_sec -= par_sec - live_par_sec;
        }
    }
    let new_move_status = if move_status == MoveStatus::Failure {
        MoveStatus::None
    } else {
        move_status
    };
    // Whole-second rounding of the stage times can pass PostTotalSec by a second.
    (new_post_total_sec.max(0), new_move_status)
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
pub fn parse_dupe_backup_flag(text: &[u8]) -> DeleteStatus {
    if text.eq_ignore_ascii_case(b"YES") || text.eq_ignore_ascii_case(b"TRUE") || text == b"1" {
        DeleteStatus::Dupe
    } else {
        DeleteStatus::Manual
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
// End-to-end `HistoryCoordinator` coordinator (`daemon/queue/HistoryCoordinator.cpp`)
// ============================================================================

use crate::qdup::DupEdit;
use crate::queuesubsys_host::FileRef;
use crate::status::{
    CleanupStatus, DeleteStatus, DirectRenameStatus, DirectUnpackStatus, DupStatus, DupeMode,
    MarkStatus, MoveStatus, NzbParStatus, UnpackStatus, UrlStatus,
};
use crate::cstr::{nul_terminated, to_cstring, until_nul};
use std::ffi::c_int;

/// Path separator of the directories the coordinator deletes.
const HC_PATH_SEP: u8 = b'/';

/// Level of a coordinator log line or job message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HcLevel {
    Debug,
    Detail,
    Info,
    Warning,
    Error,
}

/// What `AddToHistory` reads back after moving a job into history.
#[derive(Debug, Default)]
pub(crate) struct AddedToHistory {
    pub par_status: NzbParStatus,
    pub unpack_status: UnpackStatus,
    pub delete_status: DeleteStatus,
    pub parking: bool,
    pub unpack_cleaned_up: bool,
    pub completed_count: c_int,
    pub failed_size: i64,
    pub par_failed_size: i64,
    pub name: Vec<u8>,
}

/// One entry of a job's completed-files list.
#[derive(Debug, Default)]
pub(crate) struct CompletedFile {
    pub status: c_int,
    pub id: c_int,
    pub filename: Vec<u8>,
}

/// What hiding a history record needs to know about it.
#[derive(Debug, Default)]
pub(crate) struct HideInfo {
    pub nzb: NzbRef,
    pub mark_status: MarkStatus,
    pub delete_status: DeleteStatus,
    pub is_dupe_success: bool,
    pub name: Vec<u8>,
}

/// A history record seen while servicing the history.
#[derive(Debug)]
pub(crate) struct HistoryEntry {
    pub hist_ptr: usize,
    pub kind: c_int,
    pub time: i64,
    /// The record's job for an NZB record, else none.
    pub nzb: NzbRef,
    pub name: Vec<u8>,
}

/// How [`HistoryHost::find_history`] picks a record.
#[derive(Clone, Copy, Debug)]
pub(crate) enum HistoryKey {
    Id(c_int),
    Ptr(usize),
}

/// A history record found by id or address.
#[derive(Debug)]
pub(crate) struct FoundHistory {
    pub hist_ptr: usize,
    pub idx: c_int,
    pub len: c_int,
    pub kind: c_int,
    pub delete_status: DeleteStatus,
    pub par_status: NzbParStatus,
    pub unpack_status: UnpackStatus,
    pub parked_count: c_int,
    pub nzb: NzbRef,
    pub name: Vec<u8>,
    pub dest_dir: Vec<u8>,
}

/// A job's timings and state read before it returns to the queue.
#[derive(Debug, Default)]
pub(crate) struct RequeueInfo {
    pub unpack_cleaned_up_disk: bool,
    pub post_total_sec: c_int,
    pub unpack_sec: c_int,
    pub par_sec: c_int,
    pub move_status: MoveStatus,
    pub file_count: c_int,
    pub live_unpack_sec: c_int,
    pub live_par_sec: c_int,
    pub dest_dir: Vec<u8>,
}

/// A job's names and pause flag.
#[derive(Debug, Default)]
pub(crate) struct NzbNames {
    pub delete_paused: bool,
    pub name: Vec<u8>,
    pub queued_filename: Vec<u8>,
    pub dest_dir: Vec<u8>,
}

/// A completed file loaded back from disk for a retry.
#[derive(Debug)]
pub(crate) struct RetryFile {
    pub file: FileRef,
    pub state_loaded: bool,
    pub success_articles: c_int,
    pub partial_state: c_int,
    pub article_count: usize,
    pub par_file: bool,
    pub total_articles: c_int,
    pub missed_articles: c_int,
    pub failed_articles: c_int,
    pub completed_articles: c_int,
    pub nzb_failed_articles: c_int,
    pub nzb_success_articles: c_int,
    pub size: i64,
    pub missed_size: i64,
    pub failed_size: i64,
    pub remaining_size: i64,
    pub success_size: i64,
    pub nzb_failed_size: i64,
    pub nzb_success_size: i64,
    pub nzb_par_failed_size: i64,
    pub nzb_par_success_size: i64,
    pub filename: Vec<u8>,
}

/// File and job counters after resetting a requeued file's articles.
#[derive(Debug)]
pub(crate) struct RequeueCounts {
    pub file_failed_articles: c_int,
    pub file_completed_articles: c_int,
    pub file_success_articles: c_int,
    pub nzb_failed_articles: c_int,
    pub nzb_success_articles: c_int,
    pub file_failed_size: i64,
    pub file_remaining_size: i64,
    pub file_success_size: i64,
    pub nzb_failed_size: i64,
    pub nzb_success_size: i64,
    pub nzb_par_failed_size: i64,
    pub nzb_par_success_size: i64,
}

/// One edit of an NZB or URL history record's job.
#[derive(Clone, Copy, Debug)]
pub(crate) enum HistEdit<'a> {
    Param { name: &'a [u8], value: &'a [u8] },
    Category(&'a [u8]),
    Name(&'a [u8]),
    DupeKey(&'a [u8]),
    DupeScore(c_int),
    DupeMode(DupeMode),
    DeleteStatus(DeleteStatus),
}

/// The queue the history coordinator works on. Each default does nothing
/// and reports no data, so a test host implements only what it needs.
pub(crate) trait HistoryHost {
    fn log(&self, _level: HcLevel, _msg: &[u8]) {}
    fn nzb_message(&self, _nzb: NzbRef, _level: HcLevel, _msg: &[u8]) {}
    fn discard_disk_files(&self, _nzb: NzbRef, _delete_log: bool) {}
    /// Clears the job's file list and returns its queued nzb file names.
    fn clear_file_list(&self, _nzb: NzbRef) -> Vec<u8> {
        Vec::new()
    }
    fn move_to_history(&self, _dq: usize, _nzb: NzbRef) -> AddedToHistory {
        AddedToHistory::default()
    }
    fn completed_file(&self, _nzb: NzbRef, _idx: c_int) -> Option<CompletedFile> {
        None
    }
    fn finish_add_to_history(&self, _nzb: NzbRef, _parked_count: c_int) {}
    fn forget_par_damage(&self, _nzb: NzbRef) {}
    /// Marks the job for a dupe recheck and returns its failed articles.
    fn failed_article_samples(&self, _nzb: NzbRef) -> Vec<crate::dupeprobe::ProbeSample> {
        Vec::new()
    }
    fn hide_info(&self, _hist: usize) -> HideInfo {
        HideInfo::default()
    }
    fn replace_with_dup(&self, _dq: usize, _hist: usize, _rindex: c_int, _status: DupStatus) {}
    /// The record `rindex` places from the end of the history.
    fn history_entry_from_end(&self, _dq: usize, _rindex: c_int) -> Option<HistoryEntry> {
        None
    }
    fn erase_history(&self, _dq: usize, _idx: c_int, _from_end: bool) {}
    fn save_history(&self, _dq: usize) {}
    fn mark_bad(&self, _dq: usize, _id: c_int) {}
    fn find_history(&self, _dq: usize, _key: HistoryKey) -> Option<FoundHistory> {
        None
    }
    fn requeue_info(&self, _nzb: NzbRef) -> RequeueInfo {
        RequeueInfo::default()
    }
    fn file_name_at(&self, _nzb: NzbRef, _idx: c_int) -> Vec<u8> {
        Vec::new()
    }
    #[allow(clippy::too_many_arguments)]
    fn return_to_queue(
        &self,
        _dq: usize,
        _hist: usize,
        _hidx: c_int,
        _reprocess: bool,
        _has_main_pars: bool,
        _post_total_sec: c_int,
        _move_status: MoveStatus,
    ) {
    }
    fn notify_returned(&self, _dq: usize, _nzb: NzbRef, _added: bool) {}
    fn build_dest_dir(&self, _nzb: NzbRef) -> Vec<u8> {
        Vec::new()
    }
    fn return_url_to_queue(&self, _dq: usize, _hist: usize, _hidx: c_int) {}
    fn nzb_names(&self, _nzb: NzbRef) -> NzbNames {
        NzbNames::default()
    }
    /// Parses an nzb file into a new job; returns the job's owner address.
    fn parse_nzb(&self, _path: &[u8]) -> Option<usize> {
        None
    }
    fn reset_for_redownload(&self, _nzb: NzbRef, _new_nzb: usize, _paused: bool) {}
    fn load_file_for_retry(&self, _nzb: NzbRef, _id: c_int, _status: c_int) -> Option<RetryFile> {
        None
    }
    fn delete_file_info(&self, _file: FileRef) {}
    fn rename_file(&self, _file: FileRef, _name: &[u8]) {}
    fn article_states(&self, _file: FileRef, _statuses: &mut [c_int], _sizes: &mut [c_int]) {}
    /// Puts a completed file back in the queue; returns its file id.
    fn requeue_file(
        &self,
        _nzb: NzbRef,
        _file: FileRef,
        _completed_idx: c_int,
        _partial_state: c_int,
        _counts: &RequeueCounts,
        _reset: &[u8],
    ) -> c_int {
        0
    }
    /// Drops the requeued files' repair jobs; returns whether the job is
    /// completed and its id.
    fn finish_retry(&self, _nzb: NzbRef, _requeued: &[c_int], _health_pause: bool) -> (bool, c_int) {
        (false, 0)
    }
    fn pause_extra_pars(&self, _dq: usize, _nzb_id: c_int) {}
    /// Edits an NZB or URL record; returns the job id when an NZB job's
    /// name changed.
    fn edit_history(&self, _hist: usize, _edit: HistEdit<'_>) -> Option<c_int> {
        None
    }
    /// Edits a hidden dupe record.
    fn edit_dup(&self, _hist: usize, _edit: DupEdit<'_>) {}
}

#[cfg(unix)]
fn hc_trunc(bytes: &[u8]) -> Vec<u8> {
    until_nul(bytes).to_vec()
}

/// The `HistoryCoordinator` operations on one download queue.
pub(crate) struct HistoryCoord<'a> {
    pub host: &'a dyn HistoryHost,
    pub dq: usize,
}

impl HistoryCoord<'_> {
    fn log(&self, level: HcLevel, msg: &[u8]) {
        self.host.log(level, until_nul(msg));
    }

    fn nzb_msg(&self, nzb: NzbRef, level: HcLevel, msg: &[u8]) {
        self.host.nzb_message(nzb, level, until_nul(msg));
    }

    /// `DeleteDiskFiles`.
    pub(crate) fn delete_disk_files(&self, nzb: NzbRef, server_mode: bool, nzb_cleanup_disk: bool) {
        if nzb.is_none() {
            return;
        }
        if server_mode {
            self.host.discard_disk_files(nzb, true);
        }
        let queued_fn = self.host.clear_file_list(nzb);
        if !nzb_cleanup_disk {
            return;
        }
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
                let cstr = to_cstring(name1);
                let mut msg = b"Deleting file ".to_vec();
                msg.extend_from_slice(name1);
                self.log(HcLevel::Info, &msg);
                crate::fsops::delete_file(&cstr);
            }
        }
    }

    fn delete_dest_dir_if_present(&self, dest_dir: &[u8], level: HcLevel) {
        if crate::fsops::directory_exists(dest_dir, &crate::fsops::posix_stat) {
            let mut msg = b"Deleting ".to_vec();
            msg.extend_from_slice(dest_dir);
            self.log(level, &msg);
            let mut err_text = Vec::new();
            let done = crate::fsdelete::delete_directory_with_content(
                &mut crate::fsops::PosixFsOps,
                dest_dir,
                HC_PATH_SEP,
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
                self.log(HcLevel::Error, &em);
            }
        }
    }

    fn recheck_failed_articles(&self, nzb_ptr: NzbRef) {
        #[cfg(unix)]
        if let Some(qh) = crate::ffi::queue_host() {
            let dq_ptr = self.dq;
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
                if crate::dupecoord::same_name_or_key(&q.name, &q.dupe_key, &nzb.name, &nzb.dupe_key)
                {
                    return;
                }
            }
            if opts.dupe_check
                && nzb.dupe_mode == DupeMode::Score
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
            let failed = self.host.failed_article_samples(nzb_ptr);
            if failed.len() < 2 {
                return;
            }
            let idxs = crate::dupeprobe::sample_indexes(failed.len(), 10);
            let samples: Vec<_> = idxs.into_iter().map(|i| failed[i].clone()).collect();
            crate::dupeprobe::start_recheck_rs(nzb.id, samples);
        }
        #[cfg(not(unix))]
        let _ = nzb_ptr;
    }

    /// `AddToHistory` (+ `RecheckFailedArticles`).
    pub(crate) fn add_to_history(&self, nzb: NzbRef) {
        let added = self.host.move_to_history(self.dq, nzb);
        let mut completed_count = added.completed_count;
        let cleanup = added.par_status == NzbParStatus::Success
            || added.unpack_status == UnpackStatus::Success
            || (added.par_status == NzbParStatus::None
                && added.unpack_status == UnpackStatus::None
                && (added.failed_size - added.par_failed_size) == 0)
            || (added.delete_status != DeleteStatus::None && !added.parking)
            || added.unpack_cleaned_up;
        if cleanup {
            self.host.discard_disk_files(nzb, false);
            completed_count = 0;
        }

        let mut parked_count = 0i32;
        for cidx in 0..completed_count {
            let cf = self.host.completed_file(nzb, cidx).unwrap_or_default();
            let is_last = cidx + 1 == completed_count;
            if should_park_completed_file(cf.status, 0, 2, is_last) {
                let mut pm = b"Parking file ".to_vec();
                pm.extend_from_slice(&cf.filename);
                self.nzb_msg(nzb, HcLevel::Detail, &pm);
                parked_count += 1;
            }
        }

        self.host.finish_add_to_history(nzb, parked_count);

        let mut msg = b"Collection ".to_vec();
        msg.extend_from_slice(&added.name);
        msg.extend_from_slice(b" added to history");
        self.nzb_msg(nzb, HcLevel::Info, &msg);

        self.recheck_failed_articles(nzb);

        self.host.forget_par_damage(nzb);
    }

    /// `HistoryHide`.
    pub(crate) fn history_hide(
        &self,
        hist_ptr: usize,
        rindex: c_int,
        server_mode: bool,
        nzb_cleanup_disk: bool,
    ) {
        let info = self.host.hide_info(hist_ptr);
        let dup_status = classify_dup_status(info.mark_status, info.delete_status, info.is_dupe_success);

        self.delete_disk_files(info.nzb, server_mode, nzb_cleanup_disk);

        let mut msg = b"Collection ".to_vec();
        msg.extend_from_slice(&info.name);
        msg.extend_from_slice(b" removed from history");
        self.log(HcLevel::Info, &msg);

        self.host.replace_with_dup(self.dq, hist_ptr, rindex, dup_status);
    }

    /// `ServiceWork`: hides or removes the records older than the history keep time.
    pub(crate) fn service_work(
        &self,
        keep_history: c_int,
        dupe_check: bool,
        server_mode: bool,
        nzb_cleanup_disk: bool,
        cur_time: i64,
    ) {
        let min_time = calc_min_time(cur_time, keep_history);
        let mut changed = false;
        let mut rindex = 0i32;

        while let Some(entry) = self.host.history_entry_from_end(self.dq, rindex) {
            let act =
                classify_service_work_item(entry.kind, 3, 1, entry.time, min_time, dupe_check);
            if act != 0 {
                if act == 1 {
                    self.history_hide(entry.hist_ptr, rindex, server_mode, nzb_cleanup_disk);
                    rindex += 1;
                } else {
                    if act == 2 {
                        self.delete_disk_files(entry.nzb, server_mode, nzb_cleanup_disk);
                    }
                    let mut rm = b"Collection ".to_vec();
                    rm.extend_from_slice(&entry.name);
                    rm.extend_from_slice(b" removed from history");
                    self.log(HcLevel::Info, &rm);
                    self.host.erase_history(self.dq, rindex, true);
                }
                changed = true;
            } else {
                rindex += 1;
            }
        }

        if changed {
            self.host.save_history(self.dq);
        }
    }

    fn move_to_queue(
        &self,
        hist_ptr: usize,
        hidx: c_int,
        nzb_ptr: NzbRef,
        hist_name: &[u8],
        reprocess: bool,
    ) {
        let mut dbg_msg = b"Returning ".to_vec();
        dbg_msg.extend_from_slice(hist_name);
        dbg_msg.extend_from_slice(b" from history back to download queue");
        self.log(HcLevel::Debug, &dbg_msg);

        let nicename = hist_name.to_vec();

        let info = self.host.requeue_info(nzb_ptr);
        for fidx in 0..info.file_count {
            let mut unpark = b"Unparking file ".to_vec();
            unpark.extend_from_slice(&self.host.file_name_at(nzb_ptr, fidx));
            self.nzb_msg(nzb_ptr, HcLevel::Detail, &unpark);
        }

        let has_main_pars = if !info.unpack_cleaned_up_disk {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&info.dest_dir));
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
            info.unpack_cleaned_up_disk,
            info.post_total_sec,
            info.unpack_sec,
            info.live_unpack_sec,
            has_main_pars,
            info.par_sec,
            info.live_par_sec,
            info.move_status,
        );
        self.host.return_to_queue(
            self.dq,
            hist_ptr,
            hidx,
            reprocess,
            has_main_pars,
            new_post_total_sec,
            new_move_status,
        );

        let mut ret_msg = nicename.clone();
        ret_msg.extend_from_slice(b" returned from history back to download queue");
        self.nzb_msg(nzb_ptr, HcLevel::Info, &ret_msg);

        if reprocess {
            let mut rst_msg = b"Restarting postprocessing for ".to_vec();
            rst_msg.extend_from_slice(&nicename);
            self.log(HcLevel::Debug, &rst_msg);
            self.host.notify_returned(self.dq, nzb_ptr, false);
            #[cfg(unix)]
            {
                crate::feed::notify_active_feed_queue_aspect(4, None);
                crate::urlcoord::notify_active_urlcoord_queue_update(4);
            }
        }
    }

    /// `Redownload`: downloads the record `hist_ptr` again. Only the dupe failover and rescue
    /// paths call this, so the copy always comes back running: a backup appended paused (as
    /// the add-on sends fleets) went to history as a paused duplicate, and restoring that
    /// state left the promoted copy paused and the fleet stalled forever.
    pub(crate) fn redownload(&self, hist_ptr: usize) {
        if let Some(found) = self.host.find_history(self.dq, HistoryKey::Ptr(hist_ptr)) {
            self.history_redownload(
                found.hist_ptr,
                found.idx,
                found.kind,
                found.nzb,
                &found.name,
                false,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn history_redownload(
        &self,
        hist_ptr: usize,
        hidx: c_int,
        kind: c_int,
        nzb_ptr: NzbRef,
        hist_name: &[u8],
        restore_pause_state: bool,
    ) {
        if kind == 2 {
            self.host.return_url_to_queue(self.dq, hist_ptr, hidx);
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
            self.log(HcLevel::Error, &em);
            return;
        }
        // downloaded again: the health it was judged on, and its keeping as a
        // donor, belong to the earlier try
        #[cfg(unix)]
        if let Some(qh) = crate::ffi::queue_host() {
            qh.nzb_set_param(nzb_ptr, crate::dupecoord::PROJECTED_PARAM, None);
            qh.nzb_set_param(nzb_ptr, crate::dupecoord::PARKED_PARAM, None);
        }

        let names = self.host.nzb_names(nzb_ptr);
        let paused = restore_pause_state && names.delete_paused;
        let nzb_name = names.name;
        let queued_fn = names.queued_filename;
        let old_dest_dir = names.dest_dir;

        if !crate::fsops::file_exists(&queued_fn, &crate::fsops::posix_stat) {
            let mut em = b"Could not download again ".to_vec();
            em.extend_from_slice(&nzb_name);
            em.extend_from_slice(b": could not find source nzb-file ");
            em.extend_from_slice(&queued_fn);
            self.log(HcLevel::Error, &em);
            return;
        }

        let Some(new_nzb) = self.host.parse_nzb(&queued_fn) else {
            let mut em = b"Could not download again ".to_vec();
            em.extend_from_slice(&nzb_name);
            em.extend_from_slice(b": could not parse nzb-file");
            self.log(HcLevel::Error, &em);
            return;
        };

        let mut info_m = b"Downloading again ".to_vec();
        info_m.extend_from_slice(&nzb_name);
        self.log(HcLevel::Info, &info_m);

        self.delete_dest_dir_if_present(&old_dest_dir, HcLevel::Detail);

        let new_dest_dir = self.host.build_dest_dir(nzb_ptr);
        self.delete_dest_dir_if_present(&new_dest_dir, HcLevel::Detail);

        self.host.reset_for_redownload(nzb_ptr, new_nzb, paused);

        self.move_to_queue(hist_ptr, hidx, nzb_ptr, hist_name, false);

        self.host.notify_returned(self.dq, nzb_ptr, true);
        #[cfg(unix)]
        {
            crate::feed::notify_active_feed_queue_aspect(4, None);
            crate::urlcoord::notify_active_urlcoord_queue_update(4);
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn history_retry(
        &self,
        hist_ptr: usize,
        hidx: c_int,
        kind: c_int,
        nzb_ptr: NzbRef,
        hist_name: &[u8],
        reset_failed: bool,
        reprocess: bool,
        pause_extra_pars: bool,
    ) {
        let (wrong_type, missing_dir, start_msg) = retry_action_labels(reset_failed, reprocess);
        if kind != 1 {
            let mut em = b"Could not ".to_vec();
            em.extend_from_slice(&wrong_type[..wrong_type.len() - 1]);
            em.extend_from_slice(b" for ");
            em.extend_from_slice(hist_name);
            em.extend_from_slice(b": history item has wrong type");
            self.log(HcLevel::Error, &em);
            return;
        }

        let names = self.host.nzb_names(nzb_ptr);
        let nzb_name = names.name;
        let dest_dir = names.dest_dir;

        if !crate::fsops::directory_exists(&dest_dir, &crate::fsops::posix_stat) {
            let mut em = b"Could not ".to_vec();
            em.extend_from_slice(&missing_dir[..missing_dir.len() - 1]);
            em.push(b' ');
            em.extend_from_slice(hist_name);
            em.extend_from_slice(b": destination directory ");
            em.extend_from_slice(&dest_dir);
            em.extend_from_slice(b" doesn't exist");
            self.log(HcLevel::Error, &em);
            return;
        }

        let mut sm = start_msg[..start_msg.len() - 1].to_vec();
        sm.push(b' ');
        sm.extend_from_slice(&nzb_name);
        self.nzb_msg(nzb_ptr, HcLevel::Info, &sm);

        let mut requeued: Vec<c_int> = Vec::new();
        let mut cidx = 0i32;

        while let Some(cf) = self.host.completed_file(nzb_ptr, cidx) {
            let cf_status = cf.status;
            let cf_id = cf.id;
            let recorded_name = cf.filename;

            if cf_status != 1 && (cf_status != 3 || reset_failed) && cf_id > 0 {
                if let Some(rf) = self.host.load_file_for_retry(nzb_ptr, cf_id, cf_status) {
                    let file = rf.file;
                    let article_count = rf.article_count;
                    let mut file_failed_articles = rf.failed_articles;
                    let mut file_completed_articles = rf.completed_articles;
                    let mut file_success_articles = rf.success_articles;
                    let mut nzb_failed_articles = rf.nzb_failed_articles;
                    let mut nzb_success_articles = rf.nzb_success_articles;
                    let mut file_failed_size = rf.failed_size;
                    let mut file_remaining_size = rf.remaining_size;
                    let mut file_success_size = rf.success_size;
                    let mut nzb_failed_size = rf.nzb_failed_size;
                    let mut nzb_success_size = rf.nzb_success_size;
                    let mut nzb_par_failed_size = rf.nzb_par_failed_size;
                    let mut nzb_par_success_size = rf.nzb_par_success_size;
                    let cur_filename = rf.filename;

                    if eval_requeue_loaded_file(
                        cf_status,
                        0,
                        3,
                        2,
                        reset_failed,
                        rf.state_loaded,
                        file_remaining_size,
                    ) {
                        let mut statuses = vec![0i32; article_count];
                        let mut sizes = vec![0i32; article_count];
                        if article_count > 0 {
                            self.host.article_states(file, &mut statuses, &mut sizes);
                        }

                        let prep = prepare_requeued_loaded_file(
                            &dest_dir,
                            HC_PATH_SEP,
                            &recorded_name,
                            &cur_filename,
                            rf.success_articles,
                            rf.partial_state,
                            0,
                            2,
                            reprocess,
                            reset_failed,
                            &statuses,
                            ARTICLE_UNDEFINED,
                            ARTICLE_FAILED,
                            |p| {
                                crate::fsops::file_size(&to_cstring(p))
                            },
                            |p| {
                                crate::fsops::delete_file(&to_cstring(p));
                            },
                            |p| crate::fsops::file_exists(p, &crate::fsops::posix_stat),
                        );

                        let effective_filename = if prep.set_recorded_filename {
                            self.host.rename_file(file, &recorded_name);
                            recorded_name.clone()
                        } else {
                            cur_filename
                        };

                        if prep.warn_missing_on_disk {
                            let mut wm = b"File ".to_vec();
                            wm.extend_from_slice(&effective_filename);
                            wm.extend_from_slice(b" could not be found on disk, downloading again");
                            self.nzb_msg(nzb_ptr, HcLevel::Warning, &wm);
                        }

                        if prep.no_retryable_articles {
                            self.host.delete_file_info(file);
                            cidx += 1;
                            continue;
                        }

                        let mut reset_flags = vec![0u8; article_count];
                        reset_articles_batch(
                            cf_status == 3,
                            reset_failed,
                            rf.par_file,
                            prep.new_partial_state,
                            rf.size,
                            rf.missed_size,
                            rf.total_articles,
                            rf.missed_articles,
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
                            &mut statuses,
                            &sizes,
                            &mut reset_flags,
                        );

                        let counts = RequeueCounts {
                            file_failed_articles,
                            file_completed_articles,
                            file_success_articles,
                            nzb_failed_articles,
                            nzb_success_articles,
                            file_failed_size,
                            file_remaining_size,
                            file_success_size,
                            nzb_failed_size,
                            nzb_success_size,
                            nzb_par_failed_size,
                            nzb_par_success_size,
                        };
                        let req_id = self.host.requeue_file(
                            nzb_ptr,
                            file,
                            cidx,
                            prep.new_partial_state,
                            &counts,
                            &reset_flags,
                        );
                        requeued.push(req_id);
                        continue;
                    }

                    self.host.delete_file_info(file);
                }
            }
            cidx += 1;
        }

        let (is_completed, nzb_id) =
            self.host
                .finish_retry(nzb_ptr, &requeued, !reset_failed && !reprocess);

        self.move_to_queue(hist_ptr, hidx, nzb_ptr, hist_name, reprocess || is_completed);

        if pause_extra_pars {
            self.host.pause_extra_pars(self.dq, nzb_id);
        }
    }

    fn set_parameter(
        &self,
        hist_ptr: usize,
        kind: c_int,
        nzb_ptr: NzbRef,
        hist_name: &[u8],
        text: Option<&[u8]>,
    ) -> bool {
        let text_bytes = text.unwrap_or_default();
        let mut dm = b"Setting post-process-parameter '".to_vec();
        dm.extend_from_slice(text_bytes);
        dm.extend_from_slice(b"' for '");
        dm.extend_from_slice(hist_name);
        dm.push(b'\'');
        self.log(HcLevel::Debug, &dm);

        if kind != 1 && kind != 2 {
            let mut em = b"Could not set post-process-parameter for ".to_vec();
            em.extend_from_slice(hist_name);
            em.extend_from_slice(b": history item has wrong type");
            self.log(HcLevel::Error, &em);
            return false;
        }

        if let Some((name, val)) = text.and_then(split_param_assignment) {
            self.host.edit_history(hist_ptr, HistEdit::Param { name, value: val });
            return true;
        }

        let names = self.host.nzb_names(nzb_ptr);
        let mut em = b"Could not set post-process-parameter for ".to_vec();
        em.extend_from_slice(&names.name);
        em.extend_from_slice(b": invalid argument: ");
        em.extend_from_slice(text_bytes);
        self.log(HcLevel::Error, &em);
        true
    }

    fn set_category(&self, hist_ptr: usize, kind: c_int, hist_name: &[u8], text: &[u8]) -> bool {
        let mut dm = b"Setting category '".to_vec();
        dm.extend_from_slice(text);
        dm.extend_from_slice(b"' for '");
        dm.extend_from_slice(hist_name);
        dm.push(b'\'');
        self.log(HcLevel::Debug, &dm);

        if kind != 1 && kind != 2 {
            let mut em = b"Could not set category for ".to_vec();
            em.extend_from_slice(hist_name);
            em.extend_from_slice(b": history item has wrong type");
            self.log(HcLevel::Error, &em);
            return false;
        }

        self.host.edit_history(hist_ptr, HistEdit::Category(text));
        true
    }

    fn set_name(&self, hist_ptr: usize, hist_name: &[u8], text: &[u8]) -> bool {
        let mut dm = b"Setting name '".to_vec();
        dm.extend_from_slice(text);
        dm.extend_from_slice(b"' for '");
        dm.extend_from_slice(hist_name);
        dm.push(b'\'');
        self.log(HcLevel::Debug, &dm);

        if text.is_empty() {
            let mut em = b"Could not rename ".to_vec();
            em.extend_from_slice(hist_name);
            em.extend_from_slice(b". The new name cannot be empty");
            self.log(HcLevel::Error, &em);
            return false;
        }

        let changed_id = self.host.edit_history(hist_ptr, HistEdit::Name(text));
        #[cfg(unix)]
        if let Some(id) = changed_id {
            crate::dupesearch_coord::notify_active_dupesearch_name_changed(id);
        }
        #[cfg(not(unix))]
        let _ = changed_id;
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn set_dupe_param(
        &self,
        hist_ptr: usize,
        kind: c_int,
        delete_status: DeleteStatus,
        hist_name: &[u8],
        action: c_int,
        text: Option<&[u8]>,
    ) {
        let text_bytes = text.unwrap_or_default();
        let mut dm = format!("Setting dupe-parameter '{action}'='").into_bytes();
        dm.extend_from_slice(text_bytes);
        dm.extend_from_slice(b"' for '");
        dm.extend_from_slice(hist_name);
        dm.push(b'\'');
        self.log(HcLevel::Debug, &dm);

        let mut mode = DupeMode::Score;
        if action == 45 {
            let Some(parsed) = crate::queueeditor::parse_dupe_mode(text_bytes) else {
                let mut em = b"Could not set duplicate mode for ".to_vec();
                em.extend_from_slice(hist_name);
                em.extend_from_slice(b": incorrect mode (");
                em.extend_from_slice(text_bytes);
                em.push(b')');
                self.log(HcLevel::Error, &em);
                return;
            };
            mode = parsed;
        }

        let score = match text {
            None => 0,
            Some(t) => {
                let t_c = nul_terminated(t);
                // SAFETY: `t_c` is NUL-terminated.
                unsafe { libc::atoi(t_c.as_ptr().cast()) }
            }
        };

        if kind == 1 || kind == 2 {
            match action {
                43 => {
                    self.host.edit_history(hist_ptr, HistEdit::DupeKey(text_bytes));
                }
                44 => {
                    self.host.edit_history(hist_ptr, HistEdit::DupeScore(score));
                }
                45 => {
                    self.host.edit_history(hist_ptr, HistEdit::DupeMode(mode));
                }
                46 => {
                    if kind == 2 {
                        let mut em = b"Could not set duplicate parameter for ".to_vec();
                        em.extend_from_slice(hist_name);
                        em.extend_from_slice(b": history item has wrong type");
                        self.log(HcLevel::Error, &em);
                        return;
                    }
                    if delete_status != DeleteStatus::Dupe && delete_status != DeleteStatus::Manual
                    {
                        let mut em = b"Could not set duplicate parameter for ".to_vec();
                        em.extend_from_slice(hist_name);
                        em.extend_from_slice(b": history item has wrong delete status");
                        self.log(HcLevel::Error, &em);
                        return;
                    }
                    let status = parse_dupe_backup_flag(text_bytes);
                    self.host
                        .edit_history(hist_ptr, HistEdit::DeleteStatus(status));
                }
                _ => {}
            }
        } else if kind == 3 {
            match action {
                43 => self.host.edit_dup(hist_ptr, DupEdit::DupeKey(text_bytes)),
                44 => self.host.edit_dup(hist_ptr, DupEdit::Score(score)),
                45 => self.host.edit_dup(hist_ptr, DupEdit::Mode(mode)),
                46 => {
                    let mut em = b"Could not set duplicate parameter for ".to_vec();
                    em.extend_from_slice(hist_name);
                    em.extend_from_slice(b": history item has wrong type");
                    self.log(HcLevel::Error, &em);
                }
                _ => {}
            }
        }
    }

    /// `EditList`: applies a history edit `action` to the records `ids`.
    /// Returns whether any record was edited.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(crate) fn edit_list(
        &self,
        ids: &[i32],
        action: c_int,
        args: Option<&[u8]>,
        dupe_check: bool,
        server_mode: bool,
        nzb_cleanup_disk: bool,
        pause_extra_pars: bool,
    ) -> bool {
        if action == 47 {
            for &id in ids {
                self.host.mark_bad(self.dq, id);
            }
        }

        let mut ok = false;
        for &id in ids {
            let Some(found) = self.host.find_history(self.dq, HistoryKey::Id(id)) else {
                continue;
            };
            let hist_ptr = found.hist_ptr;
            let hidx = found.idx;
            let kind = found.kind;
            let nzb_ptr = found.nzb;
            let hist_name = found.name;
            let dest_dir = found.dest_dir;

            let mut done = true;
            match action {
                36 | 37 => {
                    let is_final = action == 37;
                    let mut dm = b"Deleting ".to_vec();
                    dm.extend_from_slice(&hist_name);
                    dm.extend_from_slice(b" from history");
                    self.log(HcLevel::Info, &dm);

                    if kind == 1 {
                        self.delete_disk_files(nzb_ptr, server_mode, nzb_cleanup_disk);
                        let dir_exists =
                            crate::fsops::directory_exists(&dest_dir, &crate::fsops::posix_stat);
                        if should_delete_dest_dir(
                            true,
                            found.delete_status,
                            found.par_status,
                            found.unpack_status,
                            dir_exists,
                        ) {
                            self.delete_dest_dir_if_present(&dest_dir, HcLevel::Info);
                        }
                    }

                    if is_final || !dupe_check || kind == 2 {
                        self.host.erase_history(self.dq, hidx, false);
                    } else if kind == 1 {
                        let rindex = found.len - 1 - hidx;
                        self.history_hide(hist_ptr, rindex, server_mode, nzb_cleanup_disk);
                    }
                }
                38 => {
                    if kind == 2 {
                        self.history_redownload(hist_ptr, hidx, kind, nzb_ptr, &hist_name, false);
                    } else if kind == 1 && found.parked_count == 0 {
                        let mut wm = b"Could not download remaining files for ".to_vec();
                        wm.extend_from_slice(&hist_name);
                        wm.extend_from_slice(
                            b": history item does not have any files left for download",
                        );
                        self.log(HcLevel::Warning, &wm);
                    } else if kind == 1 {
                        self.history_retry(
                            hist_ptr,
                            hidx,
                            kind,
                            nzb_ptr,
                            &hist_name,
                            false,
                            false,
                            pause_extra_pars,
                        );
                    } else {
                        let mut em = b"Could not download remaining files for ".to_vec();
                        em.extend_from_slice(&hist_name);
                        em.extend_from_slice(b": history item has wrong type");
                        self.log(HcLevel::Error, &em);
                    }
                }
                39 => {
                    if kind != 1 {
                        let mut em = b"Could not post-process again ".to_vec();
                        em.extend_from_slice(&hist_name);
                        em.extend_from_slice(b": history item has wrong type");
                        self.log(HcLevel::Error, &em);
                    } else {
                        self.history_retry(
                            hist_ptr,
                            hidx,
                            kind,
                            nzb_ptr,
                            &hist_name,
                            false,
                            true,
                            pause_extra_pars,
                        );
                    }
                }
                40 => {
                    self.history_redownload(hist_ptr, hidx, kind, nzb_ptr, &hist_name, false);
                }
                41 => {
                    self.history_retry(
                        hist_ptr,
                        hidx,
                        kind,
                        nzb_ptr,
                        &hist_name,
                        true,
                        false,
                        pause_extra_pars,
                    );
                }
                42 => {
                    done = self.set_parameter(hist_ptr, kind, nzb_ptr, &hist_name, args);
                }
                50 => {
                    done = self.set_category(hist_ptr, kind, &hist_name, args.unwrap_or_default());
                }
                51 => {
                    done = self.set_name(hist_ptr, &hist_name, args.unwrap_or_default());
                }
                43..=46 => {
                    self.set_dupe_param(
                        hist_ptr,
                        kind,
                        found.delete_status,
                        &hist_name,
                        action,
                        args,
                    );
                }
                47..=49 =>
                {
                    #[cfg(unix)]
                    if let Some(qh) = crate::ffi::queue_host() {
                        crate::dupecoord::history_mark_host(
                            &qh,
                            self.dq,
                            hist_ptr,
                            MarkStatus::from(action - 46),
                        );
                    }
                }
                _ => {}
            }
            ok = ok || done;
        }

        if ok {
            self.host.save_history(self.dq);
        }
        ok
    }
}

#[cfg(unix)]
#[allow(clippy::wildcard_imports)]
mod native {
    use super::*;
    use crate::queuesubsys_host::{
        QueueSubsysHost, AI_FAILED, ASPECT_NZB_RETURNED, ASPECT_URL_RETURNED, CF_FAILURE,
        CF_NONE, CF_PARTIAL, DH_NONE, DH_REDOWNLOAD_MANUAL, DP_NONE, EA_GROUP_PAUSE_EXTRA_PARS,
        FILE_ACT_CLEAR_ARTICLES, FILE_I64_FAILED_SIZE, FILE_I64_REMAINING_SIZE,
        FILE_I64_SUCCESS_SIZE, FILE_INT_COMPLETED_ARTICLES, FILE_INT_FAILED_ARTICLES,
        FILE_INT_PARTIAL_STATE, FILE_INT_PAUSED, FILE_INT_SUCCESS_ARTICLES, FILE_STR_FILENAME,
        HK_DUP, HK_NZB, HK_URL, MK_DEBUG, MK_DETAIL, MK_ERROR, MK_INFO, MK_WARNING,
        NZB_ACT_BUILD_DEST_DIR, NZB_ACT_CLEAR_EXTRACTED_ARCHIVES, NZB_ACT_CLEAR_FILE_LIST,
        NZB_ACT_CLEAR_LIVE_REPAIR_THREAD, NZB_ACT_CLEAR_SCRIPT_STATUSES,
        NZB_ACT_UPDATE_CURRENT_STATS, NZB_I64_DOWNLOADED_SIZE, NZB_I64_DUPE_RECOVERED_BYTES,
        NZB_I64_FAILED_SIZE, NZB_I64_PAR_FAILED_SIZE, NZB_I64_PAR_SUCCESS_SIZE,
        NZB_I64_SUCCESS_SIZE, NZB_INT_ALL_FIRST, NZB_INT_DEAD_PICK_PROBED, NZB_INT_DELETE_PAUSED,
        NZB_INT_DOWNLOAD_SEC, NZB_INT_DUPE_ATTEMPTED_ARTICLES, NZB_INT_DUPE_FAILED_RUN,
        NZB_INT_DUPE_FAILOVER_CHECKED, NZB_INT_DUPE_HINT, NZB_INT_DUPE_NO_PAR_NOTED,
        NZB_INT_DUPE_PAR_DEFER_STATE, NZB_INT_DUPE_RECOVERED_ARTICLES,
        NZB_INT_DUPE_RECOVERED_HOLES, NZB_INT_DUPE_SCORE, NZB_INT_DUPE_UNSOURCED_ARTICLES,
        NZB_INT_EXTRA_PAR_BLOCKS, NZB_INT_FAILED_ARTICLES, NZB_INT_HEALTH_PAUSED,
        NZB_INT_LIVE_PAR_SEC, NZB_INT_LIVE_UNPACK_SEC, NZB_INT_LOADING_PAR,
        NZB_INT_PARKED_FILE_COUNT, NZB_INT_PARKING, NZB_INT_PAR_FULL, NZB_INT_PAR_RENAME_STATUS,
        NZB_INT_PAR_REPAIRED, NZB_INT_PAR_SEC, NZB_INT_POST_TOTAL_SEC, NZB_INT_RAR_RENAME_STATUS,
        NZB_INT_REMAINING_PAR_COUNT, NZB_INT_REPAIR_SEC, NZB_INT_REPROCESS,
        NZB_INT_SUCCESS_ARTICLES, NZB_INT_UNPACK_CLEANED_UP_DISK, NZB_INT_UNPACK_SEC,
        NZB_INT_UNPACK_TRIED, NZB_INT_WAITING_PAR, NZB_STR_CATEGORY, NZB_STR_DUPE_KEY,
        NZB_STR_FINAL_DIR, NZB_STR_NAME, PARTIAL_COMPLETED, RS_NONE,
    };

    impl HistoryHost for QueueSubsysHost {
        fn log(&self, level: HcLevel, msg: &[u8]) {
            let mk = match level {
                HcLevel::Debug => MK_DEBUG,
                HcLevel::Detail => MK_DETAIL,
                HcLevel::Info => MK_INFO,
                HcLevel::Warning => MK_WARNING,
                HcLevel::Error => MK_ERROR,
            };
            crate::log::log_active_bytes(mk, msg);
        }

        fn nzb_message(&self, nzb: NzbRef, level: HcLevel, msg: &[u8]) {
            let mk = match level {
                HcLevel::Detail => MK_DETAIL,
                HcLevel::Warning => MK_WARNING,
                _ => MK_INFO,
            };
            self.nzb_print_message(nzb, mk, msg);
        }

        fn discard_disk_files(&self, nzb: NzbRef, delete_log: bool) {
            self.disk_discard_files_ex(nzb, delete_log);
        }

        fn clear_file_list(&self, nzb: NzbRef) -> Vec<u8> {
            let snap = self.nzb_snap(nzb, false);
            self.nzb_action(nzb, NZB_ACT_CLEAR_FILE_LIST);
            hc_trunc(&snap.queued_filename)
        }

        fn move_to_history(&self, dq: usize, job: NzbRef) -> AddedToHistory {
            let nzb0 = self.nzb_snap(job, false);
            if nzb0.has_live_repair_thread {
                let t = nzb0.live_repair_thread_ptr;
                self.nzb_action(job, NZB_ACT_CLEAR_LIVE_REPAIR_THREAD);
                self.thread_stop(t);
            }
            crate::postproc::live_par_time_settle(self, job, true);
            self.history_move_queue_item(dq, job);
            self.dq_history_changed(dq);
            let fn_len = self.nzb_file_len(job);
            for i in 0..fn_len {
                let fp = self.nzb_file_at(job, i);
                let fs = self.file_snap(fp);
                self.nzb_update_completed_stats(job, fp);
                self.nzb_push_completed_file(job, fp, &fs.filename, CF_NONE, 0);
            }
            let nzb = self.nzb_snap(job, false);
            AddedToHistory {
                par_status: nzb.par_status,
                unpack_status: nzb.unpack_status,
                delete_status: nzb.delete_status,
                parking: nzb.parking,
                unpack_cleaned_up: nzb.unpack_cleaned_up_disk,
                completed_count: self.nzb_completed_files_len(job) as c_int,
                failed_size: nzb.failed_size,
                par_failed_size: nzb.par_failed_size,
                name: hc_trunc(&nzb.name),
            }
        }

        fn completed_file(&self, job: NzbRef, idx: c_int) -> Option<CompletedFile> {
            let cfs_len = self.nzb_completed_files_len(job);
            if idx < 0 || (idx as usize) >= cfs_len {
                return None;
            }
            let cf = self.nzb_completed_file_at(job, idx as usize);
            Some(CompletedFile {
                status: cf.status,
                id: cf.id,
                filename: hc_trunc(&cf.filename),
            })
        }

        fn finish_add_to_history(&self, job: NzbRef, parked_count: c_int) {
            let nzb = self.nzb_snap(job, false);
            self.nzb_set_int(job, NZB_INT_PARKED_FILE_COUNT, parked_count);
            self.nzb_action(job, NZB_ACT_CLEAR_FILE_LIST);
            self.nzb_set_int(job, NZB_INT_REMAINING_PAR_COUNT, 0);
            self.nzb_set_int(job, NZB_INT_PARKING, 0);
            if nzb.direct_rename_status == DirectRenameStatus::Running {
                self.nzb_set_direct_rename_status(job, DirectRenameStatus::Failure);
            }
            if nzb.direct_unpack_status == DirectUnpackStatus::Running {
                self.nzb_set_direct_unpack_status(job, DirectUnpackStatus::None);
            }
            self.nzb_set_int(job, NZB_INT_DUPE_HINT, DH_NONE);
        }

        fn forget_par_damage(&self, job: NzbRef) {
            let nzb = self.nzb_snap(job, false);
            crate::dupefallback::par_damage_forget_all(nzb.id);
        }

        fn failed_article_samples(&self, job: NzbRef) -> Vec<crate::dupeprobe::ProbeSample> {
            self.nzb_set_param(job, b"*DupeRecheck", Some(b"1"));
            let mut out = Vec::new();
            for cf in self.nzb_completed_files(job) {
                if (cf.status != CF_PARTIAL && cf.status != CF_FAILURE) || cf.id <= 0 {
                    continue;
                }
                let partial = cf.status == CF_PARTIAL;
                // a file deleted from the queue with the download (parked on
                // health) has no article list left: nothing to sample, and no
                // "Error reading diskstate" for it (B4)
                if !crate::diskstate_format::active_file_info_exists(cf.id) {
                    continue;
                }
                let (fi_ptr, state_loaded) = self.disk_load_file_for_inspect(cf.id, false, partial);
                if fi_ptr.is_none() {
                    continue;
                }
                if partial && !state_loaded {
                    self.delete_file_info(fi_ptr);
                    continue;
                }
                let groups: Vec<Vec<u8>> =
                    self.file_groups(fi_ptr).iter().map(|g| hc_trunc(g)).collect();
                let fs = self.file_snap(fi_ptr);
                for aidx in 0..fs.articles_len {
                    let art = self.article_at(fi_ptr, aidx);
                    if (!partial || art.status == AI_FAILED) && !art.message_id.is_empty() {
                        let mid = hc_trunc(&art.message_id);
                        if !mid.is_empty() {
                            out.push(crate::dupeprobe::ProbeSample {
                                message_id: mid,
                                groups: groups.clone(),
                            });
                        }
                    }
                }
                self.delete_file_info(fi_ptr);
            }
            out
        }

        fn hide_info(&self, hist: usize) -> HideInfo {
            let hi = self.hist_item_snap(hist);
            let nzb_ptr = hi.nzb();
            let nzb = self.nzb_snap(nzb_ptr, false);
            HideInfo {
                nzb: nzb_ptr,
                mark_status: nzb.mark_status,
                delete_status: nzb.delete_status,
                is_dupe_success: nzb.is_dupe_success,
                name: hc_trunc(&hi.name),
            }
        }

        fn replace_with_dup(&self, dq: usize, hist: usize, rindex: c_int, status: DupStatus) {
            self.history_replace_with_dup(dq, hist, rindex as usize, status);
        }

        fn history_entry_from_end(&self, dq: usize, rindex: c_int) -> Option<HistoryEntry> {
            let hl_len = self.history_len(dq);
            if rindex < 0 || (rindex as usize) >= hl_len {
                return None;
            }
            let hi = self.history_at(dq, hl_len - 1 - (rindex as usize));
            if hi.hi_ptr == 0 {
                return None;
            }
            Some(HistoryEntry {
                hist_ptr: hi.hi_ptr,
                kind: hi.kind,
                time: hi.time,
                nzb: NzbRef::from_addr(if hi.kind == HK_NZB { hi.inner_ptr } else { 0 }),
                name: hc_trunc(&hi.name),
            })
        }

        fn erase_history(&self, dq: usize, idx: c_int, from_end: bool) {
            let hl_len = self.history_len(dq);
            let pos = if from_end {
                hl_len - 1 - (idx as usize)
            } else {
                idx as usize
            };
            self.history_erase_at(dq, pos);
        }

        fn save_history(&self, dq: usize) {
            self.dq_history_changed(dq);
            self.dq_save(dq);
        }

        fn mark_bad(&self, dq: usize, id: c_int) {
            let hi_ptr = self.history_find_by_id(dq, id);
            if hi_ptr != 0 {
                let hi = self.hist_item_snap(hi_ptr);
                if hi.kind == HK_NZB && hi.nzb().is_some() {
                    self.nzb_set_mark_status(hi.nzb(), MarkStatus::Bad);
                }
            }
        }

        fn find_history(&self, dq: usize, key: HistoryKey) -> Option<FoundHistory> {
            let hl_len = self.history_len(dq);
            let (idx, hi) = (0..hl_len)
                .map(|idx| (idx, self.history_at(dq, idx)))
                .find(|(_, hi)| match key {
                    HistoryKey::Ptr(p) => hi.hi_ptr == p,
                    HistoryKey::Id(id) => hi.id == id,
                })?;
            if hi.hi_ptr == 0 {
                return None;
            }
            let nzb_ptr = hi.nzb();
            let nzb = nzb_ptr.is_some().then(|| self.nzb_snap(nzb_ptr, false));
            Some(FoundHistory {
                hist_ptr: hi.hi_ptr,
                idx: idx as c_int,
                len: hl_len as c_int,
                kind: hi.kind,
                delete_status: nzb.as_ref().map_or_else(DeleteStatus::default, |n| n.delete_status),
                par_status: nzb.as_ref().map_or_else(NzbParStatus::default, |n| n.par_status),
                unpack_status: nzb.as_ref().map_or_else(UnpackStatus::default, |n| n.unpack_status),
                parked_count: nzb.as_ref().map_or(0, |n| n.parked_file_count),
                nzb: nzb_ptr,
                name: hc_trunc(&hi.name),
                dest_dir: hc_trunc(nzb.as_ref().map_or(b"", |n| n.dest_dir.as_slice())),
            })
        }

        fn requeue_info(&self, job: NzbRef) -> RequeueInfo {
            let nzb = self.nzb_snap(job, false);
            RequeueInfo {
                unpack_cleaned_up_disk: nzb.unpack_cleaned_up_disk,
                post_total_sec: nzb.post_total_sec,
                unpack_sec: nzb.unpack_sec,
                par_sec: nzb.par_sec,
                move_status: nzb.move_status,
                file_count: self.nzb_file_len(job) as c_int,
                live_unpack_sec: nzb.live_unpack_sec,
                live_par_sec: nzb.live_par_sec,
                dest_dir: hc_trunc(&nzb.dest_dir),
            }
        }

        fn file_name_at(&self, job: NzbRef, idx: c_int) -> Vec<u8> {
            let fp = self.nzb_file_at(job, idx as usize);
            hc_trunc(&self.file_snap(fp).filename)
        }

        fn return_to_queue(
            &self,
            dq: usize,
            hist: usize,
            hidx: c_int,
            reprocess: bool,
            has_main_pars: bool,
            post_total_sec: c_int,
            move_status: MoveStatus,
        ) {
            let hi = self.hist_item_snap(hist);
            let nzb_ptr = hi.nzb();
            let nzb = self.nzb_snap(nzb_ptr, false);
            // Live work that ended after the job reached history is no part of it.
            crate::postproc::live_par_time_discard(nzb.id);
            if !nzb.unpack_cleaned_up_disk {
                self.nzb_set_unpack_status(nzb_ptr, UnpackStatus::None);
                self.nzb_set_direct_unpack_status(nzb_ptr, DirectUnpackStatus::None);
                self.nzb_set_int(nzb_ptr, NZB_INT_PAR_REPAIRED, 0);
                self.nzb_set_int(nzb_ptr, NZB_INT_UNPACK_TRIED, 0);
                self.nzb_set_last_unpack_status(nzb_ptr, UnpackStatus::None);
                self.nzb_action(nzb_ptr, NZB_ACT_CLEAR_EXTRACTED_ARCHIVES);
                self.nzb_set_cleanup_status(nzb_ptr, CleanupStatus::None);
                self.nzb_set_int(nzb_ptr, NZB_INT_PAR_RENAME_STATUS, RS_NONE);
                self.nzb_set_int(nzb_ptr, NZB_INT_RAR_RENAME_STATUS, RS_NONE);
                self.nzb_set_int(nzb_ptr, NZB_INT_POST_TOTAL_SEC, post_total_sec);
                self.nzb_set_int(nzb_ptr, NZB_INT_UNPACK_SEC, 0);
                self.nzb_set_int(nzb_ptr, NZB_INT_LIVE_UNPACK_SEC, 0);
                if has_main_pars {
                    self.nzb_set_par_status(nzb_ptr, NzbParStatus::None);
                    self.nzb_set_int(nzb_ptr, NZB_INT_PAR_SEC, 0);
                    self.nzb_set_int(nzb_ptr, NZB_INT_LIVE_PAR_SEC, 0);
                    self.nzb_set_int(nzb_ptr, NZB_INT_REPAIR_SEC, 0);
                    self.nzb_set_int(nzb_ptr, NZB_INT_PAR_FULL, 0);
                }
            }
            self.nzb_set_delete_status(nzb_ptr, DeleteStatus::None);
            self.nzb_set_int(nzb_ptr, NZB_INT_DELETE_PAUSED, 0);
            self.nzb_set_int(nzb_ptr, NZB_INT_DUPE_FAILOVER_CHECKED, -1);
            self.nzb_set_int(nzb_ptr, NZB_INT_DEAD_PICK_PROBED, 0);
            self.nzb_set_int(nzb_ptr, NZB_INT_DUPE_ATTEMPTED_ARTICLES, 0);
            self.nzb_set_int(nzb_ptr, NZB_INT_DUPE_UNSOURCED_ARTICLES, 0);
            self.nzb_set_int(nzb_ptr, NZB_INT_DUPE_FAILED_RUN, 0);
            self.nzb_set_int(nzb_ptr, NZB_INT_DUPE_PAR_DEFER_STATE, DP_NONE);
            self.nzb_set_mark_status(nzb_ptr, MarkStatus::None);
            self.nzb_action(nzb_ptr, NZB_ACT_CLEAR_SCRIPT_STATUSES);
            self.nzb_set_int(nzb_ptr, NZB_INT_PARKED_FILE_COUNT, 0);
            self.nzb_set_move_status(nzb_ptr, move_status);
            self.nzb_set_int(nzb_ptr, NZB_INT_REPROCESS, c_int::from(reprocess));
            self.nzb_set_str(nzb_ptr, NZB_STR_FINAL_DIR, b"");
            crate::postproc::forget_dead_volume_verdicts(nzb.id);
            crate::donorstore::forget(nzb.id);
            crate::postproc::forget_put_back(nzb.id);
            self.history_return_to_queue(dq, hist, hidx as usize);
        }

        fn notify_returned(&self, dq: usize, job: NzbRef, added: bool) {
            if let Some(pp) = crate::ffi::active_prepost() {
                if added {
                    pp.nzb_added(self, dq, job);
                } else {
                    pp.nzb_downloaded(self, dq, job);
                }
            }
            self.prepost_queue_changed(dq, ASPECT_NZB_RETURNED, job, FileRef::NONE);
        }

        fn build_dest_dir(&self, job: NzbRef) -> Vec<u8> {
            self.nzb_action(job, NZB_ACT_BUILD_DEST_DIR);
            hc_trunc(&self.nzb_snap(job, false).dest_dir)
        }

        fn return_url_to_queue(&self, dq: usize, hist: usize, hidx: c_int) {
            let hi = self.hist_item_snap(hist);
            let nzb_ptr = hi.nzb();
            let nzb = self.nzb_snap(nzb_ptr, false);
            self.nzb_set_url_status(nzb_ptr, UrlStatus::None);
            self.nzb_set_delete_status(nzb_ptr, DeleteStatus::None);
            if nzb.dupe_hint == DH_NONE {
                self.nzb_set_int(nzb_ptr, NZB_INT_DUPE_HINT, DH_REDOWNLOAD_MANUAL);
            }
            self.history_return_to_queue(dq, hist, hidx as usize);
            self.prepost_queue_changed(dq, ASPECT_URL_RETURNED, nzb_ptr, FileRef::NONE);
        }

        fn nzb_names(&self, job: NzbRef) -> NzbNames {
            let nzb = self.nzb_snap(job, false);
            NzbNames {
                delete_paused: nzb.delete_paused,
                name: hc_trunc(&nzb.name),
                queued_filename: hc_trunc(&nzb.queued_filename),
                dest_dir: hc_trunc(&nzb.dest_dir),
            }
        }

        fn parse_nzb(&self, path: &[u8]) -> Option<usize> {
            let parsed = crate::nzbfile::parse_and_materialize_nzb(self, path, b"");
            if !parsed.ok {
                self.destroy_nzb_uptr(parsed.uptr_addr);
                return None;
            }
            (parsed.uptr_addr != 0).then_some(parsed.uptr_addr)
        }

        fn reset_for_redownload(&self, job: NzbRef, new_nzb: usize, paused: bool) {
            // SAFETY: `new_nzb` is the address of the owning pointer that
            // `parse_nzb` returned, which holds the new job's address.
            let src_nzb_ptr =
                NzbRef::from_addr(if new_nzb != 0 { unsafe { *(new_nzb as *const usize) } } else { 0 });
            if src_nzb_ptr.is_some() {
                let n = self.nzb_file_len(src_nzb_ptr);
                let paused_val = c_int::from(paused);
                for idx in 0..n {
                    let fp = self.nzb_file_at(src_nzb_ptr, idx);
                    self.file_set_int(fp, FILE_INT_PAUSED, paused_val);
                }
            }
            self.disk_discard_files(job);
            crate::postproc::live_par_time_discard(self.nzb_snap(job, false).id);
            self.nzb_set_move_status(job, MoveStatus::None);
            self.nzb_set_int(job, NZB_INT_UNPACK_CLEANED_UP_DISK, 0);
            self.nzb_set_par_status(job, NzbParStatus::None);
            self.nzb_set_int(job, NZB_INT_PAR_RENAME_STATUS, RS_NONE);
            self.nzb_set_int(job, NZB_INT_RAR_RENAME_STATUS, RS_NONE);
            self.nzb_set_direct_rename_status(job, DirectRenameStatus::None);
            self.nzb_set_direct_unpack_status(job, DirectUnpackStatus::None);
            self.nzb_set_int(job, NZB_INT_PAR_REPAIRED, 0);
            self.nzb_set_int(job, NZB_INT_UNPACK_TRIED, 0);
            self.nzb_set_last_unpack_status(job, UnpackStatus::None);
            self.nzb_action(job, NZB_ACT_CLEAR_EXTRACTED_ARCHIVES);
            self.nzb_set_int(job, NZB_INT_HEALTH_PAUSED, 0);
            self.nzb_set_i64(job, NZB_I64_DOWNLOADED_SIZE, 0);
            self.nzb_set_int(job, NZB_INT_DOWNLOAD_SEC, 0);
            self.nzb_set_int(job, NZB_INT_POST_TOTAL_SEC, 0);
            self.nzb_set_int(job, NZB_INT_PAR_SEC, 0);
            self.nzb_set_int(job, NZB_INT_REPAIR_SEC, 0);
            self.nzb_set_int(job, NZB_INT_UNPACK_SEC, 0);
            self.nzb_set_int(job, NZB_INT_LIVE_UNPACK_SEC, 0);
            self.nzb_set_int(job, NZB_INT_LIVE_PAR_SEC, 0);
            self.nzb_set_int(job, NZB_INT_EXTRA_PAR_BLOCKS, 0);
            self.nzb_set_int(job, NZB_INT_ALL_FIRST, 0);
            self.nzb_set_int(job, NZB_INT_WAITING_PAR, 0);
            self.nzb_set_int(job, NZB_INT_LOADING_PAR, 0);
            self.nzb_set_int(job, NZB_INT_DUPE_RECOVERED_ARTICLES, 0);
            self.nzb_set_i64(job, NZB_I64_DUPE_RECOVERED_BYTES, 0);
            self.nzb_set_int(job, NZB_INT_DUPE_RECOVERED_HOLES, 0);
            self.nzb_set_param(job, crate::queuecoord::PAR_ARTICLES_PARAM, None);
            self.nzb_set_param(job, crate::queuecoord::PAR_FILES_PARAM, None);
            self.nzb_set_int(job, NZB_INT_DUPE_PAR_DEFER_STATE, DP_NONE);
            self.nzb_set_int(job, NZB_INT_DUPE_NO_PAR_NOTED, 0);
            self.nzb_redownload_replace_files(job, src_nzb_ptr);
            self.destroy_nzb_uptr(new_nzb);
            crate::postproc::forget_put_back(self.nzb_snap(job, false).id);
            if let Some((qc, qh)) = crate::ffi::active_queue_coord_and_host() {
                qc.check_dupe_file_infos(&qh, job);
            }
        }

        fn load_file_for_retry(&self, job: NzbRef, id: c_int, status: c_int) -> Option<RetryFile> {
            let (fi_ptr, state_loaded) =
                self.disk_load_file_for_inspect(id, true, status == CF_PARTIAL);
            if fi_ptr.is_none() {
                return None;
            }
            self.file_set_nzb_info(fi_ptr, job);
            let fs = self.file_snap(fi_ptr);
            let nzb = self.nzb_snap(job, false);
            Some(RetryFile {
                file: fi_ptr,
                state_loaded,
                success_articles: fs.success_articles,
                partial_state: fs.partial_state,
                article_count: (fs.articles_len as c_int).max(0) as usize,
                par_file: fs.par_file,
                total_articles: fs.total_articles,
                missed_articles: fs.missed_articles,
                failed_articles: fs.failed_articles,
                completed_articles: fs.completed_articles,
                nzb_failed_articles: nzb.failed_articles,
                nzb_success_articles: nzb.success_articles,
                size: fs.size,
                missed_size: fs.missed_size,
                failed_size: fs.failed_size,
                remaining_size: fs.remaining_size,
                success_size: fs.success_size,
                nzb_failed_size: nzb.failed_size,
                nzb_success_size: nzb.success_size,
                nzb_par_failed_size: nzb.par_failed_size,
                nzb_par_success_size: nzb.par_success_size,
                filename: hc_trunc(&fs.filename),
            })
        }

        fn delete_file_info(&self, file: FileRef) {
            QueueSubsysHost::delete_file_info(self, file);
        }

        fn rename_file(&self, file: FileRef, name: &[u8]) {
            self.file_set_str(file, FILE_STR_FILENAME, Some(name));
        }

        fn article_states(&self, file: FileRef, statuses: &mut [c_int], sizes: &mut [c_int]) {
            for (idx, (status, size)) in statuses.iter_mut().zip(sizes.iter_mut()).enumerate() {
                let art = self.article_at(file, idx);
                *status = art.status;
                *size = art.size;
            }
        }

        fn requeue_file(
            &self,
            job: NzbRef,
            file: FileRef,
            completed_idx: c_int,
            partial_state: c_int,
            counts: &RequeueCounts,
            reset: &[u8],
        ) -> c_int {
            self.file_set_int(file, FILE_INT_PARTIAL_STATE, partial_state);
            self.file_subtract_server_stats_from_nzb_total(file, job);
            self.nzb_set_i64(job, NZB_I64_FAILED_SIZE, counts.nzb_failed_size);
            self.nzb_set_i64(job, NZB_I64_SUCCESS_SIZE, counts.nzb_success_size);
            self.nzb_set_int(job, NZB_INT_FAILED_ARTICLES, counts.nzb_failed_articles);
            self.nzb_set_int(job, NZB_INT_SUCCESS_ARTICLES, counts.nzb_success_articles);
            self.nzb_set_i64(job, NZB_I64_PAR_FAILED_SIZE, counts.nzb_par_failed_size);
            self.nzb_set_i64(job, NZB_I64_PAR_SUCCESS_SIZE, counts.nzb_par_success_size);
            for (idx, _) in reset.iter().enumerate().filter(|(_, &flag)| flag != 0) {
                let art = self.article_at(file, idx);
                if art.ptr != 0 {
                    self.invalidate_source_replacement(file, art.ptr);
                    self.article_edit(art.ptr, crate::qarticle::ArticleEdit::ResetForRetry);
                }
            }
            self.file_set_i64(file, FILE_I64_FAILED_SIZE, counts.file_failed_size);
            self.file_set_i64(file, FILE_I64_REMAINING_SIZE, counts.file_remaining_size);
            self.file_set_i64(file, FILE_I64_SUCCESS_SIZE, counts.file_success_size);
            self.file_set_int(file, FILE_INT_FAILED_ARTICLES, counts.file_failed_articles);
            self.file_set_int(file, FILE_INT_COMPLETED_ARTICLES, counts.file_completed_articles);
            self.file_set_int(file, FILE_INT_SUCCESS_ARTICLES, counts.file_success_articles);
            let fs = self.file_snap(file);
            self.disk_discard_file(fs.id, false, true, partial_state != PARTIAL_COMPLETED);
            if partial_state == PARTIAL_COMPLETED {
                self.disk_save_file_state(file, true);
            }
            self.file_action(file, FILE_ACT_CLEAR_ARTICLES);
            self.nzb_add_requeued_file(job, file);
            self.nzb_erase_completed_file(job, completed_idx as usize);
            fs.id
        }

        fn finish_retry(&self, job: NzbRef, requeued: &[c_int], health_pause: bool) -> (bool, c_int) {
            if !requeued.is_empty() {
                let jobs = self.nzb_stream_repair_jobs(job);
                for j in (0..jobs.len()).rev() {
                    if requeued.contains(&jobs[j].file_id) {
                        self.nzb_stream_repair_erase_job(job, j, false);
                    }
                }
            }
            self.nzb_action(job, NZB_ACT_UPDATE_CURRENT_STATS);
            if health_pause {
                self.nzb_set_int(job, NZB_INT_HEALTH_PAUSED, 1);
            }
            let nzb = self.nzb_snap(job, false);
            (nzb.is_download_completed, nzb.id)
        }

        fn pause_extra_pars(&self, dq: usize, nzb_id: c_int) {
            self.dq_edit_entry(dq, nzb_id, EA_GROUP_PAUSE_EXTRA_PARS, None);
        }

        fn edit_history(&self, hist: usize, edit: HistEdit<'_>) -> Option<c_int> {
            let hi = self.hist_item_snap(hist);
            let nzb = hi.nzb();
            match edit {
                HistEdit::Param { name, value } => self.nzb_set_param(nzb, name, Some(value)),
                HistEdit::Category(category) => self.nzb_set_str(nzb, NZB_STR_CATEGORY, category),
                HistEdit::Name(name) => {
                    if hi.kind == HK_NZB || hi.kind == HK_URL {
                        let prev = self.nzb_snap(nzb, false).name;
                        self.nzb_set_str(nzb, NZB_STR_NAME, name);
                        let after = self.nzb_snap(nzb, false);
                        if hi.kind == HK_NZB && prev != after.name && after.id != 0 {
                            return Some(after.id);
                        }
                    } else if hi.kind == HK_DUP {
                        self.dup_edit(hi.inner_ptr, DupEdit::Name(name));
                    }
                }
                HistEdit::DupeKey(key) => {
                    self.nzb_set_param(nzb, b"DupeSearchOwnedKey", None);
                    self.nzb_set_str(nzb, NZB_STR_DUPE_KEY, key);
                }
                HistEdit::DupeScore(score) => self.nzb_set_int(nzb, NZB_INT_DUPE_SCORE, score),
                HistEdit::DupeMode(mode) => self.nzb_set_dupe_mode(nzb, mode),
                HistEdit::DeleteStatus(status) => self.nzb_set_delete_status(nzb, status),
            }
            None
        }

        fn edit_dup(&self, hist: usize, edit: DupEdit<'_>) {
            self.dup_edit(self.hist_item_snap(hist).inner_ptr, edit);
        }
    }
}

#[cfg(unix)]
fn hc_nzb_cleanup_disk() -> bool {
    crate::options::active_options().is_some_and(|o| o.scalars().nzb_cleanup_disk != 0)
}

#[cfg(unix)]
pub(crate) fn delete_disk_files_active(nzb_ptr: NzbRef) {
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    let server_mode = host.opts().server_mode;
    HistoryCoord { host: &host, dq: 0 }.delete_disk_files(
        nzb_ptr,
        server_mode,
        hc_nzb_cleanup_disk(),
    );
}

#[cfg(unix)]
pub(crate) fn add_to_history_active(dq_ptr: usize, nzb_ptr: NzbRef) {
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    HistoryCoord { host: &host, dq: dq_ptr }.add_to_history(nzb_ptr);
}

#[cfg(unix)]
pub(crate) fn history_hide_active(dq_ptr: usize, hist_ptr: usize, rindex: i32) {
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    let server_mode = host.opts().server_mode;
    HistoryCoord { host: &host, dq: dq_ptr }.history_hide(
        hist_ptr,
        rindex,
        server_mode,
        hc_nzb_cleanup_disk(),
    );
}

#[cfg(unix)]
pub(crate) fn service_work_active() {
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    host.with_locked_queue(|dq_ptr| {
        let opts = host.opts();
        HistoryCoord { host: &host, dq: dq_ptr }.service_work(
            opts.keep_history,
            opts.dupe_check,
            opts.server_mode,
            hc_nzb_cleanup_disk(),
            opts.cur_time,
        );
    });
}

#[cfg(unix)]
pub(crate) fn redownload_active(dq_ptr: usize, hist_ptr: usize) {
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    HistoryCoord { host: &host, dq: dq_ptr }.redownload(hist_ptr);
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
    let opts = host.opts();
    HistoryCoord { host: &host, dq: dq_ptr }.edit_list(
        ids,
        action,
        args.map(until_nul),
        opts.dupe_check,
        opts.server_mode,
        hc_nzb_cleanup_disk(),
        opts.par_check != crate::queuesubsys_host::PC_FORCE,
    )
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
    fn delete_dest_dir_only_for_failed_jobs() {
        let del = |delete_status, par, unpack, exists| {
            should_delete_dest_dir(true, delete_status, par, unpack, exists)
        };
        assert!(!del(
            DeleteStatus::None,
            NzbParStatus::Success,
            UnpackStatus::Success,
            true
        ));
        assert!(del(
            DeleteStatus::None,
            NzbParStatus::Failure,
            UnpackStatus::None,
            true
        ));
        assert!(del(
            DeleteStatus::None,
            NzbParStatus::None,
            UnpackStatus::Failure,
            true
        ));
        assert!(del(
            DeleteStatus::None,
            NzbParStatus::None,
            UnpackStatus::Password,
            true
        ));
        assert!(!del(
            DeleteStatus::None,
            NzbParStatus::None,
            UnpackStatus::Space,
            true
        ));
        assert!(del(
            DeleteStatus::Health,
            NzbParStatus::None,
            UnpackStatus::None,
            true
        ));
        assert!(!del(
            DeleteStatus::Health,
            NzbParStatus::None,
            UnpackStatus::None,
            false
        ));
        // A stored value that names no status counts as none of them, as the raw value did.
        assert!(!del(
            DeleteStatus::None,
            NzbParStatus::from(9),
            UnpackStatus::from(9),
            true
        ));
        assert!(!del(
            DeleteStatus::None,
            NzbParStatus::from(-1),
            UnpackStatus::from(-1),
            true
        ));
        // An unknown delete status is still not None, so the directory goes.
        assert!(del(
            DeleteStatus::from(9),
            NzbParStatus::None,
            UnpackStatus::None,
            true
        ));
        assert!(!should_delete_dest_dir(
            false,
            DeleteStatus::Health,
            NzbParStatus::Failure,
            UnpackStatus::None,
            true
        ));
    }

    #[test]
    fn move_to_queue_reset_keeps_unknown_move_status() {
        let (_, ms) = calc_move_to_queue_reset(true, 0, 0, 0, false, 0, 0, MoveStatus::from(7));
        assert_eq!(i32::from(ms), 7);
    }

    #[test]
    fn dupe_backup_flag_sets_dupe_or_manual() {
        assert_eq!(parse_dupe_backup_flag(b"yes"), DeleteStatus::Dupe);
        assert_eq!(parse_dupe_backup_flag(b"1"), DeleteStatus::Dupe);
        assert_eq!(parse_dupe_backup_flag(b"no"), DeleteStatus::Manual);
    }

    #[test]
    fn move_to_queue_reset_takes_only_post_download_stage_time() {
        // 60 s of post-processing; UnpackSec 40 of which 35 ran while downloading
        // (direct unpack), ParSec 12 of which 9 ran while downloading (live PAR).
        assert_eq!(
            calc_move_to_queue_reset(false, 60, 40, 35, true, 12, 9, MoveStatus::None),
            (52, MoveStatus::None)
        );
        // Without main pars only the unpack part is redone.
        assert_eq!(
            calc_move_to_queue_reset(false, 60, 40, 35, false, 12, 9, MoveStatus::None),
            (55, MoveStatus::None)
        );
        // Nothing while downloading: the whole stage times, as before.
        assert_eq!(
            calc_move_to_queue_reset(false, 60, 40, 0, true, 12, 0, MoveStatus::Failure),
            (8, MoveStatus::None)
        );
        // Cleaned-up disk: nothing is redone.
        assert_eq!(
            calc_move_to_queue_reset(true, 60, 40, 35, true, 12, 9, MoveStatus::Success),
            (60, MoveStatus::Success)
        );
    }

    #[test]
    fn test_history_coord_reset_and_dup_status() {
        assert_eq!(calc_min_time(100_000, 1), 13_600);
        assert_eq!(
            classify_dup_status(MarkStatus::Good, DeleteStatus::None, false),
            DupStatus::Good
        );
        assert_eq!(
            classify_dup_status(MarkStatus::Bad, DeleteStatus::None, true),
            DupStatus::Bad
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
    fn history_coord_service_work_hides_and_purges_expired() {
        use std::cell::{Cell, RefCell};
        struct MockHist {
            items: RefCell<Vec<(i32, i64, &'static [u8])>>,
            saved: Cell<bool>,
        }
        impl HistoryHost for MockHist {
            fn hide_info(&self, hist: usize) -> HideInfo {
                let items = self.items.borrow();
                HideInfo {
                    nzb: NzbRef::NONE,
                    mark_status: MarkStatus::None,
                    delete_status: DeleteStatus::None,
                    is_dupe_success: true,
                    name: items[hist - 1].2.to_vec(),
                }
            }
            fn replace_with_dup(&self, _dq: usize, _hist: usize, rindex: c_int, _: DupStatus) {
                self.items.borrow_mut()[rindex as usize].0 = 3;
            }
            fn history_entry_from_end(&self, _dq: usize, rindex: c_int) -> Option<HistoryEntry> {
                let items = self.items.borrow();
                let it = items.get(rindex as usize)?;
                Some(HistoryEntry {
                    hist_ptr: rindex as usize + 1,
                    kind: it.0,
                    time: it.1,
                    nzb: NzbRef::NONE,
                    name: it.2.to_vec(),
                })
            }
            fn erase_history(&self, _dq: usize, idx: c_int, _from_end: bool) {
                self.items.borrow_mut().remove(idx as usize);
            }
            fn save_history(&self, _dq: usize) {
                self.saved.set(true);
            }
        }

        let mock = MockHist {
            items: RefCell::new(vec![
                (1, 1_000_000 - 10 * 86_400, b"expired-nzb"),
                (2, 1_000_000 - 10 * 86_400, b"expired-url"),
                (1, 1_000_000 - 86_400, b"recent-nzb"),
            ]),
            saved: Cell::new(false),
        };
        HistoryCoord { host: &mock, dq: 1 }.service_work(7, true, false, false, 1_000_000);
        assert!(mock.saved.get());
        let items = mock.items.borrow();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, 3);
        assert_eq!(items[1].0, 1);
    }

    #[test]
    fn a_backup_appended_paused_comes_back_running_on_failover() {
        use std::cell::Cell;
        struct MockHist {
            queued: Vec<u8>,
            paused: Cell<Option<bool>>,
        }
        impl HistoryHost for MockHist {
            fn find_history(&self, _dq: usize, _key: HistoryKey) -> Option<FoundHistory> {
                Some(FoundHistory {
                    hist_ptr: 1,
                    idx: 0,
                    len: 1,
                    kind: 1,
                    delete_status: DeleteStatus::Dupe,
                    par_status: NzbParStatus::None,
                    unpack_status: UnpackStatus::None,
                    parked_count: 0,
                    nzb: NzbRef::NONE,
                    name: b"Alt1".to_vec(),
                    dest_dir: Vec::new(),
                })
            }
            fn nzb_names(&self, _nzb: NzbRef) -> NzbNames {
                NzbNames {
                    delete_paused: true,
                    name: b"Alt1".to_vec(),
                    queued_filename: self.queued.clone(),
                    dest_dir: Vec::new(),
                }
            }
            fn parse_nzb(&self, _path: &[u8]) -> Option<usize> {
                Some(1)
            }
            fn reset_for_redownload(&self, _nzb: NzbRef, _new_nzb: usize, paused: bool) {
                self.paused.set(Some(paused));
            }
        }

        let dir = std::env::temp_dir().join(format!("hc-paused-alt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let queued = dir.join("Alt1.nzb");
        std::fs::write(&queued, b"<nzb/>").unwrap();
        let mock = MockHist {
            queued: queued.to_string_lossy().as_bytes().to_vec(),
            paused: Cell::new(None),
        };
        HistoryCoord { host: &mock, dq: 1 }.redownload(1);
        assert_eq!(mock.paused.get(), Some(false));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
