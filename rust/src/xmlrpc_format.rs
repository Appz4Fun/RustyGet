//! XML-RPC and JSON-RPC response serialization and command routing (`daemon/remote/XmlRpc.cpp`).

use std::ffi::{c_char, c_int, CStr};
use std::io::Write;

use crate::ffi::{input, into_buf, RsBuf};

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
const PAR_STATUS_NAMES: [&str; 6] = ["NONE", "NONE", "FAILURE", "SUCCESS", "REPAIR_POSSIBLE", "MANUAL"];
const UNPACK_STATUS_NAMES: [&str; 6] = ["NONE", "NONE", "FAILURE", "SUCCESS", "SPACE", "PASSWORD"];
const MOVE_STATUS_NAMES: [&str; 3] = ["NONE", "FAILURE", "SUCCESS"];
const SCRIPT_STATUS_NAMES: [&str; 3] = ["NONE", "FAILURE", "SUCCESS"];
const DELETE_STATUS_NAMES: [&str; 8] = ["NONE", "MANUAL", "HEALTH", "DUPE", "BAD", "GOOD", "COPY", "SCAN"];
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
const DUP_STATUS_NAMES: [&str; 7] = ["UNKNOWN", "SUCCESS", "FAILURE", "DELETED", "DUPE", "BAD", "GOOD"];
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
        b"loadextensions" => 31,
        b"downloadextension" => 32,
        b"updateextension" => 33,
        b"deleteextension" => 34,
        b"testextension" => 35,
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
        b"startscript" => 55,
        b"logscript" => 56,
        _ => 0,
    }
}

/// # Safety
/// `method_name` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_match_command(method_name: *const c_char) -> c_int {
    match_command(unsafe { input(method_name) })
}

/// # Safety
/// `err_text` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_error(
    is_json: c_int,
    err_code: c_int,
    err_text: *const c_char,
) -> RsBuf {
    let json = is_json != 0;
    let encoded = encode_str(json, unsafe { input(err_text) });
    let mut out = Vec::with_capacity(256);
    if json {
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
    into_buf(out)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_xmlrpc_format_bool(is_json: c_int, ok: c_int) -> RsBuf {
    let json = is_json != 0;
    let b = bool_str(json, ok != 0);
    let mut out = Vec::with_capacity(32);
    if json {
        out.extend_from_slice(b.as_bytes());
    } else {
        let _ = write!(out, "<boolean>{}</boolean>", b);
    }
    into_buf(out)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_xmlrpc_format_int(is_json: c_int, value: c_int) -> RsBuf {
    let mut out = Vec::with_capacity(32);
    if is_json != 0 {
        let _ = write!(out, "{}", value);
    } else {
        let _ = write!(out, "<i4>{}</i4>", value);
    }
    into_buf(out)
}

/// # Safety
/// `raw_str` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_string(
    is_json: c_int,
    raw_str: *const c_char,
    encode: c_int,
    max_len: usize,
) -> RsBuf {
    let json = is_json != 0;
    let raw = unsafe { input(raw_str) };
    let mut out = Vec::with_capacity(raw.len() + 32);
    if json {
        out.push(b'"');
        if encode != 0 {
            crate::escape::json_encode(raw, &mut out);
        } else {
            out.extend_from_slice(raw);
        }
        out.push(b'"');
    } else {
        out.extend_from_slice(b"<string>");
        if encode != 0 {
            crate::escape::xml_encode(raw, &mut out);
        } else {
            out.extend_from_slice(raw);
        }
        out.extend_from_slice(b"</string>");
    }
    if max_len > 0 && out.len() > max_len {
        out.truncate(max_len);
    }
    into_buf(out)
}

/// # Safety
/// `inp` must be a valid pointer to `NzbgetRsRpcStatusInput`, and `servers` must be valid for `servers_len` items.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_status(
    is_json: c_int,
    inp: *const NzbgetRsRpcStatusInput,
    servers: *const NzbgetRsRpcServerStatus,
    servers_len: usize,
) -> RsBuf {
    if inp.is_null() {
        return into_buf(Vec::new());
    }
    let s = unsafe { &*inp };
    let json = is_json != 0;

    let (rem_hi, rem_lo) = split_i64(s.remaining_size);
    let (forced_hi, forced_lo) = split_i64(s.forced_size);
    let (cache_hi, cache_lo) = split_i64(s.article_cache);
    let (dl_rate_hi, dl_rate_lo) = split_i64(s.download_rate);
    let (dl_size_hi, dl_size_lo) = split_i64(s.all_bytes);

    let avg_dl_rate =
        xmlrpc_calc_average_download_rate(s.download_time_sec, s.all_bytes);
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

    let mem_pressure = String::from_utf8_lossy(unsafe { input(s.memory_pressure) });
    let dl_paused_str = bool_str(json, s.download_paused != 0);
    let stand_by_str = bool_str(json, s.server_stand_by != 0);
    let post_paused_str = bool_str(json, s.post_paused != 0);
    let scan_paused_str = bool_str(json, s.scan_paused != 0);
    let quota_str = bool_str(json, s.quota_reached != 0);
    let feed_active_str = bool_str(json, s.feed_active != 0);

    let dl_rate_i32 = safe_i64_to_i32(s.download_rate);
    let avg_rate_i32 = safe_i64_to_i32(avg_dl_rate);
    let thread_count_u32 = s.thread_count as u32;

    let mut out = Vec::with_capacity(4096);
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
            let _ = write!(out, "{{\n\"ID\" : {},\n\"Active\" : {}\n}}", srv.id, active_str);
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

    into_buf(out)
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

/// # Safety
/// `entries` must be valid for `len` items.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_log_list(
    is_json: c_int,
    entries: *const NzbgetRsRpcLogEntry,
    len: usize,
) -> RsBuf {
    let json = is_json != 0;
    let slice = if entries.is_null() || len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(entries, len) }
    };
    let mut out = Vec::with_capacity(slice.len() * 128 + 32);
    if json {
        out.extend_from_slice(b"[\n");
    } else {
        out.extend_from_slice(b"<array><data>\n");
    }
    unsafe {
        write_log_entries(json, slice, &mut out);
    }
    if json {
        out.extend_from_slice(b"\n]");
    } else {
        out.extend_from_slice(b"</data></array>\n");
    }
    into_buf(out)
}

