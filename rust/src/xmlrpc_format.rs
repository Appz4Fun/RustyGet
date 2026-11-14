//! XML-RPC and JSON-RPC response serialization and command routing (`daemon/remote/XmlRpc.cpp`).

use std::ffi::{c_char, c_int, c_void, CStr};
use std::io::Write;

use crate::ffi::{input, nzbget_rs_free, RsBuf};

#[repr(C)]
pub struct NzbgetRsRpcStatusInput {
    pub remaining_size: i64,
    pub forced_size: i64,
    pub article_cache: i64,
    pub memory_pressure: *const c_char,
    pub effective_article_cache_mb: c_int,
    pub effective_par_buffer_mb: c_int,
    pub download_rate: i64,
    pub download_limit: c_int,
    pub thread_count: c_int,
    pub post_job_count: c_int,
    pub url_count: c_int,
    pub up_time_sec: c_int,
    pub download_time_sec: c_int,
    pub all_bytes: i64,
    pub month_bytes: i64,
    pub day_bytes: i64,
    pub download_paused: c_int,
    pub server_stand_by: c_int,
    pub post_paused: c_int,
    pub scan_paused: c_int,
    pub quota_reached: c_int,
    pub free_disk_space: i64,
    pub total_disk_space: i64,
    pub inter_dir_empty: c_int,
    pub free_inter_disk_space: i64,
    pub total_inter_disk_space: i64,
    pub server_time: c_int,
    pub resume_time: c_int,
    pub feed_active: c_int,
    pub queued_scripts: c_int,
}

#[repr(C)]
pub struct NzbgetRsRpcServerStatus {
    pub id: c_int,
    pub active: c_int,
}

#[repr(C)]
pub struct NzbgetRsRpcLogEntry {
    pub id: c_int,
    pub kind: c_int,
    pub time: c_int,
    pub text: *const c_char,
}

#[repr(C)]
pub struct NzbgetRsRpcFileItem {
    pub id: c_int,
    pub file_size: i64,
    pub remaining_size: i64,
    pub post_time: c_int,
    pub filename_confirmed: c_int,
    pub paused: c_int,
    pub nzb_id: c_int,
    pub nzb_name: *const c_char,
    pub nzb_filename: *const c_char,
    pub subject: *const c_char,
    pub filename: *const c_char,
    pub dest_dir: *const c_char,
    pub category: *const c_char,
    pub priority: c_int,
    pub active_downloads: c_int,
    pub progress: c_int,
}

#[repr(C)]
pub struct NzbgetRsRpcNzbInfoInput {
    pub id: c_int,
    pub kind: c_int,
    pub name: *const c_char,
    pub url: *const c_char,
    pub filename: *const c_char,
    pub dest_dir: *const c_char,
    pub final_dir: *const c_char,
    pub category: *const c_char,
    pub par_status: c_int,
    pub unpack_status: c_int,
    pub move_status: c_int,
    pub script_status: c_int,
    pub delete_status: c_int,
    pub mark_status: c_int,
    pub url_status: c_int,
    pub size: i64,
    pub file_count: c_int,
    pub min_time: c_int,
    pub max_time: c_int,
    pub total_articles: c_int,
    pub current_success_articles: c_int,
    pub current_failed_articles: c_int,
    pub dupe_recovered_articles: c_int,
    pub dupe_recovered_bytes: i64,
    pub dupe_recovered_holes: c_int,
    pub live_repairing: c_int,
    pub health: c_int,
    pub critical_health: c_int,
    pub dupe_key: *const c_char,
    pub dupe_score: c_int,
    pub dupe_mode: c_int,
    pub downloaded_size: i64,
    pub download_sec: c_int,
    pub post_total_sec: c_int,
    pub has_post_start_time: c_int,
    pub cur_time: i64,
    pub post_start_time: i64,
    pub par_sec: c_int,
    pub repair_sec: c_int,
    pub unpack_sec: c_int,
    pub message_count: c_int,
    pub cached_message_count: c_int,
    pub extra_par_blocks: c_int,
}

#[repr(C)]
pub struct NzbgetRsRpcParamItem {
    pub name: *const c_char,
    pub value: *const c_char,
}

#[repr(C)]
pub struct NzbgetRsRpcScriptItem {
    pub name: *const c_char,
    pub status: c_int,
}

#[repr(C)]
pub struct NzbgetRsRpcServerStatItem {
    pub server_id: c_int,
    pub success_articles: c_int,
    pub failed_articles: c_int,
}

#[repr(C)]
pub struct NzbgetRsRpcConfigItem {
    pub name: *const c_char,
    pub value: *const c_char,
    pub restricted: c_int,
}

#[repr(C)]
pub struct NzbgetRsRpcFeedItem {
    pub title: *const c_char,
    pub filename: *const c_char,
    pub url: *const c_char,
    pub size: i64,
    pub size_mb: c_int,
    pub category: *const c_char,
    pub add_category: *const c_char,
    pub pause_nzb: c_int,
    pub priority: c_int,
    pub time: c_int,
    pub match_status: c_int,
    pub match_rule: c_int,
    pub dupe_key: *const c_char,
    pub dupe_score: c_int,
    pub dupe_mode: c_int,
    pub status: c_int,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct NzbgetRsRpcArticlePair {
    pub failed: u32,
    pub success: u32,
}

#[repr(C)]
pub struct NzbgetRsRpcServerVolumeItem {
    pub server_id: c_int,
    pub data_time: c_int,
    pub first_day: c_int,
    pub total_bytes: i64,
    pub custom_bytes: i64,
    pub custom_time: c_int,
    pub counters_reset_time: c_int,
    pub sec_slot: c_int,
    pub min_slot: c_int,
    pub hour_slot: c_int,
    pub day_slot: c_int,
    pub include_seconds: c_int,
    pub bytes_per_seconds: *const i64,
    pub bytes_per_seconds_len: usize,
    pub include_minutes: c_int,
    pub bytes_per_minutes: *const i64,
    pub bytes_per_minutes_len: usize,
    pub include_hours: c_int,
    pub bytes_per_hours: *const i64,
    pub bytes_per_hours_len: usize,
    pub include_days: c_int,
    pub bytes_per_days: *const i64,
    pub bytes_per_days_len: usize,
    pub include_articles: c_int,
    pub articles_per_days: *const NzbgetRsRpcArticlePair,
    pub articles_per_days_len: usize,
}

#[repr(C)]
pub struct NzbgetRsRpcFleetMember {
    pub nzb_id: c_int,
    pub name: *const c_char,
    pub rank: c_int,
    pub alive: c_int,
    pub status: *const c_char,
    pub same_as: c_int,
    pub same_as_rank: c_int,
    pub reason: *const c_char,
}

#[repr(C)]
pub struct NzbgetRsRpcFleetReqMember {
    pub name: *const c_char,
    pub is_url: c_int,
    pub data: *const c_char,
    pub data_len: usize,
}

pub type NzbgetRsXmlRpcLogFn =
    unsafe extern "C" fn(ctx: *mut c_void, kind: c_int, msg: *const c_char);

pub type NzbgetRsXmlRpcActionFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    a3: c_int,
    a4: c_int,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    body: *const c_char,
    body_len: usize,
    ids: *const i32,
    ids_len: usize,
    params: *const NzbgetRsRpcParamItem,
    params_len: usize,
    out_u32: *mut u32,
    out_f64: *mut f64,
    out_buf: *mut RsBuf,
) -> c_int;

pub type NzbgetRsXmlRpcEmitFn = unsafe extern "C" fn(
    emit_ctx: *mut c_void,
    sub: c_int,
    a1: c_int,
    a2: c_int,
    a3: c_int,
    a4: c_int,
    a5: c_int,
    a6: c_int,
    i1: i64,
    i2: i64,
    i3: i64,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    p1: *const c_void,
    n1: usize,
    p2: *const c_void,
    n2: usize,
    p3: *const c_void,
    n3: usize,
    p4: *const c_void,
    n4: usize,
);

pub type NzbgetRsXmlRpcCollectFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    kind: c_int,
    a1: c_int,
    a2: c_int,
    a3: c_int,
    a4: c_int,
    a5: c_int,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    s5: *const c_char,
    s6: *const c_char,
    p1: *const c_void,
    n1: usize,
    emit_ctx: *mut c_void,
    emit_cb: NzbgetRsXmlRpcEmitFn,
) -> c_int;

#[inline]
fn split_i64(val: i64) -> (u32, u32) {
    let u = val as u64;
    ((u >> 32) as u32, u as u32)
}

#[inline]
fn safe_i64_to_i32(val: i64) -> i32 {
    i32::try_from(val).unwrap_or(0)
}

#[inline]
fn bool_str(is_json: bool, val: bool) -> &'static str {
    match (is_json, val) {
        (true, true) => "true",
        (true, false) => "false",
        (false, true) => "1",
        (false, false) => "0",
    }
}

#[inline]
fn encode_str(is_json: bool, raw: &[u8]) -> String {
    let mut v = Vec::new();
    if is_json {
        crate::escape::json_encode(raw, &mut v);
    } else {
        crate::escape::xml_encode(raw, &mut v);
    }
    // json_encode and xml_encode produce pure ASCII bytes
    unsafe { String::from_utf8_unchecked(v) }
}

const MESSAGE_TYPES: [&str; 5] = ["INFO", "WARNING", "ERROR", "DEBUG", "DETAIL"];
const KIND_NAMES: [&str; 2] = ["NZB", "URL"];
const PAR_STATUS_NAMES: [&str; 6] = [
    "NONE",
    "NONE",
    "FAILURE",
    "SUCCESS",
    "REPAIR_POSSIBLE",
    "MANUAL",
];
const UNPACK_STATUS_NAMES: [&str; 6] = ["NONE", "NONE", "FAILURE", "SUCCESS", "SPACE", "PASSWORD"];
const MOVE_STATUS_NAMES: [&str; 3] = ["NONE", "FAILURE", "SUCCESS"];
const SCRIPT_STATUS_NAMES: [&str; 3] = ["NONE", "FAILURE", "SUCCESS"];
const DELETE_STATUS_NAMES: [&str; 8] = [
    "NONE", "MANUAL", "HEALTH", "DUPE", "BAD", "GOOD", "COPY", "SCAN",
];
const MARK_STATUS_NAMES: [&str; 4] = ["NONE", "BAD", "GOOD", "SUCCESS"];
const URL_STATUS_NAMES: [&str; 7] = [
    "NONE",
    "UNKNOWN",
    "SUCCESS",
    "FAILURE",
    "UNKNOWN",
    "SCAN_SKIPPED",
    "SCAN_FAILURE",
];
const DUPE_MODE_NAMES: [&str; 3] = ["SCORE", "ALL", "FORCE"];
const POST_STAGE_NAMES: [&str; 15] = [
    "QUEUED",
    "LOADING_PARS",
    "VERIFYING_SOURCES",
    "REPAIRING",
    "VERIFYING_REPAIRED",
    "RENAMING",
    "RENAMING",
    "UNPACKING",
    "MOVING",
    "MOVING",
    "POST_UNPACK_RENAMING",
    "EXECUTING_SCRIPT",
    "FINISHED",
    "POST_DOWNLOAD_RENAMING",
    "STREAM_REPAIRING",
];
const DUP_STATUS_NAMES: [&str; 7] = [
    "UNKNOWN", "SUCCESS", "FAILURE", "DELETED", "DUPE", "BAD", "GOOD",
];
const FEED_STATUS_NAMES: [&str; 4] = ["UNKNOWN", "BACKLOG", "FETCHED", "NEW"];
const FEED_MATCH_NAMES: [&str; 3] = ["IGNORED", "ACCEPTED", "REJECTED"];

#[inline]
fn table_get<'a>(table: &[&'a str], idx: c_int, fallback: &'a str) -> &'a str {
    if idx >= 0 && (idx as usize) < table.len() {
        table[idx as usize]
    } else {
        fallback
    }
}

pub fn match_command(method: &[u8]) -> c_int {
    let mut lower = [0u8; 32];
    if method.is_empty() || method.len() > lower.len() {
        return 0;
    }
    for (i, &b) in method.iter().enumerate() {
        lower[i] = b.to_ascii_lowercase();
    }
    match &lower[..method.len()] {
        b"pause" | b"pausedownload" | b"pausedownload2" => 1,
        b"resume" | b"resumedownload" | b"resumedownload2" => 2,
        b"shutdown" => 3,
        b"reload" => 4,
        b"version" => 5,
        b"dump" => 6,
        b"rate" => 7,
        b"status" => 8,
        b"sysinfo" => 9,
        b"systemhealth" => 10,
        b"log" => 11,
        b"listfiles" => 12,
        b"listgroups" => 13,
        b"editqueue" => 14,
        b"append" | b"appendurl" => 15,
        b"appendfleet" => 16,
        b"postqueue" => 17,
        b"writelog" => 18,
        b"clearlog" => 19,
        b"loadlog" => 20,
        b"scan" => 21,
        b"pausepost" => 22,
        b"resumepost" => 23,
        b"pausescan" => 24,
        b"resumescan" => 25,
        b"scheduleresume" => 26,
        b"history" => 27,
        b"urlqueue" => 28,
        b"config" => 29,
        b"loadconfig" => 30,
        b"saveconfig" => 36,
        b"configtemplates" => 37,
        b"viewfeed" => 38,
        b"previewfeed" => 39,
        b"fetchfeed" => 40,
        b"editserver" => 41,
        b"readurl" => 42,
        b"checkupdates" => 43,
        b"startupdate" => 44,
        b"logupdate" => 45,
        b"servervolumes" => 46,
        b"resetservervolume" => 47,
        b"tlscipherstart" => 48,
        b"tlscipherstatus" => 49,
        b"tlsciphercancel" => 50,
        b"testserver" => 51,
        b"testserverspeed" => 52,
        b"testdiskspeed" => 53,
        b"testnetworkspeed" => 54,
        _ => 0,
    }
}

pub fn is_safe_command(cmd_id: c_int) -> bool {
    matches!(
        cmd_id,
        5 | 6 | 8 | 9 | 10 | 11 | 12 | 13 | 17 | 20 | 27 | 28 | 29 | 30 | 37 | 45 | 46
    )
}

unsafe fn write_status(
    out: &mut Vec<u8>,
    json: bool,
    inp: *const NzbgetRsRpcStatusInput,
    servers: *const NzbgetRsRpcServerStatus,
    servers_len: usize,
) {
    if inp.is_null() {
        return;
    }
    let s = unsafe { &*inp };

    let (rem_hi, rem_lo) = split_i64(s.remaining_size);
    let (forced_hi, forced_lo) = split_i64(s.forced_size);
    let (cache_hi, cache_lo) = split_i64(s.article_cache);
    let (dl_rate_hi, dl_rate_lo) = split_i64(s.download_rate);
    let (dl_size_hi, dl_size_lo) = split_i64(s.all_bytes);

    let avg_dl_rate = xmlrpc_calc_average_download_rate(s.download_time_sec, s.all_bytes);
    let (avg_rate_hi, avg_rate_lo) = split_i64(avg_dl_rate);

    let (month_hi, month_lo) = split_i64(s.month_bytes);
    let (day_hi, day_lo) = split_i64(s.day_bytes);

    let (mut rem_mb, mut forced_mb, mut cache_mb, mut dl_mb, mut month_mb, mut day_mb) =
        (0, 0, 0, 0, 0, 0);
    unsafe {
        roc_xmlrpc_status_calc_mbytes(
            s.remaining_size,
            s.forced_size,
            s.article_cache,
            s.all_bytes,
            s.month_bytes,
            s.day_bytes,
            &mut rem_mb,
            &mut forced_mb,
            &mut cache_mb,
            &mut dl_mb,
            &mut month_mb,
            &mut day_mb,
        );
    }

    let mut free_inter = s.free_inter_disk_space;
    let mut total_inter = s.total_inter_disk_space;
    let (mut free_disk_mb, mut total_disk_mb, mut free_inter_mb, mut total_inter_mb) = (0, 0, 0, 0);
    unsafe {
        roc_xmlrpc_status_calc_disk_space_mb(
            s.free_disk_space,
            s.total_disk_space,
            s.inter_dir_empty,
            s.free_inter_disk_space,
            s.total_inter_disk_space,
            &mut free_inter,
            &mut total_inter,
            &mut free_disk_mb,
            &mut total_disk_mb,
            &mut free_inter_mb,
            &mut total_inter_mb,
        );
    }

    let (free_disk_hi, free_disk_lo) = split_i64(s.free_disk_space);
    let (total_disk_hi, total_disk_lo) = split_i64(s.total_disk_space);
    let (free_inter_hi, free_inter_lo) = split_i64(free_inter);
    let (total_inter_hi, total_inter_lo) = split_i64(total_inter);

    let mem_pressure = encode_str(json, unsafe { input(s.memory_pressure) });
    let dl_paused_str = bool_str(json, s.download_paused != 0);
    let stand_by_str = bool_str(json, s.server_stand_by != 0);
    let post_paused_str = bool_str(json, s.post_paused != 0);
    let scan_paused_str = bool_str(json, s.scan_paused != 0);
    let quota_str = bool_str(json, s.quota_reached != 0);
    let feed_active_str = bool_str(json, s.feed_active != 0);

    let dl_rate_i32 = safe_i64_to_i32(s.download_rate);
    let avg_rate_i32 = safe_i64_to_i32(avg_dl_rate);
    let thread_count_u32 = s.thread_count as u32;

    out.reserve(4096);
    if json {
        let _ = write!(
            out,
            "{{\n\
            \"RemainingSizeLo\" : {},\n\
            \"RemainingSizeHi\" : {},\n\
            \"RemainingSizeMB\" : {},\n\
            \"ForcedSizeLo\" : {},\n\
            \"ForcedSizeHi\" : {},\n\
            \"ForcedSizeMB\" : {},\n\
            \"DownloadedSizeLo\" : {},\n\
            \"DownloadedSizeHi\" : {},\n\
            \"DownloadedSizeMB\" : {},\n\
            \"MonthSizeLo\" : {},\n\
            \"MonthSizeHi\" : {},\n\
            \"MonthSizeMB\" : {},\n\
            \"DaySizeLo\" : {},\n\
            \"DaySizeHi\" : {},\n\
            \"DaySizeMB\" : {},\n\
            \"ArticleCacheLo\" : {},\n\
            \"ArticleCacheHi\" : {},\n\
            \"ArticleCacheMB\" : {},\n\
            \"MemoryPressure\" : \"{}\",\n\
            \"EffectiveArticleCacheMB\" : {},\n\
            \"EffectiveParBufferMB\" : {},\n\
            \"DownloadRate\" : {},\n\
            \"DownloadRateLo\" : {},\n\
            \"DownloadRateHi\" : {},\n\
            \"AverageDownloadRate\" : {},\n\
            \"AverageDownloadRateLo\" : {},\n\
            \"AverageDownloadRateHi\" : {},\n\
            \"DownloadLimit\" : {},\n\
            \"ThreadCount\" : {},\n\
            \"ParJobCount\" : {},\n\
            \"PostJobCount\" : {},\n\
            \"UrlCount\" : {},\n\
            \"UpTimeSec\" : {},\n\
            \"DownloadTimeSec\" : {},\n\
            \"ServerPaused\" : {},\n\
            \"DownloadPaused\" : {},\n\
            \"Download2Paused\" : {},\n\
            \"ServerStandBy\" : {},\n\
            \"PostPaused\" : {},\n\
            \"ScanPaused\" : {},\n\
            \"QuotaReached\" : {},\n\
            \"FreeDiskSpaceLo\" : {},\n\
            \"FreeDiskSpaceHi\" : {},\n\
            \"FreeDiskSpaceMB\" : {},\n\
            \"TotalDiskSpaceLo\" : {},\n\
            \"TotalDiskSpaceHi\" : {},\n\
            \"TotalDiskSpaceMB\" : {},\n\
            \"FreeInterDiskSpaceLo\" : {},\n\
            \"FreeInterDiskSpaceHi\" : {},\n\
            \"FreeInterDiskSpaceMB\" : {},\n\
            \"TotalInterDiskSpaceLo\" : {},\n\
            \"TotalInterDiskSpaceHi\" : {},\n\
            \"TotalInterDiskSpaceMB\" : {},\n\
            \"ServerTime\" : {},\n\
            \"ResumeTime\" : {},\n\
            \"FeedActive\" : {},\n\
            \"QueueScriptCount\" : {},\n\
            \"NewsServers\" : [\n",
            rem_lo,
            rem_hi,
            rem_mb,
            forced_lo,
            forced_hi,
            forced_mb,
            dl_size_lo,
            dl_size_hi,
            dl_mb,
            month_lo,
            month_hi,
            month_mb,
            day_lo,
            day_hi,
            day_mb,
            cache_lo,
            cache_hi,
            cache_mb,
            mem_pressure,
            s.effective_article_cache_mb,
            s.effective_par_buffer_mb,
            dl_rate_i32,
            dl_rate_lo,
            dl_rate_hi,
            avg_rate_i32,
            avg_rate_lo,
            avg_rate_hi,
            s.download_limit,
            thread_count_u32,
            s.post_job_count,
            s.post_job_count,
            s.url_count,
            s.up_time_sec,
            s.download_time_sec,
            dl_paused_str,
            dl_paused_str,
            dl_paused_str,
            stand_by_str,
            post_paused_str,
            scan_paused_str,
            quota_str,
            free_disk_lo,
            free_disk_hi,
            free_disk_mb,
            total_disk_lo,
            total_disk_hi,
            total_disk_mb,
            free_inter_lo,
            free_inter_hi,
            free_inter_mb,
            total_inter_lo,
            total_inter_hi,
            total_inter_mb,
            s.server_time,
            s.resume_time,
            feed_active_str,
            s.queued_scripts,
        );
    } else {
        let _ = write!(
            out,
            "<struct>\n\
            <member><name>RemainingSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>RemainingSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>RemainingSizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>ForcedSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>ForcedSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>ForcedSizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>DownloadedSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>DownloadedSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>DownloadedSizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>MonthSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>MonthSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>MonthSizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>DaySizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>DaySizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>DaySizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>ArticleCacheLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>ArticleCacheHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>ArticleCacheMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>MemoryPressure</name><value><string>{}</string></value></member>\n\
            <member><name>EffectiveArticleCacheMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>EffectiveParBufferMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>DownloadRate</name><value><i4>{}</i4></value></member>\n\
            <member><name>DownloadRateLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>DownloadRateHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>AverageDownloadRate</name><value><i4>{}</i4></value></member>\n\
            <member><name>AverageDownloadRateLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>AverageDownloadRateHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>DownloadLimit</name><value><i4>{}</i4></value></member>\n\
            <member><name>ThreadCount</name><value><i4>{}</i4></value></member>\n\
            <member><name>ParJobCount</name><value><i4>{}</i4></value></member>\n\
            <member><name>PostJobCount</name><value><i4>{}</i4></value></member>\n\
            <member><name>UrlCount</name><value><i4>{}</i4></value></member>\n\
            <member><name>UpTimeSec</name><value><i4>{}</i4></value></member>\n\
            <member><name>DownloadTimeSec</name><value><i4>{}</i4></value></member>\n\
            <member><name>ServerPaused</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>DownloadPaused</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>Download2Paused</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>ServerStandBy</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>PostPaused</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>ScanPaused</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>QuotaReached</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>FreeDiskSpaceLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>FreeDiskSpaceHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>FreeDiskSpaceMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>TotalDiskSpaceLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>TotalDiskSpaceHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>TotalDiskSpaceMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>FreeInterDiskSpaceLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>FreeInterDiskSpaceHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>FreeInterDiskSpaceMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>TotalInterDiskSpaceLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>TotalInterDiskSpaceHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>TotalInterDiskSpaceMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>ServerTime</name><value><i4>{}</i4></value></member>\n\
            <member><name>ResumeTime</name><value><i4>{}</i4></value></member>\n\
            <member><name>FeedActive</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>QueueScriptCount</name><value><i4>{}</i4></value></member>\n\
            <member><name>NewsServers</name><value><array><data>\n",
            rem_lo,
            rem_hi,
            rem_mb,
            forced_lo,
            forced_hi,
            forced_mb,
            dl_size_lo,
            dl_size_hi,
            dl_mb,
            month_lo,
            month_hi,
            month_mb,
            day_lo,
            day_hi,
            day_mb,
            cache_lo,
            cache_hi,
            cache_mb,
            mem_pressure,
            s.effective_article_cache_mb,
            s.effective_par_buffer_mb,
            dl_rate_i32,
            dl_rate_lo as i32,
            dl_rate_hi as i32,
            avg_rate_i32,
            avg_rate_lo as i32,
            avg_rate_hi as i32,
            s.download_limit,
            thread_count_u32,
            s.post_job_count,
            s.post_job_count,
            s.url_count,
            s.up_time_sec,
            s.download_time_sec,
            dl_paused_str,
            dl_paused_str,
            dl_paused_str,
            stand_by_str,
            post_paused_str,
            scan_paused_str,
            quota_str,
            free_disk_lo,
            free_disk_hi,
            free_disk_mb,
            total_disk_lo,
            total_disk_hi,
            total_disk_mb,
            free_inter_lo,
            free_inter_hi,
            free_inter_mb,
            total_inter_lo,
            total_inter_hi,
            total_inter_mb,
            s.server_time,
            s.resume_time,
            feed_active_str,
            s.queued_scripts,
        );
    }

    let srv_slice = if servers.is_null() || servers_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(servers, servers_len) }
    };
    for (idx, srv) in srv_slice.iter().enumerate() {
        let active_str = bool_str(json, srv.active != 0);
        if json {
            if idx > 0 {
                out.extend_from_slice(b",\n");
            }
            let _ = write!(
                out,
                "{{\n\"ID\" : {},\n\"Active\" : {}\n}}",
                srv.id, active_str
            );
        } else {
            let _ = write!(
                out,
                "<value><struct>\n<member><name>ID</name><value><i4>{}</i4></value></member>\n<member><name>Active</name><value><boolean>{}</boolean></value></member>\n</struct></value>\n",
                srv.id, active_str
            );
        }
    }

    if json {
        out.extend_from_slice(b"]\n}");
    } else {
        out.extend_from_slice(b"</data></array></value></member>\n</struct>\n");
    }
}

unsafe fn write_log_entries(is_json: bool, entries: &[NzbgetRsRpcLogEntry], out: &mut Vec<u8>) {
    for (idx, entry) in entries.iter().enumerate() {
        let kind_str = table_get(&MESSAGE_TYPES, entry.kind, "INFO");
        let text_enc = encode_str(is_json, unsafe { input(entry.text) });
        if is_json {
            if idx > 0 {
                out.extend_from_slice(b",\n");
            }
            let _ = write!(
                out,
                "{{\n\"ID\" : {},\n\"Kind\" : \"{}\",\n\"Time\" : {},\n\"Text\" : \"{}\"\n}}",
                entry.id, kind_str, entry.time, text_enc
            );
        } else {
            let _ = write!(
                out,
                "<value><struct>\n<member><name>ID</name><value><i4>{}</i4></value></member>\n<member><name>Kind</name><value><string>{}</string></value></member>\n<member><name>Time</name><value><i4>{}</i4></value></member>\n<member><name>Text</name><value><string>{}</string></value></member>\n</struct></value>\n",
                entry.id, kind_str, entry.time, text_enc
            );
        }
    }
}

unsafe fn write_log_list(
    out: &mut Vec<u8>,
    json: bool,
    entries: *const NzbgetRsRpcLogEntry,
    len: usize,
) {
    let slice = if entries.is_null() || len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(entries, len) }
    };
    out.reserve(slice.len() * 128 + 32);
    if json {
        out.extend_from_slice(b"[\n");
    } else {
        out.extend_from_slice(b"<array><data>\n");
    }
    unsafe {
        write_log_entries(json, slice, out);
    }
    if json {
        out.extend_from_slice(b"\n]");
    } else {
        out.extend_from_slice(b"</data></array>\n");
    }
}

