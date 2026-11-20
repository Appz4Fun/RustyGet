//! Shared host callback wrapper for `DupeArticleFallback`, `DupeCoordinator`,
//! and `QueueCoordinator` orchestration in Rust.

use crate::options::OptStr;
use crate::qfile::{FileAction, FileI64, FileInt, FileStr};
use crate::qnzb::PostInt;
use crate::qnzb::{
    FileTotals, MessageRange, NzbAction, NzbI64, NzbInt, NzbStr, ServerStat, StatList,
};
use crate::status::{
    CleanupStatus, DeleteStatus, DirectRenameStatus, DirectUnpackStatus, DupStatus, DupeMode,
    MarkStatus, MoveStatus, NzbParStatus, PostStage, ScriptStatus, UnpackStatus, UrlStatus,
};
use std::ffi::{c_char, c_int, c_void, CStr};

mod live_namespace;

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
pub(crate) use crate::log::{MK_INFO, MK_WARNING, MK_ERROR, MK_DEBUG, MK_DETAIL};

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

// NzbInfo enums
pub(crate) const NK_NZB: i32 = 0;
pub(crate) const NK_URL: i32 = 1;

pub(crate) const RS_NONE: i32 = 0;


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
pub(crate) const NZB_INT_ID: NzbInt = NzbInt::Id;
pub(crate) const NZB_INT_DELETE_STATUS: NzbInt = NzbInt::DeleteStatus;
pub(crate) const NZB_INT_DELETE_PAUSED: NzbInt = NzbInt::DeletePaused;
pub(crate) const NZB_INT_MARK_STATUS: NzbInt = NzbInt::MarkStatus;
pub(crate) const NZB_INT_DUPE_HINT: NzbInt = NzbInt::DupeHint;
pub(crate) const NZB_INT_DUPE_SCORE: NzbInt = NzbInt::DupeScore;
pub(crate) const NZB_INT_PRIORITY: NzbInt = NzbInt::Priority;
pub(crate) const NZB_INT_FULL_HASH: NzbInt = NzbInt::FullHash;
pub(crate) const NZB_INT_FILTERED_HASH: NzbInt = NzbInt::FilteredHash;
pub(crate) const NZB_INT_CHANGED: NzbInt = NzbInt::Changed;
pub(crate) const NZB_INT_ALL_FIRST: NzbInt = NzbInt::AllFirst;
pub(crate) const NZB_INT_HEALTH_PAUSED: NzbInt = NzbInt::HealthPaused;
pub(crate) const NZB_INT_DEAD_PICK_PROBED: NzbInt = NzbInt::DeadPickProbed;
pub(crate) const NZB_INT_DUPE_PAR_DEFER_STATE: NzbInt = NzbInt::DupeParDeferState;
pub(crate) const NZB_INT_DUPE_NO_PAR_NOTED: NzbInt = NzbInt::DupeNoParNoted;
pub(crate) const NZB_INT_DIRECT_RENAME_STATUS: NzbInt = NzbInt::DirectRenameStatus;
pub(crate) const NZB_INT_ACTIVE_DOWNLOADS: NzbInt = NzbInt::ActiveDownloads;
pub(crate) const NZB_INT_CUR_SUCCESS_ARTICLES: NzbInt = NzbInt::CurSuccessArticles;
pub(crate) const NZB_INT_CUR_FAILED_ARTICLES: NzbInt = NzbInt::CurFailedArticles;
pub(crate) const NZB_INT_DUPE_RECOVERED_ARTICLES: NzbInt = NzbInt::DupeRecoveredArticles;
pub(crate) const NZB_INT_DUPE_ATTEMPTED_ARTICLES: NzbInt = NzbInt::DupeAttemptedArticles;
pub(crate) const NZB_INT_DUPE_UNSOURCED_ARTICLES: NzbInt = NzbInt::DupeUnsourcedArticles;
pub(crate) const NZB_INT_DUPE_FAILED_RUN: NzbInt = NzbInt::DupeFailedRun;
pub(crate) const NZB_INT_DUPE_FAILOVER_CHECKED: NzbInt = NzbInt::DupeFailoverChecked;
pub(crate) const NZB_INT_REMAINING_PAR_COUNT: NzbInt = NzbInt::RemainingParCount;
pub(crate) const NZB_INT_WAITING_PAR: NzbInt = NzbInt::WaitingPar;
pub(crate) const NZB_INT_LOADING_PAR: NzbInt = NzbInt::LoadingPar;
pub(crate) const NZB_INT_DELETING: NzbInt = NzbInt::Deleting;
pub(crate) const NZB_INT_PARKING: NzbInt = NzbInt::Parking;
pub(crate) const NZB_INT_AVOID_HISTORY: NzbInt = NzbInt::AvoidHistory;

pub(crate) const NZB_I64_REMAINING_SIZE: NzbI64 = NzbI64::RemainingSize;
pub(crate) const NZB_I64_PAUSED_SIZE: NzbI64 = NzbI64::PausedSize;
pub(crate) const NZB_I64_CUR_SUCCESS_SIZE: NzbI64 = NzbI64::CurSuccessSize;
pub(crate) const NZB_I64_CUR_FAILED_SIZE: NzbI64 = NzbI64::CurFailedSize;
pub(crate) const NZB_I64_PAR_CUR_SUCCESS_SIZE: NzbI64 = NzbI64::ParCurSuccessSize;
pub(crate) const NZB_I64_PAR_CUR_FAILED_SIZE: NzbI64 = NzbI64::ParCurFailedSize;
pub(crate) const NZB_I64_DOWNLOADED_SIZE: NzbI64 = NzbI64::DownloadedSize;
pub(crate) const NZB_I64_PAR_SIZE: NzbI64 = NzbI64::ParSize;
pub(crate) const NZB_I64_PAR_FAILED_SIZE: NzbI64 = NzbI64::ParFailedSize;
pub(crate) const NZB_I64_DUPE_RECOVERED_BYTES: NzbI64 = NzbI64::DupeRecoveredBytes;

pub(crate) const NZB_STR_NAME: NzbStr = NzbStr::Name;
pub(crate) const NZB_STR_FILENAME: NzbStr = NzbStr::Filename;
pub(crate) const NZB_STR_CATEGORY: NzbStr = NzbStr::Category;
pub(crate) const NZB_STR_DEST_DIR: NzbStr = NzbStr::DestDir;
pub(crate) const NZB_STR_DUPE_KEY: NzbStr = NzbStr::DupeKey;
pub(crate) const NZB_STR_QUEUED_FILENAME: NzbStr = NzbStr::QueuedFilename;
pub(crate) const NZB_STR_HARD_LINK_PATH: NzbStr = NzbStr::HardLinkPath;
pub(crate) const NZB_I64_FAILED_SIZE: NzbI64 = NzbI64::FailedSize;
pub(crate) const NZB_I64_SUCCESS_SIZE: NzbI64 = NzbI64::SuccessSize;
pub(crate) const NZB_I64_PAR_SUCCESS_SIZE: NzbI64 = NzbI64::ParSuccessSize;

pub(crate) const NZB_ACT_BUILD_DEST_DIR: NzbAction = NzbAction::BuildDestDir;
pub(crate) const NZB_ACT_UPDATE_MIN_MAX_TIME: NzbAction = NzbAction::UpdateMinMaxTime;
pub(crate) const NZB_ACT_ENTER_POST_PROCESS: NzbAction = NzbAction::EnterPostProcess;
pub(crate) const NZB_ACT_LEAVE_POST_PROCESS: NzbAction = NzbAction::LeavePostProcess;
pub(crate) const NZB_ACT_CLEAR_SCRIPT_STATUSES: NzbAction = NzbAction::ClearScriptStatuses;
pub(crate) const NZB_ACT_DISCONNECT_THREADS: NzbAction = NzbAction::DisconnectThreads;
pub(crate) const NZB_ACT_CLEAR_LIVE_REPAIR_THREAD: NzbAction = NzbAction::ClearLiveRepairThread;
pub(crate) const NZB_STR_FINAL_DIR: NzbStr = NzbStr::FinalDir;

pub(crate) const NZB_INT_CLEANUP_DISK: NzbInt = NzbInt::CleanupDisk;
pub(crate) const NZB_INT_DUPE_MODE: NzbInt = NzbInt::DupeMode;
pub(crate) const NZB_INT_PAR_STATUS: NzbInt = NzbInt::ParStatus;
pub(crate) const NZB_INT_UNPACK_STATUS: NzbInt = NzbInt::UnpackStatus;
pub(crate) const NZB_INT_DIRECT_UNPACK_STATUS: NzbInt = NzbInt::DirectUnpackStatus;
pub(crate) const NZB_INT_POST_TOTAL_SEC: NzbInt = NzbInt::PostTotalSec;
pub(crate) const NZB_INT_URL_STATUS: NzbInt = NzbInt::UrlStatus;
pub(crate) const NZB_INT_PAR_SEC: NzbInt = NzbInt::ParSec;
pub(crate) const NZB_INT_PAR_FULL: NzbInt = NzbInt::ParFull;
pub(crate) const NZB_INT_REPAIR_SEC: NzbInt = NzbInt::RepairSec;
pub(crate) const NZB_INT_EXTRA_PAR_BLOCKS: NzbInt = NzbInt::ExtraParBlocks;
pub(crate) const NZB_INT_DUPE_RECOVERED_HOLES: NzbInt = NzbInt::DupeRecoveredHoles;
pub(crate) const NZB_INT_PAR_RENAME_STATUS: NzbInt = NzbInt::ParRenameStatus;
pub(crate) const NZB_INT_UNPACK_CLEANED_UP_DISK: NzbInt = NzbInt::UnpackCleanedUpDisk;
pub(crate) const NZB_INT_UNPACK_SEC: NzbInt = NzbInt::UnpackSec;
/// The part of `UnpackSec` spent while still downloading (direct unpack), which
/// `PostTotalSec` does not include.
pub(crate) const NZB_INT_LIVE_UNPACK_SEC: NzbInt = NzbInt::LiveUnpackSec;
/// The part of `ParSec` spent while still downloading (live PAR), likewise.
pub(crate) const NZB_INT_LIVE_PAR_SEC: NzbInt = NzbInt::LiveParSec;
pub(crate) const NZB_INT_MOVE_STATUS: NzbInt = NzbInt::MoveStatus;
pub(crate) const NZB_INT_CLEANUP_STATUS: NzbInt = NzbInt::CleanupStatus;
pub(crate) const NZB_INT_RAR_RENAME_STATUS: NzbInt = NzbInt::RarRenameStatus;
pub(crate) const NZB_INT_POST_DOWNLOAD_RENAMING_STATUS: NzbInt = NzbInt::PostDownloadRenamingStatus;
pub(crate) const NZB_INT_POST_UNPACK_RENAMING_STATUS: NzbInt = NzbInt::PostUnpackRenamingStatus;
pub(crate) const NZB_ACT_CLEAR_UNPACK_THREAD: NzbAction = NzbAction::ClearUnpackThread;
pub(crate) const NZB_INT_KIND: NzbInt = NzbInt::Kind;
pub(crate) const NZB_INT_FEED_ID: NzbInt = NzbInt::FeedId;
pub(crate) const NZB_INT_ADD_URL_PAUSED: NzbInt = NzbInt::AddUrlPaused;
pub(crate) const NZB_INT_AUTO_CATEGORY: NzbInt = NzbInt::AutoCategory;
pub(crate) const NZB_INT_DESIRED_SERVER_ID: NzbInt = NzbInt::DesiredServerId;
pub(crate) const NZB_INT_SKIP_SCRIPT_PROCESSING: NzbInt = NzbInt::SkipScriptProcessing;
pub(crate) const NZB_INT_SKIP_DISK_WRITE: NzbInt = NzbInt::SkipDiskWrite;
pub(crate) const NZB_INT_EXTRA_PRIORITY: NzbInt = NzbInt::ExtraPriority;
pub(crate) const NZB_INT_MANY_DUPE_FILES: NzbInt = NzbInt::ManyDupeFiles;
pub(crate) const NZB_INT_PARKED_FILE_COUNT: NzbInt = NzbInt::ParkedFileCount;
pub(crate) const NZB_INT_REPROCESS: NzbInt = NzbInt::Reprocess;
pub(crate) const NZB_INT_PAR_REPAIRED: NzbInt = NzbInt::ParRepaired;
pub(crate) const NZB_INT_UNPACK_TRIED: NzbInt = NzbInt::UnpackTried;
pub(crate) const NZB_INT_LAST_UNPACK_STATUS: NzbInt = NzbInt::LastUnpackStatus;
pub(crate) const NZB_ACT_CLEAR_EXTRACTED_ARCHIVES: NzbAction = NzbAction::ClearExtractedArchives;
pub(crate) const NZB_STR_URL: NzbStr = NzbStr::Url;
pub(crate) const NZB_ACT_CLEAR_FILE_LIST: NzbAction = NzbAction::ClearFileList;
pub(crate) const NZB_ACT_UPDATE_CURRENT_STATS: NzbAction = NzbAction::UpdateCurrentStats;
pub(crate) const NZB_INT_DOWNLOAD_SEC: NzbInt = NzbInt::DownloadSec;
pub(crate) const NZB_INT_FAILED_ARTICLES: NzbInt = NzbInt::FailedArticles;
pub(crate) const NZB_INT_SUCCESS_ARTICLES: NzbInt = NzbInt::SuccessArticles;
pub(crate) const NZB_I64_SIZE: NzbI64 = NzbI64::Size;
pub(crate) const NZB_I64_MIN_TIME: NzbI64 = NzbI64::MinTime;
pub(crate) const NZB_I64_MAX_TIME: NzbI64 = NzbI64::MaxTime;
pub(crate) const NZB_INT_FILE_COUNT: NzbInt = NzbInt::FileCount;
pub(crate) const NZB_INT_MESSAGE_COUNT: NzbInt = NzbInt::MessageCount;
pub(crate) const NZB_INT_TOTAL_ARTICLES: NzbInt = NzbInt::TotalArticles;
pub(crate) const NZB_INT_POST_STAGE: NzbInt = NzbInt::PostStage;
pub(crate) const NZB_INT_POST_FORCE_FLAGS: NzbInt = NzbInt::PostForceFlags;
pub(crate) const NZB_ACT_CLEAR_PARAMS: NzbAction = NzbAction::ClearParams;
pub(crate) const NZB_ACT_CLEAR_REPAIR_JOBS: NzbAction = NzbAction::ClearRepairJobs;
pub(crate) const NZB_ACT_CLEAR_COMPLETED_FILES: NzbAction = NzbAction::ClearCompletedFiles;

