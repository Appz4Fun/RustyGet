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

pub(crate) const PC_AUTO: i32 = 0;
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
pub(crate) const DS_SCAN: i32 = 7;

pub(crate) const KS_NONE: i32 = 0;
pub(crate) const KS_BAD: i32 = 1;
pub(crate) const KS_GOOD: i32 = 2;
pub(crate) const KS_SUCCESS: i32 = 3;

pub(crate) const PS_FAILURE: i32 = 2;
pub(crate) const PAR_STATUS_NONE: i32 = 0;
pub(crate) const PAR_STATUS_SKIPPED: i32 = 1;
pub(crate) const PAR_STATUS_FAILURE: i32 = 2;
pub(crate) const PAR_STATUS_REPAIR_POSSIBLE: i32 = 4;
pub(crate) const PAR_STATUS_MANUAL: i32 = 5;

pub(crate) const RS_NONE: i32 = 0;
pub(crate) const US_NONE: i32 = 0;
pub(crate) const US_SKIPPED: i32 = 1;
pub(crate) const US_SUCCESS: i32 = 3;

pub(crate) const LS_NONE: i32 = 0;
pub(crate) const LS_FAILED: i32 = 3;
pub(crate) const MS_NONE: i32 = 0;
pub(crate) const CS_NONE: i32 = 0;

pub(crate) const TS_NONE: i32 = 0;
pub(crate) const TS_RUNNING: i32 = 1;
pub(crate) const TS_FAILURE: i32 = 2;
pub(crate) const TS_SUCCESS: i32 = 3;

pub(crate) const NS_NONE: i32 = 0;
pub(crate) const NS_RUNNING: i32 = 1;

pub(crate) const PT_QUEUED: i32 = 0;
pub(crate) const PT_LOADING_PARS: i32 = 1;
pub(crate) const PT_VERIFYING_REPAIRED: i32 = 4;
pub(crate) const PT_PAR_RENAMING: i32 = 5;
pub(crate) const PT_RAR_RENAMING: i32 = 6;
pub(crate) const PT_UNPACKING: i32 = 7;
pub(crate) const PT_CLEANING_UP: i32 = 8;
pub(crate) const PT_MOVING: i32 = 9;
pub(crate) const PT_POST_UNPACK_RENAMING: i32 = 10;
pub(crate) const PT_EXECUTING_SCRIPT: i32 = 11;
pub(crate) const PT_FINISHED: i32 = 12;
pub(crate) const PT_POST_DOWNLOAD_RENAMING: i32 = 13;
pub(crate) const PT_STREAM_REPAIRING: i32 = 14;

pub(crate) const DH_NONE: i32 = 0;
pub(crate) const DH_REDOWNLOAD_MANUAL: i32 = 1;
pub(crate) const DH_REDOWNLOAD_AUTO: i32 = 2;

pub(crate) const NZB_FORCE_PRIORITY: i32 = 900;

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

pub(crate) const CF_NONE: i32 = 0;
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

pub(crate) const DUP_STR_NAME: i32 = 3;
pub(crate) const DUP_STR_DUPE_KEY: i32 = 4;
pub(crate) const DUP_INT_DUPE_SCORE: i32 = 5;
pub(crate) const DUP_INT_DUPE_MODE: i32 = 6;

// DownloadQueue::EAspectAction
pub(crate) const ASPECT_NZB_FOUND: i32 = 0;
pub(crate) const ASPECT_NZB_ADDED: i32 = 1;
pub(crate) const ASPECT_NZB_DELETED: i32 = 2;
pub(crate) const ASPECT_NZB_NAMED: i32 = 3;
pub(crate) const ASPECT_NZB_RETURNED: i32 = 4;
pub(crate) const ASPECT_FILE_COMPLETED: i32 = 5;
pub(crate) const ASPECT_FILE_DELETED: i32 = 6;
pub(crate) const ASPECT_URL_FOUND: i32 = 7;
pub(crate) const ASPECT_URL_ADDED: i32 = 8;
pub(crate) const ASPECT_URL_DELETED: i32 = 9;
pub(crate) const ASPECT_URL_COMPLETED: i32 = 10;
pub(crate) const ASPECT_URL_FAILED: i32 = 11;
pub(crate) const ASPECT_URL_RETURNED: i32 = 12;

// DownloadQueue::EEditAction (eaFileMoveOffset = 1)
pub(crate) const EA_FILE_MOVE_OFFSET: i32 = 1;
pub(crate) const EA_FILE_MOVE_TOP: i32 = 2;
pub(crate) const EA_FILE_MOVE_BOTTOM: i32 = 3;
pub(crate) const EA_FILE_PAUSE: i32 = 4;
pub(crate) const EA_FILE_RESUME: i32 = 5;
pub(crate) const EA_FILE_DELETE: i32 = 6;
pub(crate) const EA_FILE_PAUSE_ALL_PARS: i32 = 7;
pub(crate) const EA_FILE_PAUSE_EXTRA_PARS: i32 = 8;
pub(crate) const EA_FILE_REORDER: i32 = 9;
pub(crate) const EA_FILE_SPLIT: i32 = 10;
pub(crate) const EA_GROUP_MOVE_OFFSET: i32 = 11;
pub(crate) const EA_GROUP_MOVE_TOP: i32 = 12;
pub(crate) const EA_GROUP_MOVE_BOTTOM: i32 = 13;
pub(crate) const EA_GROUP_MOVE_BEFORE: i32 = 14;
pub(crate) const EA_GROUP_MOVE_AFTER: i32 = 15;
pub(crate) const EA_GROUP_PAUSE: i32 = 16;
pub(crate) const EA_GROUP_RESUME: i32 = 17;
pub(crate) const EA_GROUP_DELETE: i32 = 18;
pub(crate) const EA_GROUP_PARK_DELETE: i32 = 19;
pub(crate) const EA_GROUP_DUPE_DELETE: i32 = 20;
pub(crate) const EA_GROUP_FINAL_DELETE: i32 = 21;
pub(crate) const EA_GROUP_PAUSE_ALL_PARS: i32 = 22;
pub(crate) const EA_GROUP_PAUSE_EXTRA_PARS: i32 = 23;
pub(crate) const EA_GROUP_SET_PRIORITY: i32 = 24;
pub(crate) const EA_GROUP_SET_CATEGORY: i32 = 25;
pub(crate) const EA_GROUP_APPLY_CATEGORY: i32 = 26;
pub(crate) const EA_GROUP_MERGE: i32 = 27;
pub(crate) const EA_GROUP_SET_PARAMETER: i32 = 28;
pub(crate) const EA_GROUP_SET_NAME: i32 = 29;
pub(crate) const EA_GROUP_SET_DUPE_KEY: i32 = 30;
pub(crate) const EA_GROUP_SET_DUPE_SCORE: i32 = 31;
pub(crate) const EA_GROUP_SET_DUPE_MODE: i32 = 32;
pub(crate) const EA_GROUP_SORT: i32 = 33;
pub(crate) const EA_GROUP_SORT_FILES: i32 = 34;
pub(crate) const EA_POST_DELETE: i32 = 35;
pub(crate) const EA_HISTORY_DELETE: i32 = 36;
pub(crate) const EA_HISTORY_FINAL_DELETE: i32 = 37;
pub(crate) const EA_HISTORY_RETRY_FAILED: i32 = 41;
pub(crate) const EA_HISTORY_SET_PARAMETER: i32 = 42;
pub(crate) const EA_HISTORY_SET_DUPE_SCORE: i32 = 44;
pub(crate) const EA_HISTORY_SET_NAME: i32 = 51;

// DownloadQueue::EMatchMode
pub(crate) const MM_NAME: i32 = 2;
pub(crate) const MM_REGEX: i32 = 3;

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
pub(crate) const NZB_INT_REMAINING_PAR_COUNT: i32 = 24;
pub(crate) const NZB_INT_WAITING_PAR: i32 = 25;
pub(crate) const NZB_INT_LOADING_PAR: i32 = 26;
pub(crate) const NZB_INT_DELETING: i32 = 27;
pub(crate) const NZB_INT_PARKING: i32 = 28;
pub(crate) const NZB_INT_AVOID_HISTORY: i32 = 29;

pub(crate) const NZB_I64_REMAINING_SIZE: i32 = 30;
pub(crate) const NZB_I64_PAUSED_SIZE: i32 = 31;
pub(crate) const NZB_I64_CUR_SUCCESS_SIZE: i32 = 32;
pub(crate) const NZB_I64_CUR_FAILED_SIZE: i32 = 33;
pub(crate) const NZB_I64_PAR_CUR_SUCCESS_SIZE: i32 = 34;
pub(crate) const NZB_I64_PAR_CUR_FAILED_SIZE: i32 = 35;
pub(crate) const NZB_I64_DOWNLOADED_SIZE: i32 = 36;
pub(crate) const NZB_I64_PAR_SIZE: i32 = 37;
pub(crate) const NZB_I64_PAR_FAILED_SIZE: i32 = 38;
pub(crate) const NZB_I64_DUPE_RECOVERED_BYTES: i32 = 39;

pub(crate) const NZB_STR_NAME: i32 = 40;
pub(crate) const NZB_STR_FILENAME: i32 = 41;
pub(crate) const NZB_STR_CATEGORY: i32 = 42;
pub(crate) const NZB_STR_DEST_DIR: i32 = 43;
pub(crate) const NZB_STR_DUPE_KEY: i32 = 44;
pub(crate) const NZB_STR_QUEUED_FILENAME: i32 = 45;
pub(crate) const NZB_STR_HARD_LINK_PATH: i32 = 46;
pub(crate) const NZB_I64_FAILED_SIZE: i32 = 47;
pub(crate) const NZB_I64_SUCCESS_SIZE: i32 = 48;
pub(crate) const NZB_I64_PAR_SUCCESS_SIZE: i32 = 49;