unsafe fn write_file_item(
    out: &mut Vec<u8>,
    json: bool,
    index: usize,
    item: *const NzbgetRsRpcFileItem,
) {
    if item.is_null() {
        return;
    }
    let f = unsafe { &*item };
    let (file_hi, file_lo) = split_i64(f.file_size);
    let (rem_hi, rem_lo) = split_i64(f.remaining_size);
    let nzb_name = encode_str(json, unsafe { input(f.nzb_name) });
    let nzb_filename = encode_str(json, unsafe { input(f.nzb_filename) });
    let subject = encode_str(json, unsafe { input(f.subject) });
    let filename = encode_str(json, unsafe { input(f.filename) });
    let dest_dir = encode_str(json, unsafe { input(f.dest_dir) });
    let category = encode_str(json, unsafe { input(f.category) });
    let confirmed_str = bool_str(json, f.filename_confirmed != 0);
    let paused_str = bool_str(json, f.paused != 0);

    out.reserve(512);
    if json {
        if index > 0 {
            out.extend_from_slice(b",\n");
        }
        let _ = write!(
            out,
            "{{\n\
            \"ID\" : {},\n\
            \"FileSizeLo\" : {},\n\
            \"FileSizeHi\" : {},\n\
            \"RemainingSizeLo\" : {},\n\
            \"RemainingSizeHi\" : {},\n\
            \"PostTime\" : {},\n\
            \"FilenameConfirmed\" : {},\n\
            \"Paused\" : {},\n\
            \"NZBID\" : {},\n\
            \"NZBName\" : \"{}\",\n\
            \"NZBNicename\" : \"{}\",\n\
            \"NZBFilename\" : \"{}\",\n\
            \"Subject\" : \"{}\",\n\
            \"Filename\" : \"{}\",\n\
            \"DestDir\" : \"{}\",\n\
            \"Category\" : \"{}\",\n\
            \"Priority\" : {},\n\
            \"ActiveDownloads\" : {},\n\
            \"Progress\" : {}\n\
            }}",
            f.id,
            file_lo,
            file_hi,
            rem_lo,
            rem_hi,
            f.post_time,
            confirmed_str,
            paused_str,
            f.nzb_id,
            nzb_name,
            nzb_name,
            nzb_filename,
            subject,
            filename,
            dest_dir,
            category,
            f.priority,
            f.active_downloads,
            f.progress
        );
    } else {
        let _ = write!(
            out,
            "<value><struct>\n\
            <member><name>ID</name><value><i4>{}</i4></value></member>\n\
            <member><name>FileSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>FileSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>RemainingSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>RemainingSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>PostTime</name><value><i4>{}</i4></value></member>\n\
            <member><name>FilenameConfirmed</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>Paused</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>NZBID</name><value><i4>{}</i4></value></member>\n\
            <member><name>NZBName</name><value><string>{}</string></value></member>\n\
            <member><name>NZBNicename</name><value><string>{}</string></value></member>\n\
            <member><name>NZBFilename</name><value><string>{}</string></value></member>\n\
            <member><name>Subject</name><value><string>{}</string></value></member>\n\
            <member><name>Filename</name><value><string>{}</string></value></member>\n\
            <member><name>DestDir</name><value><string>{}</string></value></member>\n\
            <member><name>Category</name><value><string>{}</string></value></member>\n\
            <member><name>Priority</name><value><i4>{}</i4></value></member>\n\
            <member><name>ActiveDownloads</name><value><i4>{}</i4></value></member>\n\
            <member><name>Progress</name><value><i4>{}</i4></value></member>\n\
            </struct></value>\n",
            f.id,
            file_lo,
            file_hi,
            rem_lo,
            rem_hi,
            f.post_time,
            confirmed_str,
            paused_str,
            f.nzb_id,
            nzb_name,
            nzb_name,
            nzb_filename,
            subject,
            filename,
            dest_dir,
            category,
            f.priority,
            f.active_downloads,
            f.progress as u32
        );
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn write_nzb_info_fields(
    out: &mut Vec<u8>,
    json: bool,
    info: *const NzbgetRsRpcNzbInfoInput,
    params: *const NzbgetRsRpcParamItem,
    params_len: usize,
    scripts: *const NzbgetRsRpcScriptItem,
    scripts_len: usize,
    stats: *const NzbgetRsRpcServerStatItem,
    stats_len: usize,
) {
    if info.is_null() {
        return;
    }
    let n = unsafe { &*info };

    let (file_hi, file_lo) = split_i64(n.size);
    let file_mb = (n.size / 1024 / 1024) as c_int;
    let (dl_hi, dl_lo) = split_i64(n.downloaded_size);
    let dl_mb = (n.downloaded_size / 1024 / 1024) as c_int;
    let (dupe_hi, dupe_lo) = split_i64(n.dupe_recovered_bytes);

    let (message_count, post_total_time_sec) = xmlrpc_nzb_message_and_post_time(
        n.message_count,
        n.cached_message_count,
        n.post_total_sec,
        n.has_post_start_time != 0,
        n.cur_time,
        n.post_start_time,
    );

    let name_enc = encode_str(json, unsafe { input(n.name) });
    let url_enc = encode_str(json, unsafe { input(n.url) });
    let filename_enc = encode_str(json, unsafe { input(n.filename) });
    let dest_dir_enc = encode_str(json, unsafe { input(n.dest_dir) });
    let final_dir_enc = encode_str(json, unsafe { input(n.final_dir) });
    let category_enc = encode_str(json, unsafe { input(n.category) });
    let dupe_key_enc = encode_str(json, unsafe { input(n.dupe_key) });

    let kind_str = table_get(&KIND_NAMES, n.kind, "NZB");
    let par_status_str = table_get(&PAR_STATUS_NAMES, n.par_status, "NONE");
    let ex_par_status_str = if n.extra_par_blocks > 0 {
        "RECIPIENT"
    } else if n.extra_par_blocks < 0 {
        "DONOR"
    } else {
        "NONE"
    };
    let unpack_status_str = table_get(&UNPACK_STATUS_NAMES, n.unpack_status, "NONE");
    let move_status_str = table_get(&MOVE_STATUS_NAMES, n.move_status, "NONE");
    let script_status_str = table_get(&SCRIPT_STATUS_NAMES, n.script_status, "NONE");
    let delete_status_str = table_get(&DELETE_STATUS_NAMES, n.delete_status, "NONE");
    let mark_status_str = table_get(&MARK_STATUS_NAMES, n.mark_status, "NONE");
    let url_status_str = table_get(&URL_STATUS_NAMES, n.url_status, "NONE");
    let dupe_mode_str = table_get(&DUPE_MODE_NAMES, n.dupe_mode, "SCORE");

    let live_repair_str = bool_str(json, n.live_repairing != 0);
    let deleted_str = bool_str(json, n.delete_status != 0);

    out.reserve(2048);
    if json {
        let _ = write!(
            out,
            "\"NZBID\" : {},\n\
            \"NZBName\" : \"{}\",\n\
            \"NZBNicename\" : \"{}\",\n\
            \"Kind\" : \"{}\",\n\
            \"URL\" : \"{}\",\n\
            \"NZBFilename\" : \"{}\",\n\
            \"DestDir\" : \"{}\",\n\
            \"FinalDir\" : \"{}\",\n\
            \"Category\" : \"{}\",\n\
            \"ParStatus\" : \"{}\",\n\
            \"ExParStatus\" : \"{}\",\n\
            \"UnpackStatus\" : \"{}\",\n\
            \"MoveStatus\" : \"{}\",\n\
            \"ScriptStatus\" : \"{}\",\n\
            \"DeleteStatus\" : \"{}\",\n\
            \"MarkStatus\" : \"{}\",\n\
            \"UrlStatus\" : \"{}\",\n\
            \"FileSizeLo\" : {},\n\
            \"FileSizeHi\" : {},\n\
            \"FileSizeMB\" : {},\n\
            \"FileCount\" : {},\n\
            \"MinPostTime\" : {},\n\
            \"MaxPostTime\" : {},\n\
            \"TotalArticles\" : {},\n\
            \"SuccessArticles\" : {},\n\
            \"FailedArticles\" : {},\n\
            \"DupeRecoveredArticles\" : {},\n\
            \"DupeRecoveredBytesLo\" : {},\n\
            \"DupeRecoveredBytesHi\" : {},\n\
            \"DupeRecoveredHoles\" : {},\n\
            \"LiveRepairing\" : {},\n\
            \"Health\" : {},\n\
            \"CriticalHealth\" : {},\n\
            \"DupeKey\" : \"{}\",\n\
            \"DupeScore\" : {},\n\
            \"DupeMode\" : \"{}\",\n\
            \"Deleted\" : {},\n\
            \"DownloadedSizeLo\" : {},\n\
            \"DownloadedSizeHi\" : {},\n\
            \"DownloadedSizeMB\" : {},\n\
            \"DownloadTimeSec\" : {},\n\
            \"PostTotalTimeSec\" : {},\n\
            \"ParTimeSec\" : {},\n\
            \"RepairTimeSec\" : {},\n\
            \"UnpackTimeSec\" : {},\n\
            \"MessageCount\" : {},\n\
            \"ExtraParBlocks\" : {},\n\
            \"Parameters\" : [\n",
            n.id,
            name_enc,
            name_enc,
            kind_str,
            url_enc,
            filename_enc,
            dest_dir_enc,
            final_dir_enc,
            category_enc,
            par_status_str,
            ex_par_status_str,
            unpack_status_str,
            move_status_str,
            script_status_str,
            delete_status_str,
            mark_status_str,
            url_status_str,
            file_lo,
            file_hi,
            file_mb,
            n.file_count,
            n.min_time,
            n.max_time,
            n.total_articles,
            n.current_success_articles,
            n.current_failed_articles,
            n.dupe_recovered_articles,
            dupe_lo,
            dupe_hi,
            n.dupe_recovered_holes,
            live_repair_str,
            n.health,
            n.critical_health,
            dupe_key_enc,
            n.dupe_score,
            dupe_mode_str,
            deleted_str,
            dl_lo,
            dl_hi,
            dl_mb,
            n.download_sec,
            post_total_time_sec,
            n.par_sec,
            n.repair_sec,
            n.unpack_sec,
            message_count,
            n.extra_par_blocks
        );
    } else {
        let _ = write!(
            out,
            "<member><name>NZBID</name><value><i4>{}</i4></value></member>\n\
            <member><name>NZBName</name><value><string>{}</string></value></member>\n\
            <member><name>NZBNicename</name><value><string>{}</string></value></member>\n\
            <member><name>Kind</name><value><string>{}</string></value></member>\n\
            <member><name>URL</name><value><string>{}</string></value></member>\n\
            <member><name>NZBFilename</name><value><string>{}</string></value></member>\n\
            <member><name>DestDir</name><value><string>{}</string></value></member>\n\
            <member><name>FinalDir</name><value><string>{}</string></value></member>\n\
            <member><name>Category</name><value><string>{}</string></value></member>\n\
            <member><name>ParStatus</name><value><string>{}</string></value></member>\n\
            <member><name>ExParStatus</name><value><string>{}</string></value></member>\n\
            <member><name>UnpackStatus</name><value><string>{}</string></value></member>\n\
            <member><name>MoveStatus</name><value><string>{}</string></value></member>\n\
            <member><name>ScriptStatus</name><value><string>{}</string></value></member>\n\
            <member><name>DeleteStatus</name><value><string>{}</string></value></member>\n\
            <member><name>MarkStatus</name><value><string>{}</string></value></member>\n\
            <member><name>UrlStatus</name><value><string>{}</string></value></member>\n\
            <member><name>FileSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>FileSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>FileSizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>FileCount</name><value><i4>{}</i4></value></member>\n\
            <member><name>MinPostTime</name><value><i4>{}</i4></value></member>\n\
            <member><name>MaxPostTime</name><value><i4>{}</i4></value></member>\n\
            <member><name>TotalArticles</name><value><i4>{}</i4></value></member>\n\
            <member><name>SuccessArticles</name><value><i4>{}</i4></value></member>\n\
            <member><name>FailedArticles</name><value><i4>{}</i4></value></member>\n\
            <member><name>DupeRecoveredArticles</name><value><i4>{}</i4></value></member>\n\
            <member><name>DupeRecoveredBytesLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>DupeRecoveredBytesHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>DupeRecoveredHoles</name><value><i4>{}</i4></value></member>\n\
            <member><name>LiveRepairing</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>Health</name><value><i4>{}</i4></value></member>\n\
            <member><name>CriticalHealth</name><value><i4>{}</i4></value></member>\n\
            <member><name>DupeKey</name><value><string>{}</string></value></member>\n\
            <member><name>DupeScore</name><value><i4>{}</i4></value></member>\n\
            <member><name>DupeMode</name><value><string>{}</string></value></member>\n\
            <member><name>Deleted</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>DownloadedSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>DownloadedSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>DownloadedSizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>DownloadTimeSec</name><value><i4>{}</i4></value></member>\n\
            <member><name>PostTotalTimeSec</name><value><i4>{}</i4></value></member>\n\
            <member><name>ParTimeSec</name><value><i4>{}</i4></value></member>\n\
            <member><name>RepairTimeSec</name><value><i4>{}</i4></value></member>\n\
            <member><name>UnpackTimeSec</name><value><i4>{}</i4></value></member>\n\
            <member><name>MessageCount</name><value><i4>{}</i4></value></member>\n\
            <member><name>ExtraParBlocks</name><value><i4>{}</i4></value></member>\n\
            <member><name>Parameters</name><value><array><data>\n",
            n.id,
            name_enc,
            name_enc,
            kind_str,
            url_enc,
            filename_enc,
            dest_dir_enc,
            final_dir_enc,
            category_enc,
            par_status_str,
            ex_par_status_str,
            unpack_status_str,
            move_status_str,
            script_status_str,
            delete_status_str,
            mark_status_str,
            url_status_str,
            file_lo,
            file_hi,
            file_mb,
            n.file_count,
            n.min_time,
            n.max_time,
            n.total_articles,
            n.current_success_articles,
            n.current_failed_articles,
            n.dupe_recovered_articles,
            dupe_lo,
            dupe_hi,
            n.dupe_recovered_holes,
            live_repair_str,
            n.health,
            n.critical_health,
            dupe_key_enc,
            n.dupe_score,
            dupe_mode_str,
            deleted_str,
            dl_lo,
            dl_hi,
            dl_mb,
            n.download_sec,
            post_total_time_sec,
            n.par_sec,
            n.repair_sec,
            n.unpack_sec,
            message_count,
            n.extra_par_blocks
        );
    }

    let param_slice = if params.is_null() || params_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(params, params_len) }
    };
    for (idx, p) in param_slice.iter().enumerate() {
        let p_name = encode_str(json, unsafe { input(p.name) });
        let p_val = encode_str(json, unsafe { input(p.value) });
        if json {
            if idx > 0 {
                out.extend_from_slice(b",\n");
            }
            let _ = write!(
                out,
                "{{\n\"Name\" : \"{}\",\n\"Value\" : \"{}\"\n}}",
                p_name, p_val
            );
        } else {
            let _ = write!(
                out,
                "<value><struct>\n<member><name>Name</name><value><string>{}</string></value></member>\n<member><name>Value</name><value><string>{}</string></value></member>\n</struct></value>\n",
                p_name, p_val
            );
        }
    }

    if json {
        out.extend_from_slice(b"],\n\"ScriptStatuses\" : [\n");
    } else {
        out.extend_from_slice(b"</data></array></value></member>\n<member><name>ScriptStatuses</name><value><array><data>\n");
    }

    let script_slice = if scripts.is_null() || scripts_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(scripts, scripts_len) }
    };
    for (idx, sc) in script_slice.iter().enumerate() {
        let sc_name = encode_str(json, unsafe { input(sc.name) });
        let sc_status = encode_str(
            json,
            table_get(&SCRIPT_STATUS_NAMES, sc.status, "NONE").as_bytes(),
        );
        if json {
            if idx > 0 {
                out.extend_from_slice(b",\n");
            }
            let _ = write!(
                out,
                "{{\n\"Name\" : \"{}\",\n\"Status\" : \"{}\"\n}}",
                sc_name, sc_status
            );
        } else {
            let _ = write!(
                out,
                "<value><struct>\n<member><name>Name</name><value><string>{}</string></value></member>\n<member><name>Status</name><value><string>{}</string></value></member>\n</struct></value>\n",
                sc_name, sc_status
            );
        }
    }

    if json {
        out.extend_from_slice(b"],\n\"ServerStats\" : [\n");
    } else {
        out.extend_from_slice(b"</data></array></value></member>\n<member><name>ServerStats</name><value><array><data>\n");
    }

    let stat_slice = if stats.is_null() || stats_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(stats, stats_len) }
    };
    for (idx, st) in stat_slice.iter().enumerate() {
        if json {
            if idx > 0 {
                out.extend_from_slice(b",\n");
            }
            let _ = write!(
                out,
                "{{\n\"ServerID\" : {},\n\"SuccessArticles\" : {},\n\"FailedArticles\" : {}\n}}",
                st.server_id, st.success_articles, st.failed_articles
            );
        } else {
            let _ = write!(
                out,
                "<value><struct>\n<member><name>ServerID</name><value><i4>{}</i4></value></member>\n<member><name>SuccessArticles</name><value><i4>{}</i4></value></member>\n<member><name>FailedArticles</name><value><i4>{}</i4></value></member>\n</struct></value>\n",
                st.server_id, st.success_articles, st.failed_articles
            );
        }
    }

    if json {
        out.extend_from_slice(b"]\n");
    } else {
        out.extend_from_slice(b"</data></array></value></member>\n");
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn write_post_info_fields(
    out: &mut Vec<u8>,
    json: bool,
    is_post_queue: bool,
    has_post_info: bool,
    progress_label: *const c_char,
    stage_progress: c_int,
    cur_time: i64,
    stage_time: i64,
    start_time: i64,
    logs: *const NzbgetRsRpcLogEntry,
    logs_len: usize,
) {
    let (label, progress, stage_sec, total_sec) = if has_post_info {
        let mut st_sec = 0;
        let mut tot_sec = 0;
        unsafe {
            roc_xmlrpc_post_info_elapsed_sec(
                cur_time,
                stage_time,
                start_time,
                &mut st_sec,
                &mut tot_sec,
            );
        }
        (
            encode_str(json, unsafe { input(progress_label) }),
            stage_progress,
            st_sec,
            tot_sec,
        )
    } else {
        ("NONE".to_string(), 0, 0, 0)
    };

    out.reserve(512);
    match (json, is_post_queue) {
        (true, true) => {
            let _ = write!(
                out,
                "\"ProgressLabel\" : \"{}\",\n\"StageProgress\" : {},\n\"StageTimeSec\" : {},\n\"TotalTimeSec\" : {},\n\"Log\" : [\n",
                label, progress, stage_sec, total_sec
            );
        }
        (true, false) => {
            let _ = write!(
                out,
                "\"PostInfoText\" : \"{}\",\n\"PostStageProgress\" : {},\n\"PostStageTimeSec\" : {},\n\"Log\" : [\n",
                label, progress, stage_sec
            );
        }
        (false, true) => {
            let _ = write!(
                out,
                "<member><name>ProgressLabel</name><value><string>{}</string></value></member>\n\
                <member><name>StageProgress</name><value><i4>{}</i4></value></member>\n\
                <member><name>StageTimeSec</name><value><i4>{}</i4></value></member>\n\
                <member><name>TotalTimeSec</name><value><i4>{}</i4></value></member>\n\
                <member><name>Log</name><value><array><data>\n",
                label, progress, stage_sec, total_sec
            );
        }
        (false, false) => {
            let _ = write!(
                out,
                "<member><name>PostInfoText</name><value><string>{}</string></value></member>\n\
                <member><name>PostStageProgress</name><value><i4>{}</i4></value></member>\n\
                <member><name>PostStageTimeSec</name><value><i4>{}</i4></value></member>\n\
                <member><name>Log</name><value><array><data>\n",
                label, progress, stage_sec
            );
        }
    }

    let log_slice = if logs.is_null() || logs_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(logs, logs_len) }
    };
    unsafe {
        write_log_entries(json, log_slice, out);
    }

    if json {
        out.extend_from_slice(b"]\n");
    } else {
        out.extend_from_slice(b"</data></array></value></member>\n");
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn write_group_item_start(
    out: &mut Vec<u8>,
    json: bool,
    index: usize,
    nzb_id: c_int,
    remaining_size: i64,
    paused_size: i64,
    remaining_file_count: c_int,
    remaining_par_count: c_int,
    priority: c_int,
    active_downloads: c_int,
    has_post_info: bool,
    post_stage: c_int,
    has_queue_script_job: bool,
    queue_script_active: bool,
    direct_unpack_status: c_int,
    kind: c_int,
) {
    let (rem_hi, rem_lo) = split_i64(remaining_size);
    let (pau_hi, pau_lo) = split_i64(paused_size);
    let mut rem_mb = 0;
    let mut pau_mb = 0;
    unsafe {
        roc_xmlrpc_list_groups_calc_mb(remaining_size, paused_size, &mut rem_mb, &mut pau_mb);
    }
    let status_str = rpc_detect_group_status(
        has_post_info,
        post_stage,
        has_queue_script_job,
        queue_script_active,
        direct_unpack_status,
        active_downloads,
        kind,
        paused_size,
        remaining_size,
    )
    .to_str()
    .unwrap_or("QUEUED");

    out.reserve(512);
    if json {
        if index > 0 {
            out.extend_from_slice(b",\n");
        }
        let _ = write!(
            out,
            "{{\n\
            \"FirstID\" : {},\n\
            \"LastID\" : {},\n\
            \"RemainingSizeLo\" : {},\n\
            \"RemainingSizeHi\" : {},\n\
            \"RemainingSizeMB\" : {},\n\
            \"PausedSizeLo\" : {},\n\
            \"PausedSizeHi\" : {},\n\
            \"PausedSizeMB\" : {},\n\
            \"RemainingFileCount\" : {},\n\
            \"RemainingParCount\" : {},\n\
            \"MinPriority\" : {},\n\
            \"MaxPriority\" : {},\n\
            \"ActiveDownloads\" : {},\n\
            \"Status\" : \"{}\",\n",
            nzb_id,
            nzb_id,
            rem_lo,
            rem_hi,
            rem_mb,
            pau_lo,
            pau_hi,
            pau_mb,
            remaining_file_count,
            remaining_par_count,
            priority,
            priority,
            active_downloads,
            status_str
        );
    } else {
        let _ = write!(
            out,
            "<value><struct>\n\
            <member><name>FirstID</name><value><i4>{}</i4></value></member>\n\
            <member><name>LastID</name><value><i4>{}</i4></value></member>\n\
            <member><name>RemainingSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>RemainingSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>RemainingSizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>PausedSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>PausedSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>PausedSizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>RemainingFileCount</name><value><i4>{}</i4></value></member>\n\
            <member><name>RemainingParCount</name><value><i4>{}</i4></value></member>\n\
            <member><name>MinPriority</name><value><i4>{}</i4></value></member>\n\
            <member><name>MaxPriority</name><value><i4>{}</i4></value></member>\n\
            <member><name>ActiveDownloads</name><value><i4>{}</i4></value></member>\n\
            <member><name>Status</name><value><string>{}</string></value></member>\n",
            nzb_id,
            nzb_id,
            rem_lo,
            rem_hi,
            rem_mb,
            pau_lo,
            pau_hi,
            pau_mb,
            remaining_file_count,
            remaining_par_count,
            priority,
            priority,
            active_downloads,
            status_str
        );
    }
}

unsafe fn write_postqueue_item_start(
    out: &mut Vec<u8>,
    json: bool,
    index: usize,
    nzb_id: c_int,
    info_name: *const c_char,
    stage: c_int,
    file_progress: c_int,
) {
    let info_name_enc = encode_str(json, unsafe { input(info_name) });
    let stage_str = table_get(&POST_STAGE_NAMES, stage, "QUEUED");

    out.reserve(256);
    if json {
        if index > 0 {
            out.extend_from_slice(b",\n");
        }
        let _ = write!(
            out,
            "{{\n\
            \"ID\" : {},\n\
            \"InfoName\" : \"{}\",\n\
            \"ParFilename\" : \"\",\n\
            \"Stage\" : \"{}\",\n\
            \"FileProgress\" : {},\n",
            nzb_id, info_name_enc, stage_str, file_progress
        );
    } else {
        let _ = write!(
            out,
            "<value><struct>\n\
            <member><name>ID</name><value><i4>{}</i4></value></member>\n\
            <member><name>InfoName</name><value><string>{}</string></value></member>\n\
            <member><name>ParFilename</name><value><string></string></value></member>\n\
            <member><name>Stage</name><value><string>{}</string></value></member>\n\
            <member><name>FileProgress</name><value><i4>{}</i4></value></member>\n",
            nzb_id, info_name_enc, stage_str, file_progress
        );
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn write_history_nzb_start(
    out: &mut Vec<u8>,
    json: bool,
    index: usize,
    id: c_int,
    name: *const c_char,
    remaining_file_count: c_int,
    retry_data: bool,
    history_time: c_int,
    status: *const c_char,
) {
    let name_enc = encode_str(json, unsafe { input(name) });
    let retry_str = bool_str(json, retry_data);
    let status_enc = encode_str(json, unsafe { input(status) });

    out.reserve(256);
    if json {
        if index > 0 {
            out.extend_from_slice(b",\n");
        }
        let _ = write!(
            out,
            "{{\n\
            \"ID\" : {},\n\
            \"Name\" : \"{}\",\n\
            \"RemainingFileCount\" : {},\n\
            \"RetryData\" : {},\n\
            \"HistoryTime\" : {},\n\
            \"Status\" : \"{}\",\n\
            \"Log\" : [],\n",
            id, name_enc, remaining_file_count, retry_str, history_time, status_enc
        );
    } else {
        let _ = write!(
            out,
            "<value><struct>\n\
            <member><name>ID</name><value><i4>{}</i4></value></member>\n\
            <member><name>Name</name><value><string>{}</string></value></member>\n\
            <member><name>RemainingFileCount</name><value><i4>{}</i4></value></member>\n\
            <member><name>RetryData</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>HistoryTime</name><value><i4>{}</i4></value></member>\n\
            <member><name>Status</name><value><string>{}</string></value></member>\n\
            <member><name>Log</name><value><array><data></data></array></value></member>\n",
            id, name_enc, remaining_file_count, retry_str, history_time, status_enc
        );
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn write_history_dup_item(
    out: &mut Vec<u8>,
    json: bool,
    index: usize,
    id: c_int,
    name: *const c_char,
    history_time: c_int,
    size: i64,
    dupe_key: *const c_char,
    dupe_score: c_int,
    dupe_mode: c_int,
    dup_status: c_int,
) {
    let (size_hi, size_lo) = split_i64(size);
    let size_mb = (size / 1024 / 1024) as c_int;
    let name_enc = encode_str(json, unsafe { input(name) });
    let dupe_key_enc = encode_str(json, unsafe { input(dupe_key) });
    let dupe_mode_str = table_get(&DUPE_MODE_NAMES, dupe_mode, "SCORE");
    let dup_status_str = table_get(&DUP_STATUS_NAMES, dup_status, "UNKNOWN");
    let status_str = rpc_detect_history_dup_status(dup_status)
        .to_str()
        .unwrap_or("FAILURE/INTERNAL_ERROR");

    out.reserve(512);
    if json {
        if index > 0 {
            out.extend_from_slice(b",\n");
        }
        let _ = write!(
            out,
            "{{\n\
            \"ID\" : {},\n\
            \"NZBID\" : {},\n\
            \"Kind\" : \"DUP\",\n\
            \"Name\" : \"{}\",\n\
            \"HistoryTime\" : {},\n\
            \"FileSizeLo\" : {},\n\
            \"FileSizeHi\" : {},\n\
            \"FileSizeMB\" : {},\n\
            \"DupeKey\" : \"{}\",\n\
            \"DupeScore\" : {},\n\
            \"DupeMode\" : \"{}\",\n\
            \"DupStatus\" : \"{}\",\n\
            \"Status\" : \"{}\"\n",
            id,
            id,
            name_enc,
            history_time,
            size_lo,
            size_hi,
            size_mb,
            dupe_key_enc,
            dupe_score,
            dupe_mode_str,
            dup_status_str,
            status_str
        );
    } else {
        let _ = write!(
            out,
            "<value><struct>\n\
            <member><name>ID</name><value><i4>{}</i4></value></member>\n\
            <member><name>NZBID</name><value><i4>{}</i4></value></member>\n\
            <member><name>Kind</name><value><string>DUP</string></value></member>\n\
            <member><name>Name</name><value><string>{}</string></value></member>\n\
            <member><name>HistoryTime</name><value><i4>{}</i4></value></member>\n\
            <member><name>FileSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>FileSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>FileSizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>DupeKey</name><value><string>{}</string></value></member>\n\
            <member><name>DupeScore</name><value><i4>{}</i4></value></member>\n\
            <member><name>DupeMode</name><value><string>{}</string></value></member>\n\
            <member><name>DupStatus</name><value><string>{}</string></value></member>\n\
            <member><name>Status</name><value><string>{}</string></value></member>\n",
            id,
            id,
            name_enc,
            history_time,
            size_lo,
            size_hi,
            size_mb,
            dupe_key_enc,
            dupe_score,
            dupe_mode_str,
            dup_status_str,
            status_str
        );
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn write_urlqueue_item(
    out: &mut Vec<u8>,
    json: bool,
    index: usize,
    id: c_int,
    filename: *const c_char,
    url: *const c_char,
    name: *const c_char,
    category: *const c_char,
    priority: c_int,
) {
    let filename_enc = encode_str(json, unsafe { input(filename) });
    let url_enc = encode_str(json, unsafe { input(url) });
    let name_enc = encode_str(json, unsafe { input(name) });
    let category_enc = encode_str(json, unsafe { input(category) });

    out.reserve(256);
    if json {
        if index > 0 {
            out.extend_from_slice(b",\n");
        }
        let _ = write!(
            out,
            "{{\n\
            \"ID\" : {},\n\
            \"NZBFilename\" : \"{}\",\n\
            \"URL\" : \"{}\",\n\
            \"Name\" : \"{}\",\n\
            \"Category\" : \"{}\",\n\
            \"Priority\" : {}\n\
            }}",
            id, filename_enc, url_enc, name_enc, category_enc, priority
        );
    } else {
        let _ = write!(
            out,
            "<value><struct>\n\
            <member><name>ID</name><value><i4>{}</i4></value></member>\n\
            <member><name>NZBFilename</name><value><string>{}</string></value></member>\n\
            <member><name>URL</name><value><string>{}</string></value></member>\n\
            <member><name>Name</name><value><string>{}</string></value></member>\n\
            <member><name>Category</name><value><string>{}</string></value></member>\n\
            <member><name>Priority</name><value><i4>{}</i4></value></member>\n\
            </struct></value>\n",
            id, filename_enc, url_enc, name_enc, category_enc, priority
        );
    }
}

unsafe fn write_config_list(
    out: &mut Vec<u8>,
    json: bool,
    user_access: c_int,
    ua_restricted: c_int,
    entries: *const NzbgetRsRpcConfigItem,
    len: usize,
) {
    let slice = if entries.is_null() || len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(entries, len) }
    };
    out.reserve(slice.len() * 96 + 32);
    if json {
        out.extend_from_slice(b"[\n");
    } else {
        out.extend_from_slice(b"<array><data>\n");
    }
    for (idx, entry) in slice.iter().enumerate() {
        let mask =
            roc_xmlrpc_config_should_mask_value(user_access, ua_restricted, entry.restricted) != 0;
        let name_enc = encode_str(json, unsafe { input(entry.name) });
        let val_enc = if mask {
            "***".to_string()
        } else {
            encode_str(json, unsafe { input(entry.value) })
        };
        if json {
            if idx > 0 {
                out.extend_from_slice(b",\n");
            }
            let _ = write!(
                out,
                "{{\n\"Name\" : \"{}\",\n\"Value\" : \"{}\"\n}}",
                name_enc, val_enc
            );
        } else {
            let _ = write!(
                out,
                "<value><struct>\n<member><name>Name</name><value><string>{}</string></value></member>\n<member><name>Value</name><value><string>{}</string></value></member>\n</struct></value>\n",
                name_enc, val_enc
            );
        }
    }
    if json {
        out.extend_from_slice(b"\n]");
    } else {
        out.extend_from_slice(b"</data></array>\n");
    }
}

unsafe fn write_config_templates(
    out: &mut Vec<u8>,
    json: bool,
    templates: *const *const c_char,
    len: usize,
) {
    let slice = if templates.is_null() || len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(templates, len) }
    };
    out.reserve(slice.len() * 256 + 32);
    if json {
        out.extend_from_slice(b"[\n");
    } else {
        out.extend_from_slice(b"<array><data>\n");
    }
    for (idx, &tpl_ptr) in slice.iter().enumerate() {
        let tpl_enc = encode_str(json, unsafe { input(tpl_ptr) });
        if json {
            if idx > 0 {
                out.extend_from_slice(b",\n");
            }
            let _ = write!(out, "{{\n\"Template\" : \"{}\"\n}}", tpl_enc);
        } else {
            let _ = write!(
                out,
                "<value><struct>\n<member><name>Template</name><value><string>{}</string></value></member>\n</struct></value>\n",
                tpl_enc
            );
        }
    }
    if json {
        out.extend_from_slice(b"\n]");
    } else {
        out.extend_from_slice(b"</data></array>\n");
    }
}

unsafe fn write_feed_item(
    out: &mut Vec<u8>,
    json: bool,
    index: usize,
    item: *const NzbgetRsRpcFeedItem,
) {
    if item.is_null() {
        return;
    }
    let f = unsafe { &*item };
    let (size_hi, size_lo) = split_i64(f.size);
    let title_enc = encode_str(json, unsafe { input(f.title) });
    let filename_enc = encode_str(json, unsafe { input(f.filename) });
    let url_enc = encode_str(json, unsafe { input(f.url) });
    let category_enc = encode_str(json, unsafe { input(f.category) });
    let add_category_enc = encode_str(json, unsafe { input(f.add_category) });
    let pause_str = bool_str(json, f.pause_nzb != 0);
    let match_str = table_get(&FEED_MATCH_NAMES, f.match_status, "IGNORED");
    let dupe_key_enc = encode_str(json, unsafe { input(f.dupe_key) });
    let dupe_mode_str = table_get(&DUPE_MODE_NAMES, f.dupe_mode, "SCORE");
    let status_str = table_get(&FEED_STATUS_NAMES, f.status, "UNKNOWN");

    out.reserve(512);
    if json {
        if index > 0 {
            out.extend_from_slice(b",\n");
        }
        let _ = write!(
            out,
            "{{\n\
            \"Title\" : \"{}\",\n\
            \"Filename\" : \"{}\",\n\
            \"URL\" : \"{}\",\n\
            \"SizeLo\" : {},\n\
            \"SizeHi\" : {},\n\
            \"SizeMB\" : {},\n\
            \"Category\" : \"{}\",\n\
            \"AddCategory\" : \"{}\",\n\
            \"PauseNzb\" : {},\n\
            \"Priority\" : {},\n\
            \"Time\" : {},\n\
            \"Match\" : \"{}\",\n\
            \"Rule\" : {},\n\
            \"DupeKey\" : \"{}\",\n\
            \"DupeScore\" : {},\n\
            \"DupeMode\" : \"{}\",\n\
            \"Status\" : \"{}\"\n\
            }}",
            title_enc,
            filename_enc,
            url_enc,
            size_lo,
            size_hi,
            f.size_mb,
            category_enc,
            add_category_enc,
            pause_str,
            f.priority,
            f.time,
            match_str,
            f.match_rule,
            dupe_key_enc,
            f.dupe_score,
            dupe_mode_str,
            status_str
        );
    } else {
        let _ = write!(
            out,
            "<value><struct>\n\
            <member><name>Title</name><value><string>{}</string></value></member>\n\
            <member><name>Filename</name><value><string>{}</string></value></member>\n\
            <member><name>URL</name><value><string>{}</string></value></member>\n\
            <member><name>SizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>SizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>SizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>Category</name><value><string>{}</string></value></member>\n\
            <member><name>AddCategory</name><value><string>{}</string></value></member>\n\
            <member><name>PauseNzb</name><value><boolean>{}</boolean></value></member>\n\
            <member><name>Priority</name><value><i4>{}</i4></value></member>\n\
            <member><name>Time</name><value><i4>{}</i4></value></member>\n\
            <member><name>Match</name><value><string>{}</string></value></member>\n\
            <member><name>Rule</name><value><i4>{}</i4></value></member>\n\
            <member><name>DupeKey</name><value><string>{}</string></value></member>\n\
            <member><name>DupeScore</name><value><i4>{}</i4></value></member>\n\
            <member><name>DupeMode</name><value><string>{}</string></value></member>\n\
            <member><name>Status</name><value><string>{}</string></value></member>\n\
            </struct></value>\n",
            title_enc,
            filename_enc,
            url_enc,
            size_lo,
            size_hi,
            f.size_mb,
            category_enc,
            add_category_enc,
            pause_str,
            f.priority,
            f.time,
            match_str,
            f.match_rule,
            dupe_key_enc,
            f.dupe_score,
            dupe_mode_str,
            status_str
        );
    }
}

unsafe fn write_server_volume_item(
    out: &mut Vec<u8>,
    json: bool,
    index: usize,
    item: *const NzbgetRsRpcServerVolumeItem,
) {
    if item.is_null() {
        return;
    }
    let v = unsafe { &*item };
    let (total_hi, total_lo) = split_i64(v.total_bytes);
    let (custom_hi, custom_lo) = split_i64(v.custom_bytes);
    let mut total_mb: u32 = 0;
    let mut custom_mb: u32 = 0;
    unsafe {
        roc_xmlrpc_server_volumes_calc_mb(
            v.total_bytes,
            v.custom_bytes,
            &mut total_mb,
            &mut custom_mb,
        );
    }

    out.reserve(4096);
    if json {
        if index > 0 {
            out.extend_from_slice(b",\n");
        }
        let _ = write!(
            out,
            "{{\n\
            \"ServerID\" : {},\n\
            \"DataTime\" : {},\n\
            \"FirstDay\" : {},\n\
            \"TotalSizeLo\" : {},\n\
            \"TotalSizeHi\" : {},\n\
            \"TotalSizeMB\" : {},\n\
            \"CustomSizeLo\" : {},\n\
            \"CustomSizeHi\" : {},\n\
            \"CustomSizeMB\" : {},\n\
            \"CustomTime\" : {},\n\
            \"CountersResetTime\" : {},\n\
            \"SecSlot\" : {},\n\
            \"MinSlot\" : {},\n\
            \"HourSlot\" : {},\n\
            \"DaySlot\" : {}\n",
            v.server_id,
            v.data_time,
            v.first_day,
            total_lo,
            total_hi,
            total_mb as i32,
            custom_lo,
            custom_hi,
            custom_mb as i32,
            v.custom_time,
            v.counters_reset_time,
            v.sec_slot,
            v.min_slot,
            v.hour_slot,
            v.day_slot
        );
    } else {
        let _ = write!(
            out,
            "<value><struct>\n\
            <member><name>ServerID</name><value><i4>{}</i4></value></member>\n\
            <member><name>DataTime</name><value><i4>{}</i4></value></member>\n\
            <member><name>FirstDay</name><value><i4>{}</i4></value></member>\n\
            <member><name>TotalSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>TotalSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>TotalSizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>CustomSizeLo</name><value><i4>{}</i4></value></member>\n\
            <member><name>CustomSizeHi</name><value><i4>{}</i4></value></member>\n\
            <member><name>CustomSizeMB</name><value><i4>{}</i4></value></member>\n\
            <member><name>CustomTime</name><value><i4>{}</i4></value></member>\n\
            <member><name>CountersResetTime</name><value><i4>{}</i4></value></member>\n\
            <member><name>SecSlot</name><value><i4>{}</i4></value></member>\n\
            <member><name>MinSlot</name><value><i4>{}</i4></value></member>\n\
            <member><name>HourSlot</name><value><i4>{}</i4></value></member>\n\
            <member><name>DaySlot</name><value><i4>{}</i4></value></member>\n",
            v.server_id,
            v.data_time,
            v.first_day,
            total_lo,
            total_hi,
            total_mb as i32,
            custom_lo,
            custom_hi,
            custom_mb as i32,
            v.custom_time,
            v.counters_reset_time,
            v.sec_slot,
            v.min_slot,
            v.hour_slot,
            v.day_slot
        );
    }

    let byte_arrays = [
        (
            v.include_seconds != 0,
            "BytesPerSeconds",
            v.bytes_per_seconds,
            v.bytes_per_seconds_len,
        ),
        (
            v.include_minutes != 0,
            "BytesPerMinutes",
            v.bytes_per_minutes,
            v.bytes_per_minutes_len,
        ),
        (
            v.include_hours != 0,
            "BytesPerHours",
            v.bytes_per_hours,
            v.bytes_per_hours_len,
        ),
        (
            v.include_days != 0,
            "BytesPerDays",
            v.bytes_per_days,
            v.bytes_per_days_len,
        ),
    ];

    for (include, name, ptr, len) in byte_arrays {
        if !include {
            continue;
        }
        if json {
            let _ = write!(out, ",\n\"{}\" : [\n", name);
        } else {
            let _ = write!(out, "<member><name>{}</name><value><array><data>\n", name);
        }
        let slice = if ptr.is_null() || len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(ptr, len) }
        };
        for (idx2, &bytes) in slice.iter().enumerate() {
            let (b_hi, b_lo) = split_i64(bytes);
            let b_mb = (bytes / 1024 / 1024) as i32;
            if json {
                if idx2 > 0 {
                    out.extend_from_slice(b",\n");
                }
                let _ = write!(
                    out,
                    "{{\n\"SizeLo\" : {},\n\"SizeHi\" : {},\n\"SizeMB\" : {}\n}}",
                    b_lo, b_hi, b_mb
                );
            } else {
                let _ = write!(
                    out,
                    "<value><struct>\n<member><name>SizeLo</name><value><i4>{}</i4></value></member>\n<member><name>SizeHi</name><value><i4>{}</i4></value></member>\n<member><name>SizeMB</name><value><i4>{}</i4></value></member>\n</struct></value>\n",
                    b_lo, b_hi, b_mb
                );
            }
        }
        if json {
            out.extend_from_slice(b"]\n");
        } else {
            out.extend_from_slice(b"</data></array></value></member>\n");
        }
    }

    if v.include_articles != 0 {
        if json {
            out.extend_from_slice(b",\n\"ArticlesPerDays\" : [\n");
        } else {
            out.extend_from_slice(b"<member><name>ArticlesPerDays</name><value><array><data>\n");
        }
        let art_slice = if v.articles_per_days.is_null() || v.articles_per_days_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(v.articles_per_days, v.articles_per_days_len) }
        };
        for (idx2, art) in art_slice.iter().enumerate() {
            if json {
                let _ = write!(
                    out,
                    "{{\n\"Failed\" : {},\n\"Success\" : {}\n}}",
                    art.failed, art.success
                );
                if idx2 + 1 < art_slice.len() {
                    out.extend_from_slice(b",\n");
                }
            } else {
                let _ = write!(
                    out,
                    "<value><struct>\n<member><name>Failed</name><value><i4>{}</i4></value></member>\n<member><name>Success</name><value><i4>{}</i4></value></member>\n</struct></value>\n",
                    art.failed, art.success
                );
            }
        }
        if json {
            out.extend_from_slice(b"]\n");
        } else {
            out.extend_from_slice(b"</data></array></value></member>\n");
        }
    }

    if json {
        out.extend_from_slice(b"}\n");
    } else {
        out.extend_from_slice(b"</struct></value>\n");
    }
}