/// # Safety
/// `item` must be a valid pointer to `NzbgetRsRpcFileItem`.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_file_item(
    is_json: c_int,
    index: c_int,
    item: *const NzbgetRsRpcFileItem,
) -> RsBuf {
    if item.is_null() {
        return into_buf(Vec::new());
    }
    let f = unsafe { &*item };
    let json = is_json != 0;
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

    let mut out = Vec::with_capacity(512);
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
    into_buf(out)
}

/// # Safety
/// `info` must be valid, and `params`, `scripts`, `stats` must be valid for their respective lengths.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_nzb_info_fields(
    is_json: c_int,
    info: *const NzbgetRsRpcNzbInfoInput,
    params: *const NzbgetRsRpcParamItem,
    params_len: usize,
    scripts: *const NzbgetRsRpcScriptItem,
    scripts_len: usize,
    stats: *const NzbgetRsRpcServerStatItem,
    stats_len: usize,
) -> RsBuf {
    if info.is_null() {
        return into_buf(Vec::new());
    }
    let n = unsafe { &*info };
    let json = is_json != 0;

    let (file_hi, file_lo) = split_i64(n.size);
    let file_mb = (n.size / 1024 / 1024) as c_int;
    let (dl_hi, dl_lo) = split_i64(n.downloaded_size);
    let dl_mb = (n.downloaded_size / 1024 / 1024) as c_int;
    let (dupe_hi, dupe_lo) = split_i64(n.dupe_recovered_bytes);

    let (message_count, post_total_time_sec) =
        xmlrpc_nzb_message_and_post_time(
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

    let mut out = Vec::with_capacity(2048);
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
            let _ = write!(out, "{{\n\"Name\" : \"{}\",\n\"Value\" : \"{}\"\n}}", p_name, p_val);
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
        let sc_status = encode_str(json, table_get(&SCRIPT_STATUS_NAMES, sc.status, "NONE").as_bytes());
        if json {
            if idx > 0 {
                out.extend_from_slice(b",\n");
            }
            let _ = write!(out, "{{\n\"Name\" : \"{}\",\n\"Status\" : \"{}\"\n}}", sc_name, sc_status);
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

    into_buf(out)
}

/// # Safety
/// `progress_label` is null or NUL-terminated; `logs` is valid for `logs_len` items.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_post_info_fields(
    is_json: c_int,
    post_queue: c_int,
    has_post_info: c_int,
    progress_label: *const c_char,
    stage_progress: c_int,
    cur_time: i64,
    stage_time: i64,
    start_time: i64,
    logs: *const NzbgetRsRpcLogEntry,
    logs_len: usize,
) -> RsBuf {
    let json = is_json != 0;
    let is_post_queue = post_queue != 0;

    let (label, progress, stage_sec, total_sec) = if has_post_info != 0 {
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

    let mut out = Vec::with_capacity(512);
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
        write_log_entries(json, log_slice, &mut out);
    }

    if json {
        out.extend_from_slice(b"]\n");
    } else {
        out.extend_from_slice(b"</data></array></value></member>\n");
    }
    into_buf(out)
}

/// # Safety
/// `status` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_group_item_start(
    is_json: c_int,
    index: c_int,
    nzb_id: c_int,
    remaining_size: i64,
    paused_size: i64,
    remaining_file_count: c_int,
    remaining_par_count: c_int,
    priority: c_int,
    active_downloads: c_int,
    status: *const c_char,
) -> RsBuf {
    let json = is_json != 0;
    let (rem_hi, rem_lo) = split_i64(remaining_size);
    let (pau_hi, pau_lo) = split_i64(paused_size);
    let mut rem_mb = 0;
    let mut pau_mb = 0;
    unsafe {
        roc_xmlrpc_list_groups_calc_mb(
            remaining_size,
            paused_size,
            &mut rem_mb,
            &mut pau_mb,
        );
    }
    let status_str = String::from_utf8_lossy(unsafe { input(status) });

    let mut out = Vec::with_capacity(512);
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
    into_buf(out)
}

