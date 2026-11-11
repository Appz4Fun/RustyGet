//! Shared host callback wrapper for `DupeArticleFallback`, `DupeCoordinator`,
//! and `QueueCoordinator` orchestration in Rust.

use std::ffi::{c_char, c_int, c_void, CStr};

pub type NzbgetRsQueueSubsysHostFn = unsafe extern "C" fn(
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
) -> usize;

// Message / Log kinds (Message::EKind)
pub(crate) const MK_INFO: i32 = 0;
pub(crate) const MK_WARNING: i32 = 1;
pub(crate) const MK_ERROR: i32 = 2;
pub(crate) const MK_DEBUG: i32 = 3;
pub(crate) const MK_DETAIL: i32 = 4;

// Options enums
pub(crate) const DAF_NONE: i32 = 0;
pub(crate) const DAF_STREAM: i32 = 2;
pub(crate) const DAF_LIVE: i32 = 3;

pub(crate) const PC_FORCE: i32 = 2;
pub(crate) const PC_MANUAL: i32 = 3;

pub(crate) const HC_PAUSE: i32 = 0;
pub(crate) const HC_DELETE: i32 = 1;
pub(crate) const HC_PARK: i32 = 2;
pub(crate) const HC_NONE: i32 = 3;
pub(crate) const HC_DUPE: i32 = 4;

pub(crate) const PS_DUPE: i32 = 3;

pub(crate) const NF_AUTO: i32 = 0;
pub(crate) const NF_ARTICLE: i32 = 1;

// EDupeMode
pub(crate) const DM_SCORE: i32 = 0;
pub(crate) const DM_FORCE: i32 = 2;

// NzbInfo enums
pub(crate) const NK_NZB: i32 = 0;
pub(crate) const NK_URL: i32 = 1;

pub(crate) const DS_NONE: i32 = 0;
pub(crate) const DS_MANUAL: i32 = 1;
pub(crate) const DS_HEALTH: i32 = 2;
pub(crate) const DS_DUPE: i32 = 3;
pub(crate) const DS_GOOD: i32 = 5;
pub(crate) const DS_COPY: i32 = 6;

pub(crate) const KS_BAD: i32 = 1;
pub(crate) const KS_GOOD: i32 = 2;
pub(crate) const KS_SUCCESS: i32 = 3;

pub(crate) const PS_FAILURE: i32 = 2;

pub(crate) const TS_RUNNING: i32 = 1;
pub(crate) const TS_SUCCESS: i32 = 3;

pub(crate) const PT_QUEUED: i32 = 0;
pub(crate) const NS_RUNNING: i32 = 1;

pub(crate) const DH_NONE: i32 = 0;
pub(crate) const DH_REDOWNLOAD_AUTO: i32 = 2;

pub(crate) const DP_NONE: i32 = 0;
pub(crate) const DP_DEFERRED: i32 = 1;
pub(crate) const DP_LIFTED: i32 = 2;

// FileInfo / ArticleInfo / CompletedFile / ArticleDownloader enums
pub(crate) const FC_UNKNOWN: i32 = 0;
pub(crate) const FC_PAR2: i32 = 1;
pub(crate) const FC_OTHER: i32 = 2;

pub(crate) const PARTIAL_NONE: i32 = 0;
pub(crate) const PARTIAL_PARTIAL: i32 = 1;
pub(crate) const PARTIAL_COMPLETED: i32 = 2;

pub(crate) const AI_UNDEFINED: i32 = 0;
pub(crate) const AI_FINISHED: i32 = 2;
pub(crate) const AI_FAILED: i32 = 3;

pub(crate) const CF_SUCCESS: i32 = 1;
pub(crate) const CF_PARTIAL: i32 = 2;
pub(crate) const CF_FAILURE: i32 = 3;

pub(crate) const AD_RUNNING: i32 = 1;
pub(crate) const AD_FINISHED: i32 = 3;
pub(crate) const AD_FAILED: i32 = 4;
pub(crate) const AD_RETRY: i32 = 5;
pub(crate) const AD_FATAL_ERROR: i32 = 10;

// HistoryInfo / DupInfo enums
pub(crate) const HK_NZB: i32 = 1;
pub(crate) const HK_URL: i32 = 2;
pub(crate) const HK_DUP: i32 = 3;

pub(crate) const DUP_SUCCESS: i32 = 1;
pub(crate) const DUP_FAILED: i32 = 2;
pub(crate) const DUP_BAD: i32 = 5;
pub(crate) const DUP_GOOD: i32 = 6;

// DownloadQueue::EAspectAction
pub(crate) const ASPECT_NZB_FOUND: i32 = 0;
pub(crate) const ASPECT_NZB_ADDED: i32 = 1;
pub(crate) const ASPECT_NZB_NAMED: i32 = 3;
pub(crate) const ASPECT_FILE_COMPLETED: i32 = 5;
pub(crate) const ASPECT_FILE_DELETED: i32 = 6;

// DownloadQueue::EEditAction (eaFileMoveOffset = 1)
pub(crate) const EA_GROUP_PAUSE: i32 = 16;
pub(crate) const EA_GROUP_RESUME: i32 = 17;
pub(crate) const EA_GROUP_DELETE: i32 = 18;
pub(crate) const EA_GROUP_PARK_DELETE: i32 = 19;
pub(crate) const EA_GROUP_DUPE_DELETE: i32 = 20;
pub(crate) const EA_GROUP_PAUSE_ALL_PARS: i32 = 22;
pub(crate) const EA_GROUP_SORT_FILES: i32 = 34;
pub(crate) const EA_POST_DELETE: i32 = 35;

// QueueScriptCoordinator::EEvent
pub(crate) const QE_NZB_MARKED: i32 = 2;

// NzbInfo field indices (op = 32)
pub(crate) const NZB_INT_ID: i32 = 0;
pub(crate) const NZB_INT_DELETE_STATUS: i32 = 1;
pub(crate) const NZB_INT_DELETE_PAUSED: i32 = 2;
pub(crate) const NZB_INT_MARK_STATUS: i32 = 3;
pub(crate) const NZB_INT_DUPE_HINT: i32 = 4;
pub(crate) const NZB_INT_DUPE_SCORE: i32 = 5;
pub(crate) const NZB_INT_PRIORITY: i32 = 6;
pub(crate) const NZB_INT_FULL_HASH: i32 = 7;
pub(crate) const NZB_INT_FILTERED_HASH: i32 = 8;
pub(crate) const NZB_INT_CHANGED: i32 = 9;
pub(crate) const NZB_INT_ALL_FIRST: i32 = 10;
pub(crate) const NZB_INT_HEALTH_PAUSED: i32 = 11;
pub(crate) const NZB_INT_DEAD_PICK_PROBED: i32 = 12;
pub(crate) const NZB_INT_DUPE_PAR_DEFER_STATE: i32 = 13;
pub(crate) const NZB_INT_DUPE_NO_PAR_NOTED: i32 = 14;
pub(crate) const NZB_INT_DIRECT_RENAME_STATUS: i32 = 15;
pub(crate) const NZB_INT_ACTIVE_DOWNLOADS: i32 = 16;
pub(crate) const NZB_INT_CUR_SUCCESS_ARTICLES: i32 = 17;
pub(crate) const NZB_INT_CUR_FAILED_ARTICLES: i32 = 18;
pub(crate) const NZB_INT_DUPE_RECOVERED_ARTICLES: i32 = 19;
pub(crate) const NZB_INT_DUPE_ATTEMPTED_ARTICLES: i32 = 20;
pub(crate) const NZB_INT_DUPE_UNSOURCED_ARTICLES: i32 = 21;
pub(crate) const NZB_INT_DUPE_FAILED_RUN: i32 = 22;
pub(crate) const NZB_INT_DUPE_FAILOVER_CHECKED: i32 = 23;