unsafe fn write_fleet_result(
    out: &mut Vec<u8>,
    chosen: c_int,
    complete: bool,
    reason: *const c_char,
    members: *const NzbgetRsRpcFleetMember,
    members_len: usize,
) {
    let reason_enc = encode_str(true, unsafe { input(reason) });
    let complete_str = if complete { "true" } else { "false" };
    let mut head = Vec::with_capacity(256);
    let _ = write!(
        head,
        "{{\"Chosen\" : {}, \"Complete\" : {}, \"Reason\" : \"{}\", \"Members\" : [",
        chosen, complete_str, reason_enc
    );
    if head.len() > 1023 {
        head.truncate(1023);
    }

    out.extend_from_slice(&head);
    let slice = if members.is_null() || members_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(members, members_len) }
    };
    for (idx, m) in slice.iter().enumerate() {
        let name_enc = encode_str(true, unsafe { input(m.name) });
        let status_enc = encode_str(true, unsafe { input(m.status) });
        let m_reason_enc = encode_str(true, unsafe { input(m.reason) });
        let sep = if idx > 0 { ", " } else { "" };
        let _ = write!(
            out,
            "{}{{\"NZBID\" : {}, \"Name\" : \"{}\", \"Rank\" : {}, \"Alive\" : {}, \"Status\" : \"{}\", \"SameAs\" : {}, \"SameAsRank\" : {}, \"Reason\" : \"{}\"}}",
            sep,
            m.nzb_id,
            name_enc,
            m.rank,
            m.alive,
            status_enc,
            m.same_as,
            m.same_as_rank,
            m_reason_enc
        );
    }
    out.extend_from_slice(b"]}");
}

fn format_json_f64(val: f64) -> String {
    if !val.is_finite() {
        return "0.0".to_string();
    }
    let s = val.to_string();
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{}.0", s)
    }
}

pub fn format_disk_speed_bytes(
    is_json: bool,
    write_bytes: u64,
    write_ms: f64,
    read_bytes: u64,
    read_ms: f64,
) -> Vec<u8> {
    let size_mb = write_bytes / 1024 / 1024;
    let read_size_mb = read_bytes / 1024 / 1024;
    let mut out = Vec::with_capacity(256);
    if is_json {
        let _ = write!(
            out,
            "{{\"SizeMB\":{},\"DurationMS\":{},\"ReadSizeMB\":{},\"ReadDurationMS\":{}}}",
            size_mb,
            format_json_f64(write_ms),
            read_size_mb,
            format_json_f64(read_ms),
        );
    } else {
        let _ = write!(
            out,
            "<value><struct><member><name>SizeMB</name><value><i4>{}</i4></value></member><member><name>DurationMS</name><value><double>{:.6}</double></value></member><member><name>ReadSizeMB</name><value><i4>{}</i4></value></member><member><name>ReadDurationMS</name><value><double>{:.6}</double></value></member></struct></value>",
            size_mb, write_ms, read_size_mb, read_ms
        );
    }
    out
}

pub fn format_network_speed_bytes(is_json: bool, speed_mbps: f64) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    if is_json {
        let _ = write!(out, "{{\"SpeedMbps\":{}}}", format_json_f64(speed_mbps));
    } else {
        let _ = write!(
            out,
            "<value><struct><member><name>SpeedMbps</name><value><double>{:.6}</double></value></member></struct></value>",
            speed_mbps
        );
    }
    out
}

/// Splits `XmlRpcProcessor::SetUrl` into `(path_set_len, query_offset)`.
/// When `'?'` is absent, `path_set_len` is `0` (matching `CString::Set(url, 0)` which copies the full string).
pub fn xmlrpc_split_url_path_and_query(url: &[u8]) -> (usize, Option<usize>) {
    match url.iter().position(|&b| b == b'?') {
        Some(pos) => (pos, Some(pos)),
        None => (0, None),
    }
}

/// Evaluates whether `EditQueueXmlCommand::Execute` should replace `args` with the formatted `offset`.
pub fn xmlrpc_should_use_edit_offset(
    has_offset: bool,
    action: i32,
    ea_file_move_offset: i32,
    ea_group_move_offset: i32,
) -> bool {
    has_offset && (action == ea_file_move_offset || action == ea_group_move_offset)
}

/// Returns `true` when `name == b"_TlsCipherBaseline"` for `SaveConfigXmlCommand::Execute`.
pub fn xmlrpc_is_tls_cipher_baseline_entry(name: &[u8]) -> bool {
    name == b"_TlsCipherBaseline"
}

/// Concatenates `decoded_path` and (when `has_query` is true) `&url[query_offset..]`
/// for `XmlRpcProcessor::SetUrl`.
pub fn xmlrpc_concat_decoded_path_and_query(
    decoded_path: &[u8],
    url: &[u8],
    has_query: bool,
    query_offset: usize,
) -> Vec<u8> {
    let query = if has_query && query_offset <= url.len() {
        &url[query_offset..]
    } else {
        &[][..]
    };
    let mut out = Vec::with_capacity(decoded_path.len() + query.len());
    out.extend_from_slice(decoded_path);
    out.extend_from_slice(query);
    out
}

/// Classifies the `TlsCipherXmlCommand::Execute` operation code:
/// `0` = `Start`, `1` = `Status`, `2` = `Cancel`.
pub fn xmlrpc_classify_tls_cipher_op(operation: i32) -> i32 {
    if operation == 0 {
        0
    } else if operation == 1 {
        1
    } else {
        2
    }
}

/// Returns true when `AppendFleetXmlCommand::Execute` has reached or exceeded `max_members`.
pub fn xmlrpc_is_fleet_member_limit_reached(current_count: usize, max_members: i32) -> bool {
    (current_count as i32) >= max_members
}

/// Returns true when `certVerifLevel` is within `[0, count)` in `TestServerXmlCommand::Execute`.
pub fn xmlrpc_is_valid_cert_verif_level(level: i32, count: i32) -> bool {
    level >= 0 && level < count
}

/// Checks individual argument bounds in `TestDiskSpeedXmlCommand::Execute`:
/// `1` = `write_buffer_kib < 0`, `2` = `max_file_size_gib < 1`, `3` = `timeout_sec < 1`, `0` = valid.
pub fn xmlrpc_check_disk_speed_arg_ranges(
    write_buffer_kib: i32,
    max_file_size_gib: i32,
    timeout_sec: i32,
) -> i32 {
    if write_buffer_kib < 0 {
        1
    } else if max_file_size_gib < 1 {
        2
    } else if timeout_sec < 1 {
        3
    } else {
        0
    }
}

/// Returns true when `XmlCommand::IsJson` is active (`protocol == rp_json_rpc || protocol == rp_jsonp_rpc`).
pub fn xmlrpc_is_json_protocol(protocol: i32, rp_json_rpc: i32, rp_jsonp_rpc: i32) -> bool {
    protocol == rp_json_rpc || protocol == rp_jsonp_rpc
}

/// Returns true when `XmlCommand::PrepareParams` should advance `m_requestPtr` to `"params"` (`is_json && http_method == hm_post`).
pub fn xmlrpc_should_skip_to_json_post_params(
    is_json: bool,
    http_method: i32,
    hm_post: i32,
) -> bool {
    is_json && http_method == hm_post
}

/// Returns true when `TestServerXmlCommand::Execute` should parse JSON-RPC POST parameters via `nzbget_rs_test_server_params` (`is_json && has_request && http_method == hm_post`).
pub fn xmlrpc_should_parse_json_post_test_server(
    is_json: bool,
    has_request: bool,
    http_method: i32,
    hm_post: i32,
) -> bool {
    is_json && has_request && http_method == hm_post
}

/// Returns true when `StartScriptXmlCommand::Execute` has extracted a complete option `(name, value)` pair (`(is_json && has_p1 && has_p2 && has_p3 && has_p4) || (!is_json && has_p1 && has_p2)`).
pub fn xmlrpc_classify_start_script_param_mode(
    is_json: bool,
    has_p1: bool,
    has_p2: bool,
    has_p3: bool,
    has_p4: bool,
) -> bool {
    if is_json {
        has_p1 && has_p2 && has_p3 && has_p4
    } else {
        has_p1 && has_p2
    }
}

/// Encodes `raw` using JSON escaping when `is_json` is true or XML entity escaping otherwise
/// (`XmlCommand::EncodeStr`).
pub fn xmlrpc_encode_str(is_json: bool, raw: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    if is_json {
        crate::escape::json_encode(raw, &mut v);
    } else {
        crate::escape::xml_encode(raw, &mut v);
    }
    v
}

/// Decodes `buf` in place using JSON unescaping when `is_json` is true or XML entity
/// decoding otherwise (`XmlCommand::DecodeStr`), returning the decoded byte length.
pub fn xmlrpc_decode_str(is_json: bool, buf: &mut [u8]) -> usize {
    if is_json {
        crate::decode::json_decode(buf)
    } else {
        crate::text::xml_decode(buf, &|c| c.to_ascii_lowercase() as c_int)
    }
}

/// Classifies `PauseUnpauseXmlCommand::Execute` pause action:
/// `1` = `pa_download`, `2` = `pa_post_process`, `3` = `pa_scan`, `0` = invalid.
pub fn xmlrpc_classify_pause_action(
    pause_action: i32,
    pa_download: i32,
    pa_post_process: i32,
    pa_scan: i32,
) -> i32 {
    if pause_action == pa_download {
        1
    } else if pause_action == pa_post_process {
        2
    } else if pause_action == pa_scan {
        3
    } else {
        0
    }
}

/// Returns the opening array delimiter for JSON-RPC (`"[\n"`) or XML-RPC (`"<array><data>\n"`).
pub fn xmlrpc_array_open_tag(is_json: bool) -> &'static CStr {
    if is_json {
        c"[\n"
    } else {
        c"<array><data>\n"
    }
}

/// Returns the closing array delimiter for JSON-RPC (`"\n]"`) or XML-RPC (`"</data></array>\n"`).
pub fn xmlrpc_array_close_tag(is_json: bool) -> &'static CStr {
    if is_json {
        c"\n]"
    } else {
        c"</data></array>\n"
    }
}

/// Computes `ThreadCount` reported by `StatusXmlCommand::Execute` (`total_threads - 1`, excluding the RPC worker thread itself).
pub fn xmlrpc_status_effective_thread_count(total_threads: i32) -> i32 {
    total_threads.wrapping_sub(1)
}