/// # Safety
/// `info_name` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_postqueue_item_start(
    is_json: c_int,
    index: c_int,
    nzb_id: c_int,
    info_name: *const c_char,
    stage: c_int,
    file_progress: c_int,
) -> RsBuf {
    let json = is_json != 0;
    let info_name_enc = encode_str(json, unsafe { input(info_name) });
    let stage_str = table_get(&POST_STAGE_NAMES, stage, "QUEUED");

    let mut out = Vec::with_capacity(256);
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
    into_buf(out)
}

/// # Safety
/// `name` and `status` are null or valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_history_nzb_start(
    is_json: c_int,
    index: c_int,
    id: c_int,
    name: *const c_char,
    remaining_file_count: c_int,
    retry_data: c_int,
    history_time: c_int,
    status: *const c_char,
) -> RsBuf {
    let json = is_json != 0;
    let name_enc = encode_str(json, unsafe { input(name) });
    let retry_str = bool_str(json, retry_data != 0);
    let status_str = String::from_utf8_lossy(unsafe { input(status) });

    let mut out = Vec::with_capacity(256);
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
            id, name_enc, remaining_file_count, retry_str, history_time, status_str
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
            id, name_enc, remaining_file_count, retry_str, history_time, status_str
        );
    }
    into_buf(out)
}

/// # Safety
/// `name`, `dupe_key`, and `status` are null or valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_history_dup_item(
    is_json: c_int,
    index: c_int,
    id: c_int,
    name: *const c_char,
    history_time: c_int,
    size: i64,
    dupe_key: *const c_char,
    dupe_score: c_int,
    dupe_mode: c_int,
    dup_status: c_int,
    status: *const c_char,
) -> RsBuf {
    let json = is_json != 0;
    let (size_hi, size_lo) = split_i64(size);
    let size_mb = (size / 1024 / 1024) as c_int;
    let name_enc = encode_str(json, unsafe { input(name) });
    let dupe_key_enc = encode_str(json, unsafe { input(dupe_key) });
    let dupe_mode_str = table_get(&DUPE_MODE_NAMES, dupe_mode, "SCORE");
    let dup_status_str = table_get(&DUP_STATUS_NAMES, dup_status, "UNKNOWN");
    let status_str = String::from_utf8_lossy(unsafe { input(status) });

    let mut out = Vec::with_capacity(512);
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
    into_buf(out)
}