// FileInfo field indices (op = 42)
pub(crate) const FILE_INT_PAUSED: FileInt = FileInt::Paused;
pub(crate) const FILE_INT_DELETED: FileInt = FileInt::Deleted;
pub(crate) const FILE_INT_DUPE_DELETED: FileInt = FileInt::DupeDeleted;
pub(crate) const FILE_INT_FILENAME_CONFIRMED: FileInt = FileInt::FilenameConfirmed;
pub(crate) const FILE_INT_FIRST_CONTENT: FileInt = FileInt::FirstContent;
pub(crate) const FILE_INT_EXTRA_PRIORITY: FileInt = FileInt::ExtraPriority;
pub(crate) const FILE_INT_FORCE_DIRECT_WRITE: FileInt = FileInt::ForceDirectWrite;
pub(crate) const FILE_INT_OUTPUT_INITIALIZED: FileInt = FileInt::OutputInitialized;
pub(crate) const FILE_INT_PARTIAL_STATE: FileInt = FileInt::PartialState;
pub(crate) const FILE_INT_PARTIAL_CHANGED: FileInt = FileInt::PartialChanged;
pub(crate) const FILE_INT_DUPE_CUTOVER: FileInt = FileInt::DupeCutover;
pub(crate) const FILE_INT_DUPE_NO_SOURCE_LOGGED: FileInt = FileInt::DupeNoSourceLogged;
pub(crate) const FILE_INT_DUPE_LEAD_DONOR_ID: FileInt = FileInt::DupeLeadDonorId;
pub(crate) const FILE_INT_DUPE_LEAD_FAILURES: FileInt = FileInt::DupeLeadFailures;
pub(crate) const FILE_INT_DUPE_LEAD_SWITCHES: FileInt = FileInt::DupeLeadSwitches;
pub(crate) const FILE_INT_DUPE_RECOVERED_ARTICLES: FileInt = FileInt::DupeRecoveredArticles;
pub(crate) const FILE_INT_DUPE_ATTEMPTED_ARTICLES: FileInt = FileInt::DupeAttemptedArticles;
pub(crate) const FILE_INT_COMPLETED_ARTICLES: FileInt = FileInt::CompletedArticles;
pub(crate) const FILE_INT_SUCCESS_ARTICLES: FileInt = FileInt::SuccessArticles;
pub(crate) const FILE_INT_FAILED_ARTICLES: FileInt = FileInt::FailedArticles;
pub(crate) const FILE_INT_ACTIVE_DOWNLOADS: FileInt = FileInt::ActiveDownloads;
pub(crate) const FILE_INT_CACHED_ARTICLES: FileInt = FileInt::CachedArticles;
pub(crate) const FILE_INT_PAR_FILE: FileInt = FileInt::ParFile;

pub(crate) const FILE_I64_REMAINING_SIZE: FileI64 = FileI64::RemainingSize;
pub(crate) const FILE_I64_SUCCESS_SIZE: FileI64 = FileI64::SuccessSize;
pub(crate) const FILE_I64_FAILED_SIZE: FileI64 = FileI64::FailedSize;
pub(crate) const FILE_I64_DECODED_FILE_SIZE: FileI64 = FileI64::DecodedFileSize;

pub(crate) const FILE_STR_FILENAME: FileStr = FileStr::Filename;
pub(crate) const FILE_STR_OUTPUT_FILENAME: FileStr = FileStr::OutputFilename;
pub(crate) const FILE_STR_ORIGNAME: FileStr = FileStr::Origname;
pub(crate) const FILE_STR_HASH16K: FileStr = FileStr::Hash16k;
pub(crate) const FILE_STR_PAR_SET_ID: FileStr = FileStr::ParSetId;
pub(crate) const FILE_STR_HARD_LINK_PATH: FileStr = FileStr::HardLinkPath;

pub(crate) const FILE_ACT_MAKE_VALID_FILENAME: FileAction = FileAction::MakeValidFilename;
pub(crate) const FILE_ACT_CLEAR_ARTICLES: FileAction = FileAction::ClearArticles;
pub(crate) const FILE_ACT_RESET_ARTICLES_NONSERV: FileAction = FileAction::ResetArticlesNonserv;
pub(crate) const FILE_ACT_FAIL_FINISHED_ARTICLES: FileAction = FileAction::FailFinishedArticles;
pub(crate) const FILE_STR_SUBJECT: FileStr = FileStr::Subject;
pub(crate) const FILE_I64_TIME: FileI64 = FileI64::Time;
pub(crate) const FILE_I64_SIZE: FileI64 = FileI64::Size;
pub(crate) const FILE_I64_MISSED_SIZE: FileI64 = FileI64::MissedSize;
pub(crate) const FILE_INT_TOTAL_ARTICLES: FileInt = FileInt::TotalArticles;
pub(crate) const FILE_INT_MISSED_ARTICLES: FileInt = FileInt::MissedArticles;

// ArticleDownloader actions (op = 51)
pub(crate) const AD_ACT_STOP: i32 = 1;
pub(crate) const AD_ACT_LOG_DEBUG: i32 = 2;
pub(crate) const AD_ACT_ADD_SERVER_STATS: i32 = 3;
pub(crate) const AD_ACT_COMPLETE_FILE_PARTS: i32 = 5;
pub(crate) const AD_ACT_CANCEL_HANGING: i32 = 6;

// PostInfo field indices (op = 60, a1 = 1)
pub(crate) const POST_INT_STAGE: PostInt = PostInt::Stage;
pub(crate) const POST_INT_WORKING: PostInt = PostInt::Working;
pub(crate) const POST_INT_DELETED: PostInt = PostInt::Deleted;
pub(crate) const POST_INT_REQUEST_PAR_CHECK: PostInt = PostInt::RequestParCheck;
pub(crate) const POST_INT_FORCE_PAR_FULL: PostInt = PostInt::ForceParFull;
pub(crate) const POST_INT_PAR_CANNOT_COVER: PostInt = PostInt::ParCannotCover;
pub(crate) const POST_INT_NEED_PAR_CHECK: PostInt = PostInt::NeedParCheck;
pub(crate) const POST_INT_NEED_UNPACK: PostInt = PostInt::NeedUnpack;
pub(crate) const POST_INT_FORCE_REPAIR: PostInt = PostInt::ForceRepair;
pub(crate) const POST_INT_PAR_REPAIRED: PostInt = PostInt::ParRepaired;
pub(crate) const POST_INT_UNPACK_TRIED: PostInt = PostInt::UnpackTried;
pub(crate) const POST_INT_PASS_LIST_TRIED: PostInt = PostInt::PassListTried;
pub(crate) const POST_INT_LAST_UNPACK_STATUS: PostInt = PostInt::LastUnpackStatus;
pub(crate) const POST_INT_FILE_PROGRESS: PostInt = PostInt::FileProgress;
pub(crate) const POST_INT_STAGE_PROGRESS: PostInt = PostInt::StageProgress;
pub(crate) const POST_INT_STREAM_REPAIR_DONE: PostInt = PostInt::StreamRepairDone;
pub(crate) const POST_INT_STREAM_REPAIR_RECOVERED: PostInt = PostInt::StreamRepairRecovered;

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

/// A download's post-processing job (`PostInfo`) in the queue, or none.
///
/// Wraps the queue's object address so it can't be mixed up with a download, a file or a
/// plain number; only the queue host turns it back into an address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct PostRef(usize);

impl PostRef {
    /// No post-processing job.
    pub const NONE: Self = Self(0);

    pub(crate) fn from_addr(addr: usize) -> Self {
        Self(addr)
    }

    pub(crate) fn addr(self) -> usize {
        self.0
    }

    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    pub fn is_some(self) -> bool {
        self.0 != 0
    }
}

/// A file (`FileInfo`) of a download in the queue, or none.
///
/// Wraps the queue's object address like [`PostRef`]; only the queue host turns it back
/// into an address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub(crate) struct FileRef(usize);

impl FileRef {
    /// No file.
    pub const NONE: Self = Self(0);

    pub(crate) fn from_addr(addr: usize) -> Self {
        Self(addr)
    }

    pub(crate) fn addr(self) -> usize {
        self.0
    }

    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    pub fn is_some(self) -> bool {
        self.0 != 0
    }
}

/// Typed handle for a queued (or history) NZB job.
///
/// Wraps the queue's object address like [`PostRef`]; only the queue host turns it back
/// into an address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub(crate) struct NzbRef(usize);

impl NzbRef {
    /// No NZB.
    pub const NONE: Self = Self(0);

    pub(crate) fn from_addr(addr: usize) -> Self {
        Self(addr)
    }