/// Returns `tier_name` when present and non-empty, or `"NORMAL"` when `MemoryMonitor` is absent (`StatusXmlCommand::Execute`).
pub fn xmlrpc_status_default_memory_pressure_tier(tier_name: Option<&CStr>) -> &CStr {
    match tier_name {
        Some(t) if !t.to_bytes().is_empty() => t,
        _ => c"NORMAL",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_match_command_all_methods() {
        assert_eq!(match_command(b"pause"), 1);
        assert_eq!(match_command(b"PAUSEDOWnload2"), 1);
        assert_eq!(match_command(b"resume"), 2);
        assert_eq!(match_command(b"status"), 8);
        assert_eq!(match_command(b"appendfleet"), 16);
        assert_eq!(match_command(b"servervolumes"), 46);
        assert_eq!(match_command(b"logscript"), 0);
        assert_eq!(match_command(b"loadextensions"), 0);
        assert_eq!(match_command(b"startscript"), 0);
        assert_eq!(match_command(b"unknown"), 0);
        assert_eq!(match_command(b""), 0);
        assert!(!is_safe_command(1));
        assert!(is_safe_command(8));
        assert!(is_safe_command(13));
        assert!(is_safe_command(20));
        assert!(is_safe_command(45));
        assert!(!is_safe_command(31));
        assert!(!is_safe_command(56));
        assert!(!is_safe_command(0));
    }

    #[test]
    fn test_primitives_and_error_formatting() {
        assert_eq!(format_bool_bytes(true, true), b"true");
        assert_eq!(format_bool_bytes(false, false), b"<boolean>0</boolean>");
        assert_eq!(format_int_bytes(true, -42), b"-42");
        assert_eq!(format_int_bytes(false, 42), b"<i4>42</i4>");

        let msg = CString::new("Bad <param> \"x\"").unwrap();
        let err_json = String::from_utf8(format_error_bytes(true, 2, msg.as_bytes())).unwrap();
        assert!(err_json.contains("\"code\" : 2"));
        assert!(err_json.contains("\\\"x\\\""));

        let err_xml = String::from_utf8(format_error_bytes(false, 2, msg.as_bytes())).unwrap();
        assert!(err_xml.contains("<i4>2</i4>"));
        assert!(err_xml.contains("&lt;param&gt;"));

        assert_eq!(xmlrpc_encode_str(true, b""), b"");
        assert_eq!(
            xmlrpc_encode_str(true, msg.as_bytes()),
            b"Bad <param> \\\"x\\\""
        );
        assert_eq!(
            xmlrpc_encode_str(false, msg.as_bytes()),
            b"Bad &lt;param&gt; &quot;x&quot;"
        );

        let mut json_buf = b"Bad <param> \\\"x\\\"".to_vec();
        let jlen = xmlrpc_decode_str(true, &mut json_buf);
        assert_eq!(&json_buf[..jlen], b"Bad <param> \"x\"");

        let mut xml_buf = b"Bad &lt;param&gt; &quot;x&quot;".to_vec();
        let xlen = xmlrpc_decode_str(false, &mut xml_buf);
        assert_eq!(&xml_buf[..xlen], b"Bad <param> \"x\"");

        assert_eq!(xmlrpc_classify_pause_action(0, 0, 1, 2), 1);
        assert_eq!(xmlrpc_classify_pause_action(1, 0, 1, 2), 2);
        assert_eq!(xmlrpc_classify_pause_action(2, 0, 1, 2), 3);
        assert_eq!(xmlrpc_classify_pause_action(99, 0, 1, 2), 0);

        assert_eq!(xmlrpc_array_open_tag(true).to_bytes(), b"[\n");
        assert_eq!(xmlrpc_array_open_tag(false).to_bytes(), b"<array><data>\n");
        assert_eq!(xmlrpc_array_close_tag(true).to_bytes(), b"\n]");
        assert_eq!(
            xmlrpc_array_close_tag(false).to_bytes(),
            b"</data></array>\n"
        );
        assert_eq!(xmlrpc_status_effective_thread_count(5), 4);
        assert_eq!(xmlrpc_status_effective_thread_count(1), 0);
        assert_eq!(
            xmlrpc_status_default_memory_pressure_tier(None).to_bytes(),
            b"NORMAL"
        );
        assert_eq!(
            xmlrpc_status_default_memory_pressure_tier(Some(c"HIGH")).to_bytes(),
            b"HIGH"
        );
    }

    #[test]
    fn test_log_and_config_list_formatting() {
        unsafe {
            let text = CString::new("Hello & world").unwrap();
            let entry = NzbgetRsRpcLogEntry {
                id: 7,
                kind: 1,
                time: 1700000000,
                text: text.as_ptr(),
            };
            let mut json_buf = Vec::new();
            write_log_list(&mut json_buf, true, &entry, 1);
            let json = String::from_utf8(json_buf).unwrap();
            assert!(json.contains("\"Kind\" : \"WARNING\""));
            assert!(json.contains("\"Text\" : \"Hello & world\""));

            let mut xml_buf = Vec::new();
            write_log_list(&mut xml_buf, false, &entry, 1);
            let xml = String::from_utf8(xml_buf).unwrap();
            assert!(xml.contains("<string>Hello &amp; world</string>"));

            let k = CString::new("Server1.Password").unwrap();
            let v = CString::new("secret").unwrap();
            let cfg = NzbgetRsRpcConfigItem {
                name: k.as_ptr(),
                value: v.as_ptr(),
                restricted: 1,
            };
            let mut masked_buf = Vec::new();
            write_config_list(&mut masked_buf, true, 1, 1, &cfg, 1);
            let masked = String::from_utf8(masked_buf).unwrap();
            assert!(masked.contains("\"Value\" : \"***\""));
            let mut unmasked_buf = Vec::new();
            write_config_list(&mut unmasked_buf, true, 0, 1, &cfg, 1);
            let unmasked = String::from_utf8(unmasked_buf).unwrap();
            assert!(unmasked.contains("\"Value\" : \"secret\""));

            let name = CString::new("Release").unwrap();
            let key = CString::new("series=1").unwrap();
            let mut dup_json_buf = Vec::new();
            write_history_dup_item(
                &mut dup_json_buf,
                true,
                0,
                12,
                name.as_ptr(),
                1700000000,
                1048576,
                key.as_ptr(),
                100,
                0,
                4,
            );
            let dup_json = String::from_utf8(dup_json_buf).unwrap();
            assert!(dup_json.ends_with("\"Status\" : \"DELETED/DUPE\"\n"));
            let mut dup_xml_buf = Vec::new();
            write_history_dup_item(
                &mut dup_xml_buf,
                false,
                0,
                12,
                name.as_ptr(),
                1700000000,
                1048576,
                key.as_ptr(),
                100,
                0,
                4,
            );
            let dup_xml = String::from_utf8(dup_xml_buf).unwrap();
            assert!(dup_xml.ends_with("<member><name>Status</name><value><string>DELETED/DUPE</string></value></member>\n"));

            let d_json = String::from_utf8(format_disk_speed_bytes(
                true,
                100 * 1024 * 1024,
                250.5,
                200 * 1024 * 1024,
                125.0,
            ))
            .unwrap();
            assert_eq!(
                d_json,
                "{\"SizeMB\":100,\"DurationMS\":250.5,\"ReadSizeMB\":200,\"ReadDurationMS\":125.0}"
            );
            let n_xml = String::from_utf8(format_network_speed_bytes(false, 940.25)).unwrap();
            assert_eq!(
                n_xml,
                "<value><struct><member><name>SpeedMbps</name><value><double>940.250000</double></value></member></struct></value>"
            );
        }
    }
}

/// Checks whether `XmlRpcProcessor::CreateCommand` should deny `method_name` for `user_access`.
pub fn xmlrpc_check_user_access_denied(
    user_access: i32,
    ua_add: i32,
    ua_restricted: i32,
    method_name: &[u8],
) -> i32 {
    if user_access == ua_add {
        let allowed = method_name.eq_ignore_ascii_case(b"append")
            || method_name.eq_ignore_ascii_case(b"appendurl")
            || method_name.eq_ignore_ascii_case(b"appendfleet")
            || method_name.eq_ignore_ascii_case(b"version");
        if !allowed {
            return 1;
        }
    } else if user_access == ua_restricted {
        let forbidden = method_name.eq_ignore_ascii_case(b"saveconfig")
            || method_name.eq_ignore_ascii_case(b"tlscipherstart")
            || method_name.eq_ignore_ascii_case(b"tlscipherstatus")
            || method_name.eq_ignore_ascii_case(b"tlsciphercancel");
        if forbidden {
            return 2;
        }
    }
    0
}

/// Parses `"score"`, `"all"`, or `"force"` (case-insensitive) into `EDupeMode`, or `-1` on invalid.
pub fn xmlrpc_parse_dupe_mode(
    dupe_mode_str: &[u8],
    dm_score: i32,
    dm_all: i32,
    dm_force: i32,
) -> i32 {
    if dupe_mode_str.eq_ignore_ascii_case(b"score") {
        dm_score
    } else if dupe_mode_str.eq_ignore_ascii_case(b"all") {
        dm_all
    } else if dupe_mode_str.eq_ignore_ascii_case(b"force") {
        dm_force
    } else {
        -1
    }
}

/// Returns whether `content` begins with `"http://"` or `"https://"` (case-insensitive).
pub fn xmlrpc_is_http_url(content: &[u8]) -> bool {
    (content.len() >= 7 && content[..7].eq_ignore_ascii_case(b"http://"))
        || (content.len() >= 8 && content[..8].eq_ignore_ascii_case(b"https://"))
}

/// Parses `CategorySource` (`"FeedFile"`, `"Auto"`, or default `NZBFile`) in `ViewFeedXmlCommand`.
pub fn xmlrpc_parse_category_source(
    cat_src_str: &[u8],
    cs_nzb_file: i32,
    cs_feed_file: i32,
    cs_auto: i32,
) -> i32 {
    if cat_src_str.eq_ignore_ascii_case(b"FeedFile") {
        cs_feed_file
    } else if cat_src_str.eq_ignore_ascii_case(b"Auto") {
        cs_auto
    } else {
        cs_nzb_file
    }
}

/// `StatusXmlCommand::Execute`: computes `averageDownloadRate`.
pub fn xmlrpc_calc_average_download_rate(download_time_sec: i32, all_bytes: i64) -> i64 {
    if download_time_sec > 0 {
        all_bytes / i64::from(download_time_sec)
    } else {
        0
    }
}

/// `NzbInfoXmlCommand::AppendNzbInfoFields`: resolves effective `messageCount` and `postTotalTimeSec`.
pub fn xmlrpc_nzb_message_and_post_time(
    message_count: i32,
    cached_message_count: i32,
    post_total_sec: i32,
    has_post_start_time: bool,
    cur_time: i64,
    post_start_time: i64,
) -> (i32, i32) {
    let eff_msg_count = if message_count > 0 {
        message_count
    } else {
        cached_message_count
    };
    let active_post_sec = if has_post_start_time {
        cur_time.wrapping_sub(post_start_time)
    } else {
        0
    };
    let post_total_time_sec = i64::from(post_total_sec).wrapping_add(active_post_sec) as i32;
    (eff_msg_count, post_total_time_sec)
}

/// `SaveConfigXmlCommand::Execute`: validates JSON option entry keys `(key1, key2)`.
pub unsafe fn xmlrpc_save_config_check_json_keys(key1: *const c_char, key2: *const c_char) -> i32 {
    if key1.is_null() || key2.is_null() {
        return 0;
    }
    let k1 = CStr::from_ptr(key1).to_bytes();
    let k2 = CStr::from_ptr(key2).to_bytes();
    if k1.eq_ignore_ascii_case(b"Value") && k2.eq_ignore_ascii_case(b"Name") {
        2
    } else if k1.eq_ignore_ascii_case(b"Name") && k2.eq_ignore_ascii_case(b"Value") {
        1
    } else {
        0
    }
}

/// `SaveConfigXmlCommand::Execute`: checks that the raw `_TlsCipherBaseline` value is valid.
pub fn xmlrpc_save_config_raw_baseline_valid(raw_value: &[u8]) -> bool {
    !raw_value.windows(6).any(|w| w == b"\\u0000") && !raw_value.windows(2).any(|w| w == b"&#")
}

/// `SaveConfigXmlCommand::Execute`: checks that the decoded `_TlsCipherBaseline` value is valid.
pub fn xmlrpc_save_config_decoded_baseline_valid(
    has_cipher_baseline: bool,
    decoded_value: &[u8],
) -> bool {
    !has_cipher_baseline && decoded_value.len() <= 1024 * 1024 && decoded_value.starts_with(b"v1\n")
}

/// Returns 1 when a TLS cipher optimizer RPC call has control access over JSON-RPC POST.
pub fn roc_xmlrpc_tls_cipher_access_allowed(
    user_access: c_int,
    ua_control: c_int,
    protocol: c_int,
    rp_json_rpc: c_int,
    http_method: c_int,
    hm_post: c_int,
) -> c_int {
    if user_access == ua_control && protocol == rp_json_rpc && http_method == hm_post {
        1
    } else {
        0
    }
}

/// Validates TLS cipher optimizer job ID parameter arity, string type, length (<= 128),
/// non-emptiness, and absence of embedded NUL bytes.
pub unsafe fn roc_xmlrpc_tls_cipher_validate_job_id(
    operation: c_int,
    param_count: usize,
    first_is_string: c_int,
    id_ptr: *const c_char,
    id_len: usize,
) -> c_int {
    if (operation == 0 && param_count != 0)
        || (operation == 1 && param_count > 1)
        || (operation == 2 && param_count != 1)
    {
        return 0;
    }
    if param_count > 0 {
        if first_is_string == 0 || id_ptr.is_null() || id_len == 0 || id_len > 128 {
            return 0;
        }
        let slice = unsafe { std::slice::from_raw_parts(id_ptr as *const u8, id_len) };
        if slice.contains(&0) {
            return 0;
        }
    }
    1
}

/// Computes megabyte fields for `StatusXmlCommand::Execute`.
#[allow(clippy::too_many_arguments)]
pub unsafe fn roc_xmlrpc_status_calc_mbytes(
    remaining_size: i64,
    forced_size: i64,
    article_cache: i64,
    all_bytes: i64,
    month_bytes: i64,
    day_bytes: i64,
    out_remaining_mb: *mut c_int,
    out_forced_mb: *mut c_int,
    out_cache_mb: *mut c_int,
    out_downloaded_mb: *mut c_int,
    out_month_mb: *mut c_int,
    out_day_mb: *mut c_int,
) {
    if !out_remaining_mb.is_null() {
        unsafe { *out_remaining_mb = (remaining_size / 1024 / 1024) as c_int };
    }
    if !out_forced_mb.is_null() {
        unsafe { *out_forced_mb = (forced_size / 1024 / 1024) as c_int };
    }
    if !out_cache_mb.is_null() {
        unsafe { *out_cache_mb = (article_cache / 1024 / 1024) as c_int };
    }
    if !out_downloaded_mb.is_null() {
        unsafe { *out_downloaded_mb = (all_bytes / 1024 / 1024) as c_int };
    }
    if !out_month_mb.is_null() {
        unsafe { *out_month_mb = (month_bytes / 1024 / 1024) as c_int };
    }
    if !out_day_mb.is_null() {
        unsafe { *out_day_mb = (day_bytes / 1024 / 1024) as c_int };
    }
}

/// Resolves `InterDir` disk space fallback when `InterDir` is empty and computes MB values.
#[allow(clippy::too_many_arguments)]
pub unsafe fn roc_xmlrpc_status_calc_disk_space_mb(
    free_disk: i64,
    total_disk: i64,
    inter_dir_empty: c_int,
    free_inter: i64,
    total_inter: i64,
    out_free_inter: *mut i64,
    out_total_inter: *mut i64,
    out_free_disk_mb: *mut c_int,
    out_total_disk_mb: *mut c_int,
    out_free_inter_mb: *mut c_int,
    out_total_inter_mb: *mut c_int,
) {
    let eff_free_inter = if inter_dir_empty != 0 {
        free_disk
    } else {
        free_inter
    };
    let eff_total_inter = if inter_dir_empty != 0 {
        total_disk
    } else {
        total_inter
    };
    if !out_free_inter.is_null() {
        unsafe { *out_free_inter = eff_free_inter };
    }
    if !out_total_inter.is_null() {
        unsafe { *out_total_inter = eff_total_inter };
    }
    if !out_free_disk_mb.is_null() {
        unsafe { *out_free_disk_mb = (free_disk / 1024 / 1024) as c_int };
    }
    if !out_total_disk_mb.is_null() {
        unsafe { *out_total_disk_mb = (total_disk / 1024 / 1024) as c_int };
    }
    if !out_free_inter_mb.is_null() {
        unsafe { *out_free_inter_mb = (eff_free_inter / 1024 / 1024) as c_int };
    }
    if !out_total_inter_mb.is_null() {
        unsafe { *out_total_inter_mb = (eff_total_inter / 1024 / 1024) as c_int };
    }
}

/// Computes `StageTimeSec` and `TotalTimeSec` for `NzbInfoXmlCommand::AppendPostInfoFields`.
pub unsafe fn roc_xmlrpc_post_info_elapsed_sec(
    cur_time: i64,
    stage_time: i64,
    start_time: i64,
    out_stage_sec: *mut c_int,
    out_total_sec: *mut c_int,
) {
    let stage_sec = if stage_time != 0 {
        (cur_time - stage_time) as c_int
    } else {
        0
    };
    let total_sec = if start_time != 0 {
        (cur_time - start_time) as c_int
    } else {
        0
    };
    if !out_stage_sec.is_null() {
        unsafe { *out_stage_sec = stage_sec };
    }
    if !out_total_sec.is_null() {
        unsafe { *out_total_sec = total_sec };
    }
}

/// Computes `RemainingSizeMB` and `PausedSizeMB` for `ListGroupsXmlCommand::Execute`.
pub unsafe fn roc_xmlrpc_list_groups_calc_mb(
    remaining_size: i64,
    paused_size: i64,
    out_remaining_mb: *mut c_int,
    out_paused_mb: *mut c_int,
) {
    if !out_remaining_mb.is_null() {
        unsafe { *out_remaining_mb = (remaining_size / 1024 / 1024) as c_int };
    }
    if !out_paused_mb.is_null() {
        unsafe { *out_paused_mb = (paused_size / 1024 / 1024) as c_int };
    }
}

/// Returns 1 when an option value should be masked as `"***"` for restricted RPC users.
pub fn roc_xmlrpc_config_should_mask_value(
    user_access: c_int,
    ua_restricted: c_int,
    opt_restricted: c_int,
) -> c_int {
    if user_access == ua_restricted && opt_restricted != 0 {
        1
    } else {
        0
    }
}

/// Computes `totalSizeMB` and `customSizeMB` for `ServerVolumesXmlCommand::Execute`.
pub unsafe fn roc_xmlrpc_server_volumes_calc_mb(
    total_bytes: i64,
    custom_bytes: i64,
    out_total_mb: *mut u32,
    out_custom_mb: *mut u32,
) {
    if !out_total_mb.is_null() {
        unsafe { *out_total_mb = ((total_bytes / 1024 / 1024) as i32) as u32 };
    }
    if !out_custom_mb.is_null() {
        unsafe { *out_custom_mb = ((custom_bytes / 1024 / 1024) as i32) as u32 };
    }
}

/// Validates `TestDiskSpeedXmlCommand::Execute` parameters and computes byte sizes.
pub unsafe fn roc_xmlrpc_test_disk_speed_validate_params(
    write_buffer_kib: c_int,
    max_file_size_gib: c_int,
    timeout_sec: c_int,
    out_buffer_bytes: *mut usize,
    out_max_file_bytes: *mut u64,
) -> c_int {
    if write_buffer_kib < 0 {
        return 1;
    }
    if max_file_size_gib < 1 {
        return 2;
    }
    if timeout_sec < 1 {
        return 3;
    }
    if !out_buffer_bytes.is_null() {
        unsafe { *out_buffer_bytes = (write_buffer_kib as usize) * 1024usize };
    }
    if !out_max_file_bytes.is_null() {
        unsafe {
            *out_max_file_bytes = (max_file_size_gib as u64) * 1024u64 * 1024u64 * 1024u64;
        }
    }
    0
}

const POST_STAGE_CS_NAMES: [&CStr; 15] = [
    c"PP_QUEUED",
    c"LOADING_PARS",
    c"VERIFYING_SOURCES",
    c"REPAIRING",
    c"VERIFYING_REPAIRED",
    c"RENAMING",
    c"RENAMING",
    c"UNPACKING",
    c"MOVING",
    c"MOVING",
    c"POST_UNPACK_RENAMING",
    c"EXECUTING_SCRIPT",
    c"PP_FINISHED",
    c"POST_DOWNLOAD_RENAMING",
    c"STREAM_REPAIRING",
];

/// Maps `NzbInfo` state to the RPC group status string for `ListGroupsXmlCommand::DetectStatus`.
#[allow(clippy::too_many_arguments)]
pub fn rpc_detect_group_status(
    has_post_info: bool,
    post_stage: i32,
    has_queue_script_job: bool,
    queue_script_active: bool,
    direct_unpack_status: i32,
    active_downloads: i32,
    kind: i32,
    paused_size: i64,
    remaining_size: i64,
) -> &'static CStr {
    if has_post_info {
        if post_stage == 0 && has_queue_script_job {
            if queue_script_active {
                c"QS_EXECUTING"
            } else {
                c"QS_QUEUED"
            }
        } else if direct_unpack_status == 1 {
            c"UNPACKING"
        } else if (0..POST_STAGE_CS_NAMES.len() as i32).contains(&post_stage) {
            POST_STAGE_CS_NAMES[post_stage as usize]
        } else {
            c"PP_QUEUED"
        }
    } else if active_downloads > 0 {
        if kind == 1 {
            c"FETCHING"
        } else {
            c"DOWNLOADING"
        }
    } else if paused_size > 0 && remaining_size == paused_size {
        c"PAUSED"
    } else {
        c"QUEUED"
    }
}

const EDIT_COMMAND_MAP: [(&[u8], i32); 51] = [
    (b"FileMoveOffset", 1),
    (b"FileMoveTop", 2),
    (b"FileMoveBottom", 3),
    (b"FilePause", 4),
    (b"FileResume", 5),
    (b"FileDelete", 6),
    (b"FilePauseAllPars", 7),
    (b"FilePauseExtraPars", 8),
    (b"FileReorder", 9),
    (b"FileSplit", 10),
    (b"GroupMoveOffset", 11),
    (b"GroupMoveTop", 12),
    (b"GroupMoveBottom", 13),
    (b"GroupMoveBefore", 14),
    (b"GroupMoveAfter", 15),
    (b"GroupPause", 16),
    (b"GroupResume", 17),
    (b"GroupDelete", 18),
    (b"GroupParkDelete", 19),
    (b"GroupDupeDelete", 20),
    (b"GroupFinalDelete", 21),
    (b"GroupPauseAllPars", 22),
    (b"GroupPauseExtraPars", 23),
    (b"GroupSetPriority", 24),
    (b"GroupSetCategory", 25),
    (b"GroupApplyCategory", 26),
    (b"GroupMerge", 27),
    (b"GroupSetParameter", 28),
    (b"GroupSetName", 29),
    (b"GroupSetDupeKey", 30),
    (b"GroupSetDupeScore", 31),
    (b"GroupSetDupeMode", 32),
    (b"GroupSort", 33),
    (b"GroupSortFiles", 34),
    (b"PostDelete", 35),
    (b"HistoryDelete", 36),
    (b"HistoryFinalDelete", 37),
    (b"HistoryReturn", 38),
    (b"HistoryProcess", 39),
    (b"HistoryRedownload", 40),
    (b"HistoryRetryFailed", 41),
    (b"HistorySetParameter", 42),
    (b"HistorySetDupeKey", 43),
    (b"HistorySetDupeScore", 44),
    (b"HistorySetDupeMode", 45),
    (b"HistorySetDupeBackup", 46),
    (b"HistoryMarkBad", 47),
    (b"HistoryMarkGood", 48),
    (b"HistoryMarkSuccess", 49),
    (b"HistorySetCategory", 50),
    (b"HistorySetName", 51),
];

/// Looks up `DownloadQueue::EEditAction` (`1..=51`) by case-insensitive command name.
pub fn rpc_lookup_edit_command(edit_command: &[u8]) -> i32 {
    for &(name, action_id) in &EDIT_COMMAND_MAP {
        if edit_command.eq_ignore_ascii_case(name) {
            return action_id;
        }
    }
    -1
}

/// Parses an integer prefix like `strtoll(text, &end, 10)` and checks `INT_MIN..=INT_MAX`.
pub fn rpc_parse_int_param(text: &[u8]) -> Option<i32> {
    let mut i = 0;
    while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let rest = &text[i..];
    if rest.is_empty() {
        return None;
    }
    let (neg, digits_slice) = if rest[0] == b'-' {
        (true, &rest[1..])
    } else if rest[0] == b'+' {
        (false, &rest[1..])
    } else {
        (false, rest)
    };
    let d_len = digits_slice
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if d_len == 0 {
        return None;
    }
    let mut acc: i64 = 0;
    for &b in &digits_slice[..d_len] {
        let digit = (b - b'0') as i64;
        acc = acc.checked_mul(10)?;
        acc = if neg {
            acc.checked_sub(digit)?
        } else {
            acc.checked_add(digit)?
        };
    }
    i32::try_from(acc).ok()
}

/// Validates `EditQueueXmlCommand::Execute` arguments for score/priority, parameter, and dupe mode actions.
pub fn rpc_validate_edit_queue_args(action: i32, args: &[u8], parsed_int_ok: bool) -> bool {
    if (action == 24 || action == 31 || action == 44)
        && !(parsed_int_ok && rpc_parse_int_param(args).is_some())
    {
        return false;
    }
    if (action == 28 || action == 42)
        && (args.is_empty() || args[0] == b'=' || !args.contains(&b'='))
    {
        return false;
    }
    if action == 32 || action == 45 {
        let valid_mode = args.eq_ignore_ascii_case(b"SCORE")
            || args.eq_ignore_ascii_case(b"ALL")
            || args.eq_ignore_ascii_case(b"FORCE");
        if !valid_mode {
            return false;
        }
    }
    true
}

const HISTORY_DUP_STATUS_CS_NAMES: [&CStr; 7] = [
    c"FAILURE/INTERNAL_ERROR",
    c"SUCCESS/HIDDEN",
    c"FAILURE/HIDDEN",
    c"DELETED/MANUAL",
    c"DELETED/DUPE",
    c"FAILURE/BAD",
    c"SUCCESS/GOOD",
];

/// Maps `DupInfo::EStatus` (`0..=6`) to its RPC status string for `HistoryXmlCommand::DetectStatus`.
pub fn rpc_detect_history_dup_status(dup_status: i32) -> &'static CStr {
    if (0..HISTORY_DUP_STATUS_CS_NAMES.len() as i32).contains(&dup_status) {
        HISTORY_DUP_STATUS_CS_NAMES[dup_status as usize]
    } else {
        c"FAILURE/INTERNAL_ERROR"
    }
}

/// Parses a boolean option string in `SaveConfigXmlCommand::Execute`, falling back to `def_val`.
pub fn rpc_save_config_parse_bool(val: Option<&[u8]>, def_val: bool) -> bool {
    let Some(v) = val else {
        return def_val;
    };
    if v.eq_ignore_ascii_case(b"yes")
        || v.eq_ignore_ascii_case(b"true")
        || v == b"1"
        || v.eq_ignore_ascii_case(b"on")
        || v.eq_ignore_ascii_case(b"enable")
        || v.eq_ignore_ascii_case(b"enabled")
    {
        return true;
    }
    if v.eq_ignore_ascii_case(b"no")
        || v.eq_ignore_ascii_case(b"false")
        || v == b"0"
        || v.eq_ignore_ascii_case(b"off")
        || v.eq_ignore_ascii_case(b"disable")
        || v.eq_ignore_ascii_case(b"disabled")
    {
        return false;
    }
    def_val
}

/// Evaluates global configuration conflict rules R7.2, R7.3, and R7.4 in `SaveConfigXmlCommand::Execute`.
pub fn rpc_save_config_check_global_conflicts(
    dupe_search_val: Option<&[u8]>,
    health_check_val: Option<&[u8]>,
    flush_queue_val: Option<&[u8]>,
    direct_write_val: Option<&[u8]>,
    unpack_pause_queue_val: Option<&[u8]>,
    direct_unpack_val: Option<&[u8]>,
) -> i32 {
    if dupe_search_val.is_some() || health_check_val.is_some() {
        let dupe_search = rpc_save_config_parse_bool(dupe_search_val, false);
        let health_check_park = health_check_val
            .is_some_and(|v| v.eq_ignore_ascii_case(b"park") || v.eq_ignore_ascii_case(b"hcpark"));
        if dupe_search && health_check_park {
            return 2;
        }
    }

    if flush_queue_val.is_some() || direct_write_val.is_some() {
        let flush_queue = rpc_save_config_parse_bool(flush_queue_val, true);
        let direct_write = rpc_save_config_parse_bool(direct_write_val, true);
        if direct_write && !flush_queue {
            return 3;
        }
    }

    if unpack_pause_queue_val.is_some() || direct_unpack_val.is_some() {
        let unpack_pause_queue = rpc_save_config_parse_bool(unpack_pause_queue_val, false);
        let direct_unpack = rpc_save_config_parse_bool(direct_unpack_val, false);
        if direct_unpack && !unpack_pause_queue {
            return 4;
        }
    }

    0
}

fn parse_server_dot_index(after_server: &[u8]) -> Option<i32> {
    let mut i = 0;
    while i < after_server.len()
        && matches!(after_server[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
    {
        i += 1;
    }
    let rest = &after_server[i..];
    if rest.is_empty() {
        return None;
    }
    let (neg, digits) = if rest[0] == b'-' {
        (true, &rest[1..])
    } else if rest[0] == b'+' {
        (false, &rest[1..])
    } else {
        (false, rest)
    };
    let d_len = digits.iter().take_while(|b| b.is_ascii_digit()).count();
    if d_len == 0 || digits.get(d_len) != Some(&b'.') || neg {
        return None;
    }
    let s_idx = std::str::from_utf8(&digits[..d_len])
        .ok()?
        .parse::<i32>()
        .ok()?;
    if s_idx > 0 {
        Some(s_idx)
    } else {
        None
    }
}

/// Evaluates Rule 7.1 in `SaveConfigXmlCommand::Execute`.
pub fn rpc_save_config_find_cipher_conflict(names: &[&[u8]], values: &[&[u8]]) -> i32 {
    let count = names.len().min(values.len());
    let mut checked = Vec::new();
    for i in 0..count {
        let entry_name = names[i];
        if entry_name.len() > 6 && entry_name[..6].eq_ignore_ascii_case(b"Server") {
            if let Some(s_index) = parse_server_dot_index(&entry_name[6..]) {
                if checked.contains(&s_index) {
                    continue;
                }
                checked.push(s_index);

                let cipher_opt = format!("Server{}.Cipher", s_index);
                let enc_opt = format!("Server{}.Encryption", s_index);

                let cipher_val = (0..count)
                    .find(|&j| names[j].eq_ignore_ascii_case(cipher_opt.as_bytes()))
                    .map(|j| values[j]);

                if let Some(cv) = cipher_val {
                    if !cv.is_empty() {
                        let enc_val = (0..count)
                            .find(|&j| names[j].eq_ignore_ascii_case(enc_opt.as_bytes()))
                            .map(|j| values[j]);
                        let enc_disabled = match enc_val {
                            Some(ev) => !rpc_save_config_parse_bool(Some(ev), false),
                            None => true,
                        };
                        if enc_disabled {
                            return s_index;
                        }
                    }
                }
            }
        }
    }
    0
}

pub fn format_error_bytes(is_json: bool, err_code: i32, err_text: &[u8]) -> Vec<u8> {
    let encoded = encode_str(is_json, err_text);
    let mut out = Vec::with_capacity(256);
    if is_json {
        let _ = write!(
            out,
            "{{\n\"name\" : \"JSONRPCError\",\n\"code\" : {},\n\"message\" : \"{}\"\n}}",
            err_code, encoded
        );
    } else {
        let _ = write!(
            out,
            "<struct>\n<member><name>faultCode</name><value><i4>{}</i4></value></member>\n<member><name>faultString</name><value><string>{}</string></value></member>\n</struct>\n",
            err_code, encoded
        );
    }
    if out.len() > 1023 {
        out.truncate(1023);
    }
    out
}

pub fn format_bool_bytes(is_json: bool, ok: bool) -> Vec<u8> {
    let b = bool_str(is_json, ok);
    let mut out = Vec::with_capacity(32);
    if is_json {
        out.extend_from_slice(b.as_bytes());
    } else {
        let _ = write!(out, "<boolean>{}</boolean>", b);
    }
    out
}

pub fn format_int_bytes(is_json: bool, value: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    if is_json {
        let _ = write!(out, "{}", value);
    } else {
        let _ = write!(out, "<i4>{}</i4>", value);
    }
    out
}

pub fn format_string_bytes(is_json: bool, raw: &[u8], encode: bool, max_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + 32);
    if is_json {
        out.push(b'"');
        if encode {
            crate::escape::json_encode(raw, &mut out);
        } else {
            out.extend_from_slice(raw);
        }
        out.push(b'"');
    } else {
        out.extend_from_slice(b"<string>");
        if encode {
            crate::escape::xml_encode(raw, &mut out);
        } else {
            out.extend_from_slice(raw);
        }
        out.extend_from_slice(b"</string>");
    }
    if max_len > 0 && out.len() > max_len {
        out.truncate(max_len);
    }
    out
}

pub fn format_config_list_bytes(
    is_json: bool,
    user_access: i32,
    ua_restricted: i32,
    entries: &[(&[u8], &[u8], bool)],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(entries.len() * 96 + 32);
    if is_json {
        out.extend_from_slice(b"[\n");
    } else {
        out.extend_from_slice(b"<array><data>\n");
    }
    for (idx, &(name, value, restricted)) in entries.iter().enumerate() {
        let mask =
            roc_xmlrpc_config_should_mask_value(user_access, ua_restricted, i32::from(restricted))
                != 0;
        let name_enc = encode_str(is_json, name);
        let val_enc = if mask {
            "***".to_string()
        } else {
            encode_str(is_json, value)
        };
        if is_json {
            if idx > 0 {
                out.extend_from_slice(b",\n");
            }
            let _ = write!(
                out,
                "{{\n\"Name\" : \"{}\",\n\"Value\" : \"{}\"\n}}",
                name_enc, val_enc
            );
        } else {
            let _ = write!(
                out,
                "<value><struct>\n<member><name>Name</name><value><string>{}</string></value></member>\n<member><name>Value</name><value><string>{}</string></value></member>\n</struct></value>\n",
                name_enc, val_enc
            );
        }
    }
    if is_json {
        out.extend_from_slice(b"\n]");
    } else {
        out.extend_from_slice(b"</data></array>\n");
    }
    out
}

/// Normalizes an RPC URL (`XmlRpcProcessor::SetUrl`): splits path and query at `'?'`,
/// URL-decodes the path portion only, and reattaches the raw query string.
pub fn xmlrpc_normalize_url(url: &[u8]) -> Vec<u8> {
    let (path_set_len, query_offset) = xmlrpc_split_url_path_and_query(url);
    let n = path_set_len as c_int;
    let mut path = if n > 0 {
        url[..(n as usize).min(url.len())].to_vec()
    } else {
        url.to_vec()
    };
    let decoded_len = crate::text::url_decode(&mut path);
    path.truncate(decoded_len);
    xmlrpc_concat_decoded_path_and_query(
        &path,
        url,
        query_offset.is_some(),
        query_offset.unwrap_or(0),
    )
}

#[cfg(test)]
fn raw_param_at(buf: &[u8], val_off: usize) -> &[u8] {
    let len = buf[val_off..].iter().position(|&b| b == 0).unwrap_or(0);
    &buf[val_off..val_off + len]
}

struct ParamCursor<'a> {
    raw: crate::rpcparams::Raw,
    xml_buf: Option<&'a mut [u8]>,
    cur_off: usize,
    kind: crate::rpcparams::Kind,
}

impl<'a> ParamCursor<'a> {
    unsafe fn from_raw(raw: crate::rpcparams::Raw, is_get: bool, is_json: bool) -> Self {
        let kind = if is_get {
            crate::rpcparams::Kind::Get
        } else if is_json {
            crate::rpcparams::Kind::Json
        } else {
            crate::rpcparams::Kind::Xml
        };
        let xml_buf = if kind == crate::rpcparams::Kind::Xml {
            Some(raw.terminated())
        } else {
            None
        };
        Self {
            raw,
            xml_buf,
            cur_off: 0,
            kind,
        }
    }

    fn from_slice(buf: &'a mut [u8], is_get: bool, is_json: bool) -> Option<Self> {
        let end = crate::bytes::find_byte(buf, 0)?;
        let terminated = &mut buf[..=end];
        let raw = unsafe { crate::rpcparams::Raw::new(terminated.as_mut_ptr()) };
        let kind = if is_get {
            crate::rpcparams::Kind::Get
        } else if is_json {
            crate::rpcparams::Kind::Json
        } else {
            crate::rpcparams::Kind::Xml
        };
        let xml_buf = if kind == crate::rpcparams::Kind::Xml {
            Some(terminated)
        } else {
            None
        };
        Some(Self {
            raw,
            xml_buf,
            cur_off: 0,
            kind,
        })
    }

    fn next_str(&mut self) -> Option<usize> {
        let (step, rel_val) = match self.kind {
            crate::rpcparams::Kind::Get | crate::rpcparams::Kind::Json => unsafe {
                self.raw.add(self.cur_off).next_str(self.kind)
            },
            crate::rpcparams::Kind::Xml => {
                let xml = self.xml_buf.as_deref_mut()?;
                if self.cur_off >= xml.len() {
                    return None;
                }
                crate::rpcparams::next_str_terminated(&mut xml[self.cur_off..], self.kind)
            }
        };
        let rel = rel_val?;
        let val_off = self.cur_off + rel;
        self.cur_off += step;
        Some(val_off)
    }