pub(crate) const NZB_I64_REMAINING_SIZE: i32 = 30;
pub(crate) const NZB_I64_PAUSED_SIZE: i32 = 31;
pub(crate) const NZB_I64_CUR_SUCCESS_SIZE: i32 = 32;
pub(crate) const NZB_I64_CUR_FAILED_SIZE: i32 = 33;
pub(crate) const NZB_I64_PAR_CUR_SUCCESS_SIZE: i32 = 34;
pub(crate) const NZB_I64_PAR_CUR_FAILED_SIZE: i32 = 35;
pub(crate) const NZB_I64_DOWNLOADED_SIZE: i32 = 36;

pub(crate) const NZB_STR_NAME: i32 = 40;
pub(crate) const NZB_STR_FILENAME: i32 = 41;
pub(crate) const NZB_STR_CATEGORY: i32 = 42;
pub(crate) const NZB_STR_DEST_DIR: i32 = 43;
pub(crate) const NZB_STR_DUPE_KEY: i32 = 44;
pub(crate) const NZB_STR_QUEUED_FILENAME: i32 = 45;

pub(crate) const NZB_ACT_BUILD_DEST_DIR: i32 = 50;
pub(crate) const NZB_ACT_UPDATE_MIN_MAX_TIME: i32 = 51;
pub(crate) const NZB_ACT_UPDATE_COMPLETED_STATS: i32 = 52;
pub(crate) const NZB_ACT_UPDATE_DELETED_STATS: i32 = 53;

// FileInfo field indices (op = 42)
pub(crate) const FILE_INT_PAUSED: i32 = 0;
pub(crate) const FILE_INT_DELETED: i32 = 1;
pub(crate) const FILE_INT_DUPE_DELETED: i32 = 2;
pub(crate) const FILE_INT_FILENAME_CONFIRMED: i32 = 3;
pub(crate) const FILE_INT_FIRST_CONTENT: i32 = 4;
pub(crate) const FILE_INT_EXTRA_PRIORITY: i32 = 5;
pub(crate) const FILE_INT_FORCE_DIRECT_WRITE: i32 = 6;
pub(crate) const FILE_INT_OUTPUT_INITIALIZED: i32 = 7;
pub(crate) const FILE_INT_PARTIAL_STATE: i32 = 8;
pub(crate) const FILE_INT_PARTIAL_CHANGED: i32 = 9;
pub(crate) const FILE_INT_DUPE_CUTOVER: i32 = 10;
pub(crate) const FILE_INT_DUPE_NO_SOURCE_LOGGED: i32 = 11;
pub(crate) const FILE_INT_DUPE_LEAD_DONOR_ID: i32 = 12;
pub(crate) const FILE_INT_DUPE_LEAD_FAILURES: i32 = 13;
pub(crate) const FILE_INT_DUPE_LEAD_SWITCHES: i32 = 14;
pub(crate) const FILE_INT_DUPE_RECOVERED_ARTICLES: i32 = 15;
pub(crate) const FILE_INT_DUPE_ATTEMPTED_ARTICLES: i32 = 16;
pub(crate) const FILE_INT_COMPLETED_ARTICLES: i32 = 17;
pub(crate) const FILE_INT_SUCCESS_ARTICLES: i32 = 18;
pub(crate) const FILE_INT_FAILED_ARTICLES: i32 = 19;
pub(crate) const FILE_INT_ACTIVE_DOWNLOADS: i32 = 20;
pub(crate) const FILE_INT_CACHED_ARTICLES: i32 = 21;

pub(crate) const FILE_I64_REMAINING_SIZE: i32 = 30;
pub(crate) const FILE_I64_SUCCESS_SIZE: i32 = 31;
pub(crate) const FILE_I64_FAILED_SIZE: i32 = 32;
pub(crate) const FILE_I64_DECODED_FILE_SIZE: i32 = 33;

pub(crate) const FILE_STR_FILENAME: i32 = 40;
pub(crate) const FILE_STR_OUTPUT_FILENAME: i32 = 41;

pub(crate) const FILE_ACT_MAKE_VALID_FILENAME: i32 = 50;
pub(crate) const FILE_ACT_CLEAR_ARTICLES: i32 = 51;
pub(crate) const FILE_ACT_RESET_ARTICLES_NONSERV: i32 = 52;
pub(crate) const FILE_ACT_SUBTRACT_AND_CLEAR_SERVER_STATS: i32 = 53;
pub(crate) const FILE_ACT_FAIL_FINISHED_ARTICLES: i32 = 54;
pub(crate) const FILE_ACT_DISCARD_TEMP_ARTICLES: i32 = 55;

// ArticleInfo field indices (op = 43)
pub(crate) const ART_INT_STATUS: i32 = 0;
pub(crate) const ART_INT_DUPE_FALLBACK_ROUND: i32 = 1;
pub(crate) const ART_INT_DUPE_DONOR_ID: i32 = 2;
pub(crate) const ART_INT_DUPE_PROACTIVE: i32 = 3;

pub(crate) const ART_STR_MESSAGE_ID: i32 = 0;
pub(crate) const ART_STR_DUPE_ORIG_MESSAGE_ID: i32 = 1;

// ArticleDownloader actions (op = 51)
pub(crate) const AD_ACT_STOP: i32 = 1;
pub(crate) const AD_ACT_LOG_DEBUG: i32 = 2;
pub(crate) const AD_ACT_ADD_SERVER_STATS: i32 = 3;
pub(crate) const AD_ACT_DIRECT_RENAME_ARTICLE: i32 = 4;
pub(crate) const AD_ACT_COMPLETE_FILE_PARTS: i32 = 5;
pub(crate) const AD_ACT_CANCEL_HANGING: i32 = 6;

#[inline]
pub(crate) fn c_bytes(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let mut v = Vec::with_capacity(end + 1);
    v.extend_from_slice(&bytes[..end]);
    v.push(0);
    v
}