    pub(crate) fn addr(self) -> usize {
        self.0
    }

    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    pub fn is_some(self) -> bool {
        self.0 != 0
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NzbSnap {
    pub ptr: NzbRef,
    pub post_info_ptr: PostRef,
    pub id: i32,
    pub kind: i32,
    pub dupe_mode: DupeMode,
    pub dupe_score: i32,
    pub dupe_hint: i32,
    pub delete_status: DeleteStatus,
    pub mark_status: MarkStatus,
    pub par_status: NzbParStatus,
    pub direct_rename_status: DirectRenameStatus,
    pub direct_unpack_status: DirectUnpackStatus,
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
    pub post_stage: PostStage,
    pub has_live_repair_thread: bool,
    pub live_repair_thread_ptr: usize,
    pub has_unpack_thread: bool,
    pub has_stream_repair_jobs: bool,
    pub url_status: UrlStatus,
    pub par_sec: i32,
    pub repair_sec: i32,
    pub unpack_sec: i32,
    pub live_unpack_sec: i32,
    pub live_par_sec: i32,
    pub reprocess: bool,
    pub extra_par_blocks: i32,
    pub dupe_recovered_holes: i32,
    pub script_total_status: ScriptStatus,
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
    pub last_unpack_status: UnpackStatus,
    pub message_count: i32,
    pub cached_message_count: i32,
    pub post_total_sec: i32,
    pub par_rename_status: i32,
    pub rar_rename_status: i32,
    pub unpack_status: UnpackStatus,
    pub cleanup_status: CleanupStatus,
    pub move_status: MoveStatus,
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
    pub ptr: FileRef,
    pub nzb_ptr: NzbRef,
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
    pub decoded_file_size: i64,
    pub failed_size: i64,
    pub missed_size: i64,
    pub steps_hash: u64,
    pub filename: Vec<u8>,
    pub holes: Vec<crate::dupefallback::NzbgetRsByteRange>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PostSnap {
    pub ptr: PostRef,
    pub nzb_ptr: NzbRef,
    pub stage: PostStage,
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
    pub last_unpack_status: UnpackStatus,
    pub stage_progress: i32,
    pub file_progress: i32,
    pub start_time: i64,
    pub stage_time: i64,
    pub progress_label: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DupSnap {
    pub dupe_mode: DupeMode,
    pub dupe_score: i32,
    pub status: DupStatus,
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

impl HistEntrySnap {
    /// The NZB this entry holds, or [`NzbRef::NONE`] for a duplicate record.
    pub fn nzb(&self) -> NzbRef {
        if self.kind == HK_DUP {
            NzbRef::NONE
        } else {
            NzbRef::from_addr(self.inner_ptr)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RunningDownloadSnap {
    pub ad_ptr: usize,
    pub file_ptr: FileRef,
    pub status: i32,
    pub last_update_time: i64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ActiveDownloadSnap {
    pub file_ptr: FileRef,
    pub article_ptr: usize,
    pub status: i32,
    pub has_content_analyzer: bool,
    pub analyzed_par_file: bool,
    pub decoded_file_size: i64,
    pub downloaded_size: i64,
    pub has_article_filename: bool,
    pub article_filename: Vec<u8>,
}

/// The queue host. The daemon's (`use_active`) answers every op in Rust from
/// the live queue objects; a test mock (`new_mock`) sends every op to its
/// callback instead.
#[derive(Clone, Copy)]
pub(crate) struct QueueSubsysHost {
    pub ctx: *mut c_void,
    pub cb: Option<NzbgetRsQueueSubsysHostFn>,
    pub use_active: bool,
}

/// The optional arguments of a queue-object op, built from `OpArgs::NONE`
/// (all 0 and null) with the setters.
#[derive(Clone, Copy)]
pub(crate) struct OpArgs {
    a2: i32,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
}

impl OpArgs {
    pub(crate) const NONE: Self = Self {
        a2: 0,
        p2: 0,
        s1: std::ptr::null(),
        s2: std::ptr::null(),
        ints: std::ptr::null_mut(),
        i64s: std::ptr::null_mut(),
        strs: std::ptr::null_mut(),
    };

    pub(crate) const fn a2(self, a2: i32) -> Self {
        Self { a2, ..self }
    }

    pub(crate) const fn p2(self, p2: usize) -> Self {
        Self { p2, ..self }
    }

    pub(crate) const fn s1(self, s1: *const c_char) -> Self {
        Self { s1, ..self }
    }

    pub(crate) const fn s2(self, s2: *const c_char) -> Self {
        Self { s2, ..self }
    }

    pub(crate) const fn ints(self, ints: *mut c_int) -> Self {
        Self { ints, ..self }
    }

    pub(crate) const fn i64s(self, i64s: *mut i64) -> Self {
        Self { i64s, ..self }
    }

    pub(crate) const fn strs(self, strs: *mut *const c_char) -> Self {
        Self { strs, ..self }
    }
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
    /// The daemon's host: every op is answered in Rust.
    #[inline]
    pub fn active() -> Self {
        Self {
            ctx: std::ptr::null_mut(),
            cb: None,
            use_active: true,
        }
    }

    #[cfg(test)]
    #[inline]
    pub fn new_mock(ctx: *mut c_void, cb: NzbgetRsQueueSubsysHostFn) -> Self {
        Self {
            ctx,
            cb: Some(cb),
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
        // the daemon's host never gets here: its wrappers call Rust directly
        match self.cb {
            Some(cb) => unsafe { cb(self.ctx, op, a1, a2, p1, p2, s1, s2, ints, i64s, strs) },
            None => 0,
        }
    }

    // Queue-object ops. The daemon's host runs them on the live Rust objects;
    // no test mock answers them, so on a mock host they do nothing and read
    // as 0 (output arrays untouched), as the mock callbacks did.

    fn coll_snapshot(&self, nzb: NzbRef, which: i32, x: OpArgs) -> usize {
        if self.use_active {
            let nzb = nzb.addr() as *const crate::qnzb::NzbScalars;
            unsafe { crate::qnzb::host_coll_snapshot(nzb, which, x.ints, x.i64s, x.strs) };
        }
        0
    }

    fn filelist_op(&self, nzb: NzbRef, op: i32, x: OpArgs) -> usize {
        if !self.use_active {
            return 0;
        }
        let nzb = nzb.addr() as *mut crate::qnzb::NzbScalars;
        unsafe {
            crate::qnzb::host_filelist_op(nzb, op, x.a2, x.p2, x.s1, x.s2, x.ints, x.i64s, x.strs)
        }
    }

    /// A duplicate record's op, or (op 2) a history entry's summary.
    fn record_op(&self, record: usize, op: i32, x: OpArgs) -> usize {
        if !self.use_active {
            return 0;
        }
        debug_assert_eq!(op, 2);
        let hist = record as *const crate::qhist::Hist;
        unsafe { crate::qhist::host_hist_summary(hist, x.ints, x.strs) }
    }

    /// An article op: on file `p1`'s article list (with the article in p2
    /// for 8 and 13) for the file ops, else on article `p1`.
    fn article_op(&self, p1: usize, op: i32, x: OpArgs) -> usize {
        if !self.use_active {
            return 0;
        }
        let file_op = matches!(op, 0 | 7 | 8 | 10 | 11 | 12 | 13);
        let (list, article) = if file_op {
            let list = unsafe { crate::qfile::nzbget_rs_file_articles(p1 as *const _) };
            (list, if op == 8 || op == 13 { x.p2 } else { 0 })
        } else {
            (std::ptr::null_mut(), p1)
        };
        let article = article as *mut crate::qarticle::Article;
        unsafe {
            crate::qarticle::nzbget_rs_article_host(
                list, article, op, x.a2, x.p2, x.s1, x.ints, x.i64s, x.strs,
            )
        }
    }

    fn cache_op(&self, file: FileRef, op: i32, x: OpArgs) -> usize {
        if !self.use_active {
            return 0;
        }
        let file = file.addr() as *const crate::qfile::File;
        unsafe { crate::qfile::host_cache_op(file, op, x.a2, x.p2, x.s1, x.ints, x.i64s, x.strs) }
    }

    /// The article writer's and downloader's op `op` on a file.
    pub(crate) fn output_op(&self, file: FileRef, op: i32, x: OpArgs) -> usize {
        if !self.use_active {
            return 0;
        }
        let file = file.addr() as *const crate::qfile::File;
        unsafe {
            crate::qfile::host_output_op(file, op, x.a2, x.p2, x.s1, x.s2, x.ints, x.i64s, x.strs)
        }
    }

    fn post_snapshot(&self, post: PostRef, x: OpArgs) {
        if !self.use_active {
            return;
        }
        let post = post.addr() as *const crate::qnzb::Post;
        unsafe { crate::qnzb::host_post_snapshot(post, x.ints, x.i64s, x.strs) };
    }

    fn post_job_op(&self, op: i32, p1: usize, x: OpArgs) -> usize {
        if !self.use_active {
            return 0;
        }
        unsafe { crate::qnzb::host_post_job_op(op, x.a2, p1, x.p2, x.i64s) }
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
                    queue_dir: o.str_bytes(OptStr::QueueDir).to_vec(),
                    par_ignore_ext: o.str_bytes(OptStr::ParIgnoreExt).to_vec(),
                    ext_cleanup_disk: o.str_bytes(OptStr::ExtCleanupDisk).to_vec(),
                    inter_dir: o.str_bytes(OptStr::InterDir).to_vec(),
                    hard_linking_ignore_ext: o.str_bytes(OptStr::HardLinkingIgnoreExt).to_vec(),
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
        }
    }

    pub fn nzb_print_message(&self, nzb_ptr: NzbRef, kind: i32, msg: &[u8]) {
        if nzb_ptr.is_none() {
            return;
        }
        if self.use_active {
            unsafe { crate::qqueue::nzb_message(nzb_ptr.addr(), kind, false, msg) };
        }
    }

    pub fn nzb_add_message(&self, nzb_ptr: NzbRef, kind: i32, msg: &[u8]) {
        if nzb_ptr.is_none() {
            return;
        }
        if self.use_active {
            unsafe { crate::qqueue::nzb_message(nzb_ptr.addr(), kind, true, msg) };
        }
    }

    pub fn sleep_ms(&self, ms: i32) {
        if self.use_active {
            if ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(ms as u64));
            }
        }
    }

    pub fn set_standby_mode(&self, standby: bool) {
        if self.use_active {
            let _ = standby;
        }
    }

    pub fn stat_meter_load(&self) -> (bool, bool) {
        #[cfg(unix)]
        if self.use_active {
            let mut perfect = true;
            let loaded = crate::statmeter::load_active_statmeter(&mut perfect);
            return (loaded, perfect);
        }
        (true, true)
    }

    pub fn stat_meter_save(&self) {
        #[cfg(unix)]
        if self.use_active {
            crate::statmeter::save_active_statmeter();
        }
    }

    pub fn stat_meter_interval_check(&self) {
        #[cfg(unix)]
        if self.use_active {
            crate::statmeter::interval_check_active_statmeter();
        }
    }

    pub fn stat_meter_enter_leave_standby(&self, enter: bool) {
        #[cfg(unix)]
        if self.use_active {
            crate::statmeter::enter_leave_standby_active_statmeter(enter);
        }
    }

    pub fn stat_meter_add_zero_speed(&self) {
        #[cfg(unix)]
        if self.use_active {
            crate::statmeter::add_speed_reading_active_statmeter(0);
        }
    }

    pub fn log_interval_check(&self) {
        if self.use_active {
            if let Some(lg) = crate::log::active_log() {
                lg.interval_check_active();
            }
        }
    }

    pub fn work_state_set_downloading(&self, downloading: bool) {
        if self.use_active {
            if let Some(ws) = crate::ffi::active_workstate() {
                ws.set_downloading(downloading);
            }
        }
    }

    pub fn effective_downloads_limit(&self, limit: i32) -> i32 {
        if self.use_active {
            return crate::articledl::memory_monitor_effective_downloads_limit(limit);
        }
        0
    }

    pub fn work_state_set_temp_pause_download(&self, pause: bool) {
        if self.use_active {
            if let Some(ws) = crate::ffi::active_workstate() {
                ws.set_temp_pause_download(pause);
            }
        }
    }

    /// Returns the `Unpack` setting of category `cat_name`, if the category exists.
    pub fn find_category(&self, cat_name: &[u8]) -> Option<bool> {
        if self.use_active {
            let c = c_bytes(cat_name);
            let cs = unsafe { std::ffi::CStr::from_ptr(c.as_ptr() as *const c_char) };
            return crate::options::active_options()
                .and_then(|o| o.find_category(cs, false).map(|cat| cat.unpack));
        }
        None
    }

    pub fn dq_is_loaded(&self) -> bool {
        if self.use_active {
            return crate::ffi::is_queue_loaded();
        }
        false
    }

    pub fn server_pool_generation(&self) -> i32 {
        #[cfg(unix)]
        if let Some(pool) = crate::net::active_server_pool() {
            return pool.generation();
        }
        0
    }

    pub fn server_pool_close_unused(&self) {
        #[cfg(unix)]
        if let Some(pool) = crate::net::active_server_pool() {
            pool.close_unused_connections();
        }
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
        (Vec::new(), Vec::new(), Vec::new())
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
        0
    }

    // DiskState ops (op = 5)
    pub fn disk_queue_exists(&self) -> bool {
        #[cfg(unix)]
        if self.use_active {
            return crate::diskstate_format::active_disk_queue_exists();
        }
        false
    }

    pub fn disk_load_queue(&self, dq_ptr: usize) -> bool {
        #[cfg(unix)]
        if self.use_active {
            return crate::diskstate_format::active_disk_load_queue(self, dq_ptr);
        }
        false
    }

    pub fn disk_discard_queue(&self) {
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_discard_queue();
        }
    }

    pub fn disk_legacy_queue_read(&self) -> bool {
        #[cfg(unix)]
        if self.use_active {
            return crate::diskstate_format::active_legacy_queue_read();
        }
        false
    }

    pub fn disk_cleanup_temp_dir(&self, dq_ptr: usize) {
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_cleanup_temp_dir(self, dq_ptr);
        }
    }

    pub fn disk_save_all_file_infos(&self, dq_ptr: usize) {
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_save_all_file_infos(self, dq_ptr);
        }
    }

    pub fn disk_discard_files(&self, nzb_ptr: NzbRef) {
        self.disk_discard_files_ex(nzb_ptr, true);
    }

    pub fn disk_discard_files_ex(&self, nzb_ptr: NzbRef, delete_log: bool) {
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_discard_files(self, nzb_ptr, delete_log);
        }
    }

    pub fn disk_discard_file(&self, file_id: i32, del_data: bool, del_part: bool, del_compl: bool) {
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_discard_file(
                file_id, del_data, del_part, del_compl,
            );
        }
    }

    pub fn disk_save_file(&self, file_ptr: FileRef) {
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_save_file(self, file_ptr, false);
        }
    }

    pub fn disk_save_file_state(&self, file_ptr: FileRef, completed: bool) {
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_save_file_state(self, file_ptr, completed, false);
        }
    }

    pub fn disk_load_file_state(&self, file_ptr: FileRef, completed: bool) -> bool {
        #[cfg(unix)]
        if self.use_active {
            return crate::diskstate_format::active_disk_load_file_state(self, file_ptr, completed);
        }
        false
    }