    fn next_int(&mut self) -> Option<i32> {
        let (step, val) = match self.kind {
            crate::rpcparams::Kind::Get | crate::rpcparams::Kind::Json => unsafe {
                self.raw.add(self.cur_off).next_int(self.kind)
            },
            crate::rpcparams::Kind::Xml => {
                let xml = self.xml_buf.as_deref_mut()?;
                if self.cur_off >= xml.len() {
                    return None;
                }
                crate::rpcparams::next_int_terminated(&mut xml[self.cur_off..], self.kind)
            }
        };
        let v = val?;
        self.cur_off += step;
        Some(v)
    }

    fn next_bool(&mut self, is_json: bool) -> Option<bool> {
        let (step, val) = match self.kind {
            crate::rpcparams::Kind::Get | crate::rpcparams::Kind::Json => unsafe {
                self.raw.add(self.cur_off).next_bool(self.kind, is_json)
            },
            crate::rpcparams::Kind::Xml => {
                let xml = self.xml_buf.as_deref_mut()?;
                if self.cur_off >= xml.len() {
                    return None;
                }
                crate::rpcparams::next_bool_terminated(&mut xml[self.cur_off..], self.kind, is_json)
            }
        };
        self.cur_off += step;
        val
    }

    fn raw_at(&self, val_off: usize) -> &[u8] {
        unsafe { self.raw.bytes_at(val_off) }
    }

    fn decode_at(&mut self, val_off: usize, is_json: bool) -> Vec<u8> {
        unsafe {
            self.raw
                .decode_at(val_off, is_json, &|c| c.to_ascii_lowercase() as c_int)
        }
    }
}

fn xmlrpc_execute_save_config_cursor(
    mut cur: ParamCursor<'_>,
    is_json: bool,
    config_filename: Option<&[u8]>,
) -> (usize, bool, Vec<u8>) {
    let mut opt_entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut cipher_baseline: Option<Vec<u8>> = None;
    let mut malformed = false;

    loop {
        let (name_off, val_off) = if is_json {
            let Some(key1_off) = cur.next_str() else {
                break;
            };
            let Some(mut n_off) = cur.next_str() else {
                malformed = true;
                break;
            };
            let Some(key2_off) = cur.next_str() else {
                malformed = true;
                break;
            };
            let Some(mut v_off) = cur.next_str() else {
                malformed = true;
                break;
            };
            let k1 = cur.raw_at(key1_off);
            let k2 = cur.raw_at(key2_off);
            if k1.eq_ignore_ascii_case(b"Value") && k2.eq_ignore_ascii_case(b"Name") {
                std::mem::swap(&mut n_off, &mut v_off);
            } else if !(k1.eq_ignore_ascii_case(b"Name") && k2.eq_ignore_ascii_case(b"Value")) {
                malformed = true;
                break;
            }
            (n_off, v_off)
        } else {
            let Some(n_off) = cur.next_str() else {
                break;
            };
            let Some(v_off) = cur.next_str() else {
                malformed = true;
                break;
            };
            (n_off, v_off)
        };

        let name = cur.decode_at(name_off, is_json);
        let baseline_entry = xmlrpc_is_tls_cipher_baseline_entry(&name);
        if baseline_entry && !xmlrpc_save_config_raw_baseline_valid(cur.raw_at(val_off)) {
            malformed = true;
            break;
        }
        let value = cur.decode_at(val_off, is_json);
        if baseline_entry {
            if !xmlrpc_save_config_decoded_baseline_valid(cipher_baseline.is_some(), &value) {
                malformed = true;
                break;
            }
            cipher_baseline = Some(value);
            continue;
        }
        opt_entries.push((name, value));
    }

    let cur_off = cur.cur_off;
    if malformed || opt_entries.is_empty() {
        return (
            cur_off,
            true,
            format_error_bytes(is_json, 2, b"Invalid parameter"),
        );
    }

    let names_refs: Vec<&[u8]> = opt_entries.iter().map(|(n, _)| n.as_slice()).collect();
    let values_refs: Vec<&[u8]> = opt_entries.iter().map(|(_, v)| v.as_slice()).collect();
    let cipher_conflict_server = rpc_save_config_find_cipher_conflict(&names_refs, &values_refs);
    if cipher_conflict_server > 0 {
        let msg = format!(
            "Configuration conflict: Server{cipher_conflict_server}.Cipher cannot be configured when Server{cipher_conflict_server}.Encryption is disabled. Clear the cipher suite or enable encryption."
        );
        return (
            cur_off,
            true,
            format_error_bytes(is_json, 3, msg.as_bytes()),
        );
    }

    let opt_val = |target: &[u8]| -> Option<&[u8]> {
        opt_entries
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(target))
            .map(|(_, v)| v.as_slice())
    };
    let conflict_rule = rpc_save_config_check_global_conflicts(
        opt_val(b"DupeSearch"),
        opt_val(b"HealthCheck"),
        opt_val(b"FlushQueue"),
        opt_val(b"DirectWrite"),
        opt_val(b"UnpackPauseQueue"),
        opt_val(b"DirectUnpack"),
    );
    if conflict_rule == 2 {
        return (
            cur_off,
            true,
            format_error_bytes(
                is_json,
                3,
                b"Configuration conflict: 'DupeSearch=yes' is incompatible with 'HealthCheck=park'. DupeSearch requires 'HealthCheck=dupe' to fetch alternative donor releases when an NZB fails.",
            ),
        );
    }
    if conflict_rule == 3 {
        return (
            cur_off,
            true,
            format_error_bytes(
                is_json,
                3,
                b"Configuration conflict: 'DirectWrite=yes' requires 'FlushQueue=yes'. Disabling queue flushing during direct disk writes causes disk state desynchronization and corruption upon unexpected shutdown.",
            ),
        );
    }
    if conflict_rule == 4 {
        return (
            cur_off,
            true,
            format_error_bytes(
                is_json,
                3,
                b"Configuration conflict: 'DirectUnpack=yes' requires 'UnpackPauseQueue=yes'. Disabling queue pause during direct unpack causes concurrent unpack disk thrashing.",
            ),
        );
    }

    let (ok, conflict) = match config_filename {
        Some(fname) if !fname.is_empty() => {
            let pairs: Vec<(&[u8], &[u8])> = opt_entries
                .iter()
                .map(|(n, v)| (n.as_slice(), v.as_slice()))
                .collect();
            crate::configfile::script_config_save(fname, &pairs, cipher_baseline.as_deref())
        }
        _ => (false, false),
    };
    if conflict {
        return (
            cur_off,
            true,
            format_error_bytes(
                is_json,
                3,
                b"News server settings changed in another operation. Refresh Settings before saving; your edits have been retained.",
            ),
        );
    }

    (cur_off, false, format_bool_bytes(is_json, ok))
}

/// Executes `SaveConfigXmlCommand::Execute` over a NUL-terminated request buffer.
/// Returns `(new_offset, fault, response_bytes)`.
pub fn xmlrpc_execute_save_config(
    buf: &mut [u8],
    is_get: bool,
    is_json: bool,
    config_filename: Option<&[u8]>,
) -> (usize, bool, Vec<u8>) {
    let Some(cur) = ParamCursor::from_slice(buf, is_get, is_json) else {
        return (
            0,
            true,
            format_error_bytes(is_json, 2, b"Invalid parameter"),
        );
    };
    xmlrpc_execute_save_config_cursor(cur, is_json, config_filename)
}

pub(crate) unsafe fn xmlrpc_execute_save_config_raw(
    raw: crate::rpcparams::Raw,
    is_get: bool,
    is_json: bool,
    config_filename: Option<&[u8]>,
) -> (usize, bool, Vec<u8>) {
    let cur = ParamCursor::from_raw(raw, is_get, is_json);
    xmlrpc_execute_save_config_cursor(cur, is_json, config_filename)
}

/// Executes `LoadConfigXmlCommand::Execute`: reads the configuration file via
/// `extension::script_config_load` and formats the RPC config list.
pub fn xmlrpc_execute_load_config(
    is_json: bool,
    user_access: i32,
    ua_restricted: i32,
    config_filename: Option<&[u8]>,
) -> (bool, Vec<u8>) {
    let Some(fname) = config_filename.filter(|f| !f.is_empty()) else {
        return (
            true,
            format_error_bytes(is_json, 3, b"Could not read configuration file"),
        );
    };
    let Some(loaded) = crate::configfile::script_config_load(fname) else {
        return (
            true,
            format_error_bytes(is_json, 3, b"Could not read configuration file"),
        );
    };
    let items: Vec<(&[u8], &[u8], bool)> = loaded
        .iter()
        .map(|(n, v)| {
            (
                n.to_bytes(),
                v.to_bytes(),
                crate::options::option_restricted(n, (c_char::MIN as i32) != 0),
            )
        })
        .collect();
    (
        false,
        format_config_list_bytes(is_json, user_access, ua_restricted, &items),
    )
}

/// Executes `TlsCipherXmlCommand::Execute` (`tlscipherstart`, `tlscipherstatus`, `tlsciphercancel`).
#[allow(clippy::too_many_arguments)]
pub fn xmlrpc_execute_tls_cipher(
    operation: i32,
    user_access: i32,
    ua_control: i32,
    protocol: i32,
    rp_json_rpc: i32,
    http_method: i32,
    hm_post: i32,
    is_json: bool,
    request: Option<&[u8]>,
    limits: crate::json::Limits,
    backend_fn: impl FnOnce(i32, Option<&[u8]>) -> Vec<u8>,
) -> (bool, Vec<u8>) {
    if roc_xmlrpc_tls_cipher_access_allowed(
        user_access,
        ua_control,
        protocol,
        rp_json_rpc,
        http_method,
        hm_post,
    ) == 0
    {
        return (
            true,
            format_error_bytes(
                is_json,
                401,
                b"TLS cipher optimization requires control-account JSON-RPC POST access",
            ),
        );
    }

    let parsed_req = request.and_then(|r| crate::json::parse(r, limits));
    let Some(crate::json::Value::Array(params)) =
        parsed_req.as_ref().and_then(|v| v.get(b"params"))
    else {
        return (
            true,
            format_error_bytes(is_json, 2, b"Invalid cipher optimizer parameters"),
        );
    };

    let first_str = params.first().and_then(|v| match v {
        crate::json::Value::String(s) => Some(s.as_slice()),
        _ => None,
    });
    let valid_job = unsafe {
        roc_xmlrpc_tls_cipher_validate_job_id(
            operation,
            params.len(),
            i32::from(first_str.is_some()),
            first_str.map_or(std::ptr::null(), |s| s.as_ptr().cast()),
            first_str.map_or(0, |s| s.len()),
        ) != 0
    };
    if !valid_job {
        return (
            true,
            format_error_bytes(is_json, 2, b"Invalid cipher job ID"),
        );
    }

    let op_kind = xmlrpc_classify_tls_cipher_op(operation);
    let response = backend_fn(op_kind, first_str.filter(|s| !s.is_empty()));

    let Some(parsed_resp @ crate::json::Value::Object(_)) = crate::json::parse(&response, limits)
    else {
        return (
            true,
            format_error_bytes(is_json, 3, b"Invalid TLS optimizer response"),
        );
    };
    if let Some(crate::json::Value::String(err_bytes)) = parsed_resp.get(b"Error") {
        let nul_pos = err_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(err_bytes.len());
        let err_slice = &err_bytes[..nul_pos.min(1023)];
        return (true, format_error_bytes(is_json, 3, err_slice));
    }
    (false, response)
}

fn xmlrpc_parse_edit_queue_cursor(
    mut cur: ParamCursor<'_>,
    is_json: bool,
    ea_file_move_offset: i32,
    ea_group_move_offset: i32,
) -> Result<(usize, i32, Vec<u8>, Vec<i32>), (usize, i32, &'static [u8])> {
    let Some(cmd_off) = cur.next_str() else {
        return Err((cur.cur_off, 2, b"Invalid parameter"));
    };
    let action = rpc_lookup_edit_command(cur.raw_at(cmd_off));
    if action == -1 {
        return Err((cur.cur_off, 3, b"Invalid action"));
    }

    let opt_offset = cur.next_int();

    let Some(args_off) = cur.next_str() else {
        return Err((cur.cur_off, 2, b"Invalid parameter"));
    };
    let mut args = cur.decode_at(args_off, is_json);
    if !rpc_validate_edit_queue_args(action, &args, true) {
        return Err((cur.cur_off, 2, b"Invalid parameter"));
    }
    if xmlrpc_should_use_edit_offset(
        opt_offset.is_some(),
        action,
        ea_file_move_offset,
        ea_group_move_offset,
    ) {
        args = opt_offset.unwrap_or(0).to_string().into_bytes();
    }

    let mut ids = Vec::new();
    while let Some(id) = cur.next_int() {
        ids.push(id);
    }
    Ok((cur.cur_off, action, args, ids))
}

/// Parses and validates `EditQueueXmlCommand::Execute` parameters over a NUL-terminated buffer.
/// Returns `Ok((new_offset, action, args_bytes, ids))` or `Err((new_offset, err_code, err_text))`.
pub fn xmlrpc_parse_edit_queue(
    buf: &mut [u8],
    is_get: bool,
    is_json: bool,
    ea_file_move_offset: i32,
    ea_group_move_offset: i32,
) -> Result<(usize, i32, Vec<u8>, Vec<i32>), (usize, i32, &'static [u8])> {
    let Some(cur) = ParamCursor::from_slice(buf, is_get, is_json) else {
        return Err((0, 2, b"Invalid parameter"));
    };
    xmlrpc_parse_edit_queue_cursor(cur, is_json, ea_file_move_offset, ea_group_move_offset)
}

pub(crate) unsafe fn xmlrpc_parse_edit_queue_raw(
    raw: crate::rpcparams::Raw,
    is_get: bool,
    is_json: bool,
    ea_file_move_offset: i32,
    ea_group_move_offset: i32,
) -> Result<(usize, i32, Vec<u8>, Vec<i32>), (usize, i32, &'static [u8])> {
    let cur = ParamCursor::from_raw(raw, is_get, is_json);
    xmlrpc_parse_edit_queue_cursor(cur, is_json, ea_file_move_offset, ea_group_move_offset)
}

fn xmlrpc_parse_test_disk_speed_cursor(
    mut cur: ParamCursor<'_>,
    is_json: bool,
) -> Result<(usize, Vec<u8>, usize, u64, i32), (usize, &'static CStr)> {
    let Some(path_off) = cur.next_str() else {
        return Err((cur.cur_off, c"Invalid argument (Path)"));
    };
    let dir_path = cur.decode_at(path_off, is_json);

    let Some(write_buffer_kib) = cur.next_int() else {
        return Err((cur.cur_off, c"Invalid argument (Write Buffer)"));
    };
    if xmlrpc_check_disk_speed_arg_ranges(write_buffer_kib, 1, 1) == 1 {
        return Err((cur.cur_off, c"Write Buffer cannot be negative"));
    }

    let Some(max_file_size_gib) = cur.next_int() else {
        return Err((cur.cur_off, c"Invalid argument (Max file size)"));
    };
    if xmlrpc_check_disk_speed_arg_ranges(write_buffer_kib, max_file_size_gib, 1) == 2 {
        return Err((cur.cur_off, c"Max file size must be positive"));
    }

    let Some(timeout_sec) = cur.next_int() else {
        return Err((cur.cur_off, c"Invalid argument (Timeout)"));
    };
    if xmlrpc_check_disk_speed_arg_ranges(write_buffer_kib, max_file_size_gib, timeout_sec) == 3 {
        return Err((cur.cur_off, c"Timeout must be positive"));
    }

    let mut buffer_bytes = 0usize;
    let mut max_file_bytes = 0u64;
    let rc = unsafe {
        roc_xmlrpc_test_disk_speed_validate_params(
            write_buffer_kib,
            max_file_size_gib,
            timeout_sec,
            &mut buffer_bytes,
            &mut max_file_bytes,
        )
    };
    if rc != 0 {
        return Err((cur.cur_off, c"Invalid parameter"));
    }
    Ok((
        cur.cur_off,
        dir_path,
        buffer_bytes,
        max_file_bytes,
        timeout_sec,
    ))
}

/// Parses and validates `TestDiskSpeedXmlCommand::Execute` parameters over a NUL-terminated buffer.
pub fn xmlrpc_parse_test_disk_speed(
    buf: &mut [u8],
    is_get: bool,
    is_json: bool,
) -> Result<(usize, Vec<u8>, usize, u64, i32), (usize, &'static CStr)> {
    let Some(cur) = ParamCursor::from_slice(buf, is_get, is_json) else {
        return Err((0, c"Invalid argument (Path)"));
    };
    xmlrpc_parse_test_disk_speed_cursor(cur, is_json)
}

pub(crate) unsafe fn xmlrpc_parse_test_disk_speed_raw(
    raw: crate::rpcparams::Raw,
    is_get: bool,
    is_json: bool,
) -> Result<(usize, Vec<u8>, usize, u64, i32), (usize, &'static CStr)> {
    let cur = ParamCursor::from_raw(raw, is_get, is_json);
    xmlrpc_parse_test_disk_speed_cursor(cur, is_json)
}

#[derive(Debug, PartialEq)]
pub struct ParsedAppendFleet {
    pub dupe_key: Vec<u8>,
    pub category: Vec<u8>,
    pub priority: i32,
    pub timeout_sec: i32,
    pub members: Vec<(Vec<u8>, bool, Vec<u8>)>,
}

fn xmlrpc_parse_append_fleet_cursor(
    mut cur: ParamCursor<'_>,
    is_json: bool,
    max_members: i32,
) -> Result<(usize, ParsedAppendFleet), (usize, Vec<u8>)> {
    if !is_json {
        return Err((cur.cur_off, b"appendfleet needs JSON-RPC".to_vec()));
    }
    let Some(dupe_key_off) = cur.next_str() else {
        return Err((cur.cur_off, b"Invalid parameter (DupeKey)".to_vec()));
    };
    let Some(category_off) = cur.next_str() else {
        return Err((cur.cur_off, b"Invalid parameter (Category)".to_vec()));
    };
    let Some(priority) = cur.next_int() else {
        return Err((cur.cur_off, b"Invalid parameter (Priority)".to_vec()));
    };
    let Some(timeout_sec) = cur.next_int() else {
        return Err((cur.cur_off, b"Invalid parameter (Timeout)".to_vec()));
    };
    let dupe_key = cur.decode_at(dupe_key_off, is_json);
    let category = cur.decode_at(category_off, is_json);

    let mut members = Vec::new();
    while let Some(name_off) = cur.next_str() {
        let Some(content_off) = cur.next_str() else {
            let mut msg = b"Invalid parameter (Content of ".to_vec();
            msg.extend_from_slice(cur.raw_at(name_off));
            msg.push(b')');
            return Err((cur.cur_off, msg));
        };
        if xmlrpc_is_fleet_member_limit_reached(members.len(), max_members) {
            return Err((
                cur.cur_off,
                format!("Too many members (at most {max_members})").into_bytes(),
            ));
        }
        let name = cur.decode_at(name_off, is_json);
        let mut content = cur.decode_at(content_off, is_json);
        let is_url = xmlrpc_is_http_url(&content);
        if !is_url {
            let decoded_len = unsafe {
                crate::decode::base64_in_place(
                    content.as_ptr(),
                    content.len(),
                    content.as_mut_ptr(),
                )
            };
            content.truncate(decoded_len);
        }
        members.push((name, is_url, content));
    }

    Ok((
        cur.cur_off,
        ParsedAppendFleet {
            dupe_key,
            category,
            priority,
            timeout_sec,
            members,
        },
    ))
}

pub fn xmlrpc_parse_append_fleet(
    buf: &mut [u8],
    is_get: bool,
    is_json: bool,
    max_members: i32,
) -> Result<(usize, ParsedAppendFleet), (usize, Vec<u8>)> {
    if !is_json {
        return Err((0, b"appendfleet needs JSON-RPC".to_vec()));
    }
    let Some(cur) = ParamCursor::from_slice(buf, is_get, is_json) else {
        return Err((0, b"Invalid parameter (DupeKey)".to_vec()));
    };
    xmlrpc_parse_append_fleet_cursor(cur, is_json, max_members)
}

pub(crate) unsafe fn xmlrpc_parse_append_fleet_raw(
    raw: crate::rpcparams::Raw,
    is_get: bool,
    is_json: bool,
    max_members: i32,
) -> Result<(usize, ParsedAppendFleet), (usize, Vec<u8>)> {
    let cur = ParamCursor::from_raw(raw, is_get, is_json);
    xmlrpc_parse_append_fleet_cursor(cur, is_json, max_members)
}

#[derive(Debug, PartialEq)]
pub struct ParsedDownload {
    pub v13: bool,
    pub filename: Vec<u8>,
    pub category: Vec<u8>,
    pub priority: i32,
    pub add_top: bool,
    pub is_url: bool,
    pub content: Vec<u8>,
    pub add_paused: bool,
    pub dupe_key: Option<Vec<u8>>,
    pub dupe_score: i32,
    pub dupe_mode: i32,
    pub auto_category: bool,
    pub params: Vec<(Vec<u8>, Vec<u8>)>,
}

fn xmlrpc_parse_download_cursor(
    mut cur: ParamCursor<'_>,
    is_json: bool,
    dm_score: i32,
    dm_all: i32,
    dm_force: i32,
) -> Result<(usize, ParsedDownload), (usize, &'static CStr)> {
    let Some(filename_off) = cur.next_str() else {
        return Err((cur.cur_off, c"Invalid parameter (Filename)"));
    };
    let Some(mut content_off) = cur.next_str() else {
        return Err((cur.cur_off, c"Invalid parameter (Content)"));
    };
    let (v13, category_off) = match cur.next_str() {
        Some(cat_off) => (true, cat_off),
        None => (false, content_off),
    };
    let filename = cur.decode_at(filename_off, is_json);
    let category = cur.decode_at(category_off, is_json);

    let priority = cur.next_int().unwrap_or(0);

    let Some(add_top) = cur.next_bool(is_json) else {
        return Err((cur.cur_off, c"Invalid parameter (AddTop)"));
    };

    if !v13 {
        let Some(c_off) = cur.next_str() else {
            return Err((cur.cur_off, c"Invalid parameter (FileContent)"));
        };
        content_off = c_off;
    }
    let mut content = cur.decode_at(content_off, is_json);

    let mut add_paused = false;
    let mut dupe_key = None;
    let mut dupe_score = 0;
    let mut auto_category = false;
    let mut dupe_mode = dm_score;
    if let Some(ap) = cur.next_bool(is_json) {
        add_paused = ap;
        let Some(dk_off) = cur.next_str() else {
            return Err((cur.cur_off, c"Invalid parameter (DupeKey)"));
        };
        dupe_key = Some(cur.decode_at(dk_off, is_json));
        let Some(ds) = cur.next_int() else {
            return Err((cur.cur_off, c"Invalid parameter (DupeScore)"));
        };
        dupe_score = ds;
        let parsed_dm = cur.next_str().map_or(-1, |dm_off| {
            xmlrpc_parse_dupe_mode(cur.raw_at(dm_off), dm_score, dm_all, dm_force)
        });
        if parsed_dm < 0 {
            return Err((cur.cur_off, c"Invalid parameter (DupeMode)"));
        }
        dupe_mode = parsed_dm;
        if let Some(ac) = cur.next_bool(is_json) {
            auto_category = ac;
        }
    } else if v13 {
        return Err((cur.cur_off, c"Invalid parameter (AddPaused)"));
    }

    let mut params = Vec::new();
    if v13 {
        while let Some(pname_off) = cur.next_str() {
            let Some(pval_off) = cur.next_str() else {
                return Err((cur.cur_off, c"Invalid parameter (Parameters)"));
            };
            let pname = cur.decode_at(pname_off, is_json);
            let pval = cur.decode_at(pval_off, is_json);
            params.push((pname, pval));
        }
    }

    let is_url = xmlrpc_is_http_url(&content);
    if !is_url {
        let decoded_len = unsafe {
            crate::decode::base64_in_place(content.as_ptr(), content.len(), content.as_mut_ptr())
        };
        content.truncate(decoded_len);
    }

    Ok((
        cur.cur_off,
        ParsedDownload {
            v13,
            filename,
            category,
            priority,
            add_top,
            is_url,
            content,
            add_paused,
            dupe_key,
            dupe_score,
            dupe_mode,
            auto_category,
            params,
        },
    ))
}

pub fn xmlrpc_parse_download(
    buf: &mut [u8],
    is_get: bool,
    is_json: bool,
    dm_score: i32,
    dm_all: i32,
    dm_force: i32,
) -> Result<(usize, ParsedDownload), (usize, &'static CStr)> {
    let Some(cur) = ParamCursor::from_slice(buf, is_get, is_json) else {
        return Err((0, c"Invalid parameter (Filename)"));
    };
    xmlrpc_parse_download_cursor(cur, is_json, dm_score, dm_all, dm_force)
}

pub(crate) unsafe fn xmlrpc_parse_download_raw(
    raw: crate::rpcparams::Raw,
    is_get: bool,
    is_json: bool,
    dm_score: i32,
    dm_all: i32,
    dm_force: i32,
) -> Result<(usize, ParsedDownload), (usize, &'static CStr)> {
    let cur = ParamCursor::from_raw(raw, is_get, is_json);
    xmlrpc_parse_download_cursor(cur, is_json, dm_score, dm_all, dm_force)
}

#[derive(Debug, PartialEq)]
pub struct ParsedViewFeed {
    pub id: i32,
    pub include_non_matching: bool,
    pub name: Vec<u8>,
    pub url: Vec<u8>,
    pub filter: Vec<u8>,
    pub backlog: bool,
    pub pause_nzb: bool,
    pub category: Vec<u8>,
    pub category_source: i32,
    pub priority: i32,
    pub interval: i32,
    pub feed_filter: Option<Vec<u8>>,
    pub cache_time_sec: i32,
    pub cache_id: Vec<u8>,
}

fn xmlrpc_parse_view_feed_cursor(
    mut cur: ParamCursor<'_>,
    is_json: bool,
    preview: bool,
    cs_nzb_file: i32,
    cs_feed_file: i32,
    cs_auto: i32,
) -> Option<(usize, ParsedViewFeed)> {
    if !preview {
        let id = cur.next_int()?;
        let include_non_matching = cur.next_bool(is_json)?;
        return Some((
            cur.cur_off,
            ParsedViewFeed {
                id,
                include_non_matching,
                name: Vec::new(),
                url: Vec::new(),
                filter: Vec::new(),
                backlog: true,
                pause_nzb: false,
                category: Vec::new(),
                category_source: cs_nzb_file,
                priority: 0,
                interval: 0,
                feed_filter: None,
                cache_time_sec: 0,
                cache_id: Vec::new(),
            },
        ));
    }

    let id_opt = cur.next_int();
    let v16 = id_opt.is_some();
    let id = id_opt.unwrap_or(0);

    let name_off = cur.next_str()?;
    let url_off = cur.next_str()?;
    let filter_off = cur.next_str()?;
    let backlog = if v16 { cur.next_bool(is_json)? } else { true };
    let pause_nzb = cur.next_bool(is_json)?;
    let category_off = cur.next_str()?;
    let priority = cur.next_int()?;
    let (interval, feed_filter) = if v16 {
        let iv = cur.next_int()?;
        let ff_off = cur.next_str()?;
        (iv, Some(cur.raw_at(ff_off).to_vec()))
    } else {
        (0, None)
    };
    let include_non_matching = cur.next_bool(is_json)?;
    let cache_time_sec = cur.next_int()?;
    let cache_id_off = cur.next_str()?;

    let category_source = match cur.next_str() {
        Some(cs_off) => {
            xmlrpc_parse_category_source(cur.raw_at(cs_off), cs_nzb_file, cs_feed_file, cs_auto)
        }
        None => cs_nzb_file,
    };

    let name = cur.decode_at(name_off, is_json);
    let url = cur.decode_at(url_off, is_json);
    let filter = cur.decode_at(filter_off, is_json);
    let cache_id = cur.decode_at(cache_id_off, is_json);
    let category = cur.decode_at(category_off, is_json);

    Some((
        cur.cur_off,
        ParsedViewFeed {
            id,
            include_non_matching,
            name,
            url,
            filter,
            backlog,
            pause_nzb,
            category,
            category_source,
            priority,
            interval,
            feed_filter,
            cache_time_sec,
            cache_id,
        },
    ))
}

pub fn xmlrpc_parse_view_feed(
    buf: &mut [u8],
    is_get: bool,
    is_json: bool,
    preview: bool,
    cs_nzb_file: i32,
    cs_feed_file: i32,
    cs_auto: i32,
) -> Option<(usize, ParsedViewFeed)> {
    let cur = ParamCursor::from_slice(buf, is_get, is_json)?;
    xmlrpc_parse_view_feed_cursor(cur, is_json, preview, cs_nzb_file, cs_feed_file, cs_auto)
}

pub(crate) unsafe fn xmlrpc_parse_view_feed_raw(
    raw: crate::rpcparams::Raw,
    is_get: bool,
    is_json: bool,
    preview: bool,
    cs_nzb_file: i32,
    cs_feed_file: i32,
    cs_auto: i32,
) -> Option<(usize, ParsedViewFeed)> {
    let cur = ParamCursor::from_raw(raw, is_get, is_json);
    xmlrpc_parse_view_feed_cursor(cur, is_json, preview, cs_nzb_file, cs_feed_file, cs_auto)
}