/// # Safety
/// `filename`, `url`, `name`, and `category` are null or valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_urlqueue_item(
    is_json: c_int,
    index: c_int,
    id: c_int,
    filename: *const c_char,
    url: *const c_char,
    name: *const c_char,
    category: *const c_char,
    priority: c_int,
) -> RsBuf {
    let json = is_json != 0;
    let filename_enc = encode_str(json, unsafe { input(filename) });
    let url_enc = encode_str(json, unsafe { input(url) });
    let name_enc = encode_str(json, unsafe { input(name) });
    let category_enc = encode_str(json, unsafe { input(category) });

    let mut out = Vec::with_capacity(256);
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
    into_buf(out)
}

/// # Safety
/// `entries` must be valid for `len` items.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_config_list(
    is_json: c_int,
    user_access: c_int,
    ua_restricted: c_int,
    entries: *const NzbgetRsRpcConfigItem,
    len: usize,
) -> RsBuf {
    let json = is_json != 0;
    let slice = if entries.is_null() || len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(entries, len) }
    };
    let mut out = Vec::with_capacity(slice.len() * 96 + 32);
    if json {
        out.extend_from_slice(b"[\n");
    } else {
        out.extend_from_slice(b"<array><data>\n");
    }
    for (idx, entry) in slice.iter().enumerate() {
        let mask = roc_xmlrpc_config_should_mask_value(
            user_access,
            ua_restricted,
            entry.restricted,
        ) != 0;
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
            let _ = write!(out, "{{\n\"Name\" : \"{}\",\n\"Value\" : \"{}\"\n}}", name_enc, val_enc);
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
    into_buf(out)
}

/// # Safety
/// `templates` must be valid for `len` pointers to NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_config_templates(
    is_json: c_int,
    templates: *const *const c_char,
    len: usize,
) -> RsBuf {
    let json = is_json != 0;
    let slice = if templates.is_null() || len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(templates, len) }
    };
    let mut out = Vec::with_capacity(slice.len() * 256 + 32);
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
    into_buf(out)
}

/// # Safety
/// `item` must be a valid pointer to `NzbgetRsRpcFeedItem`.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_feed_item(
    is_json: c_int,
    index: c_int,
    item: *const NzbgetRsRpcFeedItem,
) -> RsBuf {
    if item.is_null() {
        return into_buf(Vec::new());
    }
    let f = unsafe { &*item };
    let json = is_json != 0;
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

    let mut out = Vec::with_capacity(512);
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
    into_buf(out)
}

/// # Safety
/// `item` must be a valid pointer to `NzbgetRsRpcServerVolumeItem`.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_server_volume_item(
    is_json: c_int,
    index: c_int,
    item: *const NzbgetRsRpcServerVolumeItem,
) -> RsBuf {
    if item.is_null() {
        return into_buf(Vec::new());
    }
    let v = unsafe { &*item };
    let json = is_json != 0;
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

    let mut out = Vec::with_capacity(4096);
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

    into_buf(out)
}

/// # Safety
/// `reason` is null or NUL-terminated; `members` is valid for `members_len` items.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_format_fleet_result(
    chosen: c_int,
    complete: c_int,
    reason: *const c_char,
    members: *const NzbgetRsRpcFleetMember,
    members_len: usize,
) -> RsBuf {
    let reason_enc = encode_str(true, unsafe { input(reason) });
    let complete_str = if complete != 0 { "true" } else { "false" };
    let mut head = Vec::with_capacity(256);
    let _ = write!(
        head,
        "{{\"Chosen\" : {}, \"Complete\" : {}, \"Reason\" : \"{}\", \"Members\" : [",
        chosen, complete_str, reason_enc
    );
    if head.len() > 1023 {
        head.truncate(1023);
    }

    let mut out = head;
    let slice = if members.is_null() || members_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(members, members_len) }
    };
    for (idx, m) in slice.iter().enumerate() {
        let name_enc = encode_str(true, unsafe { input(m.name) });
        let status_raw = String::from_utf8_lossy(unsafe { input(m.status) });
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
            status_raw,
            m.same_as,
            m.same_as_rank,
            m_reason_enc
        );
    }
    out.extend_from_slice(b"]}");
    into_buf(out)
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