    pub fn disk_resave_completed_file_state(&self, file_id: i32) {
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_resave_completed_file_state(self, file_id);
        }
    }

    pub fn disk_load_articles(&self, file_ptr: FileRef) {
        #[cfg(unix)]
        if self.use_active {
            crate::diskstate_format::active_disk_load_file(self, file_ptr, false, true);
        }
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
        0
    }

    pub fn disk_load_file_for_inspect(
        &self,
        file_id: i32,
        load_info_data: bool,
        try_load_completed_state: bool,
    ) -> (FileRef, bool) {
        #[cfg(unix)]
        if self.use_active {
            return crate::diskstate_format::active_disk_load_file_for_inspect(
                self,
                file_id,
                load_info_data,
                try_load_completed_state,
            );
        }
        (FileRef::NONE, load_info_data)
    }

    // Subsystems (op = 6)
    pub fn history_delete_disk_files(&self, nzb_ptr: NzbRef) {
        #[cfg(unix)]
        if self.use_active {
            crate::historycoord::delete_disk_files_active(nzb_ptr);
        }
    }

    pub fn history_redownload(&self, dq_ptr: usize, hi_ptr: usize) {
        #[cfg(unix)]
        if self.use_active {
            crate::historycoord::redownload_active(dq_ptr, hi_ptr);
        }
    }

    pub fn history_hide(&self, dq_ptr: usize, hi_ptr: usize, rindex: i32) {
        #[cfg(unix)]
        if self.use_active {
            crate::historycoord::history_hide_active(dq_ptr, hi_ptr, rindex);
        }
    }

    pub fn prepost_check_download_completed(&self, dq_ptr: usize, nzb_ptr: NzbRef) {
        if self.use_active {
            if let Some(pp) = crate::ffi::active_prepost() {
                pp.check_download_completed(self, dq_ptr, nzb_ptr);
            }
        }
    }

    pub fn stream_repair_start_live(&self, nzb_ptr: NzbRef) {
        if nzb_ptr.is_none() {
            return;
        }
        #[cfg(unix)]
        if self.use_active && crate::dupefallback::should_defer_to_par_host(self, nzb_ptr) {
            return;
        }
        if self.use_active {
            unsafe { crate::qqueue::start_live_repair(nzb_ptr.addr()) };
        }
    }

    pub fn move_completed_files(&self, nzb_ptr: NzbRef, old_dest_dir: &[u8]) -> bool {
        #[cfg(unix)]
        if self.use_active {
            return crate::articledl::move_completed_files_on_qh(self, nzb_ptr, old_dest_dir);
        }
        false
    }

    pub fn history_add_to_history(&self, dq_ptr: usize, nzb_ptr: NzbRef) {
        #[cfg(unix)]
        if self.use_active {
            crate::historycoord::add_to_history_active(dq_ptr, nzb_ptr);
        }
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
        false
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
        false
    }

    pub fn url_delete_queue_entry(
        &self,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
        avoid_history: bool,
    ) -> bool {
        if self.use_active {
            return crate::urlcoord::active_url_coord()
                .is_some_and(|uc| uc.delete_queue_entry_active(dq_ptr, nzb_ptr, avoid_history));
        }
        false
    }

    pub fn dupe_search_notify_name_changed(&self, nzb_id: i32) {
        #[cfg(unix)]
        if self.use_active {
            crate::dupesearch_coord::notify_active_dupesearch_name_changed(nzb_id);
        }
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
        }
    }

    pub fn scanner_add_url_file(
        &self,
        nzb_ptr: NzbRef,
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
        0
    }

    // DownloadQueue & CoordinatorDownloadQueue ops (op = 20..=25)
    pub fn with_locked_queue<R, F: FnOnce(usize) -> R>(&self, f: F) -> R {
        if self.use_active {
            return crate::qqueue::with_current_locked(f);
        }
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
        if self.use_active {
            return crate::qqueue::queue_len(dq_ptr);
        }
        0
    }

    pub fn queue_at(&self, dq_ptr: usize, idx: usize) -> NzbRef {
        if self.use_active {
            return NzbRef::from_addr(crate::qqueue::queue_at(dq_ptr, idx));
        }
        NzbRef::NONE
    }

    pub fn queue_find_by_id(&self, dq_ptr: usize, nzb_id: i32) -> NzbRef {
        if self.use_active {
            return NzbRef::from_addr(crate::qqueue::queue_find_by_id(dq_ptr, nzb_id));
        }
        NzbRef::NONE
    }

    pub fn queue_remove(&self, dq_ptr: usize, nzb_ptr: NzbRef) {
        if self.use_active {
            crate::qqueue::queue_remove(dq_ptr, nzb_ptr.addr());
        }
    }

    pub fn queue_insert_owned(
        &self,
        dq_ptr: usize,
        uptr_addr: usize,
        url_ptr: NzbRef,
        add_first: bool,
    ) {
        if self.use_active {
            unsafe {
                crate::qqueue::queue_insert_handle(dq_ptr, uptr_addr, url_ptr.addr(), add_first)
            };
        }
    }

    pub fn alloc_new_nzb_uptr(&self) -> (usize, NzbRef) {
        if self.use_active {
            let (handle, nzb) = crate::qqueue::new_nzb_handle();
            return (handle, NzbRef::from_addr(nzb));
        }
        (0, NzbRef::NONE)
    }

    pub fn finish_new_nzb_uptr(&self, dq_ptr: usize, uptr_addr: usize) {
        if self.use_active {
            unsafe { crate::qqueue::queue_add_handle(dq_ptr, uptr_addr) };
        }
    }

    pub fn queue_reorder(&self, dq_ptr: usize, new_order: &[NzbRef]) {
        if dq_ptr == 0 || new_order.is_empty() {
            return;
        }
        if self.use_active {
            let addrs: Vec<usize> = new_order.iter().map(|nzb| nzb.addr()).collect();
            crate::qqueue::queue_reorder(dq_ptr, &addrs);
        }
    }

    pub fn destroy_nzb_uptr(&self, uptr_addr: usize) {
        if uptr_addr == 0 {
            return;
        }
        if self.use_active {
            unsafe { crate::qqueue::free_nzb_handle(uptr_addr) };
        }
    }

    pub fn id_gen_last(&self) -> (i32, i32) {
        if self.use_active {
            return crate::qqueue::id_gen_last();
        }
        (0, 0)
    }

    pub fn id_gen_raise_and_reset(&self, nzb_id: i32, file_id: i32) {
        if self.use_active {
            crate::qqueue::id_gen_raise_and_reset(nzb_id, file_id);
        }
    }

    pub fn history_push_nzb_uptr(&self, dq_ptr: usize, uptr_addr: usize, hist_time: i32) {
        if self.use_active {
            unsafe { crate::qqueue::history_push_handle(dq_ptr, uptr_addr, hist_time) };
        }
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
        if self.use_active {
            let text = |p: *const c_char| {
                if p.is_null() {
                    c""
                } else {
                    unsafe { CStr::from_ptr(p) }
                }
            };
            crate::qqueue::history_push_dup(
                dq_ptr,
                &crate::qqueue::DupRecord {
                    id,
                    time: hist_time,
                    status,
                    full_hash,
                    filtered_hash: filt_hash,
                    dupe_score,
                    dupe_mode,
                    size,
                    name: text(name),
                    dupe_key: text(dupe_key),
                },
            );
        }
    }

    pub fn history_move_queue_item(&self, dq_ptr: usize, nzb_ptr: NzbRef) {
        if self.use_active {
            crate::qqueue::history_move_queue_item(dq_ptr, nzb_ptr.addr());
        }
    }

    pub fn history_replace_with_dup(
        &self,
        dq_ptr: usize,
        hi_ptr: usize,
        rindex: usize,
        dup_status: DupStatus,
    ) {
        if self.use_active {
            unsafe {
                crate::qqueue::history_replace_with_dup(
                    dq_ptr,
                    hi_ptr,
                    rindex as i32,
                    dup_status.into(),
                )
            };
        }
    }

    pub fn history_erase_at(&self, dq_ptr: usize, idx: usize) {
        if self.use_active {
            crate::qqueue::history_erase_at(dq_ptr, idx as i32);
        }
    }

    pub fn history_return_to_queue(&self, dq_ptr: usize, hi_ptr: usize, idx: usize) -> NzbRef {
        if self.use_active {
            let nzb = unsafe { crate::qqueue::history_return_to_queue(dq_ptr, hi_ptr, idx as i32) };
            return NzbRef::from_addr(nzb);
        }
        NzbRef::NONE
    }

    pub fn history_len(&self, dq_ptr: usize) -> usize {
        if self.use_active {
            return crate::qqueue::history_len(dq_ptr);
        }
        0
    }

    /// The download with `nzb_id`, in the queue or (once it is done) in history; 0 if neither.
    pub fn nzb_find_in_queue_or_history(&self, dq_ptr: usize, nzb_id: i32) -> NzbRef {
        let np = self.queue_find_by_id(dq_ptr, nzb_id);
        if np.is_some() {
            return np;
        }
        let hi_ptr = self.history_find_by_id(dq_ptr, nzb_id);
        if hi_ptr == 0 {
            return NzbRef::NONE;
        }
        // a URL that failed to fetch or scan keeps its messages on its NzbInfo too
        let hi = self.hist_item_snap(hi_ptr);
        if hi.kind == HK_NZB || hi.kind == HK_URL {
            NzbRef::from_addr(hi.inner_ptr)
        } else {
            NzbRef::NONE
        }
    }

    pub fn history_find_by_id(&self, dq_ptr: usize, id: i32) -> usize {
        if self.use_active {
            return crate::qqueue::history_find_by_id(dq_ptr, id);
        }
        0
    }

    pub fn history_at(&self, dq_ptr: usize, idx: usize) -> HistEntrySnap {
        if self.use_active {
            return crate::qqueue::history_at(dq_ptr, idx).map_or_else(
                HistEntrySnap::default,
                |e| HistEntrySnap {
                    hi_ptr: e.hi,
                    id: e.id,
                    kind: e.kind,
                    inner_ptr: e.inner,
                    time: e.time,
                    name: unsafe { read_cstr(e.name) },
                },
            );
        }
        HistEntrySnap::default()
    }

    pub fn dq_save(&self, dq_ptr: usize) {
        #[cfg(unix)]
        if self.use_active {
            if let Some(qc) = crate::ffi::active_queue_coord() {
                qc.dq_save(self, dq_ptr);
            }
        }
    }

    pub fn dq_save_changed(&self, dq_ptr: usize) {
        #[cfg(unix)]
        if self.use_active {
            if let Some(qc) = crate::ffi::active_queue_coord() {
                qc.dq_save_changed(self, dq_ptr);
            }
        }
    }

    pub fn dq_edit_entry(&self, dq_ptr: usize, id: i32, action: i32, args: Option<&[u8]>) -> bool {
        #[cfg(unix)]
        if self.use_active {
            return crate::ffi::active_queue_coord()
                .is_some_and(|qc| qc.dq_edit_entry(self, dq_ptr, id, action, args));
        }
        false
    }

    pub fn dq_edit_list_single_id(
        &self,
        dq_ptr: usize,
        id: i32,
        action: i32,
        args: Option<&[u8]>,
    ) -> bool {
        #[cfg(unix)]
        if self.use_active {
            let ids = [id];
            return crate::ffi::active_queue_coord().is_some_and(|qc| {
                qc.dq_edit_list(self, dq_ptr, Some(&ids), None, 1, action, args)
            });
        }
        false
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
        #[cfg(unix)]
        if self.use_active {
            return crate::ffi::active_queue_coord().is_some_and(|qc| {
                qc.dq_edit_list(self, dq_ptr, ids, names, match_mode, action, Some(args))
            });
        }
        false
    }

    /// Tells post-processing that the queue changed (`aspect_action` on
    /// `nzb_ptr`, and `file_ptr` for a file aspect).
    pub fn prepost_queue_changed(
        &self,
        dq_ptr: usize,
        aspect_action: i32,
        nzb_ptr: NzbRef,
        file_ptr: FileRef,
    ) {
        if self.use_active {
            crate::jobthread::nzbget_rs_prepost_queue_changed(
                aspect_action,
                dq_ptr,
                nzb_ptr.addr(),
                file_ptr.addr(),
            );
        }
    }

    pub fn dq_notify(&self, dq_ptr: usize, aspect_action: i32, nzb_ptr: NzbRef, file_ptr: FileRef) {
        self.prepost_queue_changed(dq_ptr, aspect_action, nzb_ptr, file_ptr);
        #[cfg(unix)]
        if self.use_active {
            let url_opt = if aspect_action == ASPECT_URL_COMPLETED && nzb_ptr.is_some() {
                Some(self.nzb_snap(nzb_ptr, false).url)
            } else {
                None
            };
            crate::feed::notify_active_feed_queue_aspect(aspect_action, url_opt.as_deref());
            crate::urlcoord::notify_active_urlcoord_queue_update(aspect_action);
            if aspect_action == ASPECT_NZB_ADDED
                && nzb_ptr.is_some()
                && !self.nzb_snap(nzb_ptr, false).dupe_key.is_empty()
            {
                crate::twin::nudge_and_wake_active();
            }
            if nzb_ptr.is_some()
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
        #[cfg(unix)]
        if self.use_active {
            let mut rem = 0i64;
            let mut rem_forced = 0i64;
            let qlen = self.queue_len(dq_ptr);
            for i in 0..qlen {
                let nzb_ptr = self.queue_at(dq_ptr, i);
                let mut n_ints = [0i32; 90];
                let mut n_i64s = [0i64; 21];
                self.coll_snapshot(
                    nzb_ptr,
                    0,
                    OpArgs::NONE
                        .ints(n_ints.as_mut_ptr())
                        .i64s(n_i64s.as_mut_ptr()),
                );
                let force = n_ints[14] != 0;
                let flen = self.nzb_file_len(nzb_ptr);
                for j in 0..flen {
                    let fp = self.nzb_file_at(nzb_ptr, j);
                    let mut f_ints = [0i32; 28];
                    let mut f_i64s = [0i64; 8];
                    unsafe {
                        crate::qfile::host_snapshot(
                            fp.addr() as *const crate::qfile::File,
                            &mut f_ints,
                            &mut f_i64s,
                            None,
                        )
                    };
                    if f_ints[1] == 0 && f_ints[2] == 0 {
                        rem += f_i64s[1];
                        if force {
                            rem_forced += f_i64s[1];
                        }
                    }
                }
            }
            return (rem, rem_forced);
        }
        (0, 0)
    }

    pub fn dq_history_changed(&self, _dq_ptr: usize) {
        #[cfg(unix)]
        if self.use_active {
            if let Some(qc) = crate::ffi::active_queue_coord() {
                qc.dq_history_changed();
            }
        }
    }

    // CoordinatorDownloadQueue & QueueCoordinator C++ shell ops (op = 25)
    pub fn self_download_queue_ptr(&self) -> usize {
        if self.use_active {
            return crate::qqueue::daemon_queue();
        }
        0
    }

    pub fn coord_dq_init(&self) {
        if self.use_active {
            crate::qqueue::daemon_queue_init();
        }
    }

    pub fn coord_dq_loaded(&self) {
        if self.use_active {
            crate::qqueue::daemon_queue_loaded();
        }
    }

    pub fn coord_dq_final(&self) {
        if self.use_active {
            crate::qqueue::daemon_queue_final();
        }
    }

    pub fn coord_dq_state_changed(&self) -> bool {
        #[cfg(unix)]
        if self.use_active {
            return crate::ffi::active_queue_coord().is_some_and(|qc| qc.dq_state_changed());
        }
        false
    }

    pub fn self_set_has_more_jobs(&self, more: bool) {
        #[cfg(unix)]
        if self.use_active {
            if let Some(qc) = crate::ffi::active_queue_coord() {
                qc.set_has_more_jobs(more);
            }
        }
    }

    pub fn self_is_stopped(&self) -> bool {
        #[cfg(unix)]
        if self.use_active {
            return crate::ffi::active_queue_coord().map_or(true, |qc| qc.is_stopped());
        }
        false
    }

    pub fn self_standby_wait(&self, wait_ms: i32) {
        #[cfg(unix)]
        if self.use_active {
            if let Some(qc) = crate::ffi::active_queue_coord() {
                qc.standby_wait(wait_ms);
            }
        }
    }

    // NzbInfo ops (op = 30..=35)
    pub fn nzb_snap(&self, nzb_ptr: NzbRef, with_text_status: bool) -> NzbSnap {
        self.nzb_snap_mode(nzb_ptr, i32::from(with_text_status))
    }

    pub fn nzb_snap_mode(&self, nzb_ptr: NzbRef, text_status_mode: i32) -> NzbSnap {
        if nzb_ptr.is_none() {
            return NzbSnap::default();
        }
        let mut ints = [0i32; 90];
        let mut i64s = [0i64; 21];
        let mut strs = [std::ptr::null(); 9];
        self.coll_snapshot(
            nzb_ptr,
            text_status_mode,
            OpArgs::NONE
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr())
                .strs(strs.as_mut_ptr()),
        );
        unsafe {
            NzbSnap {
                ptr: nzb_ptr,
                post_info_ptr: PostRef::from_addr(i64s[18] as usize),
                id: ints[0],
                kind: ints[1],
                dupe_mode: DupeMode::from(ints[2]),
                dupe_score: ints[3],
                dupe_hint: ints[4],
                delete_status: DeleteStatus::from(ints[5]),
                mark_status: MarkStatus::from(ints[6]),
                par_status: NzbParStatus::from(ints[7]),
                direct_rename_status: DirectRenameStatus::from(ints[8]),
                direct_unpack_status: DirectUnpackStatus::from(ints[9]),
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
                post_stage: PostStage::from(ints[27]),
                has_live_repair_thread: ints[28] != 0,
                live_repair_thread_ptr: i64s[19] as usize,
                has_unpack_thread: ints[68] != 0,
                has_stream_repair_jobs: ints[69] != 0,
                url_status: UrlStatus::from(ints[70]),
                par_sec: ints[71],
                repair_sec: ints[72],
                unpack_sec: ints[73],
                live_unpack_sec: ints[88],
                live_par_sec: ints[89],
                reprocess: ints[74] != 0,
                extra_par_blocks: ints[75],
                dupe_recovered_holes: ints[76],
                script_total_status: ScriptStatus::from(ints[77]),
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
                last_unpack_status: UnpackStatus::from(ints[86]),
                message_count: ints[82],
                cached_message_count: ints[87],
                post_total_sec: ints[60],
                par_rename_status: ints[61],
                rar_rename_status: ints[62],
                unpack_status: UnpackStatus::from(ints[63]),
                cleanup_status: CleanupStatus::from(ints[64]),
                move_status: MoveStatus::from(ints[65]),
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

    pub fn nzb_get_param(&self, nzb_ptr: NzbRef, name: &[u8]) -> Option<Vec<u8>> {
        if !self.use_active {
            return None;
        }
        crate::qnzb::param(nzb_ptr, name)
    }

    pub fn nzb_set_param(&self, nzb_ptr: NzbRef, name: &[u8], val: Option<&[u8]>) {
        if self.use_active {
            crate::qnzb::set_param(nzb_ptr, name, val);
        }
    }

    pub fn nzb_copy_params(&self, dest_nzb: NzbRef, src_nzb: NzbRef) {
        if self.use_active {
            crate::qnzb::copy_params(dest_nzb, src_nzb);
        }
    }

    /// All post-processing parameters as (name, value), in the order they were added.
    pub fn nzb_params(&self, nzb_ptr: NzbRef) -> Vec<(Vec<u8>, Vec<u8>)> {
        if !self.use_active {
            return Vec::new();
        }
        crate::qnzb::params(nzb_ptr)
    }

    pub fn nzb_build_final_dir_name(&self, nzb_ptr: NzbRef) -> Vec<u8> {
        if !self.use_active {
            return Vec::new();
        }
        crate::qnzb::build_final_dir_name(nzb_ptr)
    }

    /// Whether `nzb_ptr` must not borrow from donor `donor_id` again.
    pub fn nzb_donor_blocked(&self, nzb_ptr: NzbRef, donor_id: i32) -> bool {
        self.use_active && crate::qnzb::donor_blocked(nzb_ptr, donor_id)
    }

    /// Bars `nzb_ptr` from borrowing from donor `donor_id` again.
    pub fn nzb_block_donor(&self, nzb_ptr: NzbRef, donor_id: i32) {
        if self.use_active {
            crate::qnzb::block_donor(nzb_ptr, donor_id);
        }
    }

    pub fn nzb_hard_link_path(&self, nzb_ptr: NzbRef) -> Vec<u8> {
        if !self.use_active {
            return Vec::new();
        }
        crate::qnzb::hard_link_path(nzb_ptr)
    }

    pub fn nzb_meta_name(&self, nzb_ptr: NzbRef) -> Vec<u8> {
        if !self.use_active {
            return Vec::new();
        }
        crate::qnzb::meta_name(nzb_ptr)
    }

    pub fn nzb_has_busy_output_file(&self, nzb_ptr: NzbRef) -> bool {
        self.use_active && crate::qnzb::has_busy_output_file(nzb_ptr)
    }

    /// The download's all-time article counts per server.
    pub fn nzb_server_stats(&self, nzb_ptr: NzbRef) -> Vec<ServerStat> {
        if !self.use_active {
            return Vec::new();
        }
        crate::qnzb::server_stats(nzb_ptr, StatList::Total)
    }

    /// The download's article counts per server since it was last loaded.
    pub fn nzb_current_server_stats(&self, nzb_ptr: NzbRef) -> Vec<ServerStat> {
        if !self.use_active {
            return Vec::new();
        }
        crate::qnzb::server_stats(nzb_ptr, StatList::Current)
    }

    /// The cached messages from id `id_from` on when it is positive, else
    /// the newest `max_entries` when that is positive, else none.
    pub fn nzb_cached_messages_slice(
        &self,
        nzb_ptr: NzbRef,
        id_from: i32,
        max_entries: i32,
    ) -> Vec<(i32, i32, i32, std::ffi::CString)> {
        if !self.use_active {
            return Vec::new();
        }
        let range = if let Ok(id @ 1..) = u32::try_from(id_from) {
            MessageRange::FromId(id)
        } else if let Ok(n @ 1..) = usize::try_from(max_entries) {
            MessageRange::Last(n)
        } else {
            return Vec::new();
        };
        crate::qnzb::cached_messages(nzb_ptr, range)
            .into_iter()
            .map(|m| (m.id as i32, m.kind, m.time as i32, m.text))
            .collect()
    }

    pub fn nzb_push_script_status(&self, nzb_ptr: NzbRef, name: &[u8], status: ScriptStatus) {
        if self.use_active {
            crate::qnzb::push_script_status(nzb_ptr, name, status);
        }
    }

    pub fn nzb_rename_completed_file(
        &self,
        nzb_ptr: NzbRef,
        old_name: &[u8],
        new_name: &[u8],
    ) -> bool {
        self.use_active && crate::qnzb::rename_completed_file(nzb_ptr, old_name, new_name)
    }

    pub fn nzb_unpack_thread_ptr(&self, nzb_ptr: NzbRef) -> usize {
        if !self.use_active {
            return 0;
        }
        crate::qnzb::unpack_thread(nzb_ptr)
    }

    pub fn nzb_push_extracted_archive(&self, nzb_ptr: NzbRef, name: &[u8]) {
        if self.use_active {
            crate::qnzb::push_extracted_archive(nzb_ptr, name);
        }
    }

    pub fn nzb_set_meta_name(&self, nzb_ptr: NzbRef, meta_name: &[u8]) {
        if self.use_active {
            crate::qnzb::set_meta_name(nzb_ptr, meta_name);
        }
    }

    pub fn nzb_copy_cached_messages_silent(&self, dst_nzb: NzbRef, src_nzb: NzbRef) {
        if self.use_active {
            crate::qnzb::copy_cached_messages(dst_nzb, src_nzb);
        }
    }

    pub fn nzb_make_nice_url_name(&self, nzb_ptr: NzbRef, url: &[u8], filename: &[u8]) -> Vec<u8> {
        if !self.use_active || nzb_ptr.is_none() {
            return Vec::new();
        }
        let url = c_bytes(url);
        let url = CStr::from_bytes_with_nul(&url).ok();
        crate::downloadinfo::make_nice_url_name(url, filename)
    }

    /// The post-processing scripts that ran, as (script name, status), in run order.
    pub fn nzb_script_statuses(&self, nzb_ptr: NzbRef) -> Vec<(Vec<u8>, ScriptStatus)> {
        if !self.use_active {
            return Vec::new();
        }
        crate::qnzb::script_statuses(nzb_ptr)
    }

    /// The archives unpacked so far, in unpack order.
    pub fn nzb_extracted_archives(&self, nzb_ptr: NzbRef) -> Vec<Vec<u8>> {
        if !self.use_active {
            return Vec::new();
        }
        crate::qnzb::extracted_archives(nzb_ptr)
    }

    pub fn nzb_server_stat_set(&self, nzb_ptr: NzbRef, server_id: i32, succ: i32, fail: i32) {
        if self.use_active {
            let stat = ServerStat {
                server_id,
                success: succ,
                failed: fail,
            };
            crate::qnzb::set_server_stat(nzb_ptr, stat);
        }
    }

    pub fn nzb_sync_current_server_stats(&self, nzb_ptr: NzbRef) {
        if self.use_active {
            crate::qnzb::sync_current_server_stats(nzb_ptr);
        }
    }

    pub fn nzb_set_int(&self, nzb_ptr: NzbRef, field: NzbInt, val: i32) {
        if self.use_active {
            crate::qnzb::set_int(nzb_ptr, field, val);
        }
    }

    pub fn nzb_set_par_status(&self, nzb_ptr: NzbRef, status: NzbParStatus) {
        self.nzb_set_int(nzb_ptr, NZB_INT_PAR_STATUS, status.into());
    }

    pub fn nzb_set_unpack_status(&self, nzb_ptr: NzbRef, status: UnpackStatus) {
        self.nzb_set_int(nzb_ptr, NZB_INT_UNPACK_STATUS, status.into());
    }

    pub fn nzb_set_move_status(&self, nzb_ptr: NzbRef, status: MoveStatus) {
        self.nzb_set_int(nzb_ptr, NZB_INT_MOVE_STATUS, status.into());
    }

    pub fn nzb_set_cleanup_status(&self, nzb_ptr: NzbRef, status: CleanupStatus) {
        self.nzb_set_int(nzb_ptr, NZB_INT_CLEANUP_STATUS, status.into());
    }

    pub fn nzb_set_last_unpack_status(&self, nzb_ptr: NzbRef, status: UnpackStatus) {
        self.nzb_set_int(nzb_ptr, NZB_INT_LAST_UNPACK_STATUS, status.into());
    }

    pub fn nzb_set_dupe_mode(&self, nzb_ptr: NzbRef, mode: DupeMode) {
        self.nzb_set_int(nzb_ptr, NZB_INT_DUPE_MODE, mode.into());
    }

    pub fn nzb_set_mark_status(&self, nzb_ptr: NzbRef, status: MarkStatus) {
        self.nzb_set_int(nzb_ptr, NZB_INT_MARK_STATUS, status.into());
    }

    pub fn nzb_set_url_status(&self, nzb_ptr: NzbRef, status: UrlStatus) {
        self.nzb_set_int(nzb_ptr, NZB_INT_URL_STATUS, status.into());
    }

    pub fn nzb_set_delete_status(&self, nzb_ptr: NzbRef, status: DeleteStatus) {
        self.nzb_set_int(nzb_ptr, NZB_INT_DELETE_STATUS, status.into());
    }

    pub fn nzb_set_direct_rename_status(&self, nzb_ptr: NzbRef, status: DirectRenameStatus) {
        self.nzb_set_int(nzb_ptr, NZB_INT_DIRECT_RENAME_STATUS, status.into());
    }

    pub fn nzb_set_direct_unpack_status(&self, nzb_ptr: NzbRef, status: DirectUnpackStatus) {
        self.nzb_set_int(nzb_ptr, NZB_INT_DIRECT_UNPACK_STATUS, status.into());
    }

    pub fn nzb_set_i64(&self, nzb_ptr: NzbRef, field: NzbI64, val: i64) {
        if self.use_active {
            crate::qnzb::set_i64(nzb_ptr, field, val);
        }
    }

    pub fn nzb_set_str(&self, nzb_ptr: NzbRef, field: NzbStr, val: &[u8]) {
        self.nzb_set_str_opt(nzb_ptr, field, Some(val));
    }

    pub fn nzb_set_str_opt(&self, nzb_ptr: NzbRef, field: NzbStr, val: Option<&[u8]>) {
        if self.use_active {
            crate::qnzb::set_str(nzb_ptr, field, val);
        }
    }

    pub fn nzb_action(&self, nzb_ptr: NzbRef, action: NzbAction) {
        if self.use_active {
            crate::qnzb::act(nzb_ptr, action);
        }
    }

    /// Adds `file`'s sizes and article counts to the download's completed totals.
    pub fn nzb_update_completed_stats(&self, nzb_ptr: NzbRef, file: FileRef) {
        if self.use_active {
            crate::qnzb::add_file_totals(nzb_ptr, file, FileTotals::Completed);
        }
    }

    /// Adds `file`'s sizes and article counts to the download's deleted totals.
    pub fn nzb_update_deleted_stats(&self, nzb_ptr: NzbRef, file: FileRef) {
        if self.use_active {
            crate::qnzb::add_file_totals(nzb_ptr, file, FileTotals::Deleted);
        }
    }

    pub fn nzb_apply_merge_split_counters(
        &self,
        nzb_ptr: NzbRef,
        c: &crate::queuecoord::NzbgetRsNzbMergeSplitCounters,
    ) {
        if !self.use_active {
            return;
        }
        let ints = [
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
        let i64s = [
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
        // the collection is the Rust object itself
        let nzb = nzb_ptr.addr() as *mut crate::qnzb::NzbScalars;
        unsafe { crate::qnzb::host_set_stats(nzb, &ints, &i64s) };
    }

    pub fn nzb_file_len(&self, nzb_ptr: NzbRef) -> usize {
        self.filelist_op(nzb_ptr, 0, OpArgs::NONE)
    }

    pub fn nzb_file_at(&self, nzb_ptr: NzbRef, idx: usize) -> FileRef {
        FileRef::from_addr(self.filelist_op(nzb_ptr, 1, OpArgs::NONE.a2(idx as i32)))
    }

    pub fn nzb_remove_file(&self, nzb_ptr: NzbRef, file_ptr: FileRef) {
        self.filelist_op(nzb_ptr, 2, OpArgs::NONE.p2(file_ptr.addr()));
    }

    pub fn nzb_remove_file_and_notify(
        &self,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
        file_ptr: FileRef,
        aspect_action: i32,
    ) {
        let mut i64s = [dq_ptr as i64];
        self.filelist_op(
            nzb_ptr,
            9,
            OpArgs::NONE
                .a2(aspect_action)
                .p2(file_ptr.addr())
                .i64s(i64s.as_mut_ptr()),
        );
        #[cfg(unix)]
        if self.use_active {
            crate::feed::notify_active_feed_queue_aspect(aspect_action, None);
            crate::urlcoord::notify_active_urlcoord_queue_update(aspect_action);
        }
    }

    pub fn nzb_transfer_split_file(&self, src_nzb: NzbRef, dest_nzb: NzbRef, file_ptr: FileRef) {
        let mut i64s = [dest_nzb.addr() as i64];
        self.filelist_op(
            src_nzb,
            3,
            OpArgs::NONE.p2(file_ptr.addr()).i64s(i64s.as_mut_ptr()),
        );
    }

    pub fn nzb_merge_lists_and_stats(&self, dest_nzb: NzbRef, src_nzb: NzbRef) {
        self.filelist_op(dest_nzb, 4, OpArgs::NONE.p2(src_nzb.addr()));
    }

    pub fn nzb_completed_files_len(&self, nzb_ptr: NzbRef) -> usize {
        self.filelist_op(nzb_ptr, 5, OpArgs::NONE.a2(-1))
    }

    pub fn nzb_completed_file_at(&self, nzb_ptr: NzbRef, idx: usize) -> CompletedFileSnap {
        let mut ints = [0i32; 4];
        let mut strs = [std::ptr::null(); 4];
        self.filelist_op(
            nzb_ptr,
            5,
            OpArgs::NONE
                .a2(idx as i32)
                .ints(ints.as_mut_ptr())
                .strs(strs.as_mut_ptr()),
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

    pub fn nzb_completed_files(&self, nzb_ptr: NzbRef) -> Vec<CompletedFileSnap> {
        let n = self.nzb_completed_files_len(nzb_ptr);
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            out.push(self.nzb_completed_file_at(nzb_ptr, i));
        }
        out
    }

    pub fn nzb_push_completed_file(
        &self,
        nzb_ptr: NzbRef,
        file_ptr: FileRef,
        filename: &[u8],
        file_status: i32,
        file_crc: u32,
    ) {
        let c = c_bytes(filename);
        let mut ints = [file_crc as i32];
        self.filelist_op(
            nzb_ptr,
            6,
            OpArgs::NONE
                .a2(file_status)
                .p2(file_ptr.addr())
                .s1(c.as_ptr() as *const c_char)
                .ints(ints.as_mut_ptr()),
        );
    }

    pub fn nzb_update_completed_file(
        &self,
        nzb_ptr: NzbRef,
        idx: usize,
        filename: &[u8],
        origname: Option<&[u8]>,
    ) {
        let c_fn = c_bytes(filename);
        let c_orig = origname.map(c_bytes);
        let s2 = c_orig
            .as_ref()
            .map_or(std::ptr::null(), |v| v.as_ptr() as *const c_char);
        self.filelist_op(
            nzb_ptr,
            7,
            OpArgs::NONE
                .a2(idx as i32)
                .s1(c_fn.as_ptr() as *const c_char)
                .s2(s2),
        );
    }

    pub fn nzb_push_stream_repair_job(
        &self,
        nzb_ptr: NzbRef,
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
        self.filelist_op(
            nzb_ptr,
            8,
            OpArgs::NONE
                .a2(holes.len() as i32)
                .p2(if holes.is_empty() {
                    0
                } else {
                    holes.as_ptr() as usize
                })
                .s1(c.as_ptr() as *const c_char)
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr()),
        );
    }

    pub fn nzb_reorder_files(&self, nzb_ptr: NzbRef, new_order: &[FileRef]) {
        if nzb_ptr.is_none() || new_order.is_empty() {
            return;
        }
        self.filelist_op(
            nzb_ptr,
            10,
            OpArgs::NONE
                .a2(new_order.len() as i32)
                .p2(new_order.as_ptr() as usize),
        );
    }

    pub fn nzb_stream_repair_jobs(&self, nzb_ptr: NzbRef) -> Vec<StreamRepairJobSnap> {
        if nzb_ptr.is_none() {
            return Vec::new();
        }
        let n = self.filelist_op(nzb_ptr, 11, OpArgs::NONE.a2(-1));
        let mut out = Vec::with_capacity(n);
        let mut ints = [0i32; 1];
        let mut i64s = [0i64; 3];
        let mut strs = [std::ptr::null(); 1];
        for i in 0..n {
            self.filelist_op(
                nzb_ptr,
                11,
                OpArgs::NONE
                    .a2(i as i32)
                    .ints(ints.as_mut_ptr())
                    .i64s(i64s.as_mut_ptr())
                    .strs(strs.as_mut_ptr()),
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

    pub fn nzb_stream_repair_jobs_len(&self, nzb_ptr: NzbRef) -> usize {
        if nzb_ptr.is_none() {
            return 0;
        }
        self.filelist_op(nzb_ptr, 11, OpArgs::NONE.a2(-1))
    }

    pub fn nzb_stream_repair_update_completed_file(
        &self,
        nzb_ptr: NzbRef,
        file_id: i32,
        mode: i32,
    ) {
        let mut ints = [mode];
        if self.filelist_op(
            nzb_ptr,
            12,
            OpArgs::NONE.a2(file_id).ints(ints.as_mut_ptr()),
        ) != 0
        {
            self.disk_discard_file(file_id, true, true, true);
        }
    }

    pub fn nzb_stream_repair_job_detail(
        &self,
        nzb_ptr: NzbRef,
        idx: usize,
    ) -> StreamRepairJobDetailSnap {
        let mut ints = [0i32; 5];
        let mut i64s = [0i64; 4];
        let mut strs = [std::ptr::null(); 2];
        self.filelist_op(
            nzb_ptr,
            13,
            OpArgs::NONE
                .a2(idx as i32)
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr())
                .strs(strs.as_mut_ptr()),
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
            decoded_file_size: i64s[0],
            failed_size: i64s[1],
            missed_size: i64s[2],
            steps_hash: i64s[3] as u64,
            filename: unsafe { read_cstr(strs[0]) },
            holes,
        }
    }

    pub fn nzb_stream_repair_mark_live_attempted(&self, nzb_ptr: NzbRef, idx: usize) -> bool {
        let mut ints = [1i32];
        self.filelist_op(
            nzb_ptr,
            13,
            OpArgs::NONE.a2(idx as i32).ints(ints.as_mut_ptr()),
        ) != 0
    }

    pub fn nzb_stream_repair_erase_job(&self, nzb_ptr: NzbRef, idx: usize, set_changed: bool) {
        let mut ints = [2i32, i32::from(set_changed)];
        self.filelist_op(
            nzb_ptr,
            13,
            OpArgs::NONE.a2(idx as i32).ints(ints.as_mut_ptr()),
        );
    }

    pub fn nzb_stream_repair_update_job_holes(
        &self,
        nzb_ptr: NzbRef,
        idx: usize,
        new_decoded_size: i64,
        holes_ptr: usize,
        holes_len: i32,
    ) {
        let mut ints = [3i32, holes_len];
        let mut i64s = [new_decoded_size];
        self.filelist_op(
            nzb_ptr,
            13,
            OpArgs::NONE
                .a2(idx as i32)
                .p2(holes_ptr)
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr()),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn nzb_add_parsed_file(
        &self,
        nzb_ptr: NzbRef,
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
    ) -> FileRef {
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
        FileRef::from_addr(self.filelist_op(
            nzb_ptr,
            14,
            OpArgs::NONE
                .s1(subject)
                .s2(filename)
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr())
                .strs(strs.as_mut_ptr()),
        ))
    }

    pub fn nzb_redownload_replace_files(&self, nzb_ptr: NzbRef, src_nzb_ptr: NzbRef) {
        self.filelist_op(nzb_ptr, 15, OpArgs::NONE.p2(src_nzb_ptr.addr()));
    }

    pub fn nzb_erase_completed_file(&self, nzb_ptr: NzbRef, idx: usize) {
        self.filelist_op(nzb_ptr, 16, OpArgs::NONE.a2(idx as i32));
    }

    pub fn nzb_add_requeued_file(&self, nzb_ptr: NzbRef, file_ptr: FileRef) {
        self.filelist_op(nzb_ptr, 17, OpArgs::NONE.p2(file_ptr.addr()));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn nzb_push_completed_file_raw(
        &self,
        nzb_ptr: NzbRef,
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
        self.filelist_op(
            nzb_ptr,
            18,
            OpArgs::NONE.ints(ints.as_mut_ptr()).strs(strs.as_mut_ptr()),
        );
    }

    pub fn nzb_add_stub_file(
        &self,
        nzb_ptr: NzbRef,
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
        self.filelist_op(
            nzb_ptr,
            19,
            OpArgs::NONE
                .a2(i32::from(time_opt.is_some()))
                .ints(ints.as_mut_ptr()),
        );
    }

    pub fn dup_snap(&self, dup_ptr: usize) -> DupSnap {
        if !self.use_active {
            return DupSnap::default();
        }
        unsafe { crate::qdup::Dup::from_addr(dup_ptr) }
            .map_or_else(DupSnap::default, |d| d.snapshot())
    }

    pub fn dup_edit(&self, dup_ptr: usize, edit: crate::qdup::DupEdit<'_>) {
        if !self.use_active {
            return;
        }
        if let Some(d) = unsafe { crate::qdup::Dup::from_addr(dup_ptr) } {
            d.edit(edit);
        }
    }

    pub fn hist_item_snap(&self, hi_ptr: usize) -> HistEntrySnap {
        let mut ints = [0i32; 2];
        let mut strs = [std::ptr::null(); 1];
        let inner_ptr = self.record_op(
            hi_ptr,
            2,
            OpArgs::NONE.ints(ints.as_mut_ptr()).strs(strs.as_mut_ptr()),
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
    pub fn file_snap(&self, file_ptr: FileRef) -> FileSnap {
        if file_ptr.is_none() {
            return FileSnap::default();
        }
        let mut ints = [0i32; 28];
        let mut i64s = [0i64; 8];
        if self.use_active {
            let mut texts: [Vec<u8>; 7] = Default::default();
            let articles_len = unsafe {
                crate::qfile::host_snapshot(
                    file_ptr.addr() as *const crate::qfile::File,
                    &mut ints,
                    &mut i64s,
                    Some(&mut texts),
                )
            };
            return file_snap_from(file_ptr, &ints, &i64s, articles_len, texts);
        }
        file_snap_from(file_ptr, &ints, &i64s, 0, Default::default())
    }

    pub fn file_groups(&self, file_ptr: FileRef) -> Vec<Vec<u8>> {
        if self.use_active {
            return unsafe { crate::qfile::groups(file_ptr.addr() as *const crate::qfile::File) };
        }
        Vec::new()
    }

    pub fn file_server_stats_len(&self, file_ptr: FileRef) -> usize {
        if file_ptr.is_none() {
            return 0;
        }
        if self.use_active {
            return unsafe {
                crate::qfile::server_stat_count(file_ptr.addr() as *const crate::qfile::File)
            };
        }
        0
    }

    pub fn file_server_stat_at(&self, file_ptr: FileRef, idx: usize) -> (i32, i32, i32) {
        if self.use_active {
            let [id, ok, bad] =
                unsafe { crate::qfile::server_stat_at(file_ptr.addr() as *const crate::qfile::File, idx) }
                    .unwrap_or([0; 3]);
            return (id, ok, bad);
        }
        (0, 0, 0)
    }

    pub fn file_server_stat_set(&self, file_ptr: FileRef, server_id: i32, succ: i32, fail: i32) {
        if self.use_active {
            unsafe {
                crate::qfile::set_server_stat(
                    file_ptr.addr() as *const crate::qfile::File,
                    server_id,
                    succ,
                    fail,
                )
            };
        }
    }

    pub fn file_push_group(&self, file_ptr: FileRef, group: *const c_char) {
        if self.use_active {
            let group = if group.is_null() {
                c""
            } else {
                unsafe { CStr::from_ptr(group) }
            };
            unsafe { crate::qfile::push_group(file_ptr.addr() as *const crate::qfile::File, group) };
        }
    }

    pub fn file_set_int(&self, file_ptr: FileRef, field: FileInt, val: i32) {
        if self.use_active {
            crate::qfile::set_int(file_ptr, field, val);
        }
    }

    pub fn file_set_i64(&self, file_ptr: FileRef, field: FileI64, val: i64) {
        if self.use_active {
            crate::qfile::set_i64(file_ptr, field, val);
        }
    }

    pub fn file_set_str(&self, file_ptr: FileRef, field: FileStr, val: Option<&[u8]>) {
        if self.use_active {
            crate::qfile::set_str(file_ptr, field, val);
        }
    }

    pub fn file_action(&self, file_ptr: FileRef, action: FileAction) {
        if self.use_active {
            crate::qfile::act(file_ptr, action);
        }
    }

    /// Subtracts the file's server stats from the download's totals and
    /// current-session counts, and clears the file's own list.
    pub fn file_subtract_and_clear_server_stats(&self, file_ptr: FileRef, nzb_ptr: NzbRef) {
        if self.use_active {
            crate::qfile::subtract_server_stats(file_ptr, nzb_ptr, true);
        }
    }

    /// Deletes the temporary result files of the file's articles that are
    /// not running: all of them when `all`, else only dupe-fallback ones.
    pub fn file_discard_temp_articles(&self, file_ptr: FileRef, all: bool) {
        if self.use_active {
            crate::qfile::discard_temp_articles(file_ptr, all);
        }
    }

    pub fn alloc_file_id(&self) -> i32 {
        if self.use_active {
            return crate::qfile::alloc_file_id();
        }
        0
    }

    pub fn delete_file_info(&self, file_ptr: FileRef) {
        if file_ptr.is_none() {
            return;
        }
        if self.use_active {
            unsafe { crate::qfile::nzbget_rs_file_free(file_ptr.addr() as *mut crate::qfile::File) };
        }
    }

    pub fn file_set_nzb_info(&self, file_ptr: FileRef, nzb_ptr: NzbRef) {
        if self.use_active {
            crate::qfile::set_nzb(file_ptr, nzb_ptr);
        }
    }

    pub fn file_subtract_server_stats_from_nzb_total(&self, file_ptr: FileRef, nzb_ptr: NzbRef) {
        if self.use_active {
            crate::qfile::subtract_server_stats(file_ptr, nzb_ptr, false);
        }
    }

    pub fn file_new_standalone(&self, id: i32, bump_gen: bool) -> FileRef {
        if self.use_active {
            return FileRef::from_addr(crate::qfile::new_standalone(id, bump_gen) as usize);
        }
        FileRef::NONE
    }

    pub fn file_article_raw_at(&self, file_ptr: FileRef, idx: usize) -> (ArticleSnap, bool) {
        let mut ints = [0i32; 7];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 2];
        self.article_op(
            file_ptr.addr(),
            10,
            OpArgs::NONE
                .a2(idx as i32)
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr())
                .strs(strs.as_mut_ptr()),
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
        file_ptr: FileRef,
        part_number: i32,
        size: i32,
        message_id: *const c_char,
    ) {
        self.article_op(
            file_ptr.addr(),
            11,
            OpArgs::NONE
                .a2(part_number)
                .p2(size as usize)
                .s1(message_id),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn file_article_disk_op(
        &self,
        file_ptr: FileRef,
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
        self.article_op(
            file_ptr.addr(),
            12,
            OpArgs::NONE
                .a2(idx)
                .p2(usize::from(apply))
                .s1(result_filename)
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr()),
        ) as i32
    }

    pub fn article_at(&self, file_ptr: FileRef, idx: usize) -> ArticleSnap {
        let mut ints = [0i32; 12];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 2];
        let ptr = self.article_op(
            file_ptr.addr(),
            0,
            OpArgs::NONE
                .a2(idx as i32)
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr())
                .strs(strs.as_mut_ptr()),
        );
        Self::decode_article_snap(ptr, &ints, &i64s, &strs)
    }

    pub fn article_snap(&self, article_ptr: usize) -> ArticleSnap {
        let mut ints = [0i32; 12];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 2];
        let ptr = self.article_op(
            article_ptr,
            1,
            OpArgs::NONE
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr())
                .strs(strs.as_mut_ptr()),
        );
        Self::decode_article_snap(ptr, &ints, &i64s, &strs)
    }

    pub fn file_first_undefined_article(&self, file_ptr: FileRef) -> Option<(usize, ArticleSnap)> {
        let mut ints = [0i32; 12];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 2];
        let ptr = self.article_op(
            file_ptr.addr(),
            7,
            OpArgs::NONE
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr())
                .strs(strs.as_mut_ptr()),
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
        file_ptr: FileRef,
        article_ptr: usize,
    ) -> Option<(usize, ArticleSnap)> {
        let mut ints = [0i32; 12];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null(); 2];
        let ptr = self.article_op(
            file_ptr.addr(),
            8,
            OpArgs::NONE
                .p2(article_ptr)
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr())
                .strs(strs.as_mut_ptr()),
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
        let n = self.article_op(article_ptr, 3, OpArgs::NONE.a2(-1));
        let mut sources = Vec::with_capacity(n);
        let mut donors = Vec::with_capacity(n);
        let mut ints = [0i32; 2];
        let mut strs = [std::ptr::null(); 1];
        for i in 0..n {
            self.article_op(
                article_ptr,
                3,
                OpArgs::NONE
                    .a2(i as i32)
                    .ints(ints.as_mut_ptr())
                    .strs(strs.as_mut_ptr()),
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
        self.article_op(
            article_ptr,
            4,
            OpArgs::NONE
                .a2(c_ptrs.len() as i32)
                .ints(ints.as_mut_ptr())
                .strs(if c_ptrs.is_empty() {
                    std::ptr::null_mut()
                } else {
                    c_ptrs.as_mut_ptr()
                }),
        );
    }

    pub fn article_edit(&self, article_ptr: usize, edit: crate::qarticle::ArticleEdit<'_>) {
        if !self.use_active {
            return;
        }
        if let Some(a) = unsafe { crate::qarticle::Article::from_addr(article_ptr) } {
            a.edit(edit);
        }
    }

    /// Caller holds the queue; dedicated claims include running articles.
    pub fn claim_source_header(
        &self,
        file_ptr: FileRef,
        article_ptr: usize,
        begin: i64,
        end: i64,
    ) -> bool {
        let mut range = [begin, end];
        self.article_op(
            file_ptr.addr(),
            13,
            OpArgs::NONE.p2(article_ptr).i64s(range.as_mut_ptr()),
        ) != 0
    }
    /// Cancel/latch before an accepted source is demoted or its history reset.
    /// This is metadata-only and requires the caller's queue lock.
    pub fn invalidate_source_replacement(&self, file_ptr: FileRef, article_ptr: usize) {
        let claimed = self.article_op(article_ptr, 14, OpArgs::NONE) != 0;
        if claimed {
            let file = self.file_snap(file_ptr);
            let nzb = self.nzb_snap(file.nzb_ptr, false);
            if let Ok(context) = crate::parlive::source_write_context(
                nzb.id,
                file.id,
                crate::fsops::path(&nzb.dest_dir),
                &file.filename,
            ) {
                context.invalidate_identity();
            }
        }
    }

    // ArticleCache & flush lock ops (op = 44)
    pub fn article_cache_file_busy(&self, file_ptr: FileRef) -> bool {
        crate::articledl::cache_file_busy(file_ptr.addr())
    }

    pub fn article_cache_snapshot_segment(
        &self,
        file_ptr: FileRef,
        article_ptr: usize,
        len: usize,
    ) -> Option<Vec<u8>> {
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len];
        let mut i64s = [buf.as_mut_ptr() as usize as i64];
        let copied = crate::articledl::with_cache_content_lock(|| {
            self.cache_op(
                file_ptr,
                1,
                OpArgs::NONE
                    .a2(len as i32)
                    .p2(article_ptr)
                    .i64s(i64s.as_mut_ptr()),
            )
        });
        if copied != 0 {
            Some(buf)
        } else {
            None
        }
    }

    pub fn article_cache_discard_segment(&self, file_ptr: FileRef, article_ptr: usize) {
        crate::articledl::with_cache_content_lock(|| {
            self.cache_op(file_ptr, 2, OpArgs::NONE.p2(article_ptr));
        });
    }

    pub fn file_try_acquire_flush_lock(&self, file_ptr: FileRef) -> bool {
        crate::articledl::with_cache_content_lock(|| {
            self.cache_op(file_ptr, 3, OpArgs::NONE.a2(1)) != 0
        })
    }

    pub fn file_release_flush_lock(&self, file_ptr: FileRef) {
        crate::articledl::with_cache_content_lock(|| {
            self.cache_op(file_ptr, 3, OpArgs::NONE);
        });
    }

    pub fn file_cache_flush_prepare(&self, file_ptr: FileRef) -> (bool, bool, NzbRef, Vec<u8>) {
        let mut ints = [0i32; 2];
        let mut i64s = [0i64; 2];
        let mut strs = [std::ptr::null(); 1];
        self.cache_op(
            file_ptr,
            4,
            OpArgs::NONE
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr())
                .strs(strs.as_mut_ptr()),
        );
        let out_fn = unsafe {
            if strs[0].is_null() || i64s[0] <= 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(strs[0].cast::<u8>(), i64s[0] as usize).to_vec()
            }
        };
        (ints[0] != 0, ints[1] != 0, NzbRef::from_addr(i64s[1] as usize), out_fn)
    }

    pub fn file_cache_flush_lock_collect(
        &self,
        file_ptr: FileRef,
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
            let ok = self.cache_op(
                file_ptr,
                5,
                OpArgs::NONE
                    .p2(
                        (&mut out as *mut Vec<(usize, i64, i32, *const u8, Option<Vec<u8>>)>)
                            as usize,
                    )
                    .s1(push_seg as *const () as *const c_char),
            ) != 0;
            if ok {
                Some(out)
            } else {
                None
            }
        })
    }

    pub fn file_cache_is_deleted_unparked(&self, file_ptr: FileRef) -> bool {
        self.cache_op(file_ptr, 6, OpArgs::NONE) != 0
    }

    pub fn file_cache_discard_article_segment(&self, file_ptr: FileRef, article_ptr: usize) {
        self.cache_op(file_ptr, 6, OpArgs::NONE.a2(1).p2(article_ptr));
    }

    pub fn file_cache_flush_finish(&self, file_ptr: FileRef, flushed_articles: i32) -> bool {
        crate::articledl::with_cache_content_lock(|| {
            let mut ints = [0i32; 1];
            self.cache_op(
                file_ptr,
                7,
                OpArgs::NONE.a2(flushed_articles).ints(ints.as_mut_ptr()),
            );
            ints[0] != 0
        })
    }

    // ActiveDownloads & ArticleDownloader ops (op = 50..=54)
    pub fn active_downloads_len(&self) -> usize {
        #[cfg(unix)]
        if self.use_active {
            return crate::ffi::active_queue_coord().map_or(0, |qc| qc.active_downloads_len());
        }
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
        #[cfg(unix)]
        if self.use_active {
            return crate::ffi::active_queue_coord().map_or(
                RunningDownloadSnap {
                    ad_ptr: 0,
                    file_ptr: FileRef::NONE,
                    status: 0,
                    last_update_time: 0,
                },
                |qc| qc.active_download_at(idx),
            );
        }
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
            file_ptr: FileRef::from_addr(i64s[0] as usize),
            status: ints[0],
            last_update_time: i64s[1],
        }
    }

    pub fn ad_snap(&self, ad_ptr: usize) -> ActiveDownloadSnap {
        #[cfg(unix)]
        if self.use_active {
            return crate::ffi::active_queue_coord()
                .map_or_else(ActiveDownloadSnap::default, |qc| qc.ad_snap(ad_ptr));
        }
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
                file_ptr: FileRef::from_addr(i64s[0] as usize),
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
        #[cfg(unix)]
        if self.use_active {
            return crate::ffi::active_queue_coord()
                .is_some_and(|qc| qc.ad_action(self, ad_ptr, action));
        }
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
        #[cfg(unix)]
        if self.use_active {
            return crate::ffi::active_queue_coord().and_then(|qc| qc.ad_finish_analyzer(ad_ptr));
        }
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
        file_ptr: FileRef,
        article_ptr: usize,
        conn_ptr: usize,
        with_content_analyzer: bool,
        info_name: &[u8],
    ) {
        #[cfg(unix)]
        if self.use_active {
            if let Some(qc) = crate::ffi::active_queue_coord() {
                qc.spawn_article_download(
                    self,
                    file_ptr,
                    article_ptr,
                    conn_ptr,
                    with_content_analyzer,
                    info_name,
                );
            }
        }
    }

    pub fn remove_active_download(&self, ad_ptr: usize) {
        #[cfg(unix)]
        if self.use_active {
            if let Some(qc) = crate::ffi::active_queue_coord() {
                qc.remove_active_download(ad_ptr);
            }
        }
    }

    pub fn delete_all_active_downloads(&self) {
        #[cfg(unix)]
        if self.use_active {
            if let Some(qc) = crate::ffi::active_queue_coord() {
                qc.delete_all_active_downloads();
            }
        }
    }

    // PostInfo & PrePostProcessor ops (op = 60..=62)
    pub fn post_snap(&self, post_ptr: PostRef) -> PostSnap {
        if post_ptr.is_none() {
            return PostSnap::default();
        }
        let mut ints = [0i32; 21];
        let mut i64s = [0i64; 3];
        let mut strs = [std::ptr::null(); 1];
        self.post_snapshot(
            post_ptr,
            OpArgs::NONE
                .ints(ints.as_mut_ptr())
                .i64s(i64s.as_mut_ptr())
                .strs(strs.as_mut_ptr()),
        );
        PostSnap {
            ptr: post_ptr,
            nzb_ptr: NzbRef::from_addr(i64s[1] as usize),
            stage: PostStage::from(ints[0]),
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
            last_unpack_status: UnpackStatus::from(ints[16]),
            stage_progress: ints[19],
            file_progress: ints[20],
            start_time: i64s[0],
            stage_time: i64s[2],
            progress_label: unsafe { read_cstr(strs[0]) },
        }
    }

    pub fn post_set_int(&self, post_ptr: PostRef, field: PostInt, val: i32) {
        if self.use_active {
            crate::qnzb::post_set_int(post_ptr, field, val);
        }
    }

    pub fn post_set_stage(&self, post_ptr: PostRef, stage: PostStage) {
        self.post_set_int(post_ptr, POST_INT_STAGE, stage.into());
    }

    pub fn post_set_start_time(&self, post_ptr: PostRef, start_time: i64) {
        if self.use_active {
            crate::qnzb::post_set_start_time(post_ptr, start_time);
        }
    }

    /// Starts a new stage at `stage_time`, clearing its progress and label.
    pub fn post_reset_stage_progress(&self, post_ptr: PostRef, stage_time: i64) {
        if self.use_active {
            crate::qnzb::post_begin_stage(post_ptr, stage_time);
        }
    }

    pub fn post_set_stage_time(&self, post_ptr: PostRef, stage_time: i64) {
        if self.use_active {
            crate::qnzb::post_set_stage_time(post_ptr, stage_time);
        }
    }

    pub fn post_set_progress_label(&self, post_ptr: PostRef, label: &[u8]) {
        if self.use_active {
            crate::qnzb::post_set_progress_label(post_ptr, label);
        }
    }

    pub fn post_push_parred_file(&self, post_ptr: PostRef, name: &[u8]) {
        if self.use_active {
            crate::qnzb::post_push_parred_file(post_ptr, name);
        }
    }

    /// The files the par check repaired or renamed, in order.
    pub fn post_parred_files(&self, post_ptr: PostRef) -> Vec<Vec<u8>> {
        if !self.use_active {
            return Vec::new();
        }
        crate::qnzb::post_parred_files(post_ptr)
    }

    pub fn post_clear_extracted_archives(&self, post_ptr: PostRef) {
        if self.use_active {
            crate::qnzb::post_clear_extracted_archives(post_ptr);
        }
    }

    /// The archives unpacked so far, in unpack order.
    pub fn post_extracted_archives(&self, post_ptr: PostRef) -> Vec<Vec<u8>> {
        if !self.use_active {
            return Vec::new();
        }
        crate::qnzb::post_extracted_archives(post_ptr)
    }

    pub fn prepost_is_stopped(&self, owner_ptr: usize) -> bool {
        let _ = self;
        crate::jobthread::owner_stopped(owner_ptr)
    }

    pub fn thread_is_stopped(&self, thread_ptr: usize) -> bool {
        let _ = self;
        unsafe { crate::jobthread::nzbget_rs_job_op(thread_ptr as *const _, 1, 0, 0, 0) != 0 }
    }

    pub fn thread_stop(&self, thread_ptr: usize) {
        let _ = self;
        unsafe { crate::jobthread::nzbget_rs_job_op(thread_ptr as *const _, 2, 0, 0, 0) };
    }

    pub fn thread_resume(&self, thread_ptr: usize) {
        let _ = self;
        unsafe { crate::jobthread::nzbget_rs_job_op(thread_ptr as *const _, 3, 0, 0, 0) };
    }

    pub fn post_stop_thread(&self, post_ptr: PostRef) {
        self.post_job_op(0, post_ptr.addr(), OpArgs::NONE);
    }

    pub fn post_delete_thread(&self, post_ptr: PostRef) {
        self.post_job_op(1, post_ptr.addr(), OpArgs::NONE);
    }

    pub fn post_kill_and_delete_thread(&self, post_ptr: PostRef) {
        self.post_job_op(2, post_ptr.addr(), OpArgs::NONE);
    }

    pub fn direct_unpack_is_active(&self, nzb_ptr: NzbRef, include_waiting_volume: bool) -> bool {
        self.post_job_op(
            3,
            nzb_ptr.addr(),
            OpArgs::NONE.a2(i32::from(include_waiting_volume)),
        ) != 0
    }

    pub fn direct_unpack_stop(&self, dq_ptr: usize, nzb_ptr: NzbRef) {
        self.post_job_op(4, dq_ptr, OpArgs::NONE.p2(nzb_ptr.addr()));
    }

    pub fn direct_unpack_nzb_downloaded(&self, dq_ptr: usize, nzb_ptr: NzbRef) {
        self.post_job_op(5, dq_ptr, OpArgs::NONE.p2(nzb_ptr.addr()));
    }

    pub fn direct_unpack_nzb_deleted(&self, dq_ptr: usize, nzb_ptr: NzbRef) {
        self.post_job_op(6, dq_ptr, OpArgs::NONE.p2(nzb_ptr.addr()));
    }

    pub fn direct_unpack_file_downloaded(&self, dq_ptr: usize, nzb_ptr: NzbRef, file_ptr: FileRef) {
        let mut i64s = [file_ptr.addr() as i64];
        self.post_job_op(7, dq_ptr, OpArgs::NONE.p2(nzb_ptr.addr()).i64s(i64s.as_mut_ptr()));
    }

    pub fn direct_unpack_start_job(&self, nzb_ptr: NzbRef) {
        self.post_job_op(8, nzb_ptr.addr(), OpArgs::NONE);
    }

    pub fn live_repair_stop(&self, nzb_ptr: NzbRef) {
        self.post_job_op(9, nzb_ptr.addr(), OpArgs::NONE);
    }

    pub fn live_repair_detach_and_stop(&self, nzb_ptr: NzbRef) {
        self.post_job_op(10, nzb_ptr.addr(), OpArgs::NONE);
    }

    pub fn repair_add_par(&self, post_ptr: PostRef, file_ptr: FileRef, deleted: bool) -> bool {
        self.post_job_op(
            11,
            post_ptr.addr(),
            OpArgs::NONE.a2(i32::from(deleted)).p2(file_ptr.addr()),
        ) != 0
    }

    pub fn post_controller_start_job(&self, post_ptr: PostRef, controller_kind: i32) {
        self.post_job_op(12, post_ptr.addr(), OpArgs::NONE.a2(controller_kind));
    }
}

fn file_snap_from(
    file_ptr: FileRef,
    ints: &[i32; 28],
    i64s: &[i64; 8],
    articles_len: usize,
    mut texts: [Vec<u8>; 7],
) -> FileSnap {
    FileSnap {
        ptr: file_ptr,
        nzb_ptr: NzbRef::from_addr(i64s[6] as usize),
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
        filename: std::mem::take(&mut texts[0]),
        subject: std::mem::take(&mut texts[1]),
        output_filename: std::mem::take(&mut texts[2]),
        hard_link_path: std::mem::take(&mut texts[3]),
        origname: std::mem::take(&mut texts[4]),
        hash16k: std::mem::take(&mut texts[5]),
        par_set_id: std::mem::take(&mut texts[6]),
    }
}