#[allow(clippy::too_many_arguments)]
fn xmlrpc_parse_test_server_cursor(
    request_body: Option<&[u8]>,
    cur: Option<ParamCursor<'_>>,
    is_json: bool,
    http_method: i32,
    hm_post: i32,
    cert_verif_count: i32,
    limits: crate::json::Limits,
) -> Result<(usize, crate::rpcparams::ServerParams), (usize, &'static CStr)> {
    let (new_off, params) = if xmlrpc_should_parse_json_post_test_server(
        is_json,
        request_body.is_some(),
        http_method,
        hm_post,
    ) {
        match crate::rpcparams::test_server_params(request_body.unwrap_or(&[]), limits) {
            Ok(p) => (0, p),
            Err(crate::rpcparams::ServerParamsError::Json) => return Err((0, c"Invalid JSON")),
            Err(crate::rpcparams::ServerParamsError::Params) => {
                return Err((0, c"Invalid parameters"))
            }
        }
    } else {
        let Some(mut cur) = cur else {
            return Err((0, c"Invalid parameter"));
        };
        let parsed = (|| {
            let host_off = cur.next_str()?;
            let port = cur.next_int()?;
            let user_off = cur.next_str()?;
            let pass_off = cur.next_str()?;
            let encryption = cur.next_bool(is_json)?;
            let cipher_off = cur.next_str()?;
            let timeout = cur.next_int()?;
            let cert_verif_level = cur.next_int()?;
            let host = cur.decode_at(host_off, is_json);
            let username = cur.decode_at(user_off, is_json);
            let password = cur.decode_at(pass_off, is_json);
            let cipher = cur.decode_at(cipher_off, is_json);
            Some(crate::rpcparams::ServerParams {
                host,
                port,
                username,
                password,
                encryption,
                cipher,
                timeout,
                cert_verif_level,
            })
        })();
        match parsed {
            Some(p) => (cur.cur_off, p),
            None => return Err((cur.cur_off, c"Invalid parameter")),
        }
    };

    if !xmlrpc_is_valid_cert_verif_level(params.cert_verif_level, cert_verif_count) {
        return Err((
            new_off,
            c"Invalid parameter (Certificate Verification Level).",
        ));
    }

    Ok((new_off, params))
}