pub(crate) const NZB_ACT_BUILD_DEST_DIR: i32 = 50;
pub(crate) const NZB_ACT_UPDATE_MIN_MAX_TIME: i32 = 51;
pub(crate) const NZB_ACT_UPDATE_COMPLETED_STATS: i32 = 52;
pub(crate) const NZB_ACT_UPDATE_DELETED_STATS: i32 = 53;
pub(crate) const NZB_ACT_ENTER_POST_PROCESS: i32 = 54;
pub(crate) const NZB_ACT_LEAVE_POST_PROCESS: i32 = 55;
pub(crate) const NZB_ACT_CLEAR_SCRIPT_STATUSES: i32 = 56;
pub(crate) const NZB_ACT_DISCONNECT_THREADS: i32 = 57;
pub(crate) const NZB_ACT_CLEAR_LIVE_REPAIR_THREAD: i32 = 58;
pub(crate) const NZB_STR_FINAL_DIR: i32 = 59;

pub(crate) const NZB_INT_CLEANUP_DISK: i32 = 60;
pub(crate) const NZB_INT_DUPE_MODE: i32 = 61;
pub(crate) const NZB_INT_PAR_STATUS: i32 = 62;
pub(crate) const NZB_INT_UNPACK_STATUS: i32 = 63;
pub(crate) const NZB_INT_DIRECT_UNPACK_STATUS: i32 = 64;
pub(crate) const NZB_INT_POST_TOTAL_SEC: i32 = 65;
pub(crate) const NZB_INT_URL_STATUS: i32 = 66;
pub(crate) const NZB_INT_PAR_SEC: i32 = 67;
pub(crate) const NZB_INT_PAR_FULL: i32 = 68;
pub(crate) const NZB_INT_REPAIR_SEC: i32 = 69;
pub(crate) const NZB_INT_EXTRA_PAR_BLOCKS: i32 = 70;
pub(crate) const NZB_INT_DUPE_RECOVERED_HOLES: i32 = 71;
pub(crate) const NZB_INT_PAR_RENAME_STATUS: i32 = 72;
pub(crate) const NZB_INT_UNPACK_CLEANED_UP_DISK: i32 = 73;
pub(crate) const NZB_INT_UNPACK_SEC: i32 = 74;
pub(crate) const NZB_INT_MOVE_STATUS: i32 = 75;
pub(crate) const NZB_INT_CLEANUP_STATUS: i32 = 76;
pub(crate) const NZB_INT_RAR_RENAME_STATUS: i32 = 77;
pub(crate) const NZB_INT_POST_DOWNLOAD_RENAMING_STATUS: i32 = 78;
pub(crate) const NZB_INT_POST_UNPACK_RENAMING_STATUS: i32 = 79;
pub(crate) const NZB_ACT_CLEAR_UNPACK_THREAD: i32 = 80;
pub(crate) const NZB_INT_KIND: i32 = 81;
pub(crate) const NZB_INT_FEED_ID: i32 = 82;
pub(crate) const NZB_INT_ADD_URL_PAUSED: i32 = 83;
pub(crate) const NZB_INT_AUTO_CATEGORY: i32 = 84;
pub(crate) const NZB_INT_DESIRED_SERVER_ID: i32 = 85;
pub(crate) const NZB_INT_SKIP_SCRIPT_PROCESSING: i32 = 86;
pub(crate) const NZB_INT_SKIP_DISK_WRITE: i32 = 87;
pub(crate) const NZB_INT_EXTRA_PRIORITY: i32 = 88;
pub(crate) const NZB_INT_MANY_DUPE_FILES: i32 = 89;
pub(crate) const NZB_INT_PARKED_FILE_COUNT: i32 = 90;
pub(crate) const NZB_INT_REPROCESS: i32 = 91;
pub(crate) const NZB_INT_PAR_REPAIRED: i32 = 92;
pub(crate) const NZB_INT_UNPACK_TRIED: i32 = 93;
pub(crate) const NZB_INT_LAST_UNPACK_STATUS: i32 = 94;
pub(crate) const NZB_ACT_CLEAR_EXTRACTED_ARCHIVES: i32 = 95;
pub(crate) const NZB_STR_URL: i32 = 96;
pub(crate) const NZB_ACT_CLEAR_FILE_LIST: i32 = 97;
pub(crate) const NZB_ACT_UPDATE_CURRENT_STATS: i32 = 98;
pub(crate) const NZB_INT_DOWNLOAD_SEC: i32 = 99;
pub(crate) const NZB_INT_FAILED_ARTICLES: i32 = 100;
pub(crate) const NZB_INT_SUCCESS_ARTICLES: i32 = 101;
pub(crate) const NZB_I64_SIZE: i32 = 102;
pub(crate) const NZB_I64_MIN_TIME: i32 = 103;
pub(crate) const NZB_I64_MAX_TIME: i32 = 104;
pub(crate) const NZB_INT_FILE_COUNT: i32 = 106;
pub(crate) const NZB_INT_MESSAGE_COUNT: i32 = 107;
pub(crate) const NZB_INT_TOTAL_ARTICLES: i32 = 108;
pub(crate) const NZB_INT_POST_STAGE: i32 = 109;
pub(crate) const NZB_INT_POST_FORCE_FLAGS: i32 = 110;
pub(crate) const NZB_ACT_CLEAR_PARAMS: i32 = 111;
pub(crate) const NZB_ACT_CLEAR_REPAIR_JOBS: i32 = 112;
pub(crate) const NZB_ACT_CLEAR_COMPLETED_FILES: i32 = 113;

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
pub(crate) const FILE_INT_PAR_FILE: i32 = 22;

pub(crate) const FILE_I64_REMAINING_SIZE: i32 = 30;
pub(crate) const FILE_I64_SUCCESS_SIZE: i32 = 31;
pub(crate) const FILE_I64_FAILED_SIZE: i32 = 32;
pub(crate) const FILE_I64_DECODED_FILE_SIZE: i32 = 33;

pub(crate) const FILE_STR_FILENAME: i32 = 40;
pub(crate) const FILE_STR_OUTPUT_FILENAME: i32 = 41;
pub(crate) const FILE_STR_ORIGNAME: i32 = 42;
pub(crate) const FILE_STR_HASH16K: i32 = 43;
pub(crate) const FILE_STR_PAR_SET_ID: i32 = 44;
pub(crate) const FILE_STR_HARD_LINK_PATH: i32 = 45;

pub(crate) const FILE_ACT_MAKE_VALID_FILENAME: i32 = 50;
pub(crate) const FILE_ACT_CLEAR_ARTICLES: i32 = 51;
pub(crate) const FILE_ACT_RESET_ARTICLES_NONSERV: i32 = 52;
pub(crate) const FILE_ACT_SUBTRACT_AND_CLEAR_SERVER_STATS: i32 = 53;
pub(crate) const FILE_ACT_FAIL_FINISHED_ARTICLES: i32 = 54;
pub(crate) const FILE_ACT_DISCARD_TEMP_ARTICLES: i32 = 55;
pub(crate) const FILE_STR_SUBJECT: i32 = 61;
pub(crate) const FILE_I64_TIME: i32 = 62;
pub(crate) const FILE_I64_SIZE: i32 = 63;
pub(crate) const FILE_I64_MISSED_SIZE: i32 = 64;
pub(crate) const FILE_INT_TOTAL_ARTICLES: i32 = 65;
pub(crate) const FILE_INT_MISSED_ARTICLES: i32 = 66;

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
pub(crate) const AD_ACT_COMPLETE_FILE_PARTS: i32 = 5;
pub(crate) const AD_ACT_CANCEL_HANGING: i32 = 6;

// PostInfo field indices (op = 60, a1 = 1)
pub(crate) const POST_INT_STAGE: i32 = 0;
pub(crate) const POST_INT_WORKING: i32 = 1;
pub(crate) const POST_INT_DELETED: i32 = 2;
pub(crate) const POST_INT_REQUEST_PAR_CHECK: i32 = 3;
pub(crate) const POST_INT_FORCE_PAR_FULL: i32 = 4;
pub(crate) const POST_INT_PAR_CANNOT_COVER: i32 = 5;
pub(crate) const POST_INT_NEED_PAR_CHECK: i32 = 6;
pub(crate) const POST_INT_NEED_UNPACK: i32 = 7;
pub(crate) const POST_INT_FORCE_REPAIR: i32 = 8;
pub(crate) const POST_INT_PAR_REPAIRED: i32 = 9;
pub(crate) const POST_INT_UNPACK_TRIED: i32 = 10;
pub(crate) const POST_INT_PASS_LIST_TRIED: i32 = 11;
pub(crate) const POST_INT_LAST_UNPACK_STATUS: i32 = 12;
pub(crate) const POST_INT_FILE_PROGRESS: i32 = 13;
pub(crate) const POST_INT_STAGE_PROGRESS: i32 = 14;
pub(crate) const POST_INT_STREAM_REPAIR_DONE: i32 = 15;
pub(crate) const POST_INT_STREAM_REPAIR_RECOVERED: i32 = 16;