#[inline]
pub(crate) unsafe fn read_cstr(ptr: *const c_char) -> Vec<u8> {
    if ptr.is_null() {
        Vec::new()
    } else {
        CStr::from_ptr(ptr).to_bytes().to_vec()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct GlobalOptsSnap {
    pub dupe_article_fallback: i32,
    pub par_check: i32,
    pub par_scan: i32,
    pub health_check: i32,
    pub dupe_check: bool,
    pub server_mode: bool,
    pub continue_partial: bool,
    pub direct_write: bool,
    pub direct_rename: bool,
    pub raw_article: bool,
    pub reorder_files: bool,
    pub file_naming: i32,
    pub propagation_delay: i32,
    pub article_timeout: i32,
    pub pause_download: bool,
    pub temp_pause_download: bool,
    pub quota_reached: bool,
    pub download_held: bool,
    pub pause_post_process: bool,
    pub pause_scan: bool,
    pub cur_time: i64,
    pub queue_dir: Vec<u8>,
    pub par_ignore_ext: Vec<u8>,
    pub ext_cleanup_disk: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NzbSnap {
    pub ptr: usize,
    pub id: i32,
    pub kind: i32,
    pub dupe_mode: i32,
    pub dupe_score: i32,
    pub dupe_hint: i32,
    pub delete_status: i32,
    pub mark_status: i32,
    pub par_status: i32,
    pub direct_rename_status: i32,
    pub direct_unpack_status: i32,
    pub dupe_par_defer_state: i32,
    pub deleting: bool,
    pub parking: bool,
    pub health_paused: bool,
    pub force_priority: bool,
    pub has_extra_priority: bool,
    pub many_dupe_files: bool,
    pub has_desired_server: bool,
    pub desired_server_id: i32,
    pub priority: i32,
    pub feed_id: i32,
    pub changed: bool,
    pub all_first: bool,
    pub dead_pick_probed: bool,
    pub dupe_no_par_noted: bool,
    pub has_post_info: bool,
    pub post_working: bool,
    pub post_stage: i32,
    pub has_live_repair_thread: bool,
    pub is_dupe_success: bool,
    pub health: i32,
    pub critical_health_opt: i32,
    pub critical_health_exact: i32,
    pub is_download_completed: bool,
    pub file_count: i32,
    pub active_downloads: i32,
    pub paused_file_count: i32,
    pub remaining_par_count: i32,
    pub total_articles: i32,
    pub success_articles: i32,
    pub failed_articles: i32,
    pub current_success_articles: i32,
    pub current_failed_articles: i32,
    pub dupe_recovered_articles: i32,
    pub dupe_attempted_articles: i32,
    pub dupe_unsourced_articles: i32,
    pub dupe_failed_run: i32,
    pub dupe_failover_checked: i32,
    pub download_sec: i32,
    pub full_content_hash: u32,
    pub filtered_content_hash: u32,
    pub size: i64,
    pub remaining_size: i64,
    pub paused_size: i64,
    pub success_size: i64,
    pub current_success_size: i64,
    pub failed_size: i64,
    pub current_failed_size: i64,
    pub par_size: i64,
    pub par_success_size: i64,
    pub par_current_success_size: i64,
    pub par_failed_size: i64,
    pub par_current_failed_size: i64,
    pub downloaded_size: i64,
    pub min_time: i64,
    pub max_time: i64,
    pub download_start_time: i64,
    pub name: Vec<u8>,
    pub filename: Vec<u8>,
    pub dest_dir: Vec<u8>,
    pub final_dir: Vec<u8>,
    pub category: Vec<u8>,
    pub dupe_key: Vec<u8>,
    pub queued_filename: Vec<u8>,
    pub text_status: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FileSnap {
    pub ptr: usize,
    pub nzb_ptr: usize,
    pub id: i32,
    pub paused: bool,
    pub deleted: bool,
    pub dupe_deleted: bool,
    pub par_file: bool,
    pub filename_confirmed: bool,
    pub first_content: i32,
    pub extra_priority: bool,
    pub force_direct_write: bool,
    pub output_initialized: bool,
    pub is_hard_linked: bool,
    pub partial_state: i32,
    pub partial_changed: bool,
    pub dupe_cutover: bool,
    pub dupe_no_source_logged: bool,
    pub dupe_lead_donor_id: i32,
    pub dupe_lead_failures: i32,
    pub dupe_lead_switches: i32,
    pub dupe_recovered_articles: i32,
    pub dupe_attempted_articles: i32,
    pub total_articles: i32,
    pub missed_articles: i32,
    pub completed_articles: i32,
    pub success_articles: i32,
    pub failed_articles: i32,
    pub active_downloads: i32,
    pub cached_articles: i32,
    pub articles_len: usize,
    pub crc: u32,
    pub size: i64,
    pub remaining_size: i64,
    pub missed_size: i64,
    pub success_size: i64,
    pub failed_size: i64,
    pub decoded_file_size: i64,
    pub time: i64,
    pub filename: Vec<u8>,
    pub subject: Vec<u8>,
    pub output_filename: Vec<u8>,
    pub hard_link_path: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ArticleSnap {
    pub ptr: usize,
    pub part_number: i32,
    pub size: i32,
    pub status: i32,
    pub segment_size: i32,
    pub crc: u32,
    pub dupe_fallback_round: i32,
    pub dupe_lead_snapshot: i32,
    pub dupe_next_lead: i32,
    pub dupe_donor_count: i32,
    pub dupe_donor_id: i32,
    pub dupe_proactive: bool,
    pub segment_offset: i64,
    pub message_id: Vec<u8>,
    pub dupe_original_message_id: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CompletedFileSnap {
    pub id: i32,
    pub status: i32,
    pub par_file: bool,
    pub filename: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DupSnap {
    pub dupe_mode: i32,
    pub dupe_score: i32,
    pub status: i32,
    pub full_content_hash: u32,
    pub filtered_content_hash: u32,
    pub name: Vec<u8>,
    pub dupe_key: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct HistEntrySnap {
    pub hi_ptr: usize,
    pub kind: i32,
    pub inner_ptr: usize,
    pub name: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RunningDownloadSnap {
    pub ad_ptr: usize,
    pub file_ptr: usize,
    pub status: i32,
    pub last_update_time: i64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ActiveDownloadSnap {
    pub file_ptr: usize,
    pub article_ptr: usize,
    pub status: i32,
    pub has_content_analyzer: bool,
    pub analyzed_par_file: bool,
    pub decoded_file_size: i64,
    pub downloaded_size: i64,
    pub has_article_filename: bool,
    pub article_filename: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(crate) struct QueueSubsysHost {
    pub ctx: *mut c_void,
    pub cb: NzbgetRsQueueSubsysHostFn,
}

unsafe extern "C" fn locked_queue_trampoline<F: FnOnce(usize)>(
    closure_ptr: *mut c_void,
    dq_ptr: usize,
) {
    let opt = &mut *(closure_ptr as *mut Option<F>);
    if let Some(f) = opt.take() {
        f(dq_ptr);
    }
}

impl QueueSubsysHost {
    #[inline]
    pub fn new(ctx: *mut c_void, cb: NzbgetRsQueueSubsysHostFn) -> Self {
        Self { ctx, cb }
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn raw(
        &self,
        op: i32,
        a1: i32,
        a2: i32,
        p1: usize,
        p2: usize,
        s1: *const c_char,
        s2: *const c_char,
        ints: *mut c_int,
        i64s: *mut i64,
        strs: *mut *const c_char,
    ) -> usize {
        unsafe { (self.cb)(self.ctx, op, a1, a2, p1, p2, s1, s2, ints, i64s, strs) }
    }

    pub fn opts(&self) -> GlobalOptsSnap {
        let mut ints = [0i32; 20];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 3];
        self.raw(
            1,
            0,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        unsafe {
            GlobalOptsSnap {
                dupe_article_fallback: ints[0],
                par_check: ints[1],
                par_scan: ints[2],
                health_check: ints[3],
                dupe_check: ints[4] != 0,
                server_mode: ints[5] != 0,
                continue_partial: ints[6] != 0,
                direct_write: ints[7] != 0,
                direct_rename: ints[8] != 0,
                raw_article: ints[9] != 0,
                reorder_files: ints[10] != 0,
                file_naming: ints[11],
                propagation_delay: ints[12],
                article_timeout: ints[13],
                pause_download: ints[14] != 0,
                temp_pause_download: ints[15] != 0,
                quota_reached: ints[16] != 0,
                download_held: ints[17] != 0,
                pause_post_process: ints[18] != 0,
                pause_scan: ints[19] != 0,
                cur_time: i64s[0],
                queue_dir: read_cstr(strs[0]),
                par_ignore_ext: read_cstr(strs[1]),
                ext_cleanup_disk: read_cstr(strs[2]),
            }
        }
    }

    pub fn log(&self, kind: i32, msg: &[u8]) {
        let c = c_bytes(msg);
        self.raw(
            2,
            kind,
            0,
            0,
            0,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_print_message(&self, nzb_ptr: usize, kind: i32, msg: &[u8]) {
        if nzb_ptr == 0 {
            return;
        }
        let c = c_bytes(msg);
        self.raw(
            2,
            kind,
            0,
            nzb_ptr,
            0,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_add_message(&self, nzb_ptr: usize, kind: i32, msg: &[u8]) {
        if nzb_ptr == 0 {
            return;
        }
        let c = c_bytes(msg);
        self.raw(
            2,
            kind,
            1,
            nzb_ptr,
            0,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn sleep_ms(&self, ms: i32) {
        self.raw(
            3,
            0,
            ms,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn set_standby_mode(&self, standby: bool) {
        self.raw(
            3,
            1,
            i32::from(standby),
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn stat_meter_load(&self) -> (bool, bool) {
        let mut ints = [1i32, 1i32];
        self.raw(
            3,
            2,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        (ints[0] != 0, ints[1] != 0)
    }

    pub fn stat_meter_save(&self) {
        self.raw(
            3,
            2,
            1,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn stat_meter_interval_check(&self) {
        self.raw(
            3,
            2,
            2,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn stat_meter_enter_leave_standby(&self, enter: bool) {
        self.raw(
            3,
            2,
            3,
            usize::from(enter),
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn stat_meter_add_zero_speed(&self) {
        self.raw(
            3,
            2,
            4,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn log_interval_check(&self) {
        self.raw(
            3,
            3,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn work_state_set_downloading(&self, downloading: bool) {
        self.raw(
            3,
            4,
            0,
            usize::from(downloading),
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn work_state_attach_self(&self) {
        self.raw(
            3,
            4,
            1,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn effective_downloads_limit(&self, limit: i32) -> i32 {
        self.raw(
            3,
            5,
            limit,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) as i32
    }

    pub fn server_pool_generation(&self) -> i32 {
        self.raw(
            4,
            0,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) as i32
    }

    pub fn server_pool_close_unused(&self) {
        self.raw(
            4,
            1,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn server_pool_servers(&self) -> (Vec<i32>, Vec<u8>, Vec<i32>) {
        let n = self.raw(
            4,
            2,
            -1,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        let mut levels = Vec::with_capacity(n);
        let mut actives = Vec::with_capacity(n);
        let mut conns = Vec::with_capacity(n);
        let mut ints = [0i32; 3];
        for i in 0..n {
            self.raw(
                4,
                2,
                i as i32,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            levels.push(ints[0]);
            actives.push(ints[1] as u8);
            conns.push(ints[2]);
        }
        (levels, actives, conns)
    }

    pub fn server_pool_get_connection(&self, has_desired: bool, desired_id: i32) -> usize {
        self.raw(
            4,
            3,
            i32::from(has_desired),
            desired_id as usize,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    // DiskState ops (op = 5)
    pub fn disk_queue_exists(&self) -> bool {
        self.raw(
            5,
            0,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn disk_load_queue(&self, dq_ptr: usize) -> bool {
        self.raw(
            5,
            1,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn disk_discard_queue(&self) {
        self.raw(
            5,
            2,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn disk_legacy_queue_read(&self) -> bool {
        self.raw(
            5,
            3,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn disk_cleanup_temp_dir(&self, dq_ptr: usize) {
        self.raw(
            5,
            4,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn disk_save_all_file_infos(&self, dq_ptr: usize) {
        self.raw(
            5,
            5,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn disk_discard_files(&self, nzb_ptr: usize) {
        self.raw(
            5,
            6,
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn disk_discard_file(
        &self,
        file_id: i32,
        del_data: bool,
        del_part: bool,
        del_compl: bool,
    ) {
        let mut ints = [i32::from(del_data), i32::from(del_part), i32::from(del_compl)];
        self.raw(
            5,
            7,
            file_id,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn disk_save_file(&self, file_ptr: usize) {
        self.raw(
            5,
            8,
            0,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn disk_save_file_state(&self, file_ptr: usize, completed: bool) {
        self.raw(
            5,
            9,
            i32::from(completed),
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn disk_load_file_state(&self, file_ptr: usize, completed: bool) -> bool {
        self.raw(
            5,
            10,
            i32::from(completed),
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn disk_resave_completed_file_state(&self, file_id: i32) {
        self.raw(
            5,
            12,
            file_id,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn disk_load_articles(&self, file_ptr: usize) {
        self.raw(
            5,
            11,
            0,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    // Subsystems (op = 6)
    pub fn history_delete_disk_files(&self, nzb_ptr: usize) {
        self.raw(
            6,
            0,
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn history_redownload(&self, dq_ptr: usize, hi_ptr: usize) {
        self.raw(
            6,
            1,
            0,
            dq_ptr,
            hi_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn history_hide(&self, dq_ptr: usize, hi_ptr: usize, rindex: i32) {
        self.raw(
            6,
            2,
            rindex,
            dq_ptr,
            hi_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn prepost_check_download_completed(&self, dq_ptr: usize, nzb_ptr: usize) {
        self.raw(
            6,
            3,
            0,
            dq_ptr,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn queue_script_enqueue(&self, nzb_ptr: usize, event: i32) {
        self.raw(
            6,
            4,
            event,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn stream_repair_start_live(&self, nzb_ptr: usize) {
        self.raw(
            6,
            5,
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn move_completed_files(&self, nzb_ptr: usize, old_dest_dir: &[u8]) -> bool {
        let c = c_bytes(old_dest_dir);
        self.raw(
            6,
            6,
            0,
            nzb_ptr,
            0,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn launch_dupe_probe(&self, probe: Box<crate::dupeprobe::NzbgetRsDupeProbe>) {
        let raw_ptr = Box::into_raw(probe) as usize;
        self.raw(
            6,
            7,
            0,
            raw_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    // DownloadQueue & CoordinatorDownloadQueue ops (op = 20..=25)
    pub fn with_locked_queue<F: FnOnce(usize)>(&self, f: F) {
        let mut opt = Some(f);
        self.raw(
            20,
            0,
            0,
            locked_queue_trampoline::<F> as *const () as usize,
            (&mut opt as *mut Option<F>) as usize,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn queue_len(&self, dq_ptr: usize) -> usize {
        self.raw(
            21,
            -1,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn queue_at(&self, dq_ptr: usize, idx: usize) -> usize {
        self.raw(
            21,
            idx as i32,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn queue_find_by_id(&self, dq_ptr: usize, nzb_id: i32) -> usize {
        self.raw(
            22,
            0,
            nzb_id,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn queue_remove(&self, dq_ptr: usize, nzb_ptr: usize) {
        self.raw(
            22,
            1,
            0,
            dq_ptr,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn queue_insert_owned(&self, dq_ptr: usize, uptr_addr: usize, url_ptr: usize, add_first: bool) {
        let mut i64s = [url_ptr as i64];
        self.raw(
            22,
            2,
            i32::from(add_first),
            dq_ptr,
            uptr_addr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn alloc_new_nzb_uptr(&self) -> (usize, usize) {
        let mut i64s = [0i64];
        let uptr_addr = self.raw(
            22,
            3,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
        (uptr_addr, i64s[0] as usize)
    }

    pub fn finish_new_nzb_uptr(&self, dq_ptr: usize, uptr_addr: usize) {
        self.raw(
            22,
            4,
            0,
            dq_ptr,
            uptr_addr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn history_len(&self, dq_ptr: usize) -> usize {
        self.raw(
            23,
            -1,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn history_find_by_id(&self, dq_ptr: usize, id: i32) -> usize {
        self.raw(
            23,
            -2,
            id,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn history_at(&self, dq_ptr: usize, idx: usize) -> HistEntrySnap {
        let mut ints = [0i32; 2];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 1];
        let hi_ptr = self.raw(
            23,
            idx as i32,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        unsafe {
            HistEntrySnap {
                hi_ptr,
                kind: ints[0],
                inner_ptr: i64s[0] as usize,
                name: read_cstr(strs[0]),
            }
        }
    }

    pub fn dq_save(&self, dq_ptr: usize) {
        self.raw(
            24,
            0,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn dq_save_changed(&self, dq_ptr: usize) {
        self.raw(
            24,
            1,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn dq_edit_entry(&self, dq_ptr: usize, id: i32, action: i32, args: Option<&[u8]>) -> bool {
        let mut ints = [action];
        let c_args = args.map(c_bytes);
        let s1 = c_args
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr() as *const c_char);
        self.raw(
            24,
            2,
            id,
            dq_ptr,
            0,
            s1,
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn dq_notify(&self, dq_ptr: usize, aspect_action: i32, nzb_ptr: usize, file_ptr: usize) {
        let mut i64s = [file_ptr as i64];
        self.raw(
            24,
            3,
            aspect_action,
            dq_ptr,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn dq_calc_remaining_size(&self, dq_ptr: usize) -> (i64, i64) {
        let mut i64s = [0i64; 2];
        self.raw(
            24,
            4,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
        (i64s[0], i64s[1])
    }

    pub fn dq_history_changed(&self, dq_ptr: usize) {
        self.raw(
            24,
            5,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    // CoordinatorDownloadQueue & QueueCoordinator C++ shell ops (op = 25)
    pub fn self_download_queue_ptr(&self) -> usize {
        self.raw(
            25,
            0,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn coord_dq_init(&self) {
        self.raw(
            25,
            1,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn coord_dq_loaded(&self) {
        self.raw(
            25,
            2,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn coord_dq_final(&self) {
        self.raw(
            25,
            3,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn coord_dq_state_changed(&self) -> bool {
        self.raw(
            25,
            4,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn self_set_has_more_jobs(&self, more: bool) {
        self.raw(
            25,
            6,
            i32::from(more),
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn self_is_stopped(&self) -> bool {
        self.raw(
            25,
            7,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn self_standby_wait(&self, wait_ms: i32) {
        self.raw(
            25,
            8,
            wait_ms,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn direct_renamer_file_downloaded(&self, dq_ptr: usize, file_ptr: usize) {
        self.raw(
            25,
            9,
            1,
            dq_ptr,
            file_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    // NzbInfo ops (op = 30..=35)
    pub fn nzb_snap(&self, nzb_ptr: usize, with_text_status: bool) -> NzbSnap {
        if nzb_ptr == 0 {
            return NzbSnap::default();
        }
        let mut ints = [0i32; 50];
        let mut i64s = [0i64; 17];
        let mut strs = [std::ptr::null(); 8];
        self.raw(
            30,
            i32::from(with_text_status),
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        unsafe {
            NzbSnap {
                ptr: nzb_ptr,
                id: ints[0],
                kind: ints[1],
                dupe_mode: ints[2],
                dupe_score: ints[3],
                dupe_hint: ints[4],
                delete_status: ints[5],
                mark_status: ints[6],
                par_status: ints[7],
                direct_rename_status: ints[8],
                direct_unpack_status: ints[9],
                dupe_par_defer_state: ints[10],
                deleting: ints[11] != 0,
                parking: ints[12] != 0,
                health_paused: ints[13] != 0,
                force_priority: ints[14] != 0,
                has_extra_priority: ints[15] != 0,
                many_dupe_files: ints[16] != 0,
                has_desired_server: ints[17] != 0,
                desired_server_id: ints[18],
                priority: ints[19],
                feed_id: ints[20],
                changed: ints[21] != 0,
                all_first: ints[22] != 0,
                dead_pick_probed: ints[23] != 0,
                dupe_no_par_noted: ints[24] != 0,
                has_post_info: ints[25] != 0,
                post_working: ints[26] != 0,
                post_stage: ints[27],
                has_live_repair_thread: ints[28] != 0,
                is_dupe_success: ints[29] != 0,
                health: ints[30],
                critical_health_opt: ints[31],
                critical_health_exact: ints[32],
                is_download_completed: ints[33] != 0,
                file_count: ints[34],
                active_downloads: ints[35],
                paused_file_count: ints[36],
                remaining_par_count: ints[37],
                total_articles: ints[38],
                success_articles: ints[39],
                failed_articles: ints[40],
                current_success_articles: ints[41],
                current_failed_articles: ints[42],
                dupe_recovered_articles: ints[43],
                dupe_attempted_articles: ints[44],
                dupe_unsourced_articles: ints[45],
                dupe_failed_run: ints[46],
                dupe_failover_checked: ints[47],
                download_sec: ints[48],
                full_content_hash: ints[49] as u32,
                filtered_content_hash: i64s[15] as u32,
                size: i64s[0],
                remaining_size: i64s[1],
                paused_size: i64s[2],
                success_size: i64s[3],
                current_success_size: i64s[4],
                failed_size: i64s[5],
                current_failed_size: i64s[6],
                par_size: i64s[7],
                par_success_size: i64s[8],
                par_current_success_size: i64s[9],
                par_failed_size: i64s[10],
                par_current_failed_size: i64s[11],
                downloaded_size: i64s[12],
                min_time: i64s[13],
                max_time: i64s[14],
                download_start_time: i64s[16],
                name: read_cstr(strs[0]),
                filename: read_cstr(strs[1]),
                dest_dir: read_cstr(strs[2]),
                final_dir: read_cstr(strs[3]),
                category: read_cstr(strs[4]),
                dupe_key: read_cstr(strs[5]),
                queued_filename: read_cstr(strs[6]),
                text_status: read_cstr(strs[7]),
            }
        }
    }

    pub fn nzb_get_param(&self, nzb_ptr: usize, name: &[u8]) -> Option<Vec<u8>> {
        if nzb_ptr == 0 {
            return None;
        }
        let c = c_bytes(name);
        let mut strs = [std::ptr::null(); 1];
        let found = self.raw(
            31,
            0,
            0,
            nzb_ptr,
            0,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        if found == 0 {
            None
        } else {
            Some(unsafe { read_cstr(strs[0]) })
        }
    }

    pub fn nzb_set_param(&self, nzb_ptr: usize, name: &[u8], val: Option<&[u8]>) {
        if nzb_ptr == 0 {
            return;
        }
        let cn = c_bytes(name);
        let cv = val.map(c_bytes);
        let s2 = cv
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr() as *const c_char);
        self.raw(
            31,
            1,
            0,
            nzb_ptr,
            0,
            cn.as_ptr() as *const c_char,
            s2,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_copy_params(&self, dest_nzb: usize, src_nzb: usize) {
        self.raw(
            31,
            2,
            0,
            dest_nzb,
            src_nzb,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_blocked_donor_op(&self, nzb_ptr: usize, add: bool, donor_id: i32) -> bool {
        self.raw(
            31,
            if add { 5 } else { 4 },
            donor_id,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn nzb_set_int(&self, nzb_ptr: usize, field: i32, val: i32) {
        self.raw(
            32,
            field,
            val,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_set_i64(&self, nzb_ptr: usize, field: i32, val: i64) {
        let mut i64s = [val];
        self.raw(
            32,
            field,
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_set_str(&self, nzb_ptr: usize, field: i32, val: &[u8]) {
        let c = c_bytes(val);
        self.raw(
            32,
            field,
            0,
            nzb_ptr,
            0,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_action(&self, nzb_ptr: usize, action: i32, p2: usize) {
        self.raw(
            32,
            action,
            0,
            nzb_ptr,
            p2,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_apply_merge_split_counters(
        &self,
        nzb_ptr: usize,
        c: &crate::queuecoord::NzbgetRsNzbMergeSplitCounters,
    ) {
        let mut ints = [
            c.file_count,
            c.active_downloads,
            c.full_content_hash as i32,
            c.filtered_content_hash as i32,
            c.paused_file_count,
            c.remaining_par_count,
            c.total_articles,
            c.success_articles,
            c.failed_articles,
            c.current_success_articles,
            c.current_failed_articles,
            c.download_sec,
        ];
        let mut i64s = [
            c.size,
            c.remaining_size,
            c.paused_size,
            c.success_size,
            c.current_success_size,
            c.failed_size,
            c.current_failed_size,
            c.par_size,
            c.par_success_size,
            c.par_current_success_size,
            c.par_failed_size,
            c.par_current_failed_size,
            c.min_time,
            c.max_time,
            c.downloaded_size,
            c.download_start_time,
        ];
        self.raw(
            33,
            0,
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_file_len(&self, nzb_ptr: usize) -> usize {
        self.raw(
            34,
            0,
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn nzb_file_at(&self, nzb_ptr: usize, idx: usize) -> usize {
        self.raw(
            34,
            1,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn nzb_remove_file(&self, nzb_ptr: usize, file_ptr: usize) {
        self.raw(
            34,
            2,
            0,
            nzb_ptr,
            file_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_remove_file_and_notify(
        &self,
        dq_ptr: usize,
        nzb_ptr: usize,
        file_ptr: usize,
        aspect_action: i32,
    ) {
        let mut i64s = [dq_ptr as i64];
        self.raw(
            34,
            9,
            aspect_action,
            nzb_ptr,
            file_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_transfer_split_file(&self, src_nzb: usize, dest_nzb: usize, file_ptr: usize) {
        let mut i64s = [dest_nzb as i64];
        self.raw(
            34,
            3,
            0,
            src_nzb,
            file_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_merge_lists_and_stats(&self, dest_nzb: usize, src_nzb: usize) {
        self.raw(
            34,
            4,
            0,
            dest_nzb,
            src_nzb,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_completed_files(&self, nzb_ptr: usize) -> Vec<CompletedFileSnap> {
        let n = self.raw(
            34,
            5,
            -1,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        let mut out = Vec::with_capacity(n);
        let mut ints = [0i32; 3];
        let mut strs = [std::ptr::null(); 1];
        for i in 0..n {
            self.raw(
                34,
                5,
                i as i32,
                nzb_ptr,
                0,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                std::ptr::null_mut(),
                strs.as_mut_ptr(),
            );
            out.push(CompletedFileSnap {
                id: ints[0],
                status: ints[1],
                par_file: ints[2] != 0,
                filename: unsafe { read_cstr(strs[0]) },
            });
        }
        out
    }

    pub fn nzb_push_completed_file(
        &self,
        nzb_ptr: usize,
        file_ptr: usize,
        filename: &[u8],
        file_status: i32,
        file_crc: u32,
    ) {
        let c = c_bytes(filename);
        let mut ints = [file_crc as i32];
        self.raw(
            34,
            6,
            file_status,
            nzb_ptr,
            file_ptr,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_push_stream_repair_job(
        &self,
        nzb_ptr: usize,
        file_id: i32,
        filename: &[u8],
        decoded_file_size: i64,
        failed_size: i64,
        missed_size: i64,
        failed_articles: i32,
        par_file: bool,
        holes: &[crate::dupefallback::NzbgetRsByteRange],
        steps_hash: u64,
    ) {
        let c = c_bytes(filename);
        let mut ints = [file_id, failed_articles, i32::from(par_file)];
        let mut i64s = [decoded_file_size, failed_size, missed_size, steps_hash as i64];
        self.raw(
            34,
            8,
            holes.len() as i32,
            nzb_ptr,
            if holes.is_empty() {
                0
            } else {
                holes.as_ptr() as usize
            },
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn raw_file_list_vec(&self, raw_list_ptr: usize) -> Vec<usize> {
        if raw_list_ptr == 0 {
            return Vec::new();
        }
        let n = self.raw(
            34,
            10,
            -1,
            raw_list_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            out.push(self.raw(
                34,
                10,
                i as i32,
                raw_list_ptr,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ));
        }
        out
    }

    pub fn push_raw_nzb_list(&self, raw_nzb_list_ptr: usize, nzb_ptr: usize) {
        if raw_nzb_list_ptr == 0 || nzb_ptr == 0 {
            return;
        }
        self.raw(
            34,
            11,
            0,
            raw_nzb_list_ptr,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn dup_snap(&self, dup_ptr: usize) -> DupSnap {
        if dup_ptr == 0 {
            return DupSnap::default();
        }
        let mut ints = [0i32; 5];
        let mut strs = [std::ptr::null(); 2];
        self.raw(
            35,
            0,
            0,
            dup_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        unsafe {
            DupSnap {
                dupe_mode: ints[0],
                dupe_score: ints[1],
                status: ints[2],
                full_content_hash: ints[3] as u32,
                filtered_content_hash: ints[4] as u32,
                name: read_cstr(strs[0]),
                dupe_key: read_cstr(strs[1]),
            }
        }
    }

    pub fn dup_set_status(&self, dup_ptr: usize, status: i32) {
        self.raw(
            35,
            1,
            status,
            dup_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn hist_item_snap(&self, hi_ptr: usize) -> HistEntrySnap {
        let mut ints = [0i32; 2];
        let mut strs = [std::ptr::null(); 1];
        let inner_ptr = self.raw(
            35,
            2,
            0,
            hi_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        HistEntrySnap {
            hi_ptr,
            kind: ints[0],
            inner_ptr,
            name: unsafe { read_cstr(strs[0]) },
        }
    }

    // FileInfo & ArticleInfo ops (op = 40..=44)
    pub fn file_snap(&self, file_ptr: usize) -> FileSnap {
        if file_ptr == 0 {
            return FileSnap::default();
        }
        let mut ints = [0i32; 28];
        let mut i64s = [0i64; 8];
        let mut strs = [std::ptr::null(); 4];
        let articles_len = self.raw(
            40,
            0,
            0,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        unsafe {
            FileSnap {
                ptr: file_ptr,
                nzb_ptr: i64s[6] as usize,
                id: ints[0],
                paused: ints[1] != 0,
                deleted: ints[2] != 0,
                dupe_deleted: ints[3] != 0,
                par_file: ints[4] != 0,
                filename_confirmed: ints[5] != 0,
                first_content: ints[6],
                extra_priority: ints[7] != 0,
                force_direct_write: ints[8] != 0,
                output_initialized: ints[9] != 0,
                is_hard_linked: ints[10] != 0,
                partial_state: ints[11],
                partial_changed: ints[12] != 0,
                dupe_cutover: ints[13] != 0,
                dupe_no_source_logged: ints[14] != 0,
                dupe_lead_donor_id: ints[15],
                dupe_lead_failures: ints[16],
                dupe_lead_switches: ints[17],
                dupe_recovered_articles: ints[18],
                dupe_attempted_articles: ints[19],
                total_articles: ints[20],
                missed_articles: ints[21],
                completed_articles: ints[22],
                success_articles: ints[23],
                failed_articles: ints[24],
                active_downloads: ints[25],
                cached_articles: ints[26],
                articles_len,
                crc: ints[27] as u32,
                size: i64s[0],
                remaining_size: i64s[1],
                missed_size: i64s[2],
                success_size: i64s[3],
                failed_size: i64s[4],
                decoded_file_size: i64s[5],
                time: i64s[7],
                filename: read_cstr(strs[0]),
                subject: read_cstr(strs[1]),
                output_filename: read_cstr(strs[2]),
                hard_link_path: read_cstr(strs[3]),
            }
        }
    }

    pub fn file_groups(&self, file_ptr: usize) -> Vec<Vec<u8>> {
        let n = self.raw(
            41,
            0,
            -1,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        let mut out = Vec::with_capacity(n);
        let mut strs = [std::ptr::null(); 1];
        for i in 0..n {
            self.raw(
                41,
                0,
                i as i32,
                file_ptr,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                strs.as_mut_ptr(),
            );
            out.push(unsafe { read_cstr(strs[0]) });
        }
        out
    }

    pub fn file_set_int(&self, file_ptr: usize, field: i32, val: i32) {
        self.raw(
            42,
            field,
            val,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn file_set_i64(&self, file_ptr: usize, field: i32, val: i64) {
        let mut i64s = [val];
        self.raw(
            42,
            field,
            0,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn file_set_str(&self, file_ptr: usize, field: i32, val: Option<&[u8]>) {
        let cv = val.map(c_bytes);
        let s1 = cv
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr() as *const c_char);
        self.raw(
            42,
            field,
            0,
            file_ptr,
            0,
            s1,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn file_action(&self, file_ptr: usize, action: i32, p2: usize) {
        self.raw(
            42,
            action,
            0,
            file_ptr,
            p2,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn article_at(&self, file_ptr: usize, idx: usize) -> ArticleSnap {
        let mut ints = [0i32; 12];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 2];
        let ptr = self.raw(
            43,
            0,
            idx as i32,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        Self::decode_article_snap(ptr, &ints, &i64s, &strs)
    }

    pub fn file_first_undefined_article(&self, file_ptr: usize) -> Option<(usize, ArticleSnap)> {
        let mut ints = [0i32; 12];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 2];
        let ptr = self.raw(
            43,
            7,
            0,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        if ptr == 0 {
            None
        } else {
            Some((
                ints[11] as usize,
                Self::decode_article_snap(ptr, &ints, &i64s, &strs),
            ))
        }
    }

    pub fn file_find_article(
        &self,
        file_ptr: usize,
        article_ptr: usize,
    ) -> Option<(usize, ArticleSnap)> {
        let mut ints = [0i32; 12];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 2];
        let ptr = self.raw(
            43,
            8,
            0,
            file_ptr,
            article_ptr,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        if ptr == 0 {
            None
        } else {
            Some((
                ints[11] as usize,
                Self::decode_article_snap(ptr, &ints, &i64s, &strs),
            ))
        }
    }

    fn decode_article_snap(
        ptr: usize,
        ints: &[i32; 12],
        i64s: &[i64; 1],
        strs: &[*const c_char; 2],
    ) -> ArticleSnap {
        unsafe {
            ArticleSnap {
                ptr,
                part_number: ints[0],
                size: ints[1],
                status: ints[2],
                segment_size: ints[3],
                crc: ints[4] as u32,
                dupe_fallback_round: ints[5],
                dupe_lead_snapshot: ints[6],
                dupe_next_lead: ints[7],
                dupe_donor_count: ints[8],
                dupe_donor_id: ints[9],
                dupe_proactive: ints[10] != 0,
                segment_offset: i64s[0],
                message_id: read_cstr(strs[0]),
                dupe_original_message_id: read_cstr(strs[1]),
            }
        }
    }

    pub fn article_pinned_sources(&self, article_ptr: usize) -> (Vec<Vec<u8>>, Vec<i32>) {
        let n = self.raw(
            43,
            3,
            -1,
            article_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        let mut sources = Vec::with_capacity(n);
        let mut donors = Vec::with_capacity(n);
        let mut ints = [0i32; 2];
        let mut strs = [std::ptr::null(); 1];
        for i in 0..n {
            self.raw(
                43,
                3,
                i as i32,
                article_ptr,
                0,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                std::ptr::null_mut(),
                strs.as_mut_ptr(),
            );
            sources.push(unsafe { read_cstr(strs[0]) });
            if ints[1] != 0 {
                donors.push(ints[0]);
            }
        }
        (sources, donors)
    }

    pub fn article_store_pin(
        &self,
        article_ptr: usize,
        lead_snapshot: i32,
        next_lead: i32,
        donor_count: i32,
        sources: &[Vec<u8>],
        contributors: &[i32],
    ) {
        let c_srcs: Vec<Vec<u8>> = sources.iter().map(|s| c_bytes(s)).collect();
        let mut c_ptrs: Vec<*const c_char> =
            c_srcs.iter().map(|c| c.as_ptr() as *const c_char).collect();
        let mut ints = Vec::with_capacity(4 + contributors.len());
        ints.push(lead_snapshot);
        ints.push(next_lead);
        ints.push(donor_count);
        ints.push(contributors.len() as i32);
        ints.extend_from_slice(contributors);
        self.raw(
            43,
            4,
            c_ptrs.len() as i32,
            article_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            if c_ptrs.is_empty() {
                std::ptr::null_mut()
            } else {
                c_ptrs.as_mut_ptr()
            },
        );
    }

    pub fn article_set_int(&self, article_ptr: usize, field: i32, val: i32) {
        self.raw(
            43,
            2,
            field,
            article_ptr,
            val as usize,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn article_set_i64_pair(&self, article_ptr: usize, exp_offset: i64, exp_end: i64) {
        let mut i64s = [exp_offset, exp_end];
        self.raw(
            43,
            5,
            0,
            article_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn article_set_str(&self, article_ptr: usize, field: i32, val: Option<&[u8]>) {
        let cv = val.map(c_bytes);
        let s1 = cv
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr() as *const c_char);
        self.raw(
            43,
            6,
            field,
            article_ptr,
            0,
            s1,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    // ArticleCache & flush lock ops (op = 44)
    pub fn article_cache_file_busy(&self, file_ptr: usize) -> bool {
        self.raw(
            44,
            0,
            0,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn article_cache_snapshot_segment(
        &self,
        file_ptr: usize,
        article_ptr: usize,
        len: usize,
    ) -> Option<Vec<u8>> {
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len];
        let mut i64s = [buf.as_mut_ptr() as usize as i64];
        let copied = self.raw(
            44,
            1,
            len as i32,
            file_ptr,
            article_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
        if copied != 0 {
            Some(buf)
        } else {
            None
        }
    }

    pub fn article_cache_discard_segment(&self, file_ptr: usize, article_ptr: usize) {
        self.raw(
            44,
            2,
            0,
            file_ptr,
            article_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn file_try_acquire_flush_lock(&self, file_ptr: usize) -> bool {
        self.raw(
            44,
            3,
            1,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn file_release_flush_lock(&self, file_ptr: usize) {
        self.raw(
            44,
            3,
            0,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    // ActiveDownloads & ArticleDownloader ops (op = 50..=54)
    pub fn active_downloads_len(&self) -> usize {
        self.raw(
            50,
            -1,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn active_download_at(&self, idx: usize) -> RunningDownloadSnap {
        let mut ints = [0i32; 1];
        let mut i64s = [0i64; 2];
        let ad_ptr = self.raw(
            50,
            idx as i32,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
        RunningDownloadSnap {
            ad_ptr,
            file_ptr: i64s[0] as usize,
            status: ints[0],
            last_update_time: i64s[1],
        }
    }

    pub fn ad_snap(&self, ad_ptr: usize) -> ActiveDownloadSnap {
        let mut ints = [0i32; 4];
        let mut i64s = [0i64; 4];
        let mut strs = [std::ptr::null(); 1];
        self.raw(
            51,
            0,
            0,
            ad_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        unsafe {
            ActiveDownloadSnap {
                file_ptr: i64s[0] as usize,
                article_ptr: i64s[1] as usize,
                status: ints[0],
                has_content_analyzer: ints[1] != 0,
                analyzed_par_file: ints[2] != 0,
                has_article_filename: ints[3] != 0,
                decoded_file_size: i64s[2],
                downloaded_size: i64s[3],
                article_filename: read_cstr(strs[0]),
            }
        }
    }

    pub fn ad_action(&self, ad_ptr: usize, action: i32, dq_ptr: usize) -> bool {
        self.raw(
            51,
            action,
            0,
            ad_ptr,
            dq_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn start_article_download(
        &self,
        file_ptr: usize,
        article_ptr: usize,
        conn_ptr: usize,
        with_content_analyzer: bool,
        info_name: &[u8],
    ) {
        let c = c_bytes(info_name);
        let mut i64s = [conn_ptr as i64];
        self.raw(
            52,
            i32::from(with_content_analyzer),
            0,
            file_ptr,
            article_ptr,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn remove_active_download(&self, ad_ptr: usize) {
        self.raw(
            53,
            0,
            0,
            ad_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn delete_all_active_downloads(&self) {
        self.raw(
            53,
            1,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
}