#[allow(clippy::too_many_arguments)]
pub fn xmlrpc_parse_test_server(
    request_body: Option<&[u8]>,
    buf: Option<&mut [u8]>,
    is_get: bool,
    is_json: bool,
    http_method: i32,
    hm_post: i32,
    cert_verif_count: i32,
    limits: crate::json::Limits,
) -> Result<(usize, crate::rpcparams::ServerParams), (usize, &'static CStr)> {
    let cur = buf.and_then(|b| ParamCursor::from_slice(b, is_get, is_json));
    xmlrpc_parse_test_server_cursor(
        request_body,
        cur,
        is_json,
        http_method,
        hm_post,
        cert_verif_count,
        limits,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn xmlrpc_parse_test_server_raw(
    request_body: Option<&[u8]>,
    raw: Option<crate::rpcparams::Raw>,
    is_get: bool,
    is_json: bool,
    http_method: i32,
    hm_post: i32,
    cert_verif_count: i32,
    limits: crate::json::Limits,
) -> Result<(usize, crate::rpcparams::ServerParams), (usize, &'static CStr)> {
    let cur = raw.map(|r| ParamCursor::from_raw(r, is_get, is_json));
    xmlrpc_parse_test_server_cursor(
        request_body,
        cur,
        is_json,
        http_method,
        hm_post,
        cert_verif_count,
        limits,
    )
}

const RPC_HM_GET: i32 = 1;
const RPC_UA_ADD: i32 = 2;
const RPC_JSON_LIMITS: crate::json::Limits = crate::json::Limits {
    string: 0x7fff_fffe,
    array: 0x7fff_ffff / 24,
    object: 0x3fff_ffff,
};

static SPEED_TEST_PAUSE_STATE: std::sync::Mutex<(usize, bool)> = std::sync::Mutex::new((0, false));

struct SpeedTestPauseGuard {
    ctx: *mut c_void,
    action_cb: Option<NzbgetRsXmlRpcActionFn>,
}

impl SpeedTestPauseGuard {
    unsafe fn new(ctx: *mut c_void, action_cb: Option<NzbgetRsXmlRpcActionFn>) -> Self {
        let mut guard = SPEED_TEST_PAUSE_STATE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if guard.0 == 0 {
            let was_paused = match action_cb {
                Some(cb) => unsafe {
                    cb(
                        ctx,
                        1,
                        0,
                        0,
                        0,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        0,
                        std::ptr::null(),
                        0,
                        std::ptr::null(),
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    ) != 0
                },
                None => false,
            };
            guard.1 = was_paused;
        }
        guard.0 += 1;
        drop(guard);
        Self { ctx, action_cb }
    }
}

impl Drop for SpeedTestPauseGuard {
    fn drop(&mut self) {
        let mut guard = SPEED_TEST_PAUSE_STATE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard.0 = guard.0.saturating_sub(1);
        if guard.0 == 0 {
            let restore = i32::from(guard.1);
            if let Some(cb) = self.action_cb {
                unsafe {
                    cb(
                        self.ctx,
                        1,
                        1,
                        restore,
                        0,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        0,
                        std::ptr::null(),
                        0,
                        std::ptr::null(),
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                }
            }
        }
    }
}

#[inline]
fn c_bytes(bytes: &[u8]) -> &[u8] {
    match crate::bytes::find_byte(bytes, 0) {
        Some(n) => &bytes[..n],
        None => bytes,
    }
}

#[inline]
fn to_c_vec(bytes: &[u8]) -> Vec<u8> {
    let clean = c_bytes(bytes);
    let mut out = Vec::with_capacity(clean.len() + 1);
    out.extend_from_slice(clean);
    out.push(0);
    out
}

unsafe fn take_rs_buf(buf: RsBuf) -> Vec<u8> {
    let out = if !buf.data.is_null() && buf.len > 0 {
        unsafe { std::slice::from_raw_parts(buf.data.cast::<u8>(), buf.len).to_vec() }
    } else {
        Vec::new()
    };
    unsafe {
        nzbget_rs_free(buf);
    }
    out
}

struct XmlRpcHost<'a> {
    ctx: *mut c_void,
    debug_build: bool,
    version: &'a [u8],
    config_filename: Option<&'a [u8]>,
    log_cb: Option<NzbgetRsXmlRpcLogFn>,
    action_cb: Option<NzbgetRsXmlRpcActionFn>,
    collect_cb: Option<NzbgetRsXmlRpcCollectFn>,
}

impl<'a> XmlRpcHost<'a> {
    unsafe fn log(&self, lvl: c_int, msg: &[u8]) {
        if lvl == 0 && !self.debug_build {
            return;
        }
        if let Some(cb) = self.log_cb {
            let c = to_c_vec(msg);
            unsafe {
                cb(self.ctx, lvl, c.as_ptr().cast());
            }
        }
    }

    unsafe fn debug_kv(&self, prefix: &[u8], val: &[u8]) {
        if !self.debug_build {
            return;
        }
        let v = c_bytes(val);
        let mut m = Vec::with_capacity(prefix.len() + v.len());
        m.extend_from_slice(prefix);
        m.extend_from_slice(v);
        unsafe {
            self.log(0, &m);
        }
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn call_action(
        &self,
        op: c_int,
        a1: c_int,
        a2: c_int,
        a3: c_int,
        a4: c_int,
        s1: *const c_char,
        s2: *const c_char,
        s3: *const c_char,
        s4: *const c_char,
        body: *const c_char,
        body_len: usize,
        ids: &[i32],
        params: &[NzbgetRsRpcParamItem],
        out_u32: *mut u32,
        out_f64: *mut f64,
        out_buf: *mut RsBuf,
    ) -> c_int {
        let Some(cb) = self.action_cb else {
            return 0;
        };
        unsafe {
            cb(
                self.ctx,
                op,
                a1,
                a2,
                a3,
                a4,
                s1,
                s2,
                s3,
                s4,
                body,
                body_len,
                if ids.is_empty() {
                    std::ptr::null()
                } else {
                    ids.as_ptr()
                },
                ids.len(),
                if params.is_empty() {
                    std::ptr::null()
                } else {
                    params.as_ptr()
                },
                params.len(),
                out_u32,
                out_f64,
                out_buf,
            )
        }
    }
}

struct EmitState {
    is_json: bool,
    eff_access: c_int,
    index: usize,
    out: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn collect_emit_trampoline(
    emit_ctx: *mut c_void,
    sub: c_int,
    a1: c_int,
    a2: c_int,
    a3: c_int,
    a4: c_int,
    a5: c_int,
    a6: c_int,
    i1: i64,
    i2: i64,
    i3: i64,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    p1: *const c_void,
    n1: usize,
    p2: *const c_void,
    n2: usize,
    p3: *const c_void,
    n3: usize,
    p4: *const c_void,
    n4: usize,
) {
    let st = unsafe { &mut *(emit_ctx as *mut EmitState) };
    let json = st.is_json;
    match sub {
        // status
        1 => unsafe {
            write_status(&mut st.out, json, p1.cast(), p2.cast(), n2);
        },
        // log / loadlog / logupdate
        2 => unsafe {
            write_log_list(&mut st.out, json, p1.cast(), n1);
        },
        // listfiles item
        3 => {
            let idx = st.index;
            st.index += 1;
            unsafe {
                write_file_item(&mut st.out, json, idx, p1.cast());
            }
        }
        // listgroups start + nzb_info_fields
        4 => {
            let idx = st.index;
            st.index += 1;
            let flags = a6;
            unsafe {
                write_group_item_start(
                    &mut st.out,
                    json,
                    idx,
                    a1,
                    i1,
                    i2,
                    a2,
                    a3,
                    a4,
                    a5,
                    (flags & 1) != 0,
                    (flags >> 4) & 0xff,
                    (flags & 2) != 0,
                    (flags & 4) != 0,
                    (flags >> 12) & 0xff,
                    (flags >> 20) & 0xff,
                );
                write_nzb_info_fields(
                    &mut st.out,
                    json,
                    p1.cast(),
                    p2.cast(),
                    n2,
                    p3.cast(),
                    n3,
                    p4.cast(),
                    n4,
                );
            }
            if json {
                st.out.extend_from_slice(b",\n");
            }
        }
        // listgroups / postqueue post_info_fields + item close
        5 => {
            let is_post_queue = a1 != 0;
            let has_post_info = a2 != 0;
            unsafe {
                write_post_info_fields(
                    &mut st.out,
                    json,
                    is_post_queue,
                    has_post_info,
                    s1,
                    a3,
                    i1,
                    i2,
                    i3,
                    p1.cast(),
                    n1,
                );
            }
            st.out
                .extend_from_slice(if json { b"}" } else { b"</struct></value>\n" });
        }
        // postqueue start + nzb_info_fields
        6 => {
            let idx = st.index;
            st.index += 1;
            unsafe {
                write_postqueue_item_start(&mut st.out, json, idx, a1, s1, a2, a3);
                write_nzb_info_fields(
                    &mut st.out,
                    json,
                    p1.cast(),
                    p2.cast(),
                    n2,
                    p3.cast(),
                    n3,
                    p4.cast(),
                    n4,
                );
            }
            if json {
                st.out.extend_from_slice(b",\n");
            }
        }
        // history nzb/url item
        7 => {
            let idx = st.index;
            st.index += 1;
            unsafe {
                write_history_nzb_start(&mut st.out, json, idx, a1, s1, a2, a3 != 0, a4, s2);
                write_nzb_info_fields(
                    &mut st.out,
                    json,
                    p1.cast(),
                    p2.cast(),
                    n2,
                    p3.cast(),
                    n3,
                    p4.cast(),
                    n4,
                );
            }
            st.out
                .extend_from_slice(if json { b"}" } else { b"</struct></value>" });
        }
        // history dup item
        8 => {
            let idx = st.index;
            st.index += 1;
            unsafe {
                write_history_dup_item(&mut st.out, json, idx, a1, s1, a2, i1, s2, a3, a4, a5);
            }
            st.out
                .extend_from_slice(if json { b"}" } else { b"</struct></value>" });
        }
        // urlqueue item
        9 => {
            let idx = st.index;
            st.index += 1;
            unsafe {
                write_urlqueue_item(&mut st.out, json, idx, a1, s1, s2, s3, s4, a2);
            }
        }
        // appendfleet result
        10 => unsafe {
            write_fleet_result(&mut st.out, a1, a2 != 0, s1, p1.cast(), n1);
        },
        // config list
        11 => unsafe {
            write_config_list(
                &mut st.out,
                json,
                st.eff_access,
                crate::rpcroute::UA_RESTRICTED,
                p1.cast(),
                n1,
            );
        },
        // configtemplates
        12 => unsafe {
            write_config_templates(&mut st.out, json, p1.cast(), n1);
        },
        // viewfeed / previewfeed item
        13 => {
            let idx = st.index;
            st.index += 1;
            unsafe {
                write_feed_item(&mut st.out, json, idx, p1.cast());
            }
        }
        // servervolumes item
        14 => {
            let idx = st.index;
            st.index += 1;
            unsafe {
                write_server_volume_item(&mut st.out, json, idx, p1.cast());
            }
        }
        _ => {}
    }
}

fn resolve_command(
    host: &XmlRpcHost<'_>,
    eff_access: c_int,
    method_name: &[u8],
) -> (c_int, c_int, &'static [u8]) {
    let denied = xmlrpc_check_user_access_denied(
        eff_access,
        RPC_UA_ADD,
        crate::rpcroute::UA_RESTRICTED,
        method_name,
    );
    if denied == 1 || denied == 2 {
        let who: &[u8] = if denied == 1 {
            b"add-user"
        } else {
            b"restricted user"
        };
        let mut msg = Vec::with_capacity(method_name.len() + 64);
        msg.extend_from_slice(b"Received request \"");
        msg.extend_from_slice(c_bytes(method_name));
        msg.extend_from_slice(b"\" from ");
        msg.extend_from_slice(who);
        msg.extend_from_slice(b", access denied");
        unsafe {
            host.log(1, &msg);
        }
        return (0, 401, b"Access denied");
    }
    let cmd_id = match_command(c_bytes(method_name));
    if cmd_id <= 0 {
        return (0, 1, b"Invalid procedure");
    }
    (cmd_id, 0, b"Invalid procedure")
}

#[allow(clippy::too_many_arguments)]
unsafe fn execute_xml_command(
    host: &XmlRpcHost<'_>,
    cmd_id: c_int,
    default_err_code: c_int,
    default_err_text: &[u8],
    proto: c_int,
    http_method: c_int,
    eff_access: c_int,
    buf: &mut [u8],
    cur_pos: usize,
) -> (bool, Vec<u8>) {
    let is_json = xmlrpc_is_json_protocol(
        proto,
        crate::rpcroute::RP_JSON_RPC,
        crate::rpcroute::RP_JSONP_RPC,
    );
    let is_get = http_method == RPC_HM_GET;
    let err_resp = |code: i32, text: &[u8]| -> (bool, Vec<u8>) {
        let clean = c_bytes(text);
        let trunc = &clean[..clean.len().min(1023)];
        (true, format_error_bytes(is_json, code, trunc))
    };

    if cmd_id <= 0 {
        return err_resp(default_err_code, default_err_text);
    }

    let raw_start = unsafe { crate::rpcparams::Raw::new(buf.as_mut_ptr().add(cur_pos)) };
    let mut cur = unsafe { ParamCursor::from_raw(raw_start, is_get, is_json) };

    let run_collect = |kind: c_int,
                       a1: c_int,
                       a2: c_int,
                       a3: c_int,
                       a4: c_int,
                       a5: c_int,
                       s1: *const c_char,
                       s2: *const c_char,
                       s3: *const c_char,
                       s4: *const c_char,
                       s5: *const c_char,
                       s6: *const c_char,
                       p1: *const c_void,
                       n1: usize,
                       wrap_array: bool|
     -> (c_int, Vec<u8>) {
        let mut st = EmitState {
            is_json,
            eff_access,
            index: 0,
            out: Vec::with_capacity(1024),
        };
        if wrap_array {
            st.out
                .extend_from_slice(if is_json { b"[\n" } else { b"<array><data>\n" });
        }
        let rc = match host.collect_cb {
            Some(cb) => unsafe {
                cb(
                    host.ctx,
                    kind,
                    a1,
                    a2,
                    a3,
                    a4,
                    a5,
                    s1,
                    s2,
                    s3,
                    s4,
                    s5,
                    s6,
                    p1,
                    n1,
                    (&mut st as *mut EmitState).cast(),
                    collect_emit_trampoline,
                )
            },
            None => 0,
        };
        if wrap_array {
            st.out.extend_from_slice(if is_json {
                b"\n]"
            } else {
                b"</data></array>\n"
            });
        }
        (rc, st.out)
    };

    match cmd_id {
        // pause / resume / pausepost / resumepost / pausescan / resumescan
        1 | 2 | 22 | 23 | 24 | 25 => {
            let (pause, action) = match cmd_id {
                1 => (true, 0),
                2 => (false, 0),
                22 => (true, 1),
                23 => (false, 1),
                24 => (true, 2),
                _ => (false, 2),
            };
            let rc = unsafe {
                host.call_action(
                    2,
                    c_int::from(pause),
                    action,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            (false, format_bool_bytes(is_json, rc != 0))
        }
        // shutdown
        3 => {
            unsafe {
                host.call_action(
                    4,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            (false, format_bool_bytes(is_json, true))
        }
        // reload
        4 => {
            unsafe {
                host.call_action(
                    5,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            (false, format_bool_bytes(is_json, true))
        }
        // version
        5 => (
            false,
            format_string_bytes(is_json, host.version, false, 1023),
        ),
        // dump
        6 => {
            unsafe {
                host.call_action(
                    6,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            (false, format_bool_bytes(is_json, true))
        }
        // rate
        7 => {
            let Some(rate) = cur.next_int() else {
                return err_resp(2, b"Invalid parameter");
            };
            if !(0..=i32::MAX / 1024).contains(&rate) {
                return err_resp(2, b"Invalid parameter");
            }
            unsafe {
                host.call_action(
                    7,
                    rate * 1024,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            (false, format_bool_bytes(is_json, true))
        }
        // status
        8 => {
            let (_, out) = run_collect(
                1,
                0,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                false,
            );
            (false, out)
        }
        // sysinfo
        9 => {
            let mut out_buf = RsBuf {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            unsafe {
                host.call_action(
                    8,
                    c_int::from(is_json),
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut out_buf,
                );
                (false, take_rs_buf(out_buf))
            }
        }
        // systemhealth
        10 => {
            let mut out_buf = RsBuf {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            let rc = unsafe {
                host.call_action(
                    9,
                    c_int::from(is_json),
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut out_buf,
                )
            };
            let bytes = unsafe { take_rs_buf(out_buf) };
            if rc == 0 {
                err_resp(1, b"System health check is disabled in configuration")
            } else {
                (false, bytes)
            }
        }
        // log / loadlog / logupdate
        11 | 20 | 45 => {
            let log_source = match cmd_id {
                11 => 0,
                45 => 1,
                _ => 3,
            };
            let mut nzb_id = 0;
            if log_source == 3 {
                let Some(id) = cur.next_int() else {
                    return err_resp(2, b"Invalid parameter");
                };
                nzb_id = id;
            }
            let Some(id_from) = cur.next_int() else {
                return err_resp(2, b"Invalid parameter");
            };
            let Some(nr_entries) = cur.next_int() else {
                return err_resp(2, b"Invalid parameter");
            };
            if nr_entries > 0 && id_from > 0 {
                return err_resp(2, b"Invalid parameter");
            }
            unsafe {
                host.log(0, format!("iIDFrom={id_from}").as_bytes());
                host.log(0, format!("iNrEntries={nr_entries}").as_bytes());
            }
            let (_, out) = run_collect(
                2,
                log_source,
                nzb_id,
                id_from,
                nr_entries,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                false,
            );
            (false, out)
        }
        // listfiles
        12 => {
            let id_start_opt = cur.next_int();
            let id_end_opt = if id_start_opt.is_some() {
                cur.next_int()
            } else {
                None
            };
            let id_start = id_start_opt.unwrap_or(0);
            let id_end = id_end_opt.unwrap_or(0);
            if id_start_opt.is_some() && (id_end_opt.is_none() || id_end < id_start) {
                return err_resp(2, b"Invalid parameter");
            }
            let nzb_id = cur.next_int().unwrap_or(0);
            if nzb_id > 0 && (id_start != 0 || id_end != 0) {
                return err_resp(2, b"Invalid parameter");
            }
            unsafe {
                host.log(0, format!("iIDStart={id_start}").as_bytes());
                host.log(0, format!("iIDEnd={id_end}").as_bytes());
            }
            let (_, out) = run_collect(
                3,
                id_start,
                id_end,
                nzb_id,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                true,
            );
            (false, out)
        }
        // listgroups
        13 => {
            let nr_entries = cur.next_int().unwrap_or(0);
            let (_, out) = run_collect(
                4,
                nr_entries,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                true,
            );
            (false, out)
        }
        // editqueue
        14 => match unsafe { xmlrpc_parse_edit_queue_raw(raw_start, is_get, is_json, 1, 11) } {
            Err((_, code, msg)) => err_resp(code, msg),
            Ok((_, action, args, ids)) => {
                let args_c = to_c_vec(&args);
                let ok = unsafe {
                    host.call_action(
                        10,
                        action,
                        0,
                        0,
                        0,
                        args_c.as_ptr().cast(),
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        0,
                        &ids,
                        &[],
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                (false, format_bool_bytes(is_json, ok != 0))
            }
        },
        // append / appendurl
        15 => match unsafe { xmlrpc_parse_download_raw(raw_start, is_get, is_json, 0, 1, 2) } {
            Err((_, msg)) => err_resp(2, msg.to_bytes()),
            Ok((_, dl)) => {
                let fname_c = to_c_vec(&dl.filename);
                let cat_c = to_c_vec(&dl.category);
                let dk_c = dl.dupe_key.as_ref().map(|d| to_c_vec(d));
                unsafe {
                    host.debug_kv(b"FileName=", &dl.filename);
                }
                let fname_clean = c_bytes(&dl.filename);
                let is_zip = !dl.is_url
                    && !fname_clean.is_empty()
                    && crate::unpack::can_extract_with_zipfast(crate::fsops::path(fname_clean));
                let mut c_param_vecs: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(dl.params.len());
                for (k, v) in &dl.params {
                    c_param_vecs.push((to_c_vec(k), to_c_vec(v)));
                }
                let c_params: Vec<NzbgetRsRpcParamItem> = c_param_vecs
                    .iter()
                    .map(|(k, v)| NzbgetRsRpcParamItem {
                        name: k.as_ptr().cast(),
                        value: v.as_ptr().cast(),
                    })
                    .collect();
                let body_len = dl.content.len();
                let mut body_buf = dl.content;
                body_buf.push(0);
                let flags = c_int::from(dl.add_top)
                    | (c_int::from(dl.add_paused) << 1)
                    | (c_int::from(dl.auto_category) << 2)
                    | (c_int::from(dl.is_url) << 3)
                    | (c_int::from(is_zip) << 4);
                let mut nzb_id_out: u32 = u32::MAX;
                let ok = unsafe {
                    host.call_action(
                        11,
                        flags,
                        dl.priority,
                        dl.dupe_score,
                        dl.dupe_mode,
                        fname_c.as_ptr().cast(),
                        cat_c.as_ptr().cast(),
                        dk_c.as_ref()
                            .map_or(std::ptr::null(), |v| v.as_ptr().cast()),
                        std::ptr::null(),
                        body_buf.as_ptr().cast(),
                        body_len,
                        &[],
                        &c_params,
                        &mut nzb_id_out,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                if is_zip {
                    (false, format_bool_bytes(is_json, ok != 0))
                } else if dl.v13 {
                    (false, format_int_bytes(is_json, nzb_id_out as i32))
                } else {
                    (false, format_bool_bytes(is_json, ok != 0))
                }
            }
        },
        // appendfleet
        16 => match unsafe { xmlrpc_parse_append_fleet_raw(raw_start, is_get, is_json, 50) } {
            Err((_, msg)) => err_resp(2, &msg),
            Ok((_, fl)) => {
                let dk_c = to_c_vec(&fl.dupe_key);
                let cat_c = to_c_vec(&fl.category);
                let name_vecs: Vec<Vec<u8>> =
                    fl.members.iter().map(|(n, _, _)| to_c_vec(n)).collect();
                let req_members: Vec<NzbgetRsRpcFleetReqMember> = fl
                    .members
                    .iter()
                    .enumerate()
                    .map(|(i, (_, is_url, data))| NzbgetRsRpcFleetReqMember {
                        name: name_vecs[i].as_ptr().cast(),
                        is_url: c_int::from(*is_url),
                        data: if data.is_empty() {
                            c"".as_ptr()
                        } else {
                            data.as_ptr().cast()
                        },
                        data_len: data.len(),
                    })
                    .collect();
                let (_, out) = run_collect(
                    8,
                    fl.priority,
                    fl.timeout_sec,
                    0,
                    0,
                    0,
                    dk_c.as_ptr().cast(),
                    cat_c.as_ptr().cast(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    if req_members.is_empty() {
                        std::ptr::null()
                    } else {
                        req_members.as_ptr().cast()
                    },
                    req_members.len(),
                    false,
                );
                (false, out)
            }
        },
        // postqueue
        17 => {
            let nr_entries = cur.next_int().unwrap_or(0);
            let (_, out) = run_collect(
                5,
                nr_entries,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                true,
            );
            (false, out)
        }
        // writelog
        18 => {
            let Some(kind_off) = cur.next_str() else {
                return err_resp(2, b"Invalid parameter");
            };
            let Some(text_off) = cur.next_str() else {
                return err_resp(2, b"Invalid parameter");
            };
            let kind_bytes = cur.raw_at(kind_off).to_vec();
            let text_bytes = cur.decode_at(text_off, is_json);
            if host.debug_build {
                let k = c_bytes(&kind_bytes);
                let t = c_bytes(&text_bytes);
                let mut dbg = Vec::with_capacity(k.len() + t.len() + 16);
                dbg.extend_from_slice(b"Kind=");
                dbg.extend_from_slice(k);
                dbg.extend_from_slice(b", Text=");
                dbg.extend_from_slice(t);
                unsafe {
                    host.log(0, &dbg);
                }
            }
            let lvl = match c_bytes(&kind_bytes) {
                b"INFO" => 3,
                b"WARNING" => 1,
                b"ERROR" => 2,
                b"DETAIL" => 4,
                b"DEBUG" => 0,
                _ => return err_resp(3, b"Invalid Kind"),
            };
            unsafe {
                host.log(lvl, &text_bytes);
            }
            (false, format_bool_bytes(is_json, true))
        }
        // clearlog
        19 => {
            unsafe {
                host.call_action(
                    12,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            (false, format_bool_bytes(is_json, true))
        }
        // scan
        21 => {
            let sync_mode = cur.next_bool(is_json).unwrap_or(false);
            unsafe {
                host.call_action(
                    13,
                    c_int::from(sync_mode),
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            (false, format_bool_bytes(is_json, true))
        }
        // scheduleresume
        26 => {
            let Some(seconds) = cur.next_int().filter(|&s| s >= 0) else {
                return err_resp(2, b"Invalid parameter");
            };
            unsafe {
                host.call_action(
                    3,
                    seconds,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            (false, format_bool_bytes(is_json, true))
        }
        // history
        27 => {
            let dup = cur.next_bool(is_json).unwrap_or(false);
            let (_, out) = run_collect(
                6,
                c_int::from(dup),
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                true,
            );
            (false, out)
        }
        // urlqueue
        28 => {
            let (_, out) = run_collect(
                7,
                0,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                true,
            );
            (false, out)
        }
        // config
        29 => {
            let (_, out) = run_collect(
                9,
                0,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                false,
            );
            (false, out)
        }
        // loadconfig
        30 => xmlrpc_execute_load_config(
            is_json,
            eff_access,
            crate::rpcroute::UA_RESTRICTED,
            host.config_filename,
        ),
        // saveconfig
        36 => {
            let (_, fault, out) = unsafe {
                xmlrpc_execute_save_config_raw(raw_start, is_get, is_json, host.config_filename)
            };
            (fault, out)
        }
        // configtemplates
        37 => {
            let load_from_disk = cur.next_bool(is_json).unwrap_or(false);
            let (ok, out) = run_collect(
                10,
                c_int::from(load_from_disk),
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                false,
            );
            if ok == 0 {
                err_resp(3, b"Could not read configuration templates")
            } else {
                (false, out)
            }
        }
        // viewfeed / previewfeed
        38 | 39 => {
            let preview = cmd_id == 39;
            let Some((_, vf)) = (unsafe {
                xmlrpc_parse_view_feed_raw(raw_start, is_get, is_json, preview, 1, 2, 0)
            }) else {
                return err_resp(2, b"Invalid parameter");
            };
            if preview {
                unsafe {
                    host.debug_kv(b"Url=", &vf.url);
                    host.debug_kv(b"Filter=", &vf.filter);
                }
            } else {
                unsafe {
                    host.log(0, format!("ID={}", vf.id).as_bytes());
                }
            }
            let name_c = to_c_vec(&vf.name);
            let url_c = to_c_vec(&vf.url);
            let filter_c = to_c_vec(&vf.filter);
            let cat_c = to_c_vec(&vf.category);
            let ff_c = vf.feed_filter.as_ref().map(|f| to_c_vec(f));
            let cid_c = to_c_vec(&vf.cache_id);
            let flags = c_int::from(preview)
                | (c_int::from(vf.backlog) << 1)
                | (c_int::from(vf.pause_nzb) << 2)
                | (c_int::from(vf.include_non_matching) << 3);
            let (ok, out) = run_collect(
                11,
                flags,
                vf.id,
                vf.category_source,
                vf.priority,
                vf.interval,
                name_c.as_ptr().cast(),
                url_c.as_ptr().cast(),
                filter_c.as_ptr().cast(),
                cat_c.as_ptr().cast(),
                ff_c.as_ref()
                    .map_or(std::ptr::null(), |v| v.as_ptr().cast()),
                cid_c.as_ptr().cast(),
                std::ptr::null(),
                vf.cache_time_sec as usize,
                true,
            );
            if ok == 0 {
                err_resp(3, b"Could not read feed")
            } else {
                (false, out)
            }
        }
        // fetchfeed
        40 => {
            let Some(id) = cur.next_int() else {
                return err_resp(2, b"Invalid parameter (ID)");
            };
            unsafe {
                host.call_action(
                    16,
                    id,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            (false, format_bool_bytes(is_json, true))
        }
        // editserver
        41 => {
            let mut pairs: Vec<i32> = Vec::new();
            while let Some(id) = cur.next_int() {
                let Some(active) = cur.next_bool(is_json) else {
                    return err_resp(2, b"Invalid parameter");
                };
                pairs.push(id);
                pairs.push(i32::from(active));
            }
            if pairs.is_empty() {
                return err_resp(2, b"Invalid parameter");
            }
            let ok = unsafe {
                host.call_action(
                    17,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &pairs,
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            (false, format_bool_bytes(is_json, ok != 0))
        }
        // readurl
        42 => {
            let Some(url_off) = cur.next_str() else {
                return err_resp(2, b"Invalid parameter (URL)");
            };
            let url_c = to_c_vec(&cur.decode_at(url_off, is_json));
            let Some(info_off) = cur.next_str() else {
                return err_resp(2, b"Invalid parameter (InfoName)");
            };
            let info_c = to_c_vec(&cur.decode_at(info_off, is_json));
            let mut out_buf = RsBuf {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            let ok = unsafe {
                host.call_action(
                    18,
                    0,
                    0,
                    0,
                    0,
                    url_c.as_ptr().cast(),
                    info_c.as_ptr().cast(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut out_buf,
                )
            };
            let bytes = unsafe { take_rs_buf(out_buf) };
            if ok == 0 {
                err_resp(3, b"Could not read url")
            } else {
                (
                    false,
                    format_string_bytes(is_json, c_bytes(&bytes), true, 0),
                )
            }
        }
        // checkupdates
        43 => {
            let mut out_buf = RsBuf {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            let ok = unsafe {
                host.call_action(
                    19,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut out_buf,
                )
            };
            let bytes = unsafe { take_rs_buf(out_buf) };
            if ok == 0 {
                err_resp(3, b"Could not read update info from update-info-script")
            } else {
                (
                    false,
                    format_string_bytes(is_json, c_bytes(&bytes), true, 0),
                )
            }
        }
        // startupdate
        44 => {
            let Some(br_off) = cur.next_str() else {
                return err_resp(2, b"Invalid parameter (Branch)");
            };
            let br_dec = cur.decode_at(br_off, is_json);
            let br_clean = c_bytes(&br_dec);
            let branch = if br_clean.eq_ignore_ascii_case(b"STABLE") {
                0
            } else if br_clean.eq_ignore_ascii_case(b"TESTING") {
                1
            } else if br_clean.eq_ignore_ascii_case(b"DEVEL") {
                2
            } else {
                return err_resp(2, b"Invalid parameter (Branch)");
            };
            let ok = unsafe {
                host.call_action(
                    20,
                    branch,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            (false, format_bool_bytes(is_json, ok != 0))
        }
        // servervolumes
        46 => {
            let mut bp = [true; 4];
            if let Some(b0) = cur.next_bool(is_json) {
                bp[0] = b0;
                if let Some(b1) = cur.next_bool(is_json) {
                    bp[1] = b1;
                    if let Some(b2) = cur.next_bool(is_json) {
                        bp[2] = b2;
                        if let Some(b3) = cur.next_bool(is_json) {
                            bp[3] = b3;
                        }
                    }
                }
            }
            let articles = cur.next_bool(is_json).unwrap_or(true);
            let (_, out) = run_collect(
                12,
                c_int::from(bp[0]),
                c_int::from(bp[1]),
                c_int::from(bp[2]),
                c_int::from(bp[3]),
                c_int::from(articles),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                true,
            );
            (false, out)
        }
        // resetservervolume
        47 => {
            let Some(server_id) = cur.next_int() else {
                return err_resp(2, b"Invalid parameter");
            };
            let Some(cnt_off) = cur.next_str() else {
                return err_resp(2, b"Invalid parameter");
            };
            let cnt = cur.raw_at(cnt_off);
            let reset_mode = if cnt.is_empty() {
                0
            } else if cnt == b"CUSTOM" {
                1
            } else {
                return err_resp(3, b"Invalid Counter");
            };
            let ok = unsafe {
                host.call_action(
                    21,
                    server_id,
                    reset_mode,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            (false, format_bool_bytes(is_json, ok != 0))
        }
        // tlscipherstart / tlscipherstatus / tlsciphercancel
        48..=50 => {
            let full_req = c_bytes(buf);
            xmlrpc_execute_tls_cipher(
                cmd_id - 48,
                eff_access,
                crate::rpcroute::UA_CONTROL,
                proto,
                crate::rpcroute::RP_JSON_RPC,
                http_method,
                crate::rpcroute::HM_POST,
                is_json,
                Some(full_req),
                RPC_JSON_LIMITS,
                |op_kind, job_id| {
                    let j_c = job_id.map(to_c_vec);
                    let mut out_buf = RsBuf {
                        data: std::ptr::null_mut(),
                        len: 0,
                        cap: 0,
                    };
                    unsafe {
                        host.call_action(
                            22,
                            op_kind,
                            0,
                            0,
                            0,
                            j_c.as_ref().map_or(std::ptr::null(), |v| v.as_ptr().cast()),
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            0,
                            &[],
                            &[],
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            &mut out_buf,
                        );
                        take_rs_buf(out_buf)
                    }
                },
            )
        }
        // testserver
        51 => {
            let full_req = c_bytes(buf);
            let parsed = unsafe {
                xmlrpc_parse_test_server_raw(
                    Some(full_req),
                    Some(raw_start),
                    is_get,
                    is_json,
                    http_method,
                    crate::rpcroute::HM_POST,
                    3,
                    RPC_JSON_LIMITS,
                )
            };
            match parsed {
                Err((_, msg)) => err_resp(2, msg.to_bytes()),
                Ok((_, ts)) => {
                    let host_c = to_c_vec(&ts.host);
                    let user_c = to_c_vec(&ts.username);
                    let pass_c = to_c_vec(&ts.password);
                    let cipher_c = to_c_vec(&ts.cipher);
                    let mut out_buf = RsBuf {
                        data: std::ptr::null_mut(),
                        len: 0,
                        cap: 0,
                    };
                    let ok = unsafe {
                        host.call_action(
                            23,
                            ts.port,
                            c_int::from(ts.encryption),
                            ts.timeout,
                            ts.cert_verif_level,
                            host_c.as_ptr().cast(),
                            user_c.as_ptr().cast(),
                            pass_c.as_ptr().cast(),
                            cipher_c.as_ptr().cast(),
                            std::ptr::null(),
                            0,
                            &[],
                            &[],
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            &mut out_buf,
                        )
                    };
                    let err_bytes = unsafe { take_rs_buf(out_buf) };
                    let text: &[u8] = if ok != 0 {
                        b""
                    } else if err_bytes.is_empty() {
                        b"Unknown error"
                    } else {
                        c_bytes(&err_bytes)
                    };
                    (false, format_string_bytes(is_json, text, true, 1023))
                }
            }
        }
        // testserverspeed
        52 => {
            let Some(url_off) = cur.next_str() else {
                return err_resp(2, b"Invalid parameter: NZB file url");
            };
            let nzb_url_c = to_c_vec(&cur.decode_at(url_off, is_json));
            let Some(server_id) = cur.next_int() else {
                return err_resp(2, b"Invalid parameter: Server ID");
            };
            let mut out_buf = RsBuf {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            let ok = unsafe {
                host.call_action(
                    24,
                    server_id,
                    0,
                    0,
                    0,
                    nzb_url_c.as_ptr().cast(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    &[],
                    &[],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut out_buf,
                )
            };
            let err_bytes = unsafe { take_rs_buf(out_buf) };
            if ok == 0 {
                err_resp(2, &err_bytes)
            } else {
                (false, format_bool_bytes(is_json, true))
            }
        }
        // testdiskspeed
        53 => match unsafe { xmlrpc_parse_test_disk_speed_raw(raw_start, is_get, is_json) } {
            Err((_, msg)) => err_resp(2, msg.to_bytes()),
            Ok((_, dir_raw, buffer_bytes, max_file_bytes, timeout_sec)) => {
                let dir_bytes = c_bytes(&dir_raw);
                let _pause = unsafe { SpeedTestPauseGuard::new(host.ctx, host.action_cb) };
                let avail = crate::fsops::statvfs_available_bytes(dir_bytes);
                match crate::util::disk_benchmark_run_bytes(
                    dir_bytes,
                    buffer_bytes,
                    max_file_bytes,
                    std::time::Duration::from_secs(timeout_sec as u64),
                    avail,
                ) {
                    Err(err) => {
                        let msg = match err {
                            crate::util::DiskBenchmarkError::InvalidArgument(m)
                            | crate::util::DiskBenchmarkError::Runtime(m) => m,
                        };
                        let mut warn_msg = Vec::with_capacity(dir_bytes.len() + msg.len() + 36);
                        warn_msg.extend_from_slice(b"Disk speed test for ");
                        warn_msg.extend_from_slice(dir_bytes);
                        warn_msg.extend_from_slice(b" failed: ");
                        warn_msg.extend_from_slice(msg.as_bytes());
                        unsafe {
                            host.log(1, &warn_msg);
                        }
                        err_resp(2, msg.as_bytes())
                    }
                    Ok(res) => {
                        let mib_per_sec = |b: u64, ms: f64| -> f64 {
                            if ms > 0.0 {
                                (b as f64 / 1024.0 / 1024.0) / (ms / 1000.0)
                            } else {
                                0.0
                            }
                        };
                        let tail = format!(
                            ": write {:.1} MiB/s, read {:.1} MiB/s",
                            mib_per_sec(res.write_bytes, res.write_ms),
                            mib_per_sec(res.read_bytes, res.read_ms)
                        );
                        let mut info_msg = Vec::with_capacity(dir_bytes.len() + tail.len() + 24);
                        info_msg.extend_from_slice(b"Disk speed test for ");
                        info_msg.extend_from_slice(dir_bytes);
                        info_msg.extend_from_slice(tail.as_bytes());
                        unsafe {
                            host.log(3, &info_msg);
                        }
                        (
                            false,
                            format_disk_speed_bytes(
                                is_json,
                                res.write_bytes,
                                res.write_ms,
                                res.read_bytes,
                                res.read_ms,
                            ),
                        )
                    }
                }
            }
        },
        // testnetworkspeed
        54 => {
            let _pause = unsafe { SpeedTestPauseGuard::new(host.ctx, host.action_cb) };
            let res = crate::webdownload::run_network_speed_test(
                |req_bytes, timeout_sec| {
                    let mut dl: u32 = 0;
                    let mut elapsed: f64 = 0.0;
                    unsafe {
                        host.call_action(
                            25,
                            timeout_sec as c_int,
                            0,
                            0,
                            0,
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            req_bytes.as_ptr().cast(),
                            req_bytes.len(),
                            &[],
                            &[],
                            &mut dl,
                            &mut elapsed,
                            std::ptr::null_mut(),
                        );
                    }
                    (dl, elapsed)
                },
                |kind, msg| unsafe {
                    host.log(if kind == 1 { 2 } else { 3 }, msg.as_bytes());
                },
            );
            match res {
                Err(msg) => err_resp(2, msg.as_bytes()),
                Ok(speed_mbps) => (false, format_network_speed_bytes(is_json, speed_mbps)),
            }
        }
        _ => err_resp(default_err_code, default_err_text),
    }
}

fn wrap_rpc_envelope(
    host: &XmlRpcHost<'_>,
    proto: c_int,
    inner: &[u8],
    callback: Option<&[u8]>,
    fault: bool,
    request_id: Option<&[u8]>,
) -> (Vec<u8>, &'static [u8]) {
    let body_c = c_bytes(inner);
    unsafe {
        host.debug_kv(b"Response=", body_c);
    }
    let (head, tail) = crate::rpcroute::envelope(proto, fault, callback, request_id);
    let mut out = Vec::with_capacity(head.len() + body_c.len() + tail.len());
    out.extend_from_slice(&head);
    out.extend_from_slice(body_c);
    out.extend_from_slice(&tail);
    let ct: &'static [u8] = if proto == crate::rpcroute::RP_XML_RPC {
        b"text/xml"
    } else {
        b"application/json"
    };
    (out, ct)
}

unsafe fn execute_multicall(
    host: &XmlRpcHost<'_>,
    proto: c_int,
    http_method: c_int,
    eff_access: c_int,
    buf: &mut [u8],
    safe_method: &mut bool,
) -> (Vec<u8>, &'static [u8]) {
    let end = crate::bytes::find_byte(buf, 0).unwrap_or(buf.len());
    let mut pos = 0usize;
    let mut parse_err = false;
    let mut mc_resp = Vec::with_capacity(1024);
    mc_resp.extend_from_slice(b"<array><data>");

    while pos < end {
        let Some(rel_end) = crate::bytes::find(&buf[pos..end], b"</struct>") else {
            break;
        };
        let call_end = pos + rel_end;
        buf[call_end] = 0;
        unsafe {
            host.debug_kv(b"MutliCall, request=", &buf[pos..call_end]);
        }
        let Some(rel_name_end) = crate::bytes::find(&buf[pos..call_end], b"</name>") else {
            parse_err = true;
            break;
        };
        let name_end_pos = pos + rel_name_end;
        let mut sub_method = Vec::new();
        if let Some((tag_off, tag_len)) =
            crate::webutil::xml_find_tag(&buf[name_end_pos..call_end], b"string")
        {
            let max_len = (tag_len.max(0) as usize).min(99);
            let start = name_end_pos + tag_off;
            let slice = &buf[start..call_end.min(start + max_len)];
            sub_method.extend_from_slice(c_bytes(slice));
        }
        unsafe {
            host.debug_kv(b"MutliCall, MethodName=", &sub_method);
        }
        let sub_start = match crate::bytes::find(&buf[name_end_pos..call_end], b"params</name>") {
            Some(rel_p) => name_end_pos + rel_p,
            None => pos,
        };
        let (sub_cmd_id, err_code, err_text) = resolve_command(host, eff_access, &sub_method);
        *safe_method |= is_safe_command(sub_cmd_id);
        let (_, sub_resp) = unsafe {
            execute_xml_command(
                host,
                sub_cmd_id,
                err_code,
                err_text,
                proto,
                http_method,
                eff_access,
                &mut buf[..=call_end],
                sub_start,
            )
        };
        let sub_c = c_bytes(&sub_resp);
        unsafe {
            host.debug_kv(b"MutliCall, Response=", sub_c);
        }
        let is_fault = sub_c.starts_with(b"<fault>");
        let is_array = !is_fault && sub_c.starts_with(b"<array>");
        if !is_fault && !is_array {
            mc_resp.extend_from_slice(b"<array><data>");
        }
        mc_resp.extend_from_slice(b"<value>");
        mc_resp.extend_from_slice(sub_c);
        mc_resp.extend_from_slice(b"</value>");
        if !is_fault && !is_array {
            mc_resp.extend_from_slice(b"</data></array>");
        }
        pos = call_end + b"</struct>".len();
    }

    if parse_err {
        let err_body = format_error_bytes(false, 4, b"Parse error");
        wrap_rpc_envelope(host, proto, &err_body, Some(b""), true, None)
    } else {
        mc_resp.extend_from_slice(b"</data></array>");
        wrap_rpc_envelope(host, proto, &mc_resp, Some(b""), false, None)
    }
}

/// Executes `XmlRpcProcessor` (`SetUrl` + `Execute`) in place on `req_body` and returns
/// `(response_body, content_type, is_safe_method)`.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn serve_xmlrpc(
    ctx: *mut c_void,
    req_body: &mut [u8],
    http_method: c_int,
    user_access: c_int,
    json_p: bool,
    debug_build: bool,
    url_bytes: &[u8],
    version: &[u8],
    config_filename: Option<&[u8]>,
    log_cb: Option<NzbgetRsXmlRpcLogFn>,
    action_cb: Option<NzbgetRsXmlRpcActionFn>,
    collect_cb: Option<NzbgetRsXmlRpcCollectFn>,
) -> (Vec<u8>, Option<&'static [u8]>, bool) {
    let host = XmlRpcHost {
        ctx,
        debug_build,
        version: c_bytes(version),
        config_filename: config_filename.map(c_bytes),
        log_cb,
        action_cb,
        collect_cb,
    };

    let url_clean = c_bytes(url_bytes);
    let q = url_clean
        .iter()
        .position(|&b| b == b'?')
        .unwrap_or(url_clean.len());
    let mut norm_url = url_clean[..q].to_vec();
    let dec_len = crate::text::url_decode(&mut norm_url);
    norm_url.truncate(dec_len);
    norm_url.extend_from_slice(&url_clean[q..]);
    let norm_url_c = c_bytes(&norm_url).to_vec();

    let proto = crate::rpcroute::protocol(&norm_url_c);
    if proto == crate::rpcroute::RP_UNDEFINED {
        let mut msg = Vec::with_capacity(norm_url_c.len() + 40);
        msg.extend_from_slice(b"internal error: invalid rpc-request: ");
        msg.extend_from_slice(&norm_url_c);
        unsafe {
            host.log(2, &msg);
        }
        return (Vec::new(), None, false);
    }

    let is_get = http_method == RPC_HM_GET;
    let req_c_len = crate::bytes::find_byte(req_body, 0).unwrap_or(req_body.len());
    let r = crate::rpcroute::route(&norm_url_c, &req_body[..req_c_len], is_get, proto);
    let request_id: Option<Vec<u8>> = r.id.map(|(at, len)| {
        let tail = if at <= req_c_len {
            &req_body[at..req_c_len]
        } else {
            &[][..]
        };
        if len > 0 {
            tail[..(len as usize).min(tail.len())].to_vec()
        } else {
            tail.to_vec()
        }
    });

    unsafe {
        host.debug_kv(b"MethodName=", &r.method);
    }

    let eff_access = crate::rpcroute::access(proto, user_access);

    if r.method.eq_ignore_ascii_case(b"system.multicall")
        && proto == crate::rpcroute::RP_XML_RPC
        && http_method == crate::rpcroute::HM_POST
    {
        let mut safe_method = false;
        let mut owned_post: Vec<u8>;
        let work_slice: &mut [u8] = if req_body.last() == Some(&0) {
            req_body
        } else {
            owned_post = req_body.to_vec();
            owned_post.push(0);
            &mut owned_post
        };
        let (out, ct) = unsafe {
            execute_multicall(
                &host,
                proto,
                http_method,
                eff_access,
                work_slice,
                &mut safe_method,
            )
        };
        return (out, Some(ct), safe_method);
    }

    let mut owned_buf: Vec<u8>;
    let work_buf: &mut [u8] = match r.params {
        Some(at) => {
            let tail = if at <= norm_url_c.len() {
                &norm_url_c[at..]
            } else {
                &[][..]
            };
            owned_buf = Vec::with_capacity(tail.len() + 1);
            owned_buf.extend_from_slice(tail);
            owned_buf.push(0);
            &mut owned_buf
        }
        None => {
            if req_body.last() == Some(&0) {
                req_body
            } else {
                owned_buf = req_body.to_vec();
                owned_buf.push(0);
                &mut owned_buf
            }
        }
    };

    let (cmd_id, default_err_code, default_err_text) =
        resolve_command(&host, eff_access, &r.method);
    let is_json = xmlrpc_is_json_protocol(
        proto,
        crate::rpcroute::RP_JSON_RPC,
        crate::rpcroute::RP_JSONP_RPC,
    );
    let mut cur_pos = 0usize;
    if xmlrpc_should_skip_to_json_post_params(is_json, http_method, crate::rpcroute::HM_POST) {
        cur_pos = crate::rpcparams::skip_to_params(work_buf);
    }
    let mut callback_func: Option<Vec<u8>> = None;
    if proto == crate::rpcroute::RP_JSONP_RPC {
        let raw = unsafe { crate::rpcparams::Raw::new(work_buf.as_mut_ptr().add(cur_pos)) };
        let mut pcur = unsafe { ParamCursor::from_raw(raw, is_get, is_json) };
        if let Some(cb_off) = pcur.next_str() {
            callback_func = Some(pcur.raw_at(cb_off).to_vec());
            cur_pos += pcur.cur_off;
        }
    }

    let safe_method = is_safe_command(cmd_id);
    let decision = crate::rpcroute::decide(proto, http_method, safe_method, cmd_id <= 0, json_p);
    let req_id_slice = request_id.as_deref();

    let (out, ct) = match decision.dispatch {
        crate::rpcroute::DP_EXECUTE => {
            let (fault, inner) = unsafe {
                execute_xml_command(
                    &host,
                    cmd_id,
                    default_err_code,
                    default_err_text,
                    proto,
                    http_method,
                    eff_access,
                    work_buf,
                    cur_pos,
                )
            };
            wrap_rpc_envelope(
                &host,
                proto,
                &inner,
                callback_func.as_deref(),
                fault,
                req_id_slice,
            )
        }
        crate::rpcroute::DP_REFUSE_IN_CALLBACK => {
            let err_text = decision
                .text
                .map_or(&b"Invalid procedure"[..], CStr::to_bytes);
            let err_body = format_error_bytes(
                proto != crate::rpcroute::RP_XML_RPC,
                decision.code,
                err_text,
            );
            wrap_rpc_envelope(
                &host,
                proto,
                &err_body,
                callback_func.as_deref(),
                true,
                req_id_slice,
            )
        }
        _ => {
            let err_text = decision
                .text
                .map_or(&b"Invalid procedure"[..], CStr::to_bytes);
            let err_body = format_error_bytes(
                proto != crate::rpcroute::RP_XML_RPC,
                decision.code,
                err_text,
            );
            wrap_rpc_envelope(&host, proto, &err_body, Some(b""), true, None)
        }
    };

    (out, Some(ct), safe_method)
}

#[cfg(test)]
mod remote_xmlrpc_tests {
    use super::*;
    use crate::options::{OptionsHost, RustOptions};
    use crate::rpcparams::{next_bool, next_int, next_str, skip_to_params, Kind};
    use crate::rpcroute::{
        access, decide, envelope, protocol, route, DP_EXECUTE, DP_REFUSE, DP_REFUSE_IN_CALLBACK,
        HM_POST, RP_JSONP_RPC, RP_JSON_RPC, RP_UNDEFINED, UA_CONTROL, UA_RESTRICTED,
    };

    const HM_GET: i32 = 1;
    const UA_ADD: i32 = 2;

    /// Pure-Rust equivalent of `XmlCommand` parameter cursor (`daemon/remote/XmlRpc.cpp`).
    struct RustXmlCommand {
        buf: Vec<u8>,
        pos: usize,
        protocol: i32,
        http_method: i32,
        callback_func: Option<Vec<u8>>,
    }

    impl RustXmlCommand {
        fn new(request: &[u8], protocol: i32, http_method: i32) -> Self {
            let mut buf = request.to_vec();
            if !buf.ends_with(&[0]) {
                buf.push(0);
            }
            Self {
                buf,
                pos: 0,
                protocol,
                http_method,
                callback_func: None,
            }
        }

        fn is_json(&self) -> bool {
            xmlrpc_is_json_protocol(self.protocol, RP_JSON_RPC, RP_JSONP_RPC)
        }

        fn kind(&self) -> Kind {
            if self.http_method == HM_GET {
                Kind::Get
            } else if self.is_json() {
                Kind::Json
            } else {
                Kind::Xml
            }
        }

        fn prepare_params(&mut self) {
            if xmlrpc_should_skip_to_json_post_params(self.is_json(), self.http_method, HM_POST) {
                let step = skip_to_params(&mut self.buf[self.pos..]);
                self.pos += step;
            }
            if self.protocol == RP_JSONP_RPC {
                self.callback_func = self.next_param_as_str();
            }
        }

        fn next_param_as_int(&mut self) -> Option<i32> {
            let k = self.kind();
            let (step, val) = next_int(&mut self.buf[self.pos..], k);
            let v = val?;
            self.pos += step;
            Some(v)
        }

        fn next_param_as_bool(&mut self) -> Option<bool> {
            let k = self.kind();
            let json = self.is_json();
            let (step, val) = next_bool(&mut self.buf[self.pos..], k, json);
            self.pos += step;
            val
        }

        fn next_param_as_str(&mut self) -> Option<Vec<u8>> {
            let k = self.kind();
            let (step, rel_off) = next_str(&mut self.buf[self.pos..], k);
            let rel = rel_off?;
            let val_off = self.pos + rel;
            self.pos += step;
            Some(raw_param_at(&self.buf, val_off).to_vec())
        }
    }

    /// Pure-Rust equivalent of `XmlRpcProcessor::Execute` (`daemon/remote/XmlRpc.cpp`).
    fn rpc_reply_with_options(
        url: &str,
        http_method: i32,
        user_access: i32,
        body: &str,
        json_p: bool,
        options: Option<&RustOptions<'_>>,
    ) -> String {
        let norm_url = xmlrpc_normalize_url(url.as_bytes());
        let proto = protocol(&norm_url);
        if proto == RP_UNDEFINED {
            return String::new();
        }
        let is_get = http_method == HM_GET;
        let r = route(&norm_url, body.as_bytes(), is_get, proto);
        let eff_access = access(proto, user_access);
        let req_id: Option<Vec<u8>> =
            r.id.map(|(start, len)| body.as_bytes()[start..start + (len as usize)].to_vec());

        let param_slice: &[u8] = match r.params {
            Some(off) if is_get => &norm_url[off.min(norm_url.len())..],
            _ => body.as_bytes(),
        };
        let mut cmd = RustXmlCommand::new(param_slice, proto, http_method);
        cmd.prepare_params();
        let is_json = cmd.is_json();

        let access_denied =
            xmlrpc_check_user_access_denied(eff_access, UA_ADD, UA_RESTRICTED, &r.method);
        let cmd_id = match_command(&r.method);
        let is_error = access_denied != 0 || cmd_id == 0;
        let is_safe = !is_error
            && matches!(
                cmd_id,
                5 | 6
                    | 8
                    | 9
                    | 10
                    | 11
                    | 12
                    | 13
                    | 17
                    | 20
                    | 27
                    | 28
                    | 29
                    | 30
                    | 31
                    | 37
                    | 45
                    | 46
                    | 56
            );

        let dec = decide(proto, http_method, is_safe, is_error, json_p);
        let (fault, payload, cb_override) = match dec.dispatch {
            DP_EXECUTE => {
                if access_denied != 0 {
                    (
                        true,
                        format_error_bytes(is_json, 401, b"Access denied"),
                        cmd.callback_func.as_deref(),
                    )
                } else {
                    let (f, p) = match cmd_id {
                        5 => (false, format_string_bytes(is_json, b"26.0", false, 1023)),
                        14 => match xmlrpc_parse_edit_queue(
                            &mut cmd.buf[cmd.pos..],
                            is_get,
                            is_json,
                            1,
                            11,
                        ) {
                            Ok(_) => (false, format_bool_bytes(is_json, true)),
                            Err((_, code, msg)) => (true, format_error_bytes(is_json, code, msg)),
                        },
                        15 => {
                            let Some(_fname) = cmd.next_param_as_str() else {
                                return wrap_reply(
                                    proto,
                                    true,
                                    &format_error_bytes(
                                        is_json,
                                        2,
                                        b"Invalid parameter (Filename)",
                                    ),
                                    cmd.callback_func.as_deref(),
                                    req_id.as_deref(),
                                );
                            };
                            (false, format_bool_bytes(is_json, true))
                        }
                        29 => {
                            let items: Vec<(&[u8], &[u8], bool)> = match options {
                                Some(opts) => opts
                                    .opt_entries
                                    .iter()
                                    .map(|e| {
                                        (
                                            e.name.to_bytes(),
                                            e.value.as_ref().map_or(&b""[..], |v| v.to_bytes()),
                                            e.restricted(),
                                        )
                                    })
                                    .collect(),
                                None => Vec::new(),
                            };
                            (
                                false,
                                format_config_list_bytes(
                                    is_json,
                                    eff_access,
                                    UA_RESTRICTED,
                                    &items,
                                ),
                            )
                        }
                        36 => {
                            let cfg_file = options
                                .and_then(|o| o.config_filename.as_ref().map(|c| c.to_bytes()));
                            let (_off, f, p) = xmlrpc_execute_save_config(
                                &mut cmd.buf[cmd.pos..],
                                is_get,
                                is_json,
                                cfg_file,
                            );
                            (f, p)
                        }
                        42 => {
                            let Some(_url) = cmd.next_param_as_str() else {
                                return wrap_reply(
                                    proto,
                                    true,
                                    &format_error_bytes(is_json, 2, b"Invalid parameter"),
                                    cmd.callback_func.as_deref(),
                                    req_id.as_deref(),
                                );
                            };
                            (false, format_string_bytes(is_json, b"", true, 0))
                        }
                        48..=50 => {
                            let op = cmd_id - 48;
                            xmlrpc_execute_tls_cipher(
                                op,
                                eff_access,
                                UA_CONTROL,
                                proto,
                                RP_JSON_RPC,
                                http_method,
                                HM_POST,
                                is_json,
                                Some(body.as_bytes()),
                                crate::json::TEST_LIMITS,
                                |op_kind, id_opt| {
                                    let id_cstr =
                                        id_opt.and_then(|s| std::ffi::CString::new(s).ok());
                                    let id_ptr =
                                        id_cstr.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
                                    let buf = unsafe {
                                        match op_kind {
                                            0 => crate::ffi::nzbget_rs_tls_cipher_start(
                                                std::ptr::null(),
                                            ),
                                            1 => crate::ffi::nzbget_rs_tls_cipher_status(id_ptr),
                                            _ => crate::ffi::nzbget_rs_tls_cipher_cancel(id_ptr),
                                        }
                                    };
                                    let out = unsafe {
                                        std::slice::from_raw_parts(buf.data as *const u8, buf.len)
                                            .to_vec()
                                    };
                                    unsafe { crate::ffi::nzbget_rs_free(buf) };
                                    out
                                },
                            )
                        }
                        _ => (true, format_error_bytes(is_json, 1, b"Invalid procedure")),
                    };
                    (f, p, cmd.callback_func.as_deref())
                }
            }
            DP_REFUSE_IN_CALLBACK => {
                let txt = dec.text.map_or(&b""[..], |c| c.to_bytes());
                (
                    true,
                    format_error_bytes(is_json, dec.code, txt),
                    cmd.callback_func.as_deref(),
                )
            }
            DP_REFUSE => {
                let txt = dec.text.map_or(&b""[..], |c| c.to_bytes());
                (
                    true,
                    format_error_bytes(is_json, dec.code, txt),
                    Some(&b""[..]),
                )
            }
            _ => unreachable!(),
        };

        wrap_reply(proto, fault, &payload, cb_override, req_id.as_deref())
    }

    fn wrap_reply(
        proto: i32,
        fault: bool,
        payload: &[u8],
        callback: Option<&[u8]>,
        req_id: Option<&[u8]>,
    ) -> String {
        let (head, tail) = envelope(proto, fault, callback, req_id);
        let mut out = head;
        out.extend_from_slice(payload);
        out.extend_from_slice(&tail);
        String::from_utf8_lossy(&out).into_owned()
    }

    fn rpc_reply(url: &str, http_method: i32, body: &str, json_p: bool) -> String {
        rpc_reply_with_options(url, http_method, UA_CONTROL, body, json_p, None)
    }

    fn has_error_code(reply: &str, code: i32) -> bool {
        reply.contains(&format!("\"code\" : {code},"))
    }

    #[test]
    fn test_save_config_params_port_from_cpp() {
        let json_input = r#"{
	"method":"saveconfig",
	"params":[[
		{"Name":"OptionName1","Value":"OptionValue1"},
		{"Name":"OptionName2","Value":"OptionValue2"}
	]],
	"id":1}"#;

        let mut cmd = RustXmlCommand::new(json_input.as_bytes(), RP_JSON_RPC, HM_POST);
        cmd.prepare_params();

        assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b"Name"[..]));
        assert_eq!(
            cmd.next_param_as_str().as_deref(),
            Some(&b"OptionName1"[..])
        );
        assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b"Value"[..]));
        assert_eq!(
            cmd.next_param_as_str().as_deref(),
            Some(&b"OptionValue1"[..])
        );

        assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b"Name"[..]));
        assert_eq!(
            cmd.next_param_as_str().as_deref(),
            Some(&b"OptionName2"[..])
        );
        assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b"Value"[..]));
        assert_eq!(
            cmd.next_param_as_str().as_deref(),
            Some(&b"OptionValue2"[..])
        );
    }

    #[test]
    fn test_next_param_as_types_port_from_cpp() {
        {
            let mut cmd = RustXmlCommand::new(b"{\"params\":[123,456]}", RP_JSON_RPC, HM_POST);
            cmd.prepare_params();
            assert_eq!(cmd.next_param_as_int(), Some(123));
            assert_eq!(cmd.next_param_as_int(), Some(456));
            assert_eq!(cmd.next_param_as_int(), None);
        }
        {
            let mut cmd = RustXmlCommand::new(b"{\"params\":[true,false]}", RP_JSON_RPC, HM_POST);
            cmd.prepare_params();
            assert_eq!(cmd.next_param_as_bool(), Some(true));
            assert_eq!(cmd.next_param_as_bool(), Some(false));
            assert_eq!(cmd.next_param_as_bool(), None);
        }
        {
            let mut cmd =
                RustXmlCommand::new(b"{\"params\":[\"abc\",\"def\"]}", RP_JSON_RPC, HM_POST);
            cmd.prepare_params();
            assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b"abc"[..]));
            assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b"def"[..]));
            assert_eq!(cmd.next_param_as_str(), None);
        }
    }

    #[test]
    fn test_config_commands_parsing_port_from_cpp() {
        {
            let mut cmd = RustXmlCommand::new(b"{\"params\":[true]}", RP_JSON_RPC, HM_POST);
            cmd.prepare_params();
            assert_eq!(cmd.next_param_as_bool(), Some(true));
        }
        {
            let mut cmd = RustXmlCommand::new(b"{\"params\":[]}", RP_JSON_RPC, HM_POST);
            cmd.prepare_params();
            assert_eq!(cmd.next_param_as_bool(), None);
        }
    }

    #[test]
    fn test_append_command_port_from_cpp() {
        let json_input = br#"{"method":"append","params":["test.nzb","PD94bWw...","",0,false,false,"",0,"FORCE"],"id":1}"#;
        let mut cmd = RustXmlCommand::new(json_input, RP_JSON_RPC, HM_POST);
        cmd.prepare_params();

        assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b"test.nzb"[..]));
        assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b"PD94bWw..."[..]));
        assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b""[..]));
        assert_eq!(cmd.next_param_as_int(), Some(0));
        assert_eq!(cmd.next_param_as_bool(), Some(false));
        assert_eq!(cmd.next_param_as_bool(), Some(false));
        assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b""[..]));
        assert_eq!(cmd.next_param_as_int(), Some(0));
        assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b"FORCE"[..]));
        assert_eq!(cmd.next_param_as_str(), None);
    }

    #[test]
    fn test_edit_queue_with_id_after_port_from_cpp() {
        let json_input = br#"{"params":["PausePost", "", [123, 456]],"id":1}"#;
        let mut cmd = RustXmlCommand::new(json_input, RP_JSON_RPC, HM_POST);
        cmd.prepare_params();

        assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b"PausePost"[..]));
        assert_eq!(cmd.next_param_as_int(), None);
        assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b""[..]));
        assert_eq!(cmd.next_param_as_int(), Some(123));
        assert_eq!(cmd.next_param_as_int(), Some(456));
        assert_eq!(cmd.next_param_as_int(), None);
    }

    #[test]
    fn test_whitespace_after_last_param_port_from_cpp() {
        for input in [
            &b"{\"params\":[123 ],\"id\":1}"[..],
            &b"{\"params\":[123\n],\"id\":1}"[..],
        ] {
            let mut cmd = RustXmlCommand::new(input, RP_JSON_RPC, HM_POST);
            cmd.prepare_params();
            assert_eq!(cmd.next_param_as_int(), Some(123));
            assert_eq!(cmd.next_param_as_int(), None);
        }
        {
            let mut cmd =
                RustXmlCommand::new(b"{\"params\":[\"abc\" ],\"id\":1}", RP_JSON_RPC, HM_POST);
            cmd.prepare_params();
            assert_eq!(cmd.next_param_as_str().as_deref(), Some(&b"abc"[..]));
            assert_eq!(cmd.next_param_as_str(), None);
        }
    }

    #[test]
    fn cipher_save_metadata_rejects_malformed_and_never_leaks_into_file_port_from_cpp() {
        for body in [
            r#"{"method":"saveconfig","params":[[{"Name":"_TlsCipherBaseline","Value":"v1\n"}]],"id":1}"#,
            r#"{"method":"saveconfig","params":[[{"Name":"_TlsCipherBaseline","Value":"v2\n"},{"Name":"X","Value":"y"}]],"id":1}"#,
            r#"{"method":"saveconfig","params":[[{"Name":"_TlsCipherBaseline","Value":"v1\n\u0000bad"},{"Name":"X","Value":"y"}]],"id":1}"#,
            r#"{"method":"saveconfig","params":[[{"Name":"_TlsCipherBaseline","Value":"v1\n"},{"Name":"_TlsCipherBaseline","Value":"v1\n"},{"Name":"X","Value":"y"}]],"id":1}"#,
        ] {
            assert!(has_error_code(
                &rpc_reply("/jsonrpc", HM_POST, body, true),
                2
            ));
        }

        let dir = std::env::temp_dir().join(format!(
            "nzbget-cipher-rpc-rs-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nzbget.conf");
        std::fs::write(
            &path,
            b"Server1.Host=example.invalid\nServer1.Encryption=yes\nServer1.Cipher=old\n",
        )
        .unwrap();

        let path_cstr = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        let mut options = RustOptions::new_no_config(None, None);
        options.config_filename = Some(path_cstr.clone());

        let loaded = crate::configfile::script_config_load(path_cstr.to_bytes()).unwrap();
        let names_refs: Vec<&[u8]> = loaded.iter().map(|(n, _)| n.to_bytes()).collect();
        let values_refs: Vec<&[u8]> = loaded.iter().map(|(_, v)| v.to_bytes()).collect();
        let token_bytes = crate::configfile::build_cipher_baseline(&names_refs, &values_refs);
        let mut token_escaped = Vec::new();
        crate::escape::json_encode(&token_bytes, &mut token_escaped);
        let token_json = String::from_utf8(token_escaped).unwrap();

        let body = format!(
            r#"{{"method":"saveconfig","params":[[{{"Name":"Server1.Host","Value":"example.invalid"}},{{"Name":"Server1.Encryption","Value":"yes"}},{{"Name":"Server1.Cipher","Value":"optimized"}},{{"Name":"_TlsCipherBaseline","Value":"{token_json}"}}]],"id":1}}"#
        );
        let reply1 =
            rpc_reply_with_options("/jsonrpc", HM_POST, UA_CONTROL, &body, true, Some(&options));
        assert!(reply1.contains("true"), "unexpected reply1: {reply1}");

        let saved_bytes =
            crate::configfile::script_config_read_cipher_snapshot(path_cstr.to_bytes()).unwrap();
        let saved = String::from_utf8(saved_bytes).unwrap();
        assert!(!saved.contains("_TlsCipherBaseline"));
        assert!(saved.contains("Server1.Cipher=optimized"));

        let reply2 =
            rpc_reply_with_options("/jsonrpc", HM_POST, UA_CONTROL, &body, true, Some(&options));
        assert!(has_error_code(&reply2, 3));
        let unchanged = String::from_utf8(
            crate::configfile::script_config_read_cipher_snapshot(path_cstr.to_bytes()).unwrap(),
        )
        .unwrap();
        assert_eq!(unchanged, saved);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_jsonp_and_http_method_safety_port_from_cpp() {
        for method in ["editqueue", "readurl", "append"] {
            let reply = rpc_reply(&format!("/jsonprpc/{method}?=cb"), HM_GET, "", true);
            assert!(has_error_code(&reply, 4));
            assert!(!has_error_code(&reply, 2));
            assert!(reply.starts_with("cb("));
            assert!(reply.ends_with(')'));
        }

        let ver_reply = rpc_reply("/jsonprpc/version?=cb", HM_GET, "", true);
        assert!(ver_reply.starts_with("cb("));
        assert!(ver_reply.contains("\"result\""));
        assert!(!ver_reply.contains("\"error\""));

        let get_edit = rpc_reply("/jsonrpc/editqueue", HM_GET, "", true);
        assert!(has_error_code(&get_edit, 4));

        let post_edit = rpc_reply(
            "/jsonrpc",
            HM_POST,
            r#"{"method":"editqueue","params":[],"id":1}"#,
            true,
        );
        assert!(has_error_code(&post_edit, 2));
        assert!(!has_error_code(&post_edit, 4));

        let jsonp_off = rpc_reply("/jsonprpc/version?=cb", HM_GET, "", false);
        assert!(has_error_code(&jsonp_off, 403));
        assert!(!jsonp_off.contains("\"result\""));
        assert!(jsonp_off.starts_with("cb("));
    }

    #[test]
    fn test_jsonp_reads_as_restricted_user_and_protected_options_port_from_cpp() {
        let options =
            RustOptions::new_no_config(Some(&[c"SevenZipCmd=7z", c"UnrarCmd=unrar"]), None);
        let masked_needle = "\"Name\" : \"ControlPassword\",\n\"Value\" : \"***\"";

        let reply = rpc_reply_with_options(
            "/jsonprpc/config?=cb",
            HM_GET,
            UA_CONTROL,
            "",
            true,
            Some(&options),
        );
        assert!(reply.starts_with("cb("));
        assert!(reply.contains(masked_needle));
        assert!(!reply.contains("tegbzn6789"));

        let direct = rpc_reply_with_options(
            "/jsonrpc",
            HM_POST,
            UA_CONTROL,
            r#"{"method":"config","params":[],"id":1}"#,
            true,
            Some(&options),
        );
        assert!(!direct.contains(masked_needle));
        assert!(direct.contains("\"Name\" : \"ControlPassword\",\n\"Value\" : \"tegbzn6789\""));

        assert!(crate::options::option_restricted(c"ControlPassword", true));
        assert!(crate::options::option_restricted(c"Server1.Username", true));
        assert!(crate::options::option_restricted(c"DupeSearchApiKey", true));
        assert!(crate::options::option_restricted(c"JsonP", true));
        assert!(!crate::options::option_restricted(c"DupeSearchUrl", true));
        assert!(!crate::options::option_restricted(c"DupeCheck", true));
    }

    #[test]
    fn tls_cipher_rpc_security_and_status_port_from_cpp() {
        for method in ["tlscipherstart", "tlscipherstatus", "tlsciphercancel"] {
            for access_lvl in [UA_RESTRICTED, UA_ADD] {
                let req = format!("{{\"method\":\"{method}\",\"params\":[],\"id\":1}}");
                let reply =
                    rpc_reply_with_options("/jsonrpc", HM_POST, access_lvl, &req, true, None);
                assert!(reply.contains("Access denied"));
            }

            let get_reply = rpc_reply(&format!("/jsonrpc/{method}"), HM_GET, "", true);
            assert!(get_reply.contains("\"error\""));
        }

        for req in [
            r#"{"method":"tlscipherstart","params":["host.invalid"],"id":1}"#,
            r#"{"method":"tlscipherstatus","params":[42],"id":1}"#,
            r#"{"method":"tlsciphercancel","params":[],"id":1}"#,
            r#"{"method":"tlscipherstatus","params":["bad\u0000id"],"id":1}"#,
            r#"{"method":"tlscipherstatus","params":["one","two"],"id":1}"#,
        ] {
            let reply = rpc_reply("/jsonrpc", HM_POST, req, true);
            assert!(
                reply.contains("Invalid cipher"),
                "unexpected reply: {reply}"
            );
        }

        let status_reply = rpc_reply(
            "/jsonrpc",
            HM_POST,
            r#"{"method":"tlscipherstatus","params":[],"id":1}"#,
            true,
        );
        let doc = crate::json::parse(status_reply.as_bytes(), crate::json::TEST_LIMITS).unwrap();
        let res = doc.get(b"result").unwrap();
        assert_eq!(res.get(b"Available"), Some(&crate::json::Value::Bool(true)));
        let stale = rpc_reply(
            "/jsonrpc",
            HM_POST,
            r#"{"method":"tlscipherstatus","params":["unknown-fixture-job"],"id":1}"#,
            true,
        );
        assert!(stale.contains("unknown cipher job"));
        assert!(status_reply.len() <= 1024 * 1024);
    }

    #[test]
    fn save_config_rejects_conflicting_settings_and_boot_reconciliation_port_from_cpp() {
        // Rule 7.1
        {
            let reply = rpc_reply(
                "/jsonrpc",
                HM_POST,
                r#"{"method":"saveconfig","params":[[{"Name":"Server1.Encryption","Value":"no"},{"Name":"Server1.Cipher","Value":"AES128"}]],"id":1}"#,
                true,
            );
            assert!(has_error_code(&reply, 3));
            assert!(reply.contains(
                "Server1.Cipher cannot be configured when Server1.Encryption is disabled"
            ));
        }
        // Rule 7.2
        {
            let reply = rpc_reply(
                "/jsonrpc",
                HM_POST,
                r#"{"method":"saveconfig","params":[[{"Name":"DupeSearch","Value":"yes"},{"Name":"HealthCheck","Value":"park"}]],"id":1}"#,
                true,
            );
            assert!(has_error_code(&reply, 3));
            assert!(reply.contains("DupeSearch=yes"));
            assert!(reply.contains("HealthCheck=park"));
        }
        // Rule 7.3
        {
            let reply = rpc_reply(
                "/jsonrpc",
                HM_POST,
                r#"{"method":"saveconfig","params":[[{"Name":"DirectWrite","Value":"yes"},{"Name":"FlushQueue","Value":"no"}]],"id":1}"#,
                true,
            );
            assert!(has_error_code(&reply, 3));
            assert!(reply.contains("DirectWrite=yes"));
            assert!(reply.contains("FlushQueue=yes"));
        }
        // Rule 7.4
        {
            let reply = rpc_reply(
                "/jsonrpc",
                HM_POST,
                r#"{"method":"saveconfig","params":[[{"Name":"DirectUnpack","Value":"yes"},{"Name":"UnpackPauseQueue","Value":"no"}]],"id":1}"#,
                true,
            );
            assert!(has_error_code(&reply, 3));
            assert!(reply.contains("DirectUnpack=yes"));
            assert!(reply.contains("UnpackPauseQueue=yes"));
        }
        // Omitted DirectWrite defaults to yes
        {
            let reply = rpc_reply(
                "/jsonrpc",
                HM_POST,
                r#"{"method":"saveconfig","params":[[{"Name":"FlushQueue","Value":"no"}]],"id":1}"#,
                true,
            );
            assert!(has_error_code(&reply, 3));
            assert!(reply.contains("DirectWrite=yes"));
        }

        // BootTimeConfigReconciliationTest
        let mut options = RustOptions::new_no_config(
            Some(&[
                c"Server1.Host=example.com",
                c"Server1.Port=119",
                c"Server1.Encryption=no",
                c"Server1.Cipher=AES128-GCM-SHA256",
                c"DupeSearch=yes",
                c"HealthCheck=park",
                c"DirectWrite=yes",
                c"FlushQueue=no",
                c"DirectUnpack=yes",
                c"UnpackPauseQueue=no",
            ]),
            None,
        );
        let cipher = options.get_option(c"Server1.Cipher");
        assert!(cipher.is_none_or(|c| c.to_bytes().is_empty()));
        assert_eq!(options.scalars.dupe_search, 0);
        assert_eq!(options.scalars.flush_queue, 1);
        assert_eq!(options.scalars.unpack_mutex_enforced, 1);
        assert_eq!(
            options
                .get_option(c"_UnpackMutexEnforced")
                .as_deref()
                .map(|c| c.to_bytes()),
            Some(&b"yes"[..])
        );
    }

    #[test]
    fn test_edit_queue_start_script_and_disk_speed_parsers() {
        // Large editqueue ID array after PrepareParams (verifying incremental linear parsing)
        let mut ids_str = String::with_capacity(70_000);
        for i in 1..=10_000 {
            if i > 1 {
                ids_str.push(',');
            }
            ids_str.push_str(&i.to_string());
        }
        let full_req = format!(
            r#"{{"method":"editqueue","params":["GroupMoveOffset",-3,"",[{ids_str}]],"id":1}}"#
        );
        let mut cmd = RustXmlCommand::new(full_req.as_bytes(), RP_JSON_RPC, HM_POST);
        cmd.prepare_params();
        let (_, action, args, ids) =
            xmlrpc_parse_edit_queue(&mut cmd.buf[cmd.pos..], false, true, 1, 11).unwrap();
        assert_eq!(action, 11);
        assert_eq!(args, b"-3");
        assert_eq!(ids.len(), 10_000);
        assert_eq!(ids[0], 1);
        assert_eq!(ids[9_999], 10_000);

        // TestDiskSpeed JSON-RPC POST
        let disk_req = br#"{"method":"testdiskspeed","params":["/tmp/test",1024,1,5],"id":1}"#;
        let mut dcmd = RustXmlCommand::new(disk_req, RP_JSON_RPC, HM_POST);
        dcmd.prepare_params();
        let (_, dir_path, buf_bytes, max_bytes, timeout_sec) =
            xmlrpc_parse_test_disk_speed(&mut dcmd.buf[dcmd.pos..], false, true).unwrap();
        assert_eq!(dir_path, b"/tmp/test");
        assert_eq!(buf_bytes, 1024 * 1024);
        assert_eq!(max_bytes, 1024 * 1024 * 1024);
        assert_eq!(timeout_sec, 5);

        // AppendFleet JSON-RPC POST
        let fleet_req = br#"{"method":"appendfleet","params":["dupe1","movies",50,60,"a.nzb","aGVsbG8=","b.nzb","https://example.com/b.nzb"],"id":1}"#;
        let mut fcmd = RustXmlCommand::new(fleet_req, RP_JSON_RPC, HM_POST);
        fcmd.prepare_params();
        let (_, fleet) =
            xmlrpc_parse_append_fleet(&mut fcmd.buf[fcmd.pos..], false, true, 16).unwrap();
        assert_eq!(fleet.dupe_key, b"dupe1");
        assert_eq!(fleet.category, b"movies");
        assert_eq!(fleet.priority, 50);
        assert_eq!(fleet.timeout_sec, 60);
        assert_eq!(
            fleet.members,
            vec![
                (b"a.nzb".to_vec(), false, b"hello".to_vec()),
                (
                    b"b.nzb".to_vec(),
                    true,
                    b"https://example.com/b.nzb".to_vec()
                ),
            ]
        );

        // Download v13 JSON-RPC POST
        let dl_req = br#"{"method":"append","params":["test.nzb","aGVsbG8=","tv",10,true,false,"k1",100,"FORCE",true,"*Unpack:Password","p\"w"],"id":1}"#;
        let mut dlcmd = RustXmlCommand::new(dl_req, RP_JSON_RPC, HM_POST);
        dlcmd.prepare_params();
        let (_, dl) =
            xmlrpc_parse_download(&mut dlcmd.buf[dlcmd.pos..], false, true, 0, 1, 2).unwrap();
        assert!(dl.v13);
        assert_eq!(dl.filename, b"test.nzb");
        assert_eq!(dl.category, b"tv");
        assert_eq!(dl.priority, 10);
        assert!(dl.add_top);
        assert!(!dl.is_url);
        assert_eq!(dl.content, b"hello");
        assert!(!dl.add_paused);
        assert_eq!(dl.dupe_key.as_deref(), Some(&b"k1"[..]));
        assert_eq!(dl.dupe_score, 100);
        assert_eq!(dl.dupe_mode, 2);
        assert!(dl.auto_category);
        assert_eq!(
            dl.params,
            vec![(b"*Unpack:Password".to_vec(), b"p\"w".to_vec())]
        );

        // ViewFeed & PreviewFeed v16 JSON-RPC POST
        let vf_req = br#"{"method":"previewfeed","params":[2,"Feed2","https://feed.invalid","A*",false,true,"movies",25,15,"Filter.py",true,300,"cid1","Auto"],"id":1}"#;
        let mut vfcmd = RustXmlCommand::new(vf_req, RP_JSON_RPC, HM_POST);
        vfcmd.prepare_params();
        let (_, vf) =
            xmlrpc_parse_view_feed(&mut vfcmd.buf[vfcmd.pos..], false, true, true, 0, 1, 2)
                .unwrap();
        assert_eq!(vf.id, 2);
        assert_eq!(vf.name, b"Feed2");
        assert_eq!(vf.url, b"https://feed.invalid");
        assert_eq!(vf.filter, b"A*");
        assert!(!vf.backlog);
        assert!(vf.pause_nzb);
        assert_eq!(vf.category, b"movies");
        assert_eq!(vf.category_source, 2);
        assert_eq!(vf.priority, 25);
        assert_eq!(vf.interval, 15);
        assert_eq!(vf.feed_filter.as_deref(), Some(&b"Filter.py"[..]));
        assert!(vf.include_non_matching);
        assert_eq!(vf.cache_time_sec, 300);
        assert_eq!(vf.cache_id, b"cid1");

        // TestServer JSON-RPC POST + certVerifLevel validation
        let ts_req = br#"{"method":"testserver","params":["news.example.com",563,"u","p",true,"AES128",30,1],"id":1}"#;
        let (_, ts) = xmlrpc_parse_test_server(
            Some(ts_req),
            None,
            false,
            true,
            HM_POST,
            HM_POST,
            3,
            crate::json::TEST_LIMITS,
        )
        .unwrap();
        assert_eq!(ts.host, b"news.example.com");
        assert_eq!(ts.port, 563);
        assert!(ts.encryption);
        assert_eq!(ts.cert_verif_level, 1);
    }

    #[test]
    fn serve_xmlrpc_end_to_end_jsonrpc_multicall_jsonp_and_zip_append() {
        unsafe extern "C" fn test_action_cb(
            ctx: *mut c_void,
            op: c_int,
            a1: c_int,
            a2: c_int,
            _a3: c_int,
            _a4: c_int,
            _s1: *const c_char,
            _s2: *const c_char,
            _s3: *const c_char,
            _s4: *const c_char,
            body: *const c_char,
            body_len: usize,
            _ids: *const i32,
            _ids_len: usize,
            _params: *const NzbgetRsRpcParamItem,
            _params_len: usize,
            out_u32: *mut u32,
            _out_f64: *mut f64,
            _out_buf: *mut RsBuf,
        ) -> c_int {
            let state = unsafe { &mut *(ctx as *mut (i32, Vec<u8>, bool)) };
            match op {
                1 => {
                    if a1 == 0 {
                        let prev = c_int::from(state.2);
                        state.2 = true;
                        prev
                    } else {
                        state.2 = a2 != 0;
                        0
                    }
                }
                7 => {
                    state.0 = a1;
                    1
                }
                11 => {
                    if !body.is_null() && body_len > 0 {
                        state.1 =
                            unsafe { std::slice::from_raw_parts(body.cast::<u8>(), body_len) }
                                .to_vec();
                    }
                    if !out_u32.is_null() {
                        unsafe {
                            *out_u32 = 77;
                        }
                    }
                    1
                }
                _ => 1,
            }
        }

        let mut host_state: (i32, Vec<u8>, bool) = (0, Vec::new(), false);
        let ctx_ptr = (&mut host_state as *mut (i32, Vec<u8>, bool)).cast::<c_void>();

        // 1. JSON-RPC POST version + malformed zero-len id suffix echo parity
        let mut req1 = br#"{"method":"version","params":[],"id":}"#.to_vec();
        req1.push(0);
        let (resp1, ct1, safe1) = unsafe {
            serve_xmlrpc(
                ctx_ptr,
                &mut req1,
                crate::rpcroute::HM_POST,
                crate::rpcroute::UA_CONTROL,
                false,
                false,
                b"/jsonrpc",
                b"26.0-testing",
                None,
                None,
                Some(test_action_cb),
                None,
            )
        };
        assert!(safe1);
        assert_eq!(ct1, Some(&b"application/json"[..]));
        assert_eq!(
            std::str::from_utf8(&resp1).unwrap(),
            "{\n\"version\" : \"1.1\",\n\"id\" : },\n\"result\" : \"26.0-testing\"\n}"
        );

        // 2. XML-RPC system.multicall with uaAdd (version allowed, rate denied)
        let mut mc = b"<?xml version=\"1.0\"?><methodCall><methodName>system.multicall</methodName>\
            <params><param><value><array><data>\
            <value><struct><member><name>methodName</name><value><string>version</string></value></member>\
            <member><name>params</name><value><array><data></data></array></value></member></struct></value>\
            <value><struct><member><name>methodName</name><value><string>rate</string></value></member>\
            <member><name>params</name><value><array><data><value><i4>50</i4></value></data></array></value></member></struct></value>\
            </data></array></value></param></params></methodCall>\0"
            .to_vec();
        let (resp_mc, ct_mc, safe_mc) = unsafe {
            serve_xmlrpc(
                ctx_ptr,
                &mut mc,
                crate::rpcroute::HM_POST,
                RPC_UA_ADD,
                false,
                false,
                b"/xmlrpc",
                b"26.0",
                None,
                None,
                Some(test_action_cb),
                None,
            )
        };
        assert!(safe_mc);
        assert_eq!(ct_mc, Some(&b"text/xml"[..]));
        let mc_str = std::str::from_utf8(&resp_mc).unwrap();
        assert!(mc_str.contains("<string>26.0</string>"), "{mc_str}");
        assert!(
            mc_str.contains("<string>Access denied</string>"),
            "{mc_str}"
        );
        assert_eq!(host_state.0, 0);

        // 3. JSON-P GET disabled refusal vs enabled callback wrapping
        let mut empty = vec![0u8];
        let (resp_jp_off, _, _) = unsafe {
            serve_xmlrpc(
                ctx_ptr,
                &mut empty,
                RPC_HM_GET,
                crate::rpcroute::UA_CONTROL,
                false,
                false,
                b"/jsonprpc/version?jsonp=myCb",
                b"26.0",
                None,
                None,
                Some(test_action_cb),
                None,
            )
        };
        let jp_off_str = std::str::from_utf8(&resp_jp_off).unwrap();
        assert!(jp_off_str.starts_with("myCb("), "{jp_off_str}");
        assert!(jp_off_str.contains("JSON-P is turned off"), "{jp_off_str}");

        let (resp_jp_on, _, _) = unsafe {
            serve_xmlrpc(
                ctx_ptr,
                &mut empty,
                RPC_HM_GET,
                crate::rpcroute::UA_CONTROL,
                true,
                false,
                b"/jsonprpc/version?jsonp=myCb",
                b"26.0",
                None,
                None,
                Some(test_action_cb),
                None,
            )
        };
        let jp_on_str = std::str::from_utf8(&resp_jp_on).unwrap();
        assert!(jp_on_str.starts_with("myCb("), "{jp_on_str}");
        assert!(jp_on_str.contains("\"result\" : \"26.0\""), "{jp_on_str}");

        // 4. append binary payload with embedded NUL in .zip vs .nzb v13 int response
        let mut zip_req =
            br#"{"method":"append","params":["bundle.zip","UEsDBAAh","tv",0,false,false,"",0,"SCORE",false],"id":9}"#
                .to_vec();
        zip_req.push(0);
        let (resp_zip, _, safe_zip) = unsafe {
            serve_xmlrpc(
                ctx_ptr,
                &mut zip_req,
                crate::rpcroute::HM_POST,
                RPC_UA_ADD,
                false,
                false,
                b"/jsonrpc",
                b"26.0",
                None,
                None,
                Some(test_action_cb),
                None,
            )
        };
        assert!(!safe_zip);
        assert_eq!(host_state.1, b"PK\x03\x04\x00!");
        assert!(std::str::from_utf8(&resp_zip)
            .unwrap()
            .contains("\"result\" : true"));

        // 5. SpeedTestPauseGuard reference counting preserves prior pause state
        host_state.2 = false;
        let g1 = unsafe { SpeedTestPauseGuard::new(ctx_ptr, Some(test_action_cb)) };
        assert!(host_state.2);
        let g2 = unsafe { SpeedTestPauseGuard::new(ctx_ptr, Some(test_action_cb)) };
        drop(g1);
        assert!(host_state.2);
        drop(g2);
        assert!(!host_state.2);
    }
}