// PostController kinds (op = 62, a1 = 12)
pub(crate) const CTRL_RENAME_PAR: i32 = 0;
pub(crate) const CTRL_RENAME_RAR: i32 = 1;
pub(crate) const CTRL_STREAM_REPAIR: i32 = 2;
pub(crate) const CTRL_REPAIR: i32 = 3;
pub(crate) const CTRL_UNPACK: i32 = 4;
pub(crate) const CTRL_CLEANUP: i32 = 5;
pub(crate) const CTRL_MOVE: i32 = 6;
pub(crate) const CTRL_POST_RENAME_DL: i32 = 7;
pub(crate) const CTRL_POST_RENAME_UNPACK: i32 = 8;
pub(crate) const CTRL_POST_SCRIPT: i32 = 9;

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
    pub temp_pause_postprocess: bool,
    pub keep_history: i32,
    pub post_strategy: i32,
    pub skip_write: bool,
    pub par_rename: bool,
    pub rar_rename: bool,
    pub direct_unpack: bool,
    pub par_pause_queue: bool,
    pub unpack_pause_queue: bool,
    pub script_pause_queue: bool,
    pub post_download_rename: bool,
    pub rename_after_unpack: bool,
    pub hard_linking: bool,
    pub unpack: bool,
    pub parcheck_enabled: bool,
    pub cur_time: i64,
    pub queue_dir: Vec<u8>,
    pub par_ignore_ext: Vec<u8>,
    pub ext_cleanup_disk: Vec<u8>,
    pub inter_dir: Vec<u8>,
    pub hard_linking_ignore_ext: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NzbSnap {
    pub ptr: usize,
    pub post_info_ptr: usize,
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
    pub live_repair_thread_ptr: usize,
    pub has_unpack_thread: bool,
    pub has_stream_repair_jobs: bool,
    pub url_status: i32,
    pub par_sec: i32,
    pub repair_sec: i32,
    pub unpack_sec: i32,
    pub reprocess: bool,
    pub extra_par_blocks: i32,
    pub dupe_recovered_holes: i32,
    pub script_total_status: i32,
    pub parked_file_count: i32,
    pub delete_paused: bool,
    pub auto_category: bool,
    pub add_url_paused: bool,
    pub is_dupe_success: bool,
    pub health: i32,
    pub critical_health_opt: i32,
    pub critical_health_exact: i32,
    pub is_download_completed: bool,
    pub is_download_completed_strict: bool,
    pub skip_disk_write: bool,
    pub waiting_par: bool,
    pub loading_par: bool,
    pub unpack_cleaned_up_disk: bool,
    pub skip_script_processing: bool,
    pub avoid_history: bool,
    pub cleanup_disk: bool,
    pub unpack_tried: bool,
    pub par_full: bool,
    pub force_par_full: bool,
    pub force_repair: bool,
    pub par_repaired: bool,
    pub last_unpack_status: i32,
    pub message_count: i32,
    pub cached_message_count: i32,
    pub post_total_sec: i32,
    pub par_rename_status: i32,
    pub rar_rename_status: i32,
    pub unpack_status: i32,
    pub cleanup_status: i32,
    pub move_status: i32,
    pub post_download_renaming_status: i32,
    pub post_unpack_renaming_status: i32,
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
    pub dupe_recovered_bytes: i64,
    pub name: Vec<u8>,
    pub filename: Vec<u8>,
    pub dest_dir: Vec<u8>,
    pub final_dir: Vec<u8>,
    pub category: Vec<u8>,
    pub dupe_key: Vec<u8>,
    pub queued_filename: Vec<u8>,
    pub text_status: Vec<u8>,
    pub url: Vec<u8>,
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
    pub origname: Vec<u8>,
    pub hash16k: Vec<u8>,
    pub par_set_id: Vec<u8>,
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
    pub crc: u32,
    pub filename: Vec<u8>,
    pub origname: Vec<u8>,
    pub hash16k: Vec<u8>,
    pub par_set_id: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StreamRepairJobSnap {
    pub file_id: i32,
    pub decoded_file_size: i64,
    pub holes_total_size: i64,
    pub failed_plus_missed_size: i64,
    pub filename: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StreamRepairJobDetailSnap {
    pub file_id: i32,
    pub failed_articles: i32,
    pub par_file: bool,
    pub live_attempted: bool,
    pub decoded_file_size: i64,
    pub failed_size: i64,
    pub missed_size: i64,
    pub steps_hash: u64,
    pub filename: Vec<u8>,
    pub holes: Vec<crate::dupefallback::NzbgetRsByteRange>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PostSnap {
    pub ptr: usize,
    pub nzb_ptr: usize,
    pub stage: i32,
    pub working: bool,
    pub deleted: bool,
    pub request_par_check: bool,
    pub force_par_full: bool,
    pub force_repair: bool,
    pub stream_repair_done: bool,
    pub stream_repair_recovered: bool,
    pub par_cannot_cover: bool,
    pub need_par_check: bool,
    pub need_unpack: bool,
    pub has_post_thread: bool,
    pub post_thread_running: bool,
    pub unpack_tried: bool,
    pub par_repaired: bool,
    pub pass_list_tried: bool,
    pub last_unpack_status: i32,
    pub extracted_archives_len: usize,
    pub parred_files_len: usize,
    pub stage_progress: i32,
    pub file_progress: i32,
    pub start_time: i64,
    pub stage_time: i64,
    pub progress_label: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DupSnap {
    pub dupe_mode: i32,
    pub dupe_score: i32,
    pub status: i32,
    pub size: i64,
    pub full_content_hash: u32,
    pub filtered_content_hash: u32,
    pub name: Vec<u8>,
    pub dupe_key: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct HistEntrySnap {
    pub hi_ptr: usize,
    pub id: i32,
    pub kind: i32,
    pub inner_ptr: usize,
    pub time: i64,
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
    pub use_active: bool,
}

struct LockedQueueCall<F, R> {
    f: Option<F>,
    out: Option<R>,
}

unsafe extern "C" fn locked_queue_trampoline<R, F: FnOnce(usize) -> R>(
    closure_ptr: *mut c_void,
    dq_ptr: usize,
) {
    let call = &mut *(closure_ptr as *mut LockedQueueCall<F, R>);
    if let Some(f) = call.f.take() {
        call.out = Some(f(dq_ptr));
    }
}

impl QueueSubsysHost {
    #[inline]
    pub fn new(ctx: *mut c_void, cb: NzbgetRsQueueSubsysHostFn) -> Self {
        Self {
            ctx,
            cb,
            use_active: true,
        }
    }

    #[cfg(test)]
    #[inline]
    pub fn new_mock(ctx: *mut c_void, cb: NzbgetRsQueueSubsysHostFn) -> Self {
        Self {
            ctx,
            cb,
            use_active: false,
        }
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
        if self.use_active {
            let opts = crate::options::active_options();
            let ws = crate::ffi::active_workstate();
            let cur_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let parcheck_enabled = crate::ffi::build_parcheck_enabled();
            let (
                pause_download,
                temp_pause_download,
                quota_reached,
                download_held,
                pause_post_process,
                pause_scan,
                temp_pause_postprocess,
            ) = ws
                .as_ref()
                .map_or((false, false, false, false, false, false, false), |w| {
                    (
                        w.pause_download(),
                        w.temp_pause_download(),
                        w.quota_reached(),
                        w.download_held(),
                        w.pause_post_process(),
                        w.pause_scan(),
                        w.temp_pause_postprocess(),
                    )
                });
            if let Some(ref o) = opts {
                let sc = o.scalars();
                return GlobalOptsSnap {
                    dupe_article_fallback: sc.dupe_article_fallback,
                    par_check: sc.par_check,
                    par_scan: sc.par_scan,
                    health_check: sc.health_check,
                    dupe_check: sc.dupe_check != 0,
                    server_mode: o.server_mode(),
                    continue_partial: sc.continue_partial != 0,
                    direct_write: sc.direct_write != 0,
                    direct_rename: sc.direct_rename != 0,
                    raw_article: sc.raw_article != 0,
                    reorder_files: sc.reorder_files != 0,
                    file_naming: sc.file_naming,
                    propagation_delay: sc.propagation_delay,
                    article_timeout: sc.article_timeout,
                    pause_download,
                    temp_pause_download,
                    quota_reached,
                    download_held,
                    pause_post_process,
                    pause_scan,
                    temp_pause_postprocess,
                    keep_history: sc.keep_history,
                    post_strategy: sc.post_strategy,
                    skip_write: sc.skip_write != 0,
                    par_rename: sc.par_rename != 0,
                    rar_rename: sc.rar_rename != 0,
                    direct_unpack: sc.direct_unpack != 0,
                    par_pause_queue: sc.par_pause_queue != 0,
                    unpack_pause_queue: sc.unpack_pause_queue != 0,
                    script_pause_queue: sc.script_pause_queue != 0,
                    post_download_rename: sc.post_download_rename != 0,
                    rename_after_unpack: sc.rename_after_unpack != 0,
                    hard_linking: sc.hard_linking != 0,
                    unpack: sc.unpack != 0,
                    parcheck_enabled,
                    cur_time,
                    queue_dir: o.str_bytes(6).to_vec(),
                    par_ignore_ext: o.str_bytes(33).to_vec(),
                    ext_cleanup_disk: o.str_bytes(32).to_vec(),
                    inter_dir: o.str_bytes(4).to_vec(),
                    hard_linking_ignore_ext: o.str_bytes(37).to_vec(),
                };
            }
            return GlobalOptsSnap {
                parcheck_enabled,
                cur_time,
                pause_download,
                temp_pause_download,
                quota_reached,
                download_held,
                pause_post_process,
                pause_scan,
                temp_pause_postprocess,
                ..GlobalOptsSnap::default()
            };
        }
        let mut ints = [0i32; 35];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 6];
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
                temp_pause_postprocess: ints[20] != 0,
                keep_history: ints[21],
                post_strategy: ints[22],
                skip_write: ints[23] != 0,
                par_rename: ints[24] != 0,
                rar_rename: ints[25] != 0,
                direct_unpack: ints[26] != 0,
                par_pause_queue: ints[27] != 0,
                unpack_pause_queue: ints[28] != 0,
                script_pause_queue: ints[29] != 0,
                post_download_rename: ints[30] != 0,
                rename_after_unpack: ints[31] != 0,
                hard_linking: ints[32] != 0,
                unpack: ints[33] != 0,
                parcheck_enabled: ints[34] != 0,
                cur_time: i64s[0],
                queue_dir: read_cstr(strs[0]),
                par_ignore_ext: read_cstr(strs[1]),
                ext_cleanup_disk: read_cstr(strs[2]),
                inter_dir: read_cstr(strs[3]),
                hard_linking_ignore_ext: read_cstr(strs[4]),
            }
        }
    }

    pub fn log(&self, kind: i32, msg: &[u8]) {
        if self.use_active {
            crate::log::log_active_bytes(kind, msg);
            return;
        }
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
        if self.use_active {
            if ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(ms as u64));
            }
            return;
        }
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
        if self.use_active {
            let _ = standby;
            return;
        }
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
        #[cfg(unix)]
        if self.use_active {
            let mut perfect = true;
            let loaded = crate::statmeter::load_active_statmeter(&mut perfect);
            return (loaded, perfect);
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::statmeter::save_active_statmeter();
            return;
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::statmeter::interval_check_active_statmeter();
            return;
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::statmeter::enter_leave_standby_active_statmeter(enter);
            return;
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::statmeter::add_speed_reading_active_statmeter(0);
            return;
        }
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
        if self.use_active {
            if let Some(lg) = crate::log::active_log() {
                lg.interval_check_active();
            }
            return;
        }
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
        if self.use_active {
            if let Some(ws) = crate::ffi::active_workstate() {
                ws.set_downloading(downloading);
            }
            return;
        }
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
        if self.use_active {
            return crate::articledl::memory_monitor_effective_downloads_limit(limit);
        }
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

    pub fn work_state_set_temp_pause_download(&self, pause: bool) {
        if self.use_active {
            if let Some(ws) = crate::ffi::active_workstate() {
                ws.set_temp_pause_download(pause);
            }
            return;
        }
        self.raw(
            3,
            4,
            2,
            usize::from(pause),
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    /// Returns the `Unpack` setting of category `cat_name`, if the category exists.
    pub fn find_category(&self, cat_name: &[u8]) -> Option<bool> {
        if self.use_active {
            let c = c_bytes(cat_name);
            let cs = unsafe { std::ffi::CStr::from_ptr(c.as_ptr() as *const c_char) };
            return crate::options::active_options()
                .and_then(|o| o.find_category(cs, false).map(|cat| cat.unpack));
        }
        let c = c_bytes(cat_name);
        let mut ints = [0i32; 1];
        let mut strs = [std::ptr::null(); 1];
        let found = self.raw(
            3,
            6,
            0,
            0,
            0,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        if found == 0 {
            None
        } else {
            Some(ints[0] != 0)
        }
    }

    pub fn dq_is_loaded(&self) -> bool {
        if self.use_active {
            return crate::ffi::is_queue_loaded();
        }
        self.raw(
            3,
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

    pub fn server_pool_generation(&self) -> i32 {
        #[cfg(unix)]
        if let Some(pool) = crate::net::active_server_pool() {
            return pool.generation();
        }
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
        #[cfg(unix)]
        if let Some(pool) = crate::net::active_server_pool() {
            pool.close_unused_connections();
            return;
        }
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
        #[cfg(unix)]
        if let Some(pool) = crate::net::active_server_pool() {
            let srvs = pool.servers_snapshot();
            let mut levels = Vec::with_capacity(srvs.len());
            let mut actives = Vec::with_capacity(srvs.len());
            let mut conns = Vec::with_capacity(srvs.len());
            for s in &srvs {
                levels.push(s.norm_level.load(std::sync::atomic::Ordering::SeqCst));
                actives.push(u8::from(s.active.load(std::sync::atomic::Ordering::SeqCst)));
                conns.push(s.max_connections);
            }
            return (levels, actives, conns);
        }
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
        #[cfg(unix)]
        if let Some(pool) = crate::net::active_server_pool() {
            let desired = if has_desired {
                pool.server_by_id(desired_id)
            } else {
                None
            };
            let use_specific = desired
                .as_ref()
                .is_some_and(|s| s.active.load(std::sync::atomic::Ordering::SeqCst));
            let conn_level = match desired.as_ref() {
                Some(d) if use_specific => d.norm_level.load(std::sync::atomic::Ordering::SeqCst),
                _ => 0,
            };
            return pool.get_connection(
                conn_level,
                if use_specific {
                    desired.as_deref()
                } else {
                    None
                },
                &[],
            ) as usize;
        }
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
        #[cfg(unix)]
        if self.use_active {
            return crate::diskstate_format::active_disk_queue_exists();
        }
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
        #[cfg(unix)]
        if self.use_active {
            return crate::diskstate_format::active_disk_load_queue(self, dq_ptr);
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_discard_queue();
            return;
        }
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
        #[cfg(unix)]
        if self.use_active {
            return crate::diskstate_format::active_legacy_queue_read();
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_cleanup_temp_dir(self, dq_ptr);
            return;
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_save_all_file_infos(self, dq_ptr);
            return;
        }
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
        self.disk_discard_files_ex(nzb_ptr, true);
    }

    pub fn disk_discard_files_ex(&self, nzb_ptr: usize, delete_log: bool) {
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_discard_files(self, nzb_ptr, delete_log);
            return;
        }
        self.raw(
            5,
            6,
            i32::from(!delete_log),
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn disk_discard_file(&self, file_id: i32, del_data: bool, del_part: bool, del_compl: bool) {
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_discard_file(
                file_id, del_data, del_part, del_compl,
            );
            return;
        }
        let mut ints = [
            i32::from(del_data),
            i32::from(del_part),
            i32::from(del_compl),
        ];
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
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_save_file(self, file_ptr, false);
            return;
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_save_file_state(self, file_ptr, completed, false);
            return;
        }
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
        #[cfg(unix)]
        if self.use_active {
            return crate::diskstate_format::active_disk_load_file_state(self, file_ptr, completed);
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_resave_completed_file_state(self, file_id);
            return;
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_load_file(self, file_ptr, false, true);
            return;
        }
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

    pub fn disk_load_completed_segments(
        &self,
        file_id: i32,
        push_ctx: usize,
        push_fn: usize,
    ) -> usize {
        #[cfg(unix)]
        if self.use_active {
            return crate::diskstate_format::active_disk_load_completed_segments(
                self, file_id, push_ctx, push_fn,
            );
        }
        self.raw(
            5,
            13,
            file_id,
            push_ctx,
            push_fn,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn disk_load_file_for_inspect(
        &self,
        file_id: i32,
        load_info_data: bool,
        try_load_completed_state: bool,
    ) -> (usize, bool) {
        #[cfg(unix)]
        if self.use_active {
            return crate::diskstate_format::active_disk_load_file_for_inspect(
                self,
                file_id,
                load_info_data,
                try_load_completed_state,
            );
        }
        let mut ints = [
            i32::from(load_info_data),
            i32::from(try_load_completed_state),
        ];
        let ptr = self.raw(
            5,
            14,
            file_id,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        (ptr, ints[0] != 0)
    }

    // Subsystems (op = 6)
    pub fn history_delete_disk_files(&self, nzb_ptr: usize) {
        #[cfg(unix)]
        if self.use_active {
            crate::historycoord::delete_disk_files_active(nzb_ptr);
            return;
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::historycoord::redownload_active(dq_ptr, hi_ptr);
            return;
        }
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
        #[cfg(unix)]
        if self.use_active {
            crate::historycoord::history_hide_active(dq_ptr, hi_ptr, rindex);
            return;
        }
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
        if self.use_active {
            if let Some(pp) = crate::ffi::active_prepost() {
                pp.check_download_completed(self, dq_ptr, nzb_ptr);
            }
            return;
        }
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

    pub fn stream_repair_start_live(&self, nzb_ptr: usize) {
        if nzb_ptr == 0 {
            return;
        }
        #[cfg(unix)]
        if self.use_active && crate::dupefallback::should_defer_to_par_host(self, nzb_ptr) {
            return;
        }
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

    pub fn history_add_to_history(&self, dq_ptr: usize, nzb_ptr: usize) {
        #[cfg(unix)]
        if self.use_active {
            crate::historycoord::add_to_history_active(dq_ptr, nzb_ptr);
            return;
        }
        self.raw(
            6,
            8,
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

    pub fn history_edit_list(
        &self,
        dq_ptr: usize,
        ids: &[i32],
        action: i32,
        args: Option<&[u8]>,
    ) -> bool {
        #[cfg(unix)]
        if self.use_active {
            return crate::historycoord::edit_list_active(dq_ptr, ids, action, args);
        }
        let c_args = args.map(c_bytes);
        let s1 = c_args
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr() as *const c_char);
        let mut id_buf = ids.to_vec();
        self.raw(
            6,
            9,
            id_buf.len() as i32,
            dq_ptr,
            action as usize,
            s1,
            std::ptr::null(),
            if id_buf.is_empty() {
                std::ptr::null_mut()
            } else {
                id_buf.as_mut_ptr()
            },
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn prepost_edit_list(
        &self,
        dq_ptr: usize,
        ids: &[i32],
        action: i32,
        args: Option<&[u8]>,
    ) -> bool {
        if self.use_active {
            return crate::ffi::active_prepost()
                .is_some_and(|pp| pp.edit_list(self, dq_ptr, ids, action, args));
        }
        let c_args = args.map(c_bytes);
        let s1 = c_args
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr() as *const c_char);
        let mut id_buf = ids.to_vec();
        self.raw(
            6,
            10,
            id_buf.len() as i32,
            dq_ptr,
            action as usize,
            s1,
            std::ptr::null(),
            if id_buf.is_empty() {
                std::ptr::null_mut()
            } else {
                id_buf.as_mut_ptr()
            },
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn url_delete_queue_entry(
        &self,
        dq_ptr: usize,
        nzb_ptr: usize,
        avoid_history: bool,
    ) -> bool {
        if self.use_active {
            return crate::urlcoord::active_url_coord()
                .is_some_and(|uc| uc.delete_queue_entry_active(dq_ptr, nzb_ptr, avoid_history));
        }
        self.raw(
            6,
            11,
            i32::from(avoid_history),
            dq_ptr,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn dupe_search_notify_name_changed(&self, nzb_id: i32) {
        #[cfg(unix)]
        if self.use_active {
            crate::dupesearch_coord::notify_active_dupesearch_name_changed(nzb_id);
            return;
        }
        self.raw(
            6,
            12,
            nzb_id,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn url_coord_add_feed_item(
        &self,
        feed_id: i32,
        url: &[u8],
        filename: &[u8],
        category: &[u8],
        priority: i32,
        add_paused: bool,
        dupe_key: &[u8],
        dupe_score: i32,
        dupe_mode: i32,
        size: i64,
        time: i64,
    ) {
        #[cfg(unix)]
        if self.use_active {
            crate::urlcoord::add_feed_url_active(
                feed_id, url, filename, category, priority, add_paused, dupe_key, dupe_score,
                dupe_mode, size, time,
            );
            return;
        }
        let c_url = c_bytes(url);
        let c_fn = c_bytes(filename);
        let c_cat = c_bytes(category);
        let c_dk = c_bytes(dupe_key);
        let packed_flags = i32::from(add_paused) | ((dupe_mode & 0xff) << 1);
        let mut ints = [feed_id, packed_flags, priority, dupe_score];
        let mut i64s = [size, time];
        let mut strs = [
            c_url.as_ptr() as *const c_char,
            c_fn.as_ptr() as *const c_char,
            c_cat.as_ptr() as *const c_char,
            c_dk.as_ptr() as *const c_char,
        ];
        self.raw(
            6,
            14,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
    }

    pub fn scanner_add_url_file(
        &self,
        nzb_ptr: usize,
        is_archive: bool,
        eff_name: &[u8],
        eff_cat: &[u8],
        out_file: &[u8],
    ) -> i32 {
        #[cfg(unix)]
        if self.use_active {
            return crate::scanner::add_url_file_active(
                nzb_ptr, is_archive, eff_name, eff_cat, out_file,
            );
        }
        let c_name = c_bytes(eff_name);
        let c_cat = c_bytes(eff_cat);
        let c_out = c_bytes(out_file);
        let mut strs = [c_out.as_ptr() as *const c_char];
        self.raw(
            6,
            15,
            i32::from(is_archive),
            nzb_ptr,
            0,
            c_name.as_ptr() as *const c_char,
            c_cat.as_ptr() as *const c_char,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        ) as i32
    }

    // DownloadQueue & CoordinatorDownloadQueue ops (op = 20..=25)
    pub fn with_locked_queue<R, F: FnOnce(usize) -> R>(&self, f: F) -> R {
        let mut call = LockedQueueCall {
            f: Some(f),
            out: None,
        };
        self.raw(
            20,
            0,
            0,
            locked_queue_trampoline::<R, F> as *const () as usize,
            (&mut call as *mut LockedQueueCall<F, R>) as usize,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        call.out.expect("locked_queue callback did not run")
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

    pub fn queue_insert_owned(
        &self,
        dq_ptr: usize,
        uptr_addr: usize,
        url_ptr: usize,
        add_first: bool,
    ) {
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

    pub fn queue_reorder(&self, dq_ptr: usize, new_order: &[usize]) {
        if dq_ptr == 0 || new_order.is_empty() {
            return;
        }
        self.raw(
            22,
            5,
            new_order.len() as i32,
            dq_ptr,
            new_order.as_ptr() as usize,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn destroy_nzb_uptr(&self, uptr_addr: usize) {
        if uptr_addr == 0 {
            return;
        }
        self.raw(
            22,
            6,
            0,
            0,
            uptr_addr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn id_gen_last(&self) -> (i32, i32) {
        let mut ints = [0i32; 2];
        self.raw(
            22,
            7,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        (ints[0], ints[1])
    }

    pub fn id_gen_raise_and_reset(&self, nzb_id: i32, file_id: i32) {
        let mut ints = [nzb_id, file_id];
        self.raw(
            22,
            7,
            1,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn history_push_nzb_uptr(&self, dq_ptr: usize, uptr_addr: usize, hist_time: i32) {
        self.raw(
            23,
            -7,
            hist_time,
            dq_ptr,
            uptr_addr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn history_push_dup(
        &self,
        dq_ptr: usize,
        id: i32,
        hist_time: i32,
        status: i32,
        size: i64,
        full_hash: u32,
        filt_hash: u32,
        dupe_score: i32,
        dupe_mode: i32,
        name: *const c_char,
        dupe_key: *const c_char,
    ) {
        let mut ints = [
            id,
            hist_time,
            status,
            full_hash as i32,
            filt_hash as i32,
            dupe_score,
            dupe_mode,
        ];
        let mut i64s = [size];
        self.raw(
            23,
            -8,
            0,
            dq_ptr,
            0,
            name,
            dupe_key,
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn history_move_queue_item(&self, dq_ptr: usize, nzb_ptr: usize) {
        self.raw(
            23,
            -3,
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

    pub fn history_replace_with_dup(
        &self,
        dq_ptr: usize,
        hi_ptr: usize,
        rindex: usize,
        dup_status: i32,
    ) {
        let mut ints = [rindex as i32];
        self.raw(
            23,
            -4,
            dup_status,
            dq_ptr,
            hi_ptr,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn history_erase_at(&self, dq_ptr: usize, idx: usize) {
        self.raw(
            23,
            -5,
            idx as i32,
            dq_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn history_return_to_queue(&self, dq_ptr: usize, hi_ptr: usize, idx: usize) -> usize {
        self.raw(
            23,
            -6,
            idx as i32,
            dq_ptr,
            hi_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
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
        let mut i64s = [0i64; 2];
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
                id: ints[1],
                kind: ints[0],
                inner_ptr: i64s[0] as usize,
                time: i64s[1],
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

    pub fn dq_edit_list_single_id(
        &self,
        dq_ptr: usize,
        id: i32,
        action: i32,
        args: Option<&[u8]>,
    ) -> bool {
        let mut ints = [action];
        let c_args = args.map(c_bytes);
        let s1 = c_args
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr() as *const c_char);
        self.raw(
            24,
            6,
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

    pub fn dq_edit_list(
        &self,
        dq_ptr: usize,
        ids: Option<&[i32]>,
        names: Option<&[&[u8]]>,
        match_mode: i32,
        action: i32,
        args: &[u8],
    ) -> bool {
        let c_args = c_bytes(args);
        let id_slice = ids.unwrap_or(&[]);
        let name_cvecs: Vec<Vec<u8>> = names.unwrap_or(&[]).iter().map(|n| c_bytes(n)).collect();
        let mut name_ptrs: Vec<*const c_char> = name_cvecs
            .iter()
            .map(|v| v.as_ptr() as *const c_char)
            .collect();
        let flags = i32::from(ids.is_some()) | (i32::from(names.is_some()) << 1);
        let mut ints = [
            action,
            match_mode,
            id_slice.len() as i32,
            name_ptrs.len() as i32,
        ];
        self.raw(
            24,
            7,
            flags,
            dq_ptr,
            if id_slice.is_empty() {
                0
            } else {
                id_slice.as_ptr() as usize
            },
            c_args.as_ptr() as *const c_char,
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            if name_ptrs.is_empty() {
                std::ptr::null_mut()
            } else {
                name_ptrs.as_mut_ptr()
            },
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
        #[cfg(unix)]
        if self.use_active {
            let url_opt = if aspect_action == ASPECT_URL_COMPLETED && nzb_ptr != 0 {
                Some(self.nzb_snap(nzb_ptr, false).url)
            } else {
                None
            };
            crate::feed::notify_active_feed_queue_aspect(aspect_action, url_opt.as_deref());
            crate::urlcoord::notify_active_urlcoord_queue_update(aspect_action);
            if aspect_action == ASPECT_NZB_ADDED
                && nzb_ptr != 0
                && !self.nzb_snap(nzb_ptr, false).dupe_key.is_empty()
            {
                crate::twin::nudge_and_wake_active();
            }
            if nzb_ptr != 0
                && (aspect_action == ASPECT_NZB_ADDED || aspect_action == ASPECT_NZB_NAMED)
            {
                crate::dupesearch_coord::notify_active_dupesearch_queue_update(
                    if aspect_action == ASPECT_NZB_ADDED {
                        1
                    } else {
                        2
                    },
                    dq_ptr,
                    nzb_ptr,
                );
            }
        }
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

    // NzbInfo ops (op = 30..=35)
    pub fn nzb_snap(&self, nzb_ptr: usize, with_text_status: bool) -> NzbSnap {
        self.nzb_snap_mode(nzb_ptr, i32::from(with_text_status))
    }

    pub fn nzb_snap_mode(&self, nzb_ptr: usize, text_status_mode: i32) -> NzbSnap {
        if nzb_ptr == 0 {
            return NzbSnap::default();
        }
        let mut ints = [0i32; 88];
        let mut i64s = [0i64; 21];
        let mut strs = [std::ptr::null(); 9];
        self.raw(
            30,
            text_status_mode,
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
                post_info_ptr: i64s[18] as usize,
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
                live_repair_thread_ptr: i64s[19] as usize,
                has_unpack_thread: ints[68] != 0,
                has_stream_repair_jobs: ints[69] != 0,
                url_status: ints[70],
                par_sec: ints[71],
                repair_sec: ints[72],
                unpack_sec: ints[73],
                reprocess: ints[74] != 0,
                extra_par_blocks: ints[75],
                dupe_recovered_holes: ints[76],
                script_total_status: ints[77],
                parked_file_count: ints[78],
                delete_paused: ints[79] != 0,
                auto_category: ints[80] != 0,
                add_url_paused: ints[81] != 0,
                is_dupe_success: ints[29] != 0,
                health: ints[30],
                critical_health_opt: ints[31],
                critical_health_exact: ints[32],
                is_download_completed: ints[33] != 0,
                is_download_completed_strict: ints[54] != 0,
                skip_disk_write: ints[50] != 0,
                waiting_par: ints[51] != 0,
                loading_par: ints[52] != 0,
                unpack_cleaned_up_disk: ints[53] != 0,
                skip_script_processing: ints[55] != 0,
                avoid_history: ints[56] != 0,
                cleanup_disk: ints[57] != 0,
                unpack_tried: ints[58] != 0,
                par_full: ints[59] != 0,
                force_par_full: ints[83] != 0,
                force_repair: ints[84] != 0,
                par_repaired: ints[85] != 0,
                last_unpack_status: ints[86],
                message_count: ints[82],
                cached_message_count: ints[87],
                post_total_sec: ints[60],
                par_rename_status: ints[61],
                rar_rename_status: ints[62],
                unpack_status: ints[63],
                cleanup_status: ints[64],
                move_status: ints[65],
                post_download_renaming_status: ints[66],
                post_unpack_renaming_status: ints[67],
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
                dupe_recovered_bytes: i64s[17],
                name: read_cstr(strs[0]),
                filename: read_cstr(strs[1]),
                dest_dir: read_cstr(strs[2]),
                final_dir: read_cstr(strs[3]),
                category: read_cstr(strs[4]),
                dupe_key: read_cstr(strs[5]),
                queued_filename: read_cstr(strs[6]),
                text_status: read_cstr(strs[7]),
                url: read_cstr(strs[8]),
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

    pub fn nzb_build_final_dir_name(&self, nzb_ptr: usize) -> Vec<u8> {
        if nzb_ptr == 0 {
            return Vec::new();
        }
        let mut strs = [std::ptr::null(); 1];
        self.raw(
            31,
            3,
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        unsafe { read_cstr(strs[0]) }
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

    pub fn nzb_hard_link_path(&self, nzb_ptr: usize) -> Vec<u8> {
        if nzb_ptr == 0 {
            return Vec::new();
        }
        let mut strs = [std::ptr::null(); 1];
        self.raw(
            31,
            6,
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        unsafe { read_cstr(strs[0]) }
    }

    pub fn nzb_meta_name(&self, nzb_ptr: usize) -> Vec<u8> {
        if nzb_ptr == 0 {
            return Vec::new();
        }
        let mut strs = [std::ptr::null(); 1];
        self.raw(
            31,
            7,
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        unsafe { read_cstr(strs[0]) }
    }

    pub fn nzb_params_len(&self, nzb_ptr: usize) -> usize {
        if nzb_ptr == 0 {
            return 0;
        }
        self.raw(
            31,
            8,
            -1,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn nzb_param_at(&self, nzb_ptr: usize, idx: usize) -> (Vec<u8>, Vec<u8>) {
        let mut strs = [std::ptr::null(); 2];
        self.raw(
            31,
            8,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        unsafe { (read_cstr(strs[0]), read_cstr(strs[1])) }
    }

    pub fn nzb_server_stats_len(&self, nzb_ptr: usize) -> usize {
        if nzb_ptr == 0 {
            return 0;
        }
        self.raw(
            31,
            9,
            -1,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn nzb_server_stat_at(&self, nzb_ptr: usize, idx: usize) -> (i32, i32, i32) {
        let mut ints = [0i32; 3];
        self.raw(
            31,
            9,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        (ints[0], ints[1], ints[2])
    }

    pub fn nzb_current_server_stats_len(&self, nzb_ptr: usize) -> usize {
        if nzb_ptr == 0 {
            return 0;
        }
        self.raw(
            31,
            21,
            -1,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn nzb_current_server_stat_at(&self, nzb_ptr: usize, idx: usize) -> (i32, i32, i32) {
        let mut ints = [0i32; 3];
        self.raw(
            31,
            21,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        (ints[0], ints[1], ints[2])
    }

    pub fn nzb_cached_messages_slice(
        &self,
        nzb_ptr: usize,
        id_from: i32,
        max_entries: i32,
    ) -> Vec<(i32, i32, i32, std::ffi::CString)> {
        if nzb_ptr == 0 {
            return Vec::new();
        }
        unsafe extern "C" fn push_msg(
            ctx: usize,
            id: c_int,
            kind: c_int,
            time: c_int,
            text: *const c_char,
        ) {
            let out = &mut *(ctx as *mut Vec<(i32, i32, i32, std::ffi::CString)>);
            let bytes = if text.is_null() {
                &[][..]
            } else {
                std::ffi::CStr::from_ptr(text).to_bytes()
            };
            out.push((
                id,
                kind,
                time,
                std::ffi::CString::from_vec_unchecked(bytes.to_vec()),
            ));
        }
        let mut out: Vec<(i32, i32, i32, std::ffi::CString)> = Vec::new();
        let mut ints = [id_from, max_entries];
        self.raw(
            31,
            20,
            0,
            nzb_ptr,
            (&mut out as *mut Vec<(i32, i32, i32, std::ffi::CString)>) as usize,
            push_msg as *const () as *const c_char,
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        out
    }

    pub fn nzb_push_script_status(&self, nzb_ptr: usize, name: &[u8], status: i32) {
        let cn = c_bytes(name);
        self.raw(
            31,
            10,
            status,
            nzb_ptr,
            0,
            cn.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_rename_completed_file(
        &self,
        nzb_ptr: usize,
        old_name: &[u8],
        new_name: &[u8],
    ) -> bool {
        let co = c_bytes(old_name);
        let cn = c_bytes(new_name);
        self.raw(
            31,
            11,
            0,
            nzb_ptr,
            0,
            co.as_ptr() as *const c_char,
            cn.as_ptr() as *const c_char,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn nzb_unpack_thread_ptr(&self, nzb_ptr: usize) -> usize {
        if nzb_ptr == 0 {
            return 0;
        }
        self.raw(
            31,
            12,
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

    pub fn nzb_push_extracted_archive(&self, nzb_ptr: usize, name: &[u8]) {
        if nzb_ptr == 0 {
            return;
        }
        let cn = c_bytes(name);
        self.raw(
            31,
            13,
            0,
            nzb_ptr,
            0,
            cn.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_set_meta_name(&self, nzb_ptr: usize, meta_name: &[u8]) {
        if nzb_ptr == 0 {
            return;
        }
        let cn = c_bytes(meta_name);
        self.raw(
            31,
            14,
            0,
            nzb_ptr,
            0,
            cn.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_copy_cached_messages_silent(&self, dst_nzb: usize, src_nzb: usize) {
        if dst_nzb == 0 || src_nzb == 0 {
            return;
        }
        self.raw(
            31,
            15,
            0,
            dst_nzb,
            src_nzb,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_make_nice_url_name(&self, nzb_ptr: usize, url: &[u8], filename: &[u8]) -> Vec<u8> {
        if nzb_ptr == 0 {
            return Vec::new();
        }
        let cu = c_bytes(url);
        let cf = c_bytes(filename);
        let mut strs = [std::ptr::null(); 1];
        self.raw(
            31,
            16,
            0,
            nzb_ptr,
            0,
            cu.as_ptr() as *const c_char,
            cf.as_ptr() as *const c_char,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        unsafe { read_cstr(strs[0]) }
    }

    pub fn nzb_script_statuses_len(&self, nzb_ptr: usize) -> usize {
        if nzb_ptr == 0 {
            return 0;
        }
        self.raw(
            31,
            17,
            -1,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn nzb_script_status_at(&self, nzb_ptr: usize, idx: usize) -> (i32, Vec<u8>) {
        let mut ints = [0i32; 1];
        let mut strs = [std::ptr::null(); 1];
        self.raw(
            31,
            17,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        (ints[0], unsafe { read_cstr(strs[0]) })
    }

    pub fn nzb_extracted_archives_len(&self, nzb_ptr: usize) -> usize {
        if nzb_ptr == 0 {
            return 0;
        }
        self.raw(
            31,
            18,
            -1,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn nzb_extracted_archive_at(&self, nzb_ptr: usize, idx: usize) -> Vec<u8> {
        let mut strs = [std::ptr::null(); 1];
        self.raw(
            31,
            18,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        unsafe { read_cstr(strs[0]) }
    }

    pub fn nzb_server_stat_set(&self, nzb_ptr: usize, server_id: i32, succ: i32, fail: i32) {
        let mut ints = [server_id, succ, fail];
        self.raw(
            31,
            19,
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_sync_current_server_stats(&self, nzb_ptr: usize) {
        self.raw(
            31,
            19,
            1,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
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
        self.nzb_set_str_opt(nzb_ptr, field, Some(val));
    }

    pub fn nzb_set_str_opt(&self, nzb_ptr: usize, field: i32, val: Option<&[u8]>) {
        let cv = val.map(c_bytes);
        let s1 = cv
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr() as *const c_char);
        self.raw(
            32,
            field,
            0,
            nzb_ptr,
            0,
            s1,
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
        #[cfg(unix)]
        if self.use_active {
            crate::feed::notify_active_feed_queue_aspect(aspect_action, None);
            crate::urlcoord::notify_active_urlcoord_queue_update(aspect_action);
        }
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

    pub fn nzb_completed_files_len(&self, nzb_ptr: usize) -> usize {
        self.raw(
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
        )
    }

    pub fn nzb_completed_file_at(&self, nzb_ptr: usize, idx: usize) -> CompletedFileSnap {
        let mut ints = [0i32; 4];
        let mut strs = [std::ptr::null(); 4];
        self.raw(
            34,
            5,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        unsafe {
            CompletedFileSnap {
                id: ints[0],
                status: ints[1],
                par_file: ints[2] != 0,
                crc: ints[3] as u32,
                filename: read_cstr(strs[0]),
                origname: read_cstr(strs[1]),
                hash16k: read_cstr(strs[2]),
                par_set_id: read_cstr(strs[3]),
            }
        }
    }

    pub fn nzb_completed_files(&self, nzb_ptr: usize) -> Vec<CompletedFileSnap> {
        let n = self.nzb_completed_files_len(nzb_ptr);
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            out.push(self.nzb_completed_file_at(nzb_ptr, i));
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

    pub fn nzb_update_completed_file(
        &self,
        nzb_ptr: usize,
        idx: usize,
        filename: &[u8],
        origname: Option<&[u8]>,
    ) {
        let c_fn = c_bytes(filename);
        let c_orig = origname.map(c_bytes);
        let s2 = c_orig
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr() as *const c_char);
        self.raw(
            34,
            7,
            idx as i32,
            nzb_ptr,
            0,
            c_fn.as_ptr() as *const c_char,
            s2,
            std::ptr::null_mut(),
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
        let mut i64s = [
            decoded_file_size,
            failed_size,
            missed_size,
            steps_hash as i64,
        ];
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

    pub fn nzb_reorder_files(&self, nzb_ptr: usize, new_order: &[usize]) {
        if nzb_ptr == 0 || new_order.is_empty() {
            return;
        }
        self.raw(
            34,
            10,
            new_order.len() as i32,
            nzb_ptr,
            new_order.as_ptr() as usize,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_stream_repair_jobs(&self, nzb_ptr: usize) -> Vec<StreamRepairJobSnap> {
        if nzb_ptr == 0 {
            return Vec::new();
        }
        let n = self.raw(
            34,
            11,
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
        let mut ints = [0i32; 1];
        let mut i64s = [0i64; 3];
        let mut strs = [std::ptr::null(); 1];
        for i in 0..n {
            self.raw(
                34,
                11,
                i as i32,
                nzb_ptr,
                0,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                strs.as_mut_ptr(),
            );
            out.push(StreamRepairJobSnap {
                file_id: ints[0],
                decoded_file_size: i64s[0],
                holes_total_size: i64s[1],
                failed_plus_missed_size: i64s[2],
                filename: unsafe { read_cstr(strs[0]) },
            });
        }
        out
    }

    pub fn nzb_stream_repair_jobs_len(&self, nzb_ptr: usize) -> usize {
        if nzb_ptr == 0 {
            return 0;
        }
        self.raw(
            34,
            11,
            -1,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn nzb_stream_repair_update_completed_file(&self, nzb_ptr: usize, file_id: i32, mode: i32) {
        let mut ints = [mode];
        if self.raw(
            34,
            12,
            file_id,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
        {
            self.disk_discard_file(file_id, true, true, true);
        }
    }

    pub fn nzb_stream_repair_job_detail(
        &self,
        nzb_ptr: usize,
        idx: usize,
    ) -> StreamRepairJobDetailSnap {
        let mut ints = [0i32; 5];
        let mut i64s = [0i64; 4];
        let mut strs = [std::ptr::null(); 2];
        self.raw(
            34,
            13,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        let holes_len = ints[4].max(0) as usize;
        let holes = if holes_len == 0 || strs[1].is_null() {
            Vec::new()
        } else {
            unsafe {
                std::slice::from_raw_parts(
                    strs[1] as *const crate::dupefallback::NzbgetRsByteRange,
                    holes_len,
                )
                .to_vec()
            }
        };
        StreamRepairJobDetailSnap {
            file_id: ints[0],
            failed_articles: ints[1],
            par_file: ints[2] != 0,
            live_attempted: ints[3] != 0,
            decoded_file_size: i64s[0],
            failed_size: i64s[1],
            missed_size: i64s[2],
            steps_hash: i64s[3] as u64,
            filename: unsafe { read_cstr(strs[0]) },
            holes,
        }
    }

    pub fn nzb_stream_repair_mark_live_attempted(&self, nzb_ptr: usize, idx: usize) -> bool {
        let mut ints = [1i32];
        self.raw(
            34,
            13,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn nzb_stream_repair_erase_job(&self, nzb_ptr: usize, idx: usize, set_changed: bool) {
        let mut ints = [2i32, i32::from(set_changed)];
        self.raw(
            34,
            13,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_stream_repair_update_job_holes(
        &self,
        nzb_ptr: usize,
        idx: usize,
        new_decoded_size: i64,
        holes_ptr: usize,
        holes_len: i32,
    ) {
        let mut ints = [3i32, holes_len];
        let mut i64s = [new_decoded_size];
        self.raw(
            34,
            13,
            idx as i32,
            nzb_ptr,
            holes_ptr,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn nzb_add_parsed_file(
        &self,
        nzb_ptr: usize,
        id: i32,
        subject: *const c_char,
        filename: *const c_char,
        time: i32,
        size: i64,
        remaining_size: i64,
        missed_size: i64,
        total_articles: i32,
        missed_articles: i32,
        par_file: bool,
        group_ptrs: &[*const c_char],
        article_parts: &[i32],
        article_sizes: &[i64],
        article_msg_ids: &[*const c_char],
    ) -> usize {
        let mut ints = [
            id,
            time,
            total_articles,
            missed_articles,
            i32::from(par_file),
            group_ptrs.len() as i32,
            article_parts.len() as i32,
        ];
        let mut i64s = [
            size,
            remaining_size,
            missed_size,
            if article_parts.is_empty() {
                0
            } else {
                article_parts.as_ptr() as usize as i64
            },
            if article_sizes.is_empty() {
                0
            } else {
                article_sizes.as_ptr() as usize as i64
            },
        ];
        let mut strs = [
            if group_ptrs.is_empty() {
                std::ptr::null()
            } else {
                group_ptrs.as_ptr() as *const c_char
            },
            if article_msg_ids.is_empty() {
                std::ptr::null()
            } else {
                article_msg_ids.as_ptr() as *const c_char
            },
        ];
        self.raw(
            34,
            14,
            0,
            nzb_ptr,
            0,
            subject,
            filename,
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        )
    }

    pub fn nzb_redownload_replace_files(&self, nzb_ptr: usize, src_nzb_ptr: usize) {
        self.raw(
            34,
            15,
            0,
            nzb_ptr,
            src_nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_erase_completed_file(&self, nzb_ptr: usize, idx: usize) {
        self.raw(
            34,
            16,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn nzb_add_requeued_file(&self, nzb_ptr: usize, file_ptr: usize) {
        self.raw(
            34,
            17,
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

    #[allow(clippy::too_many_arguments)]
    pub fn nzb_push_completed_file_raw(
        &self,
        nzb_ptr: usize,
        id: i32,
        status: i32,
        crc: u32,
        par_file: bool,
        filename: *const c_char,
        origname: *const c_char,
        hash16k: *const c_char,
        par_set_id: *const c_char,
    ) {
        let mut ints = [id, status, crc as i32, i32::from(par_file)];
        let mut strs = [filename, origname, hash16k, par_set_id];
        self.raw(
            34,
            18,
            0,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
    }

    pub fn nzb_add_stub_file(
        &self,
        nzb_ptr: usize,
        id: i32,
        paused: bool,
        time_opt: Option<i32>,
        extra_priority: bool,
    ) {
        let mut ints = [
            id,
            i32::from(paused),
            time_opt.unwrap_or(0),
            i32::from(extra_priority),
        ];
        self.raw(
            34,
            19,
            i32::from(time_opt.is_some()),
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn dup_snap(&self, dup_ptr: usize) -> DupSnap {
        if dup_ptr == 0 {
            return DupSnap::default();
        }
        let mut ints = [0i32; 5];
        let mut i64s = [0i64; 1];
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
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        unsafe {
            DupSnap {
                dupe_mode: ints[0],
                dupe_score: ints[1],
                status: ints[2],
                size: i64s[0],
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

    pub fn dup_set_str(&self, dup_ptr: usize, field: i32, val: &[u8]) {
        let c = c_bytes(val);
        self.raw(
            35,
            field,
            0,
            dup_ptr,
            0,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn dup_set_int(&self, dup_ptr: usize, field: i32, val: i32) {
        self.raw(
            35,
            field,
            val,
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
            id: ints[1],
            kind: ints[0],
            inner_ptr,
            time: 0,
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
        let mut strs = [std::ptr::null(); 7];
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
                origname: read_cstr(strs[4]),
                hash16k: read_cstr(strs[5]),
                par_set_id: read_cstr(strs[6]),
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

    pub fn file_server_stats_len(&self, file_ptr: usize) -> usize {
        if file_ptr == 0 {
            return 0;
        }
        self.raw(
            41,
            1,
            -1,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn file_server_stat_at(&self, file_ptr: usize, idx: usize) -> (i32, i32, i32) {
        let mut ints = [0i32; 3];
        self.raw(
            41,
            1,
            idx as i32,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        (ints[0], ints[1], ints[2])
    }

    pub fn file_server_stat_set(&self, file_ptr: usize, server_id: i32, succ: i32, fail: i32) {
        let mut ints = [server_id, succ, fail];
        self.raw(
            41,
            2,
            0,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn file_push_group(&self, file_ptr: usize, group: *const c_char) {
        self.raw(
            41,
            3,
            0,
            file_ptr,
            0,
            group,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
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

    pub fn alloc_file_id(&self) -> i32 {
        self.raw(
            42,
            56,
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

    pub fn delete_file_info(&self, file_ptr: usize) {
        if file_ptr == 0 {
            return;
        }
        self.raw(
            42,
            57,
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

    pub fn file_set_nzb_info(&self, file_ptr: usize, nzb_ptr: usize) {
        self.raw(
            42,
            58,
            0,
            file_ptr,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn file_subtract_server_stats_from_nzb_total(&self, file_ptr: usize, nzb_ptr: usize) {
        self.raw(
            42,
            59,
            0,
            file_ptr,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn file_new_standalone(&self, id: i32, bump_gen: bool) -> usize {
        self.raw(
            42,
            60,
            i32::from(bump_gen),
            0,
            id as usize,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }

    pub fn file_article_raw_at(&self, file_ptr: usize, idx: usize) -> (ArticleSnap, bool) {
        let mut ints = [0i32; 7];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 2];
        self.raw(
            43,
            10,
            idx as i32,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        unsafe {
            (
                ArticleSnap {
                    ptr: 0,
                    status: ints[0],
                    segment_size: ints[1],
                    crc: ints[2] as u32,
                    dupe_fallback_round: ints[3],
                    part_number: ints[5],
                    size: ints[6],
                    segment_offset: i64s[0],
                    message_id: read_cstr(strs[0]),
                    dupe_original_message_id: read_cstr(strs[1]),
                    ..Default::default()
                },
                ints[4] != 0,
            )
        }
    }

    pub fn file_push_article_basic(
        &self,
        file_ptr: usize,
        part_number: i32,
        size: i32,
        message_id: *const c_char,
    ) {
        self.raw(
            43,
            11,
            part_number,
            file_ptr,
            size as usize,
            message_id,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn file_article_disk_op(
        &self,
        file_ptr: usize,
        idx: i32,
        apply: bool,
        extra_i32: i32,
        seg_offset: i64,
        seg_size: i32,
        crc: u32,
        dupe_round: i32,
        status: i32,
        result_filename: *const c_char,
    ) -> i32 {
        let mut ints = [extra_i32, seg_size, crc as i32, dupe_round, status];
        let mut i64s = [seg_offset];
        self.raw(
            43,
            12,
            idx,
            file_ptr,
            usize::from(apply),
            result_filename,
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        ) as i32
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

    pub fn article_reset_for_retry(&self, article_ptr: usize) {
        self.raw(
            43,
            9,
            0,
            article_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    // ArticleCache & flush lock ops (op = 44)
    pub fn article_cache_file_busy(&self, file_ptr: usize) -> bool {
        crate::articledl::cache_file_busy(file_ptr)
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
        let copied = crate::articledl::with_cache_content_lock(|| {
            self.raw(
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
            )
        });
        if copied != 0 {
            Some(buf)
        } else {
            None
        }
    }

    pub fn article_cache_discard_segment(&self, file_ptr: usize, article_ptr: usize) {
        crate::articledl::with_cache_content_lock(|| {
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
        });
    }

    pub fn file_try_acquire_flush_lock(&self, file_ptr: usize) -> bool {
        crate::articledl::with_cache_content_lock(|| {
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
        })
    }

    pub fn file_release_flush_lock(&self, file_ptr: usize) {
        crate::articledl::with_cache_content_lock(|| {
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
        });
    }

    pub fn file_cache_flush_prepare(&self, file_ptr: usize) -> (bool, bool, usize, Vec<u8>) {
        let mut ints = [0i32; 2];
        let mut i64s = [0i64; 2];
        let mut strs = [std::ptr::null(); 1];
        self.raw(
            44,
            4,
            0,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        let out_fn = unsafe {
            if strs[0].is_null() || i64s[0] <= 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(strs[0].cast::<u8>(), i64s[0] as usize).to_vec()
            }
        };
        (ints[0] != 0, ints[1] != 0, i64s[1] as usize, out_fn)
    }

    pub fn file_cache_flush_lock_collect(
        &self,
        file_ptr: usize,
    ) -> Option<Vec<(usize, i64, i32, *const u8, Option<Vec<u8>>)>> {
        unsafe extern "C" fn push_seg(
            ctx: usize,
            art_ptr: usize,
            offset: i64,
            size: c_int,
            content: *const c_char,
            res_fn: *const c_char,
        ) {
            let out = &mut *(ctx as *mut Vec<(usize, i64, i32, *const u8, Option<Vec<u8>>)>);
            let rf = if res_fn.is_null() {
                None
            } else {
                Some(read_cstr(res_fn))
            };
            out.push((art_ptr, offset, size, content.cast::<u8>(), rf));
        }
        crate::articledl::with_cache_content_lock(|| {
            let mut out: Vec<(usize, i64, i32, *const u8, Option<Vec<u8>>)> = Vec::new();
            let ok = self.raw(
                44,
                5,
                0,
                file_ptr,
                (&mut out as *mut Vec<(usize, i64, i32, *const u8, Option<Vec<u8>>)>) as usize,
                push_seg as *const () as *const c_char,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0;
            if ok {
                Some(out)
            } else {
                None
            }
        })
    }

    pub fn file_cache_is_deleted_unparked(&self, file_ptr: usize) -> bool {
        self.raw(
            44,
            6,
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

    pub fn file_cache_discard_article_segment(&self, file_ptr: usize, article_ptr: usize) {
        self.raw(
            44,
            6,
            1,
            file_ptr,
            article_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn file_cache_flush_finish(&self, file_ptr: usize, flushed_articles: i32) -> bool {
        crate::articledl::with_cache_content_lock(|| {
            let mut ints = [0i32; 1];
            self.raw(
                44,
                7,
                flushed_articles,
                file_ptr,
                0,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            ints[0] != 0
        })
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

    pub fn ad_finish_analyzer(&self, ad_ptr: usize) -> Option<(bool, Vec<u8>, Vec<u8>)> {
        let mut ints = [0i32; 1];
        let mut strs = [std::ptr::null(); 2];
        let ok = self.raw(
            51,
            4,
            0,
            ad_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        if ok == 0 {
            None
        } else {
            unsafe { Some((ints[0] != 0, read_cstr(strs[0]), read_cstr(strs[1]))) }
        }
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

    // PostInfo & PrePostProcessor ops (op = 60..=62)
    pub fn post_snap(&self, post_ptr: usize) -> PostSnap {
        if post_ptr == 0 {
            return PostSnap::default();
        }
        let mut ints = [0i32; 21];
        let mut i64s = [0i64; 3];
        let mut strs = [std::ptr::null(); 1];
        self.raw(
            60,
            0,
            0,
            post_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        PostSnap {
            ptr: post_ptr,
            nzb_ptr: i64s[1] as usize,
            stage: ints[0],
            working: ints[1] != 0,
            deleted: ints[2] != 0,
            request_par_check: ints[3] != 0,
            force_par_full: ints[4] != 0,
            force_repair: ints[5] != 0,
            stream_repair_done: ints[6] != 0,
            stream_repair_recovered: ints[7] != 0,
            par_cannot_cover: ints[8] != 0,
            need_par_check: ints[9] != 0,
            need_unpack: ints[10] != 0,
            has_post_thread: ints[11] != 0,
            post_thread_running: ints[12] != 0,
            unpack_tried: ints[13] != 0,
            par_repaired: ints[14] != 0,
            pass_list_tried: ints[15] != 0,
            last_unpack_status: ints[16],
            extracted_archives_len: ints[17].max(0) as usize,
            parred_files_len: ints[18].max(0) as usize,
            stage_progress: ints[19],
            file_progress: ints[20],
            start_time: i64s[0],
            stage_time: i64s[2],
            progress_label: unsafe { read_cstr(strs[0]) },
        }
    }

    pub fn post_set_int(&self, post_ptr: usize, field: i32, val: i32) {
        if post_ptr == 0 {
            return;
        }
        self.raw(
            60,
            1,
            field,
            post_ptr,
            val as usize,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn post_set_start_time(&self, post_ptr: usize, start_time: i64) {
        if post_ptr == 0 {
            return;
        }
        let mut i64s = [start_time];
        self.raw(
            60,
            2,
            0,
            post_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn post_reset_stage_progress(&self, post_ptr: usize, stage_time: i64) {
        if post_ptr == 0 {
            return;
        }
        let mut i64s = [stage_time];
        self.raw(
            60,
            3,
            0,
            post_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn post_set_stage_time(&self, post_ptr: usize, stage_time: i64) {
        if post_ptr == 0 {
            return;
        }
        let mut i64s = [stage_time];
        self.raw(
            60,
            4,
            0,
            post_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn post_set_progress_label(&self, post_ptr: usize, label: &[u8]) {
        if post_ptr == 0 {
            return;
        }
        let c = c_bytes(label);
        self.raw(
            60,
            5,
            0,
            post_ptr,
            0,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn post_push_parred_file(&self, post_ptr: usize, name: &[u8]) {
        if post_ptr == 0 {
            return;
        }
        let c = c_bytes(name);
        self.raw(
            60,
            6,
            -1,
            post_ptr,
            0,
            c.as_ptr() as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn post_parred_file_at(&self, post_ptr: usize, idx: usize) -> Vec<u8> {
        let mut strs = [std::ptr::null(); 1];
        self.raw(
            60,
            6,
            idx as i32,
            post_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        unsafe { read_cstr(strs[0]) }
    }

    pub fn post_clear_extracted_archives(&self, post_ptr: usize) {
        if post_ptr == 0 {
            return;
        }
        self.raw(
            60,
            7,
            -1,
            post_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn post_extracted_archive_at(&self, post_ptr: usize, idx: usize) -> Vec<u8> {
        let mut strs = [std::ptr::null(); 1];
        self.raw(
            60,
            7,
            idx as i32,
            post_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        unsafe { read_cstr(strs[0]) }
    }

    pub fn prepost_is_stopped(&self, owner_ptr: usize) -> bool {
        self.raw(
            61,
            0,
            0,
            owner_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn thread_is_stopped(&self, thread_ptr: usize) -> bool {
        self.raw(
            61,
            1,
            0,
            thread_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn thread_stop(&self, thread_ptr: usize) {
        self.raw(
            61,
            2,
            0,
            thread_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn thread_resume(&self, thread_ptr: usize) {
        self.raw(
            61,
            3,
            0,
            thread_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn post_stop_thread(&self, post_ptr: usize) {
        self.raw(
            62,
            0,
            0,
            post_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn post_delete_thread(&self, post_ptr: usize) {
        self.raw(
            62,
            1,
            0,
            post_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn post_kill_and_delete_thread(&self, post_ptr: usize) {
        self.raw(
            62,
            2,
            0,
            post_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    pub fn direct_unpack_is_active(&self, nzb_ptr: usize, include_waiting_volume: bool) -> bool {
        self.raw(
            62,
            3,
            i32::from(include_waiting_volume),
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn direct_unpack_stop(&self, dq_ptr: usize, nzb_ptr: usize) {
        self.raw(
            62,
            4,
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

    pub fn direct_unpack_nzb_downloaded(&self, dq_ptr: usize, nzb_ptr: usize) {
        self.raw(
            62,
            5,
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

    pub fn direct_unpack_nzb_deleted(&self, dq_ptr: usize, nzb_ptr: usize) {
        self.raw(
            62,
            6,
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

    pub fn direct_unpack_file_downloaded(&self, dq_ptr: usize, nzb_ptr: usize, file_ptr: usize) {
        let mut i64s = [file_ptr as i64];
        self.raw(
            62,
            7,
            0,
            dq_ptr,
            nzb_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    pub fn direct_unpack_start_job(&self, nzb_ptr: usize) {
        self.raw(
            62,
            8,
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

    pub fn live_repair_stop(&self, nzb_ptr: usize) {
        self.raw(
            62,
            9,
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

    pub fn live_repair_detach_and_stop(&self, nzb_ptr: usize) {
        self.raw(
            62,
            10,
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

    pub fn repair_add_par(&self, post_ptr: usize, file_ptr: usize, deleted: bool) -> bool {
        self.raw(
            62,
            11,
            i32::from(deleted),
            post_ptr,
            file_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }

    pub fn post_controller_start_job(&self, post_ptr: usize, controller_kind: i32) {
        self.raw(
            62,
            12,
            controller_kind,
            post_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
}