#[no_mangle]
pub extern "C" fn nzbget_rs_xmlrpc_format_disk_speed(
    is_json: c_int,
    write_bytes: u64,
    write_ms: f64,
    read_bytes: u64,
    read_ms: f64,
) -> RsBuf {
    let size_mb = write_bytes / 1024 / 1024;
    let read_size_mb = read_bytes / 1024 / 1024;
    let mut out = Vec::with_capacity(256);
    if is_json != 0 {
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
    into_buf(out)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_xmlrpc_format_network_speed(
    is_json: c_int,
    speed_mbps: f64,
) -> RsBuf {
    let mut out = Vec::with_capacity(128);
    if is_json != 0 {
        let _ = write!(out, "{{\"SpeedMbps\":{}}}", format_json_f64(speed_mbps));
    } else {
        let _ = write!(
            out,
            "<value><struct><member><name>SpeedMbps</name><value><double>{:.6}</double></value></member></struct></value>",
            speed_mbps
        );
    }
    into_buf(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    unsafe fn buf_to_string(b: RsBuf) -> String {
        let s = String::from_utf8_lossy(std::slice::from_raw_parts(b.data as *const u8, b.len)).into_owned();
        crate::ffi::nzbget_rs_free(b);
        s
    }

    #[test]
    fn test_match_command_all_methods() {
        assert_eq!(match_command(b"pause"), 1);
        assert_eq!(match_command(b"PAUSEDOWnload2"), 1);
        assert_eq!(match_command(b"resume"), 2);
        assert_eq!(match_command(b"status"), 8);
        assert_eq!(match_command(b"appendfleet"), 16);
        assert_eq!(match_command(b"servervolumes"), 46);
        assert_eq!(match_command(b"logscript"), 56);
        assert_eq!(match_command(b"unknown"), 0);
        assert_eq!(match_command(b""), 0);
    }

    #[test]
    fn test_primitives_and_error_formatting() {
        unsafe {
            assert_eq!(buf_to_string(nzbget_rs_xmlrpc_format_bool(1, 1)), "true");
            assert_eq!(buf_to_string(nzbget_rs_xmlrpc_format_bool(0, 0)), "<boolean>0</boolean>");
            assert_eq!(buf_to_string(nzbget_rs_xmlrpc_format_int(1, -42)), "-42");
            assert_eq!(buf_to_string(nzbget_rs_xmlrpc_format_int(0, 42)), "<i4>42</i4>");

            let msg = CString::new("Bad <param> \"x\"").unwrap();
            let err_json = buf_to_string(nzbget_rs_xmlrpc_format_error(1, 2, msg.as_ptr()));
            assert!(err_json.contains("\"code\" : 2"));
            assert!(err_json.contains("\\\"x\\\""));

            let err_xml = buf_to_string(nzbget_rs_xmlrpc_format_error(0, 2, msg.as_ptr()));
            assert!(err_xml.contains("<i4>2</i4>"));
            assert!(err_xml.contains("&lt;param&gt;"));
        }
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
            let json = buf_to_string(nzbget_rs_xmlrpc_format_log_list(1, &entry, 1));
            assert!(json.contains("\"Kind\" : \"WARNING\""));
            assert!(json.contains("\"Text\" : \"Hello & world\""));

            let xml = buf_to_string(nzbget_rs_xmlrpc_format_log_list(0, &entry, 1));
            assert!(xml.contains("<string>Hello &amp; world</string>"));

            let k = CString::new("Server1.Password").unwrap();
            let v = CString::new("secret").unwrap();
            let cfg = NzbgetRsRpcConfigItem {
                name: k.as_ptr(),
                value: v.as_ptr(),
                restricted: 1,
            };
            let masked = buf_to_string(nzbget_rs_xmlrpc_format_config_list(1, 1, 1, &cfg, 1));
            assert!(masked.contains("\"Value\" : \"***\""));
            let unmasked = buf_to_string(nzbget_rs_xmlrpc_format_config_list(1, 0, 1, &cfg, 1));
            assert!(unmasked.contains("\"Value\" : \"secret\""));

            let name = CString::new("Release").unwrap();
            let key = CString::new("series=1").unwrap();
            let status = CString::new("DUPE").unwrap();
            let dup_json = buf_to_string(nzbget_rs_xmlrpc_format_history_dup_item(
                1,
                0,
                12,
                name.as_ptr(),
                1700000000,
                1048576,
                key.as_ptr(),
                100,
                0,
                4,
                status.as_ptr(),
            ));
            assert!(dup_json.ends_with("\"Status\" : \"DUPE\"\n"));
            let dup_xml = buf_to_string(nzbget_rs_xmlrpc_format_history_dup_item(
                0,
                0,
                12,
                name.as_ptr(),
                1700000000,
                1048576,
                key.as_ptr(),
                100,
                0,
                4,
                status.as_ptr(),
            ));
            assert!(dup_xml.ends_with("<member><name>Status</name><value><string>DUPE</string></value></member>\n"));

            let d_json = buf_to_string(nzbget_rs_xmlrpc_format_disk_speed(
                1,
                100 * 1024 * 1024,
                250.5,
                200 * 1024 * 1024,
                125.0,
            ));
            assert_eq!(
                d_json,
                "{\"SizeMB\":100,\"DurationMS\":250.5,\"ReadSizeMB\":200,\"ReadDurationMS\":125.0}"
            );
            let n_xml = buf_to_string(nzbget_rs_xmlrpc_format_network_speed(0, 940.25));
            assert_eq!(
                n_xml,
                "<value><struct><member><name>SpeedMbps</name><value><double>940.250000</double></value></member></struct></value>"
            );
        }
    }
}

extern "C" {
    fn strcasecmp(s1: *const c_char, s2: *const c_char) -> c_int;
}

/// Computes `MiB/s` from `bytes` and `ms` in `XmlRpc.cpp`.
pub fn xmlrpc_mib_per_sec(bytes: u64, ms: f64) -> f64 {
    if ms > 0.0 {
        (bytes as f64 / 1024.0 / 1024.0) / (ms / 1000.0)
    } else {
        0.0
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

/// Classifies `counter` in `ResetServerVolumeXmlCommand`.
pub fn xmlrpc_validate_reset_counter(counter: &[u8]) -> i32 {
    if counter.is_empty() {
        1
    } else if counter == b"CUSTOM" {
        2
    } else {
        0
    }
}

/// `SetDownloadRateXmlCommand::Execute`: validates that `rate` (in KiB/s) is
/// in `0..=i32::MAX / 1024` and computes `rate * 1024` (in B/s).
pub fn xmlrpc_validate_download_rate(rate: i32) -> (bool, i32) {
    if rate < 0 || rate > i32::MAX / 1024 {
        (false, 0)
    } else {
        (true, rate * 1024)
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

/// `NzbInfoXmlCommand::AppendPostInfoFields`: computes `(should_iter, start, clamped_log_entries)`.
pub fn xmlrpc_post_info_log_slice(log_entries: i32, messages_size: i32) -> (bool, i32, i32) {
    if log_entries > 0 && messages_size > 0 {
        let clamped = log_entries.min(messages_size);
        let start = messages_size - clamped;
        (true, start, clamped)
    } else {
        (false, 0, log_entries)
    }
}

/// `SaveConfigXmlCommand::Execute`: validates JSON option entry keys `(key1, key2)`.
pub unsafe fn xmlrpc_save_config_check_json_keys(
    key1: *const c_char,
    key2: *const c_char,
) -> i32 {
    if key1.is_null() || key2.is_null() {
        return 0;
    }
    let name_lit = b"Name\0".as_ptr().cast::<c_char>();
    let value_lit = b"Value\0".as_ptr().cast::<c_char>();
    if strcasecmp(key1, value_lit) == 0 && strcasecmp(key2, name_lit) == 0 {
        2
    } else if strcasecmp(key1, name_lit) == 0 && strcasecmp(key2, value_lit) == 0 {
        1
    } else {
        0
    }
}

/// `SaveConfigXmlCommand::Execute`: checks that the raw `_TlsCipherBaseline` value is valid.
pub fn xmlrpc_save_config_raw_baseline_valid(raw_value: &[u8]) -> bool {
    !raw_value.windows(6).any(|w| w == b"\\u0000")
        && !raw_value.windows(2).any(|w| w == b"&#")
}

/// `SaveConfigXmlCommand::Execute`: checks that the decoded `_TlsCipherBaseline` value is valid.
pub fn xmlrpc_save_config_decoded_baseline_valid(
    has_cipher_baseline: bool,
    decoded_value: &[u8],
) -> bool {
    !has_cipher_baseline
        && decoded_value.len() <= 1024 * 1024
        && decoded_value.starts_with(b"v1\n")
}

/// `ViewFeedXmlCommand::Execute`: checks whether a feed item should be included and computes `sizeMB`.
pub fn xmlrpc_view_feed_should_include(
    include_non_matching: bool,
    match_status: i32,
    ms_accepted: i32,
    size_bytes: i64,
) -> (bool, i32) {
    let include = include_non_matching || match_status == ms_accepted;
    let size_mb = if include {
        (size_bytes / 1024 / 1024) as i32
    } else {
        0
    };
    (include, size_mb)
}

/// `TestServerXmlCommand::Execute`: resolves effective connection timeout.
pub fn xmlrpc_test_server_effective_timeout(param_timeout: i32, article_timeout: i32) -> i32 {
    if param_timeout == 0 {
        article_timeout
    } else {
        param_timeout
    }
}

/// `TestServerXmlCommand::Execute`: checks whether the NNTP `ARTICLE` response starts with `'4'` or `'2'`.
pub fn xmlrpc_test_server_response_ok(has_response: bool, first_char: u8) -> bool {
    has_response && (first_char == b'4' || first_char == b'2')
}

/// Scans a NUL-terminated C string linearly until `"</struct>"` is matched or NUL is reached.
pub unsafe fn xmlrpc_multicall_find_struct_end_cstr(
    request_ptr: *const c_char,
) -> Option<usize> {
    if request_ptr.is_null() {
        return None;
    }
    const NEEDLE: &[u8; 9] = b"</struct>";
    let base = request_ptr.cast::<u8>();
    let mut i = 0usize;
    loop {
        let mut matched = 0usize;
        while matched < NEEDLE.len() {
            let b = *base.add(i + matched);
            if b == 0 {
                return None;
            }
            if b != NEEDLE[matched] {
                break;
            }
            matched += 1;
        }
        if matched == NEEDLE.len() {
            return Some(i);
        }
        if *base.add(i) == 0 {
            return None;
        }
        i += 1;
    }
}

/// Locates `"</name>"` and optional `"params</name>"` within a `<struct>` slice.
pub fn xmlrpc_multicall_locate_parts(call_slice: &[u8]) -> Option<(usize, usize)> {
    let name_off = call_slice.windows(7).position(|w| w == b"</name>")?;
    let params_off = call_slice[name_off..]
        .windows(13)
        .position(|w| w == b"params</name>")
        .map(|rel| name_off + rel)
        .unwrap_or(0);
    Some((name_off, params_off))
}

/// Returns 1 when an XML-RPC multicall sub-response is neither a `<fault>` nor an `<array>`.
pub unsafe fn roc_xmlrpc_multicall_needs_array_wrap(resp: *const c_char) -> c_int {
    if resp.is_null() {
        return 1;
    }
    let bytes = unsafe { CStr::from_ptr(resp).to_bytes() };
    let fault = bytes.starts_with(b"<fault>");
    let array = !fault && bytes.starts_with(b"<array>");
    if !fault && !array {
        1
    } else {
        0
    }
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

/// Validates `LogXmlCommand` parameters (`!(nr_entries > 0 && id_from > 0)`).
pub fn roc_xmlrpc_log_params_valid(id_from: c_int, nr_entries: c_int) -> c_int {
    if nr_entries > 0 && id_from > 0 {
        0
    } else {
        1
    }
}

/// Validates `ListFilesXmlCommand` range and `nzbId` parameters.
pub fn roc_xmlrpc_list_files_params_valid(
    has_id_start: c_int,
    has_id_end: c_int,
    id_start: c_int,
    id_end: c_int,
    nzb_id: c_int,
) -> c_int {
    if has_id_start != 0 && (has_id_end == 0 || id_end < id_start) {
        return 0;
    }
    if nzb_id > 0 && (id_start != 0 || id_end != 0) {
        return 0;
    }
    1
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

/// Returns 1 when a `HistoryInfo` entry of kind `hkDup` should be skipped because `dup` is false.
pub fn roc_xmlrpc_history_should_skip_dup(
    kind: c_int,
    hk_dup: c_int,
    include_dup: c_int,
) -> c_int {
    if kind == hk_dup && include_dup == 0 {
        1
    } else {
        0
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

/// Validates target server availability for `TestServerSpeedXmlCommand::Execute`.
pub fn roc_xmlrpc_test_server_speed_check_server(
    has_server: c_int,
    is_active: c_int,
    max_connections: c_int,
) -> c_int {
    if has_server == 0 {
        1
    } else if is_active == 0 {
        2
    } else if max_connections == 0 {
        3
    } else {
        0
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

/// Checks whether a request URL targets one of the XML-RPC / JSON-RPC / JSONP-RPC endpoints.
pub fn rpc_is_rpc_request(url: &[u8]) -> bool {
    url == b"/xmlrpc"
        || url.starts_with(b"/xmlrpc/")
        || url == b"/jsonrpc"
        || url.starts_with(b"/jsonrpc/")
        || url == b"/jsonprpc"
        || url.starts_with(b"/jsonprpc/")
}

/// Computes `(start_index, effective_nr_entries)` for `LogXmlCommand::Execute`.
pub fn rpc_log_slice_range(
    messages_len: i32,
    front_id: i32,
    id_from: i32,
    nr_entries: i32,
) -> (i32, i32) {
    let mut start = messages_len;
    let mut eff_entries = nr_entries;
    if eff_entries > 0 {
        if eff_entries > messages_len {
            eff_entries = messages_len;
        }
        start = messages_len - eff_entries;
    }
    if id_from > 0 && messages_len > 0 {
        eff_entries = messages_len;
        start = id_from - front_id;
        if start < 0 {
            start = 0;
        }
    }
    (start, eff_entries)
}

/// Evaluates whether a `FileInfo` matches the `ListFilesXmlCommand` filter and computes its permille progress.
#[allow(clippy::too_many_arguments)]
pub fn rpc_list_files_match_and_progress(
    nzb_id: i32,
    file_nzb_id: i32,
    file_id: i32,
    id_start: i32,
    id_end: i32,
    size: i64,
    remaining_size: i64,
    missed_size: i64,
    failed_size: i64,
    success_size: i64,
) -> Option<i32> {
    let matches = (nzb_id > 0 && nzb_id == file_nzb_id)
        || (nzb_id == 0 && (id_start == 0 || (id_start <= file_id && file_id <= id_end)));
    if !matches {
        return None;
    }
    let progress = if failed_size == 0 && success_size == 0 {
        0
    } else {
        let denom = size - missed_size;
        if denom <= 0 {
            0
        } else {
            (1000 - (remaining_size * 1000) / denom) as i32
        }
    };
    Some(progress)
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

/// Parses the log message kind string in `WriteLogXmlCommand::Execute`.
pub fn rpc_parse_write_log_kind(kind: &[u8]) -> i32 {
    match kind {
        b"INFO" => 0,
        b"WARNING" => 1,
        b"ERROR" => 2,
        b"DEBUG" => 3,
        b"DETAIL" => 4,
        _ => -1,
    }
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

/// Parses the branch name in `StartUpdateXmlCommand::Execute`.
pub fn rpc_parse_update_branch(branch_name: &[u8]) -> i32 {
    if branch_name.eq_ignore_ascii_case(b"stable") {
        0
    } else if branch_name.eq_ignore_ascii_case(b"testing") {
        1
    } else if branch_name.eq_ignore_ascii_case(b"devel") {
        2
    } else {
        -1
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
        let health_check_park = health_check_val.is_some_and(|v| {
            v.eq_ignore_ascii_case(b"park") || v.eq_ignore_ascii_case(b"hcpark")
        });
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
    let s_idx = std::str::from_utf8(&digits[..d_len]).ok()?.parse::<i32>().ok()?;
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

