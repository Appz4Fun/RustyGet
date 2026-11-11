//! `Scanner` (`daemon/queue/Scanner.cpp`) end-to-end Rust implementation.

#[inline]
fn eq_ignore_ascii_case(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// `Scanner::ServiceWork` pass state machine (`NzbDirFileAge < NzbDirInterval`).
/// Returns `(next_interval, next_pass)`.
pub fn next_pass(
    nzb_dir_interval: i32,
    nzb_dir_file_age: i32,
    has_scan_script: bool,
    current_pass: i32,
) -> (i32, i32) {
    let mut next_interval = nzb_dir_interval;
    let mut next_pass = current_pass;
    if nzb_dir_interval > 0 && nzb_dir_file_age < nzb_dir_interval {
        let max_pass = if has_scan_script { 3 } else { 1 };
        if current_pass < max_pass {
            next_interval = nzb_dir_file_age;
            next_pass = current_pass + 1;
        } else {
            next_pass = 0;
        }
    }
    (next_interval, next_pass)
}

/// `Scanner::CanProcessFile` extension filter.
pub fn can_process_extension(full_filename: &[u8]) -> bool {
    let dot_pos = match full_filename.iter().rposition(|&b| b == b'.') {
        Some(pos) => pos,
        None => return false,
    };
    let ext = &full_filename[dot_pos..];
    !eq_ignore_ascii_case(ext, b".queued")
        && !eq_ignore_ascii_case(ext, b".error")
        && !eq_ignore_ascii_case(ext, b".processed")
}

/// `Scanner::CanProcessFile` file size & age stabilization check.
/// Returns `(can_process, updated_size, updated_last_change)`.
pub fn eval_file_age(
    old_size: i64,
    new_size: i64,
    last_change: i64,
    current_time: i64,
    file_age: i32,
) -> (bool, i64, i64) {
    if old_size == new_size && current_time - last_change >= file_age as i64 {
        (true, old_size, last_change)
    } else if old_size != new_size {
        (false, new_size, current_time)
    } else {
        (false, old_size, last_change)
    }
}

/// `Scanner::DropOldFiles` stale entry predicate.
pub fn should_drop_scan_file(
    last_change: i64,
    current_time: i64,
    nzb_dir_interval: i32,
    nzb_dir_file_age: i32,
) -> bool {
    let threshold = ((nzb_dir_interval as i64) + (nzb_dir_file_age as i64)) * 2;
    (current_time - last_change >= threshold) || current_time < last_change
}

/// `Scanner::ShouldSkipItem`:
/// Skips dot-files/directories, `processed_dir`, and `broken_dir`.
pub fn should_skip_item(filename: &[u8], processed_dir: &[u8], broken_dir: &[u8]) -> bool {
    if filename.is_empty() {
        return false;
    }
    filename.starts_with(b".") || filename == processed_dir || filename == broken_dir
}

/// `Scanner::ProcessIncomingFile` extension dispatch:
/// Returns `(rename_to_processed_after_script, dispatch_action)`:
/// - `1`: `.nzb_processed` (rename `.nzb` and `AddFileToQueue`)
/// - `2`: `.nzb` when `exists` (`AddFileToQueue`)
/// - `0`: no queue action
pub fn classify_incoming_extension(
    ran_scan_script: bool,
    exists: bool,
    extension: &[u8],
) -> (bool, i32) {
    let is_nzb = extension.eq_ignore_ascii_case(b".nzb");
    let is_nzb_processed = extension.eq_ignore_ascii_case(b".nzb_processed");
    let rename_to_processed = ran_scan_script && exists && !is_nzb;
    let action = if is_nzb_processed {
        1
    } else if exists && is_nzb {
        2
    } else {
        0
    };
    (rename_to_processed, action)
}

/// `Scanner::AddFileToQueue` missed-articles severity classifier.
/// Returns `0` (none), `1` (`Message::mkDetail`), `2` (`Message::mkWarning`).
pub fn classify_missed_articles(
    missed_articles: i32,
    total_articles: i32,
    is_cut_short: bool,
) -> i32 {
    if missed_articles <= 0 {
        return 0;
    }
    let large = missed_articles >= 100 || missed_articles.saturating_mul(10) >= total_articles;
    if is_cut_short || large {
        2
    } else {
        1
    }
}

/// `Scanner::AddArchive` status aggregation across extracted NZB files.
pub fn resolve_archive_add_status(
    has_url_info: bool,
    statuses: &[i32],
    as_success: i32,
    as_skipped: i32,
) -> i32 {
    if statuses.is_empty() {
        return as_skipped;
    }
    if has_url_info {
        return statuses[0];
    }
    if statuses.contains(&as_success) {
        as_success
    } else {
        statuses[0]
    }
}

/// `Scanner::AddExternalFile` XML/NZB header detector (matches `BString<1024>` truncation).
pub fn is_nzb_xml_prefix(buf: &[u8]) -> bool {
    let max_len = buf.len().min(1023);
    let slice = &buf[..max_len];
    let slice = match slice.iter().position(|&b| b == 0) {
        Some(nul) => &slice[..nul],
        None => slice,
    };
    slice.starts_with(b"<?xml") && slice.windows(4).any(|w| w == b"<nzb")
}

/// Checks whether an NZB name or filename is an absolute path (`/`, `\`, or `<drive>:\` / `<drive>:/`).
pub fn is_absolute_nzb_name(nzb_name: &[u8]) -> bool {
    if nzb_name.is_empty() {
        return false;
    }
    if nzb_name[0] == b'/' || nzb_name[0] == b'\\' {
        return true;
    }
    nzb_name.len() >= 3
        && nzb_name[0].is_ascii_alphabetic()
        && nzb_name[1] == b':'
        && (nzb_name[2] == b'\\' || nzb_name[2] == b'/')
}

/// `Scanner::AddExternalFile` filename normalizer (leading `.` -> `_`, UTF-8-aware 200-byte truncation, `.nzb` suffix).
pub fn normalize_external_nzb_name(nzb_name: &[u8], valid_name: &[u8], is_nzb: bool) -> Vec<u8> {
    let mut name = Vec::with_capacity(valid_name.len() + 6);
    if valid_name.first() == Some(&b'.') {
        name.push(b'_');
    }
    name.extend_from_slice(valid_name);

    if name.len() > 200 {
        let ext = match name.iter().rposition(|&b| b == b'.') {
            Some(dot) if name.len() - dot <= 16 => name[dot..].to_vec(),
            _ => Vec::new(),
        };
        let mut cut = 200usize.saturating_sub(ext.len());
        while cut > 0 && (name[cut] & 0xC0) == 0x80 {
            cut -= 1;
        }
        name.truncate(cut);
        name.extend_from_slice(&ext);
    }

    if is_nzb {
        let has_nzb_ext = match nzb_name.iter().rposition(|&b| b == b'.') {
            Some(dot) => eq_ignore_ascii_case(&nzb_name[dot..], b".nzb"),
            None => false,
        };
        if !has_nzb_ext {
            name.extend_from_slice(b".nzb");
        }
    }

    name
}

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROCESSED_DIR: &[u8] = b"_processed";
const BROKEN_DIR: &[u8] = b"_broken";
const UNPACK_DIR: &[u8] = b"_unpack";

pub const AS_SKIPPED: c_int = 0;
pub const AS_SUCCESS: c_int = 1;
pub const AS_FAILED: c_int = 2;

const LOG_DEBUG: c_int = 0;
const LOG_DETAIL: c_int = 1;
const LOG_INFO: c_int = 2;
const LOG_WARN: c_int = 3;
const LOG_ERROR: c_int = 4;

static SCANNER_ID_GEN: AtomicI32 = AtomicI32::new(0);
static SCANNER_UNPACK_SEQ: AtomicU64 = AtomicU64::new(1);

#[repr(C)]
pub struct NzbgetRsScannerEnv {
    pub ctx: *mut c_void,
    pub nzb_dir: *const c_char,
    pub temp_dir: *const c_char,
    pub filename: *const c_char,
    pub nzb_name: *const c_char,
    pub category: *const c_char,
    pub dupe_key: *const c_char,
    pub buf_ptr: *const c_char,
    pub buf_len: c_int,
    pub priority: c_int,
    pub dupe_score: c_int,
    pub dupe_mode: c_int,
    pub auto_category: c_int,
    pub add_top: c_int,
    pub add_paused: c_int,
    pub nzb_dir_interval: c_int,
    pub nzb_dir_file_age: c_int,
    pub archive_scan: c_int,
    pub archive_action: c_int,
    pub queue_loaded: c_int,
    pub pause_scan: c_int,
    pub path_sep: c_char,
    pub parameters: *mut c_void,
    pub url_info: *mut c_void,
    pub out_nzb_id: *mut c_int,
    pub wake_cb: Option<unsafe extern "C" fn(*mut c_void)>,
    pub log_cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>,
    pub find_category_cb:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> *const c_char>,
    pub iter_params_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char),
        ),
    >,
    pub work_params_cb:
        Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char, *const c_char)>,
    pub scan_script_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            *mut c_void,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
            *mut c_void,
            unsafe extern "C" fn(*mut c_void, c_int, *const c_char),
            *mut c_int,
            *mut c_int,
            *mut c_int,
            *mut c_int,
            *mut c_int,
        ),
    >,
    pub parse_nzb_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            *const c_char,
            *mut *const c_char,
        ) -> c_int,
    >,
    pub commit_nzb_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            *const c_char,
            *const c_char,
            c_int,
            *const c_char,
            c_int,
            *const c_char,
            c_int,
            c_int,
            c_int,
            c_int,
            *mut c_void,
        ) -> c_int,
    >,
}

struct ScanFileData {
    filename: Vec<u8>,
    size: i64,
    last_change: i64,
}

type ResultSlot = Arc<Mutex<(c_int, Option<c_int>)>>;

struct ScanQueueData {
    filename: Vec<u8>,
    nzb_name: Vec<u8>,
    category: Vec<u8>,
    dupe_key: Vec<u8>,
    priority: c_int,
    dupe_score: c_int,
    dupe_mode: c_int,
    auto_category: bool,
    add_top: bool,
    add_paused: bool,
    params: Vec<(CString, CString)>,
    url_info: usize,
    result_slot: Option<ResultSlot>,
}

struct ScannerInner {
    file_list: Vec<ScanFileData>,
    queue_list: Vec<ScanQueueData>,
    pass: i32,
    scan_script: bool,
}

pub struct NzbgetRsScanner {
    inner: Mutex<ScannerInner>,
    requested_nzb_dir_scan: AtomicBool,
    scanning: AtomicBool,
    nzb_dir_interval: AtomicI32,
}

fn cstr_bytes<'a>(ptr: *const c_char) -> &'a [u8] {
    if ptr.is_null() {
        b""
    } else {
        unsafe { CStr::from_ptr(ptr).to_bytes() }
    }
}

fn to_cstr_sanitized(bytes: &[u8]) -> CString {
    let mut v = Vec::with_capacity(bytes.len() + 1);
    for &b in bytes {
        v.push(if b == 0 { b'?' } else { b });
    }
    v.push(0);
    unsafe { CString::from_vec_with_nul_unchecked(v) }
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn emit_log(env: &NzbgetRsScannerEnv, kind: c_int, msg: &[u8]) {
    if let Some(cb) = env.log_cb {
        let c = to_cstr_sanitized(msg);
        unsafe { cb(env.ctx, kind, c.as_ptr()) };
    }
}

fn join_sep(dir: &[u8], sep: u8, leaf: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dir.len() + 1 + leaf.len());
    out.extend_from_slice(dir);
    out.push(sep);
    out.extend_from_slice(leaf);
    out
}

fn capture_params(env: &NzbgetRsScannerEnv) -> Vec<(CString, CString)> {
    unsafe extern "C" fn push_param(raw: *mut c_void, name: *const c_char, val: *const c_char) {
        let out = unsafe { &mut *raw.cast::<Vec<(CString, CString)>>() };
        out.push((
            to_cstr_sanitized(cstr_bytes(name)),
            to_cstr_sanitized(cstr_bytes(val)),
        ));
    }
    let mut list: Vec<(CString, CString)> = Vec::new();
    if !env.parameters.is_null() {
        if let Some(iter_cb) = env.iter_params_cb {
            unsafe {
                iter_cb(
                    env.parameters,
                    (&mut list as *mut Vec<(CString, CString)>).cast(),
                    push_param,
                )
            };
        }
    }
    list
}

fn resolve_category(env: &NzbgetRsScannerEnv, category: &[u8], filename: &[u8]) -> Vec<u8> {
    let cat_c = to_cstr_sanitized(category);
    if let Some(find_cb) = env.find_category_cb {
        let matched_ptr = unsafe { find_cb(env.ctx, cat_c.as_ptr(), 1) };
        if !matched_ptr.is_null() {
            let matched = cstr_bytes(matched_ptr);
            if matched != category {
                let mut msg = b"Category '".to_vec();
                msg.extend_from_slice(category);
                msg.extend_from_slice(b"' matched to '");
                msg.extend_from_slice(matched);
                msg.extend_from_slice(b"' for '");
                msg.extend_from_slice(filename);
                msg.push(b'\'');
                emit_log(env, LOG_DETAIL, &msg);
                return matched.to_vec();
            }
        }
    }
    category.to_vec()
}

fn cleanup_stale_unpack_dir(temp_dir: &[u8], env: &NzbgetRsScannerEnv) {
    let unpack_path = crate::fsops::path(temp_dir).join(crate::fsops::path(UNPACK_DIR));
    let meta = match std::fs::symlink_metadata(&unpack_path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            let code = e.raw_os_error().unwrap_or(1);
            let err_bytes = crate::fsops::error_message(code);
            let mut msg = b"Could not remove stale unpack directory '".to_vec();
            msg.extend_from_slice(temp_dir);
            msg.push(b'/');
            msg.extend_from_slice(UNPACK_DIR);
            msg.extend_from_slice(b"': ");
            msg.extend_from_slice(&err_bytes);
            msg.extend_from_slice(format!(" (code {code})").as_bytes());
            emit_log(env, LOG_WARN, &msg);
            return;
        }
    };
    let res = if meta.file_type().is_dir() {
        std::fs::remove_dir_all(&unpack_path)
    } else {
        std::fs::remove_file(&unpack_path)
    };
    if let Err(e) = res {
        if e.kind() != std::io::ErrorKind::NotFound {
            let code = e.raw_os_error().unwrap_or(1);
            let err_bytes = crate::fsops::error_message(code);
            let mut msg = b"Could not remove stale unpack directory '".to_vec();
            msg.extend_from_slice(temp_dir);
            msg.push(b'/');
            msg.extend_from_slice(UNPACK_DIR);
            msg.extend_from_slice(b"': ");
            msg.extend_from_slice(&err_bytes);
            msg.extend_from_slice(format!(" (code {code})").as_bytes());
            emit_log(env, LOG_WARN, &msg);
        }
    }
}

fn can_process_file(
    file_list: &mut Vec<ScanFileData>,
    full_filename: &[u8],
    check_stat: bool,
    file_age: i32,
) -> bool {
    if !can_process_extension(full_filename) {
        return false;
    }
    if !check_stat {
        return true;
    }
    let full_c = to_cstr_sanitized(full_filename);
    let size = crate::fsops::file_size(&full_c);
    let current = now_unix_secs();
    if let Some(idx) = file_list.iter().position(|f| f.filename == full_filename) {
        let (ready, upd_size, upd_change) = eval_file_age(
            file_list[idx].size,
            size,
            file_list[idx].last_change,
            current,
            file_age,
        );
        if ready {
            file_list.remove(idx);
            return true;
        }
        file_list[idx].size = upd_size;
        file_list[idx].last_change = upd_change;
        return false;
    }
    file_list.push(ScanFileData {
        filename: full_filename.to_vec(),
        size,
        last_change: current,
    });
    false
}

fn drop_old_files(
    file_list: &mut Vec<ScanFileData>,
    nzb_dir_interval: i32,
    nzb_dir_file_age: i32,
    env: &NzbgetRsScannerEnv,
) {
    let current = now_unix_secs();
    file_list.retain(|fd| {
        if should_drop_scan_file(fd.last_change, current, nzb_dir_interval, nzb_dir_file_age) {
            let mut msg = b"Removing file ".to_vec();
            msg.extend_from_slice(&fd.filename);
            msg.extend_from_slice(b" from scan file list");
            emit_log(env, LOG_DEBUG, &msg);
            false
        } else {
            true
        }
    });
}

fn find_archives(
    dir: &Path,
    check_stat: bool,
    file_age: i32,
    file_list: &mut Vec<ScanFileData>,
    env: &NzbgetRsScannerEnv,
) -> Vec<PathBuf> {
    let mut archives = Vec::new();
    let root_rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return archives,
        Err(e) => {
            let code = e.raw_os_error().unwrap_or(1);
            let err_bytes = crate::fsops::error_message(code);
            let mut msg = b"Could not scan ".to_vec();
            msg.extend_from_slice(crate::fsops::path_to_bytes(dir));
            msg.extend_from_slice(b" for archives: ");
            msg.extend_from_slice(&err_bytes);
            emit_log(env, LOG_WARN, &msg);
            return archives;
        }
    };

    let mut stack = vec![root_rd];
    while let Some(mut cur_rd) = stack.pop() {
        while let Some(entry_res) = cur_rd.next() {
            let entry = match entry_res {
                Ok(e) => e,
                Err(e)
                    if e.kind() == std::io::ErrorKind::PermissionDenied
                        || e.kind() == std::io::ErrorKind::NotFound =>
                {
                    continue;
                }
                Err(e) => {
                    let code = e.raw_os_error().unwrap_or(1);
                    let err_bytes = crate::fsops::error_message(code);
                    let mut msg = b"Could not scan ".to_vec();
                    msg.extend_from_slice(crate::fsops::path_to_bytes(dir));
                    msg.extend_from_slice(b" for archives: ");
                    msg.extend_from_slice(&err_bytes);
                    emit_log(env, LOG_WARN, &msg);
                    stack.clear();
                    break;
                }
            };
            let file_name = entry.file_name();
            let name_bytes = crate::cleanup::os_str_to_bytes(&file_name);
            if should_skip_item(name_bytes, PROCESSED_DIR, BROKEN_DIR) {
                continue;
            }
            let entry_path = entry.path();
            if entry.file_type().ok().is_some_and(|ft| ft.is_dir()) {
                match std::fs::read_dir(&entry_path) {
                    Ok(sub_rd) => {
                        stack.push(cur_rd);
                        stack.push(sub_rd);
                        break;
                    }
                    Err(e)
                        if e.kind() == std::io::ErrorKind::PermissionDenied
                            || e.kind() == std::io::ErrorKind::NotFound =>
                    {
                        continue;
                    }
                    Err(e) => {
                        let code = e.raw_os_error().unwrap_or(1);
                        let err_bytes = crate::fsops::error_message(code);
                        let mut msg = b"Could not scan ".to_vec();
                        msg.extend_from_slice(crate::fsops::path_to_bytes(dir));
                        msg.extend_from_slice(b" for archives: ");
                        msg.extend_from_slice(&err_bytes);
                        emit_log(env, LOG_WARN, &msg);
                        stack.clear();
                        break;
                    }
                }
            }
            if !std::fs::metadata(&entry_path)
                .ok()
                .is_some_and(|m| m.is_file())
            {
                continue;
            }
            let full_bytes = crate::fsops::path_to_bytes(&entry_path);
            if crate::unpack::can_extract_with_zipfast(&entry_path)
                && can_process_file(file_list, full_bytes, check_stat, file_age)
            {
                archives.push(entry_path);
            }
        }
    }
    archives
}

fn process_archive(
    archive_file: &Path,
    dest_dir: &Path,
    temp_dir: &[u8],
    nzb_dir: &[u8],
    archive_action: i32,
    env: &NzbgetRsScannerEnv,
) -> Option<Vec<PathBuf>> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SCANNER_UNPACK_SEQ.fetch_add(1, Ordering::Relaxed);
    let unique_leaf = format!("{nanos}-{seq}");
    let unpack_dir = crate::fsops::path(temp_dir)
        .join(crate::fsops::path(UNPACK_DIR))
        .join(&unique_leaf);
    let processed_dir = crate::fsops::path(nzb_dir).join(crate::fsops::path(PROCESSED_DIR));
    let broken_dir = crate::fsops::path(nzb_dir).join(crate::fsops::path(BROKEN_DIR));

    let mut moved: Vec<PathBuf> = Vec::new();
    let ok = crate::cleanup::archive_processor_process(
        archive_file,
        &unpack_dir,
        dest_dir,
        &processed_dir,
        &broken_dir,
        archive_action,
        |arc_path, _| {
            let archive_name = arc_path
                .file_name()
                .map(crate::cleanup::os_str_to_bytes)
                .unwrap_or(b"");
            let ext = arc_path
                .extension()
                .map(crate::cleanup::os_str_to_bytes)
                .unwrap_or(b"");
            let mut msg = b"Unpack: Could not process '".to_vec();
            msg.extend_from_slice(archive_name);
            msg.extend_from_slice(b"': Unsupported archive format: ");
            if !ext.is_empty() {
                msg.push(b'.');
                msg.extend_from_slice(ext);
            }
            emit_log(env, LOG_ERROR, &msg);
            (false, false)
        },
        |kind, msg| {
            let mapped = match kind {
                crate::cleanup::CleanupLogKind::Info => LOG_INFO,
                crate::cleanup::CleanupLogKind::Warning => LOG_WARN,
                crate::cleanup::CleanupLogKind::Error => LOG_ERROR,
                crate::cleanup::CleanupLogKind::Detail => LOG_DETAIL,
            };
            emit_log(env, mapped, msg);
        },
        |moved_path| {
            moved.push(crate::fsops::path(moved_path).to_path_buf());
        },
    );
    if ok {
        Some(moved)
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn add_file_to_queue(
    filename: &[u8],
    nzb_name: &[u8],
    category: &[u8],
    auto_category: bool,
    priority: c_int,
    dupe_key: &[u8],
    dupe_score: c_int,
    dupe_mode: c_int,
    add_top: bool,
    add_paused: bool,
    url_info: *mut c_void,
    env: &NzbgetRsScannerEnv,
) -> (bool, c_int) {
    let basename = &filename[crate::paths::base_file_name(filename)..];
    let mut add_msg = b"Adding collection ".to_vec();
    add_msg.extend_from_slice(basename);
    add_msg.extend_from_slice(b" to queue");
    emit_log(env, LOG_INFO, &add_msg);

    let file_c = to_cstr_sanitized(filename);
    let cat_c = to_cstr_sanitized(category);
    let mut detected_cat_ptr: *const c_char = std::ptr::null();
    let mut ok = if let Some(parse_cb) = env.parse_nzb_cb {
        unsafe { parse_cb(env.ctx, file_c.as_ptr(), cat_c.as_ptr(), &mut detected_cat_ptr) != 0 }
    } else {
        false
    };
    let detected_cat = cstr_bytes(detected_cat_ptr).to_vec();
    if !ok {
        let mut err_msg = b"Could not add collection ".to_vec();
        err_msg.extend_from_slice(basename);
        err_msg.extend_from_slice(b" to queue");
        emit_log(env, LOG_ERROR, &err_msg);
    }

    let bak_suffix: &[u8] = if ok { b"queued" } else { b"error" };
    let mut bakname2 = Vec::new();
    let renamed = crate::fsdelete::rename_bak(
        &mut crate::fsops::PosixFsOps,
        filename,
        bak_suffix,
        false,
        &mut bakname2,
    )
    .unwrap_or(false);
    if !renamed {
        ok = false;
        let err_bytes = crate::fsops::error_message(crate::fsops::last_errno());
        let mut msg = b"Could not rename file ".to_vec();
        msg.extend_from_slice(filename);
        msg.extend_from_slice(b" to ");
        msg.extend_from_slice(&bakname2);
        msg.extend_from_slice(b": ");
        msg.extend_from_slice(&err_bytes);
        emit_log(env, LOG_ERROR, &msg);
    }

    let mut auto_resolved_cat: Option<CString> = None;
    if auto_category {
        let pp_cat: Vec<u8> = if detected_cat.is_empty() {
            let mut msg = b"Auto-category: no category detected in '".to_vec();
            msg.extend_from_slice(nzb_name);
            msg.push(b'\'');
            emit_log(env, LOG_DETAIL, &msg);
            category.to_vec()
        } else {
            let det_c = to_cstr_sanitized(&detected_cat);
            let matched_ptr = env
                .find_category_cb
                .map(|cb| unsafe { cb(env.ctx, det_c.as_ptr(), 1) })
                .unwrap_or(std::ptr::null());
            if !matched_ptr.is_null() {
                let matched = cstr_bytes(matched_ptr).to_vec();
                let mut msg = b"Category '".to_vec();
                msg.extend_from_slice(&matched);
                msg.extend_from_slice(b"' matched to '");
                msg.extend_from_slice(&detected_cat);
                msg.extend_from_slice(b"' for '");
                msg.extend_from_slice(nzb_name);
                msg.push(b'\'');
                emit_log(env, LOG_DETAIL, &msg);
                auto_resolved_cat = Some(to_cstr_sanitized(&matched));
                matched
            } else {
                let mut msg = b"Auto-category: using detected category '".to_vec();
                msg.extend_from_slice(&detected_cat);
                msg.extend_from_slice(b"' for '");
                msg.extend_from_slice(nzb_name);
                msg.push(b'\'');
                emit_log(env, LOG_DETAIL, &msg);
                auto_resolved_cat = Some(det_c);
                detected_cat
            }
        };
        if let Some(wp_cb) = env.work_params_cb {
            let pp_c = to_cstr_sanitized(&pp_cat);
            unsafe { wp_cb(env.ctx, 3, pp_c.as_ptr(), std::ptr::null()) };
        }
    }

    let bak_c = to_cstr_sanitized(&bakname2);
    let name_c = to_cstr_sanitized(nzb_name);
    let key_c = to_cstr_sanitized(dupe_key);
    let auto_ptr = auto_resolved_cat
        .as_ref()
        .map_or(std::ptr::null(), |c| c.as_ptr());
    let has_auto = c_int::from(auto_resolved_cat.is_some());

    let nzb_id = if let Some(commit_cb) = env.commit_nzb_cb {
        unsafe {
            commit_cb(
                env.ctx,
                c_int::from(ok),
                bak_c.as_ptr(),
                auto_ptr,
                has_auto,
                name_c.as_ptr(),
                priority,
                key_c.as_ptr(),
                dupe_score,
                dupe_mode,
                c_int::from(add_top),
                c_int::from(add_paused),
                url_info,
            )
        }
    } else {
        0
    };
    (ok, nzb_id)
}

impl NzbgetRsScanner {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ScannerInner {
                file_list: Vec::new(),
                queue_list: Vec::new(),
                pass: 0,
                scan_script: false,
            }),
            requested_nzb_dir_scan: AtomicBool::new(false),
            scanning: AtomicBool::new(false),
            nzb_dir_interval: AtomicI32::new(0),
        }
    }

    fn init_options(&self, has_scan_script: bool, env: &NzbgetRsScannerEnv) {
        self.nzb_dir_interval.store(1, Ordering::SeqCst);
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.scan_script = has_scan_script;
        }
        cleanup_stale_unpack_dir(cstr_bytes(env.temp_dir), env);
    }

    fn service_interval(&self, opt_nzb_dir_interval: i32) -> i32 {
        if self.requested_nzb_dir_scan.load(Ordering::SeqCst) {
            0
        } else if opt_nzb_dir_interval <= 0 {
            -1
        } else {
            self.nzb_dir_interval.load(Ordering::SeqCst)
        }
    }

    fn scan_nzb_dir(&self, sync_mode: bool, env: &NzbgetRsScannerEnv) {
        {
            let _guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            self.scanning.store(true, Ordering::SeqCst);
            self.requested_nzb_dir_scan.store(true, Ordering::SeqCst);
            if let Some(wake) = env.wake_cb {
                unsafe { wake(env.ctx) };
            }
        }
        while sync_mode
            && (self.scanning.load(Ordering::SeqCst)
                || self.requested_nzb_dir_scan.load(Ordering::SeqCst))
        {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn service_work(&self, env: &NzbgetRsScannerEnv) {
        emit_log(env, LOG_DEBUG, b"Scanner service work");

        if env.queue_loaded == 0 {
            return;
        }

        self.nzb_dir_interval
            .store(env.nzb_dir_interval, Ordering::SeqCst);

        if env.pause_scan != 0 && !self.requested_nzb_dir_scan.load(Ordering::SeqCst) {
            return;
        }

        emit_log(env, LOG_DEBUG, b"Scanner service work: doing work");

        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        let check_stat = !self.requested_nzb_dir_scan.swap(false, Ordering::SeqCst);
        self.scanning.store(true, Ordering::SeqCst);

        let nzb_dir = cstr_bytes(env.nzb_dir);
        let temp_dir = cstr_bytes(env.temp_dir);
        let sep = if env.path_sep == 0 {
            b'/'
        } else {
            env.path_sep as u8
        };

        if env.archive_scan != 0 {
            let nzb_dir_path = crate::fsops::path(nzb_dir);
            let archives = find_archives(
                nzb_dir_path,
                check_stat,
                env.nzb_dir_file_age,
                &mut inner.file_list,
                env,
            );
            for archive in &archives {
                let parent = archive.parent().unwrap_or(nzb_dir_path);
                let _ = process_archive(
                    archive,
                    parent,
                    temp_dir,
                    nzb_dir,
                    env.archive_action,
                    env,
                );
            }
        }

        Self::check_incoming_nzbs(&mut inner, nzb_dir, b"", check_stat, sep, env, 0);
        if !check_stat && inner.scan_script {
            Self::check_incoming_nzbs(&mut inner, nzb_dir, b"", check_stat, sep, env, 0);
        }

        self.scanning.store(false, Ordering::SeqCst);

        let (next_iv, next_p) = next_pass(
            env.nzb_dir_interval,
            env.nzb_dir_file_age,
            inner.scan_script,
            inner.pass,
        );
        inner.pass = next_p;
        self.nzb_dir_interval.store(next_iv, Ordering::SeqCst);

        drop_old_files(
            &mut inner.file_list,
            env.nzb_dir_interval,
            env.nzb_dir_file_age,
            env,
        );
        inner.queue_list.clear();
    }

    fn check_incoming_nzbs(
        inner: &mut ScannerInner,
        directory: &[u8],
        category: &[u8],
        check_stat: bool,
        sep: u8,
        env: &NzbgetRsScannerEnv,
        depth: usize,
    ) {
        if depth >= 256 {
            return;
        }
        let dir_c = to_cstr_sanitized(directory);
        let mut browser = crate::fsops::PosixDirBrowser::new(Some(&dir_c), true);
        loop {
            let ptr = browser.next_ptr();
            if ptr.is_null() {
                break;
            }
            let filename = unsafe { CStr::from_ptr(ptr).to_bytes().to_vec() };
            if should_skip_item(&filename, PROCESSED_DIR, BROKEN_DIR) {
                continue;
            }
            let full_filename = join_sep(directory, sep, &filename);
            let is_dir =
                crate::fsops::directory_exists(&full_filename, &crate::fsops::posix_stat);
            if is_dir {
                let sub_cat = if !category.is_empty() {
                    join_sep(category, sep, &filename)
                } else {
                    filename
                };
                Self::check_incoming_nzbs(
                    inner,
                    &full_filename,
                    &sub_cat,
                    check_stat,
                    sep,
                    env,
                    depth + 1,
                );
            } else if can_process_file(
                &mut inner.file_list,
                &full_filename,
                check_stat,
                env.nzb_dir_file_age,
            ) {
                Self::process_incoming_file(
                    inner,
                    directory,
                    &filename,
                    &full_filename,
                    category,
                    env,
                );
            }
        }
    }

    fn process_incoming_file(
        inner: &mut ScannerInner,
        directory: &[u8],
        base_filename: &[u8],
        full_filename: &[u8],
        category: &[u8],
        env: &NzbgetRsScannerEnv,
    ) {
        let Some(dot_pos) = base_filename.iter().rposition(|&b| b == b'.') else {
            return;
        };
        let extension = &base_filename[dot_pos..];

        let mut nzb_name: Vec<u8> = Vec::new();
        let mut nzb_category: Vec<u8> = category.to_vec();
        let mut dupe_key: Vec<u8> = Vec::new();
        let mut priority: c_int = 0;
        let mut add_top = false;
        let mut add_paused = false;
        let mut auto_category = nzb_category.is_empty();
        let mut dupe_score: c_int = 0;
        let mut dupe_mode: c_int = 0;
        let mut add_status: c_int = AS_SKIPPED;
        let mut nzb_id: c_int = 0;
        let mut url_info_usize: usize = 0;

        if let Some(wp_cb) = env.work_params_cb {
            unsafe { wp_cb(env.ctx, 0, std::ptr::null(), std::ptr::null()) };
        }

        let matched_idx = inner
            .queue_list
            .iter()
            .rposition(|q| q.filename == full_filename);
        if let Some(idx) = matched_idx {
            let q = &inner.queue_list[idx];
            nzb_name = q.nzb_name.clone();
            nzb_category = q.category.clone();
            priority = q.priority;
            dupe_key = q.dupe_key.clone();
            dupe_score = q.dupe_score;
            dupe_mode = q.dupe_mode;
            add_top = q.add_top;
            add_paused = q.add_paused;
            auto_category = q.auto_category;
            url_info_usize = q.url_info;
            if let Some(wp_cb) = env.work_params_cb {
                for (pname, pval) in &q.params {
                    unsafe { wp_cb(env.ctx, 1, pname.as_ptr(), pval.as_ptr()) };
                }
            }
        }

        if let Some(wp_cb) = env.work_params_cb {
            let cat_c = to_cstr_sanitized(&nzb_category);
            unsafe { wp_cb(env.ctx, 2, cat_c.as_ptr(), std::ptr::null()) };
        }

        let mut exists = true;
        let ran_scan_script = inner.scan_script && !extension.eq_ignore_ascii_case(b".nzb_processed");
        if ran_scan_script {
            struct ScriptStrings<'a> {
                nzb_name: &'a mut Vec<u8>,
                nzb_category: &'a mut Vec<u8>,
                dupe_key: &'a mut Vec<u8>,
            }
            unsafe extern "C" fn update_script_str(
                raw: *mut c_void,
                field: c_int,
                val: *const c_char,
            ) {
                let st = unsafe { &mut *raw.cast::<ScriptStrings<'_>>() };
                let bytes = cstr_bytes(val).to_vec();
                match field {
                    0 => *st.nzb_name = bytes,
                    1 => *st.nzb_category = bytes,
                    2 => *st.dupe_key = bytes,
                    _ => {}
                }
            }
            if let Some(sc_cb) = env.scan_script_cb {
                let full_c = to_cstr_sanitized(full_filename);
                let dir_c = to_cstr_sanitized(directory);
                let init_name_c = to_cstr_sanitized(&nzb_name);
                let init_cat_c = to_cstr_sanitized(&nzb_category);
                let init_key_c = to_cstr_sanitized(&dupe_key);
                let mut add_top_i = c_int::from(add_top);
                let mut add_paused_i = c_int::from(add_paused);
                {
                    let mut st = ScriptStrings {
                        nzb_name: &mut nzb_name,
                        nzb_category: &mut nzb_category,
                        dupe_key: &mut dupe_key,
                    };
                    unsafe {
                        sc_cb(
                            env.ctx,
                            full_c.as_ptr(),
                            url_info_usize as *mut c_void,
                            dir_c.as_ptr(),
                            init_name_c.as_ptr(),
                            init_cat_c.as_ptr(),
                            init_key_c.as_ptr(),
                            (&mut st as *mut ScriptStrings<'_>).cast(),
                            update_script_str,
                            &mut priority,
                            &mut add_top_i,
                            &mut add_paused_i,
                            &mut dupe_score,
                            &mut dupe_mode,
                        );
                    }
                }
                add_top = add_top_i != 0;
                add_paused = add_paused_i != 0;
            }
            exists = crate::fsops::file_exists(full_filename, &crate::fsops::posix_stat);
        }

        let (rename_to_processed, ext_action) =
            classify_incoming_extension(ran_scan_script, exists, extension);
        if rename_to_processed {
            let mut bakname2 = Vec::new();
            let ok = crate::fsdelete::rename_bak(
                &mut crate::fsops::PosixFsOps,
                full_filename,
                b"processed",
                false,
                &mut bakname2,
            )
            .unwrap_or(false);
            if !ok {
                let err_bytes = crate::fsops::error_message(crate::fsops::last_errno());
                let mut msg = b"Could not rename file ".to_vec();
                msg.extend_from_slice(full_filename);
                msg.extend_from_slice(b" to ");
                msg.extend_from_slice(&bakname2);
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&err_bytes);
                emit_log(env, LOG_ERROR, &msg);
            }
        }

        if ext_action == 1 {
            let mut renamed_name = Vec::new();
            let ok = crate::fsdelete::rename_bak(
                &mut crate::fsops::PosixFsOps,
                full_filename,
                b"nzb",
                true,
                &mut renamed_name,
            )
            .unwrap_or(false);
            if ok {
                let (added_ok, id) = add_file_to_queue(
                    &renamed_name,
                    &nzb_name,
                    &nzb_category,
                    auto_category,
                    priority,
                    &dupe_key,
                    dupe_score,
                    dupe_mode,
                    add_top,
                    add_paused,
                    url_info_usize as *mut c_void,
                    env,
                );
                nzb_id = id;
                add_status = if added_ok { AS_SUCCESS } else { AS_FAILED };
                if added_ok {
                    if let Some(idx) = matched_idx {
                        inner.queue_list[idx].url_info = 0;
                    }
                }
            } else {
                let err_bytes = crate::fsops::error_message(crate::fsops::last_errno());
                let mut msg = b"Could not rename file ".to_vec();
                msg.extend_from_slice(full_filename);
                msg.extend_from_slice(b" to ");
                msg.extend_from_slice(&renamed_name);
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&err_bytes);
                emit_log(env, LOG_ERROR, &msg);
                add_status = AS_FAILED;
            }
        } else if ext_action == 2 {
            let (added_ok, id) = add_file_to_queue(
                full_filename,
                &nzb_name,
                &nzb_category,
                auto_category,
                priority,
                &dupe_key,
                dupe_score,
                dupe_mode,
                add_top,
                add_paused,
                url_info_usize as *mut c_void,
                env,
            );
            nzb_id = id;
            add_status = if added_ok { AS_SUCCESS } else { AS_FAILED };
            if added_ok {
                if let Some(idx) = matched_idx {
                    inner.queue_list[idx].url_info = 0;
                }
            }
        }

        if let Some(idx) = matched_idx {
            if let Some(slot) = &inner.queue_list[idx].result_slot {
                let mut g = slot.lock().unwrap_or_else(|e| e.into_inner());
                *g = (add_status, Some(nzb_id));
            }
        }
    }

    fn add_archive_bytes(&self, buf: &[u8], env: &NzbgetRsScannerEnv) -> c_int {
        let filename = cstr_bytes(env.filename);
        if filename.is_empty() {
            emit_log(
                env,
                LOG_ERROR,
                b"Failed to add the archive to the download queue: Archive filename cannot be empty",
            );
            return AS_FAILED;
        }
        let temp_dir = cstr_bytes(env.temp_dir);
        if temp_dir.is_empty() {
            let mut msg = b"Failed to create file '".to_vec();
            msg.extend_from_slice(filename);
            msg.extend_from_slice(b"': 'TempDir' is required and cannot be empty");
            emit_log(env, LOG_ERROR, &msg);
            return AS_FAILED;
        }
        let nzb_dir = cstr_bytes(env.nzb_dir);
        if nzb_dir.is_empty() {
            let mut msg = b"Failed to create file '".to_vec();
            msg.extend_from_slice(filename);
            msg.extend_from_slice(b"': 'NzbDir' is required and cannot be empty");
            emit_log(env, LOG_ERROR, &msg);
            return AS_FAILED;
        }

        let archive_filename = crate::paths::make_valid_filename(filename, false);
        let archive_file_path =
            crate::fsops::path(temp_dir).join(crate::fsops::path(&archive_filename));

        {
            use std::io::Write;
            let mut f = match std::fs::File::create(&archive_file_path) {
                Ok(f) => f,
                Err(e) => {
                    let err_bytes =
                        crate::fsops::error_message(e.raw_os_error().unwrap_or(1));
                    let mut msg = b"Failed to create archive file '".to_vec();
                    msg.extend_from_slice(&archive_filename);
                    msg.extend_from_slice(b"' in temporary directory: ");
                    msg.extend_from_slice(&err_bytes);
                    emit_log(env, LOG_ERROR, &msg);
                    return AS_FAILED;
                }
            };
            if let Err(e) = f.write_all(buf).and_then(|()| f.flush()) {
                let err_bytes = crate::fsops::error_message(e.raw_os_error().unwrap_or(1));
                let mut msg = b"Failed to write data to archive file '".to_vec();
                msg.extend_from_slice(&archive_filename);
                msg.extend_from_slice(b"': ");
                msg.extend_from_slice(&err_bytes);
                emit_log(env, LOG_ERROR, &msg);
                drop(f);
                let _ = std::fs::remove_file(&archive_file_path);
                return AS_FAILED;
            }
        }

        if env.archive_scan == 0 {
            let final_path =
                crate::fsops::path(nzb_dir).join(crate::fsops::path(&archive_filename));
            let src_c = to_cstr_sanitized(crate::fsops::path_to_bytes(&archive_file_path));
            let dst_c = to_cstr_sanitized(crate::fsops::path_to_bytes(&final_path));
            if !crate::fsops::move_file(&src_c, &dst_c) {
                let err_bytes = crate::fsops::error_message(crate::fsops::last_errno());
                let mut msg = b"Failed to move file '".to_vec();
                msg.extend_from_slice(&archive_filename);
                msg.extend_from_slice(b"' to NzbDir: ");
                msg.extend_from_slice(&err_bytes);
                emit_log(env, LOG_ERROR, &msg);
                return AS_FAILED;
            }
            let mut msg = b"Skipping processing of '".to_vec();
            msg.extend_from_slice(&archive_filename);
            msg.extend_from_slice(b"': option 'NzbDirArchiveScan' is disabled");
            emit_log(env, LOG_INFO, &msg);
            return AS_SKIPPED;
        }

        let captured_params = capture_params(env);
        let category_bytes = cstr_bytes(env.category);
        let dupe_key_bytes = cstr_bytes(env.dupe_key);
        let slots: Vec<ResultSlot>;
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let Some(nzb_files) = process_archive(
                &archive_file_path,
                crate::fsops::path(nzb_dir),
                temp_dir,
                nzb_dir,
                env.archive_action,
                env,
            ) else {
                return AS_FAILED;
            };
            if nzb_files.is_empty() {
                return AS_SKIPPED;
            }
            slots = (0..nzb_files.len())
                .map(|_| Arc::new(Mutex::new((AS_SKIPPED, None))))
                .collect();
            for (index, nzb_file) in nzb_files.iter().enumerate() {
                let nzb_path_bytes = crate::fsops::path_to_bytes(nzb_file).to_vec();
                let leaf_bytes = nzb_file
                    .file_name()
                    .map(crate::cleanup::os_str_to_bytes)
                    .unwrap_or(b"")
                    .to_vec();
                let use_cat = resolve_category(env, category_bytes, &leaf_bytes);
                inner.queue_list.push(ScanQueueData {
                    filename: nzb_path_bytes,
                    nzb_name: leaf_bytes,
                    category: use_cat,
                    dupe_key: dupe_key_bytes.to_vec(),
                    priority: env.priority,
                    dupe_score: env.dupe_score,
                    dupe_mode: env.dupe_mode,
                    auto_category: env.auto_category != 0,
                    add_top: env.add_top != 0,
                    add_paused: env.add_paused != 0,
                    params: captured_params.clone(),
                    url_info: if index == 0 {
                        env.url_info as usize
                    } else {
                        0
                    },
                    result_slot: Some(slots[index].clone()),
                });
            }
            self.requested_nzb_dir_scan.store(true, Ordering::SeqCst);
        }

        self.scan_nzb_dir(true, env);

        let statuses: Vec<i32> = slots
            .iter()
            .map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).0)
            .collect();
        resolve_archive_add_status(!env.url_info.is_null(), &statuses, AS_SUCCESS, AS_SKIPPED)
    }

    fn add_archive_tmp_file(&self, env: &NzbgetRsScannerEnv) -> c_int {
        let tmp_bytes = cstr_bytes(env.nzb_name);
        let tmp_c = to_cstr_sanitized(tmp_bytes);
        let mut buf: Vec<u8> = Vec::new();
        let loaded =
            crate::fsops::load_file_into_buffer(&tmp_c, false, &crate::fsops::posix_stat, |sz| {
                let n = sz.max(0) as usize;
                buf.resize(n, 0);
                if n == 0 {
                    std::ptr::NonNull::<u8>::dangling().as_ptr()
                } else {
                    buf.as_mut_ptr()
                }
            });
        if !loaded {
            let mut msg = b"Could not open file '".to_vec();
            msg.extend_from_slice(tmp_bytes);
            msg.push(b'\'');
            emit_log(env, LOG_ERROR, &msg);
            return AS_FAILED;
        }
        if let Err(e) = std::fs::remove_file(crate::fsops::path(tmp_bytes)) {
            if e.kind() != std::io::ErrorKind::NotFound {
                let code = e.raw_os_error().unwrap_or(1);
                let err_bytes = crate::fsops::error_message(code);
                let mut msg = b"Failed to remove '".to_vec();
                msg.extend_from_slice(tmp_bytes);
                msg.extend_from_slice(b"': ");
                msg.extend_from_slice(&err_bytes);
                msg.extend_from_slice(format!(" (code {code})").as_bytes());
                emit_log(env, LOG_ERROR, &msg);
            }
        }
        self.add_archive_bytes(&buf, env)
    }

    fn add_external_file(&self, env: &NzbgetRsScannerEnv) -> c_int {
        let sep = if env.path_sep == 0 {
            b'/'
        } else {
            env.path_sep as u8
        };
        let nzb_dir = cstr_bytes(env.nzb_dir);
        let temp_dir = cstr_bytes(env.temp_dir);
        let nzb_name = cstr_bytes(env.nzb_name);
        let category = cstr_bytes(env.category);
        let dupe_key = cstr_bytes(env.dupe_key);
        let has_disk_file = !env.filename.is_null();
        let buf_slice: &[u8] = if env.buf_ptr.is_null() || env.buf_len <= 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(env.buf_ptr.cast::<u8>(), env.buf_len as usize) }
        };
        let captured_params = capture_params(env);
        let slot: ResultSlot = Arc::new(Mutex::new((AS_SKIPPED, None)));

        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let (temp_file_name, is_nzb) = if has_disk_file {
                (cstr_bytes(env.filename).to_vec(), false)
            } else {
                let mut num = SCANNER_ID_GEN.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
                let mut candidate: Vec<u8> = Vec::new();
                while candidate.is_empty()
                    || crate::fsops::file_exists(&candidate, &crate::fsops::posix_stat)
                {
                    candidate = join_sep(temp_dir, sep, format!("nzb-{num}.tmp").as_bytes());
                    num = num.wrapping_add(1);
                }
                let cand_c = to_cstr_sanitized(&candidate);
                if !crate::fsops::save_buffer_into_file(&cand_c, buf_slice) {
                    let mut msg = b"Could not create file ".to_vec();
                    msg.extend_from_slice(&candidate);
                    emit_log(env, LOG_ERROR, &msg);
                    return AS_FAILED;
                }
                (candidate, is_nzb_xml_prefix(buf_slice))
            };

            let raw_for_valid = if is_absolute_nzb_name(nzb_name) {
                &nzb_name[crate::paths::base_file_name(nzb_name)..]
            } else {
                nzb_name
            };
            let valid = crate::paths::make_valid_filename(raw_for_valid, false);
            let valid_nzb_name = normalize_external_nzb_name(nzb_name, &valid, is_nzb);

            let mut scan_file_name = join_sep(nzb_dir, sep, &valid_nzb_name);
            let dot_off = valid_nzb_name.iter().rposition(|&b| b == b'.');
            let (base_part, ext_part) = match dot_off {
                Some(dot) => (&valid_nzb_name[..dot], Some(&valid_nzb_name[dot + 1..])),
                None => (&valid_nzb_name[..], None),
            };

            let mut num: i32 = 2;
            while crate::fsops::file_exists(&scan_file_name, &crate::fsops::posix_stat) {
                let mut leaf = Vec::with_capacity(valid_nzb_name.len() + 12);
                leaf.extend_from_slice(base_part);
                leaf.extend_from_slice(format!("_{num}").as_bytes());
                if let Some(ext) = ext_part {
                    leaf.push(b'.');
                    leaf.extend_from_slice(ext);
                }
                scan_file_name = join_sep(nzb_dir, sep, &leaf);
                num = num.wrapping_add(1);
            }

            let temp_c = to_cstr_sanitized(&temp_file_name);
            let scan_c = to_cstr_sanitized(&scan_file_name);
            if !crate::fsops::move_file(&temp_c, &scan_c) {
                let err_bytes = crate::fsops::error_message(crate::fsops::last_errno());
                let mut msg = b"Could not move file ".to_vec();
                msg.extend_from_slice(&temp_file_name);
                msg.extend_from_slice(b" to ");
                msg.extend_from_slice(&scan_file_name);
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&err_bytes);
                emit_log(env, LOG_ERROR, &msg);
                let _ = crate::fsdelete::Ops::delete_file(
                    &mut crate::fsops::PosixFsOps,
                    &temp_file_name,
                );
                return AS_FAILED;
            }

            let use_cat = resolve_category(env, category, nzb_name);
            inner.queue_list.push(ScanQueueData {
                filename: scan_file_name,
                nzb_name: nzb_name.to_vec(),
                category: use_cat,
                dupe_key: dupe_key.to_vec(),
                priority: env.priority,
                dupe_score: env.dupe_score,
                dupe_mode: env.dupe_mode,
                auto_category: env.auto_category != 0,
                add_top: env.add_top != 0,
                add_paused: env.add_paused != 0,
                params: captured_params,
                url_info: env.url_info as usize,
                result_slot: Some(slot.clone()),
            });
            self.requested_nzb_dir_scan.store(true, Ordering::SeqCst);
        }

        self.scan_nzb_dir(true, env);

        let (status, nzb_id_opt) = *slot.lock().unwrap_or_else(|e| e.into_inner());
        if !env.out_nzb_id.is_null() {
            if let Some(id) = nzb_id_opt {
                unsafe { *env.out_nzb_id = id };
            }
        }
        status
    }
}

impl Default for NzbgetRsScanner {
    fn default() -> Self {
        Self::new()
    }
}

pub unsafe fn scanner_exec(
    scanner: *mut NzbgetRsScanner,
    op: c_int,
    env: *const NzbgetRsScannerEnv,
    a1: c_int,
    a2: c_int,
    a3: c_int,
) -> c_int {
    if op == 9 {
        return classify_missed_articles(a1, a2, a3 != 0);
    }
    let Some(sc) = (unsafe { scanner.as_ref() }) else {
        return 0;
    };
    if op == 1 {
        return sc.service_interval(a1);
    }
    let Some(env_ref) = (unsafe { env.as_ref() }) else {
        return 0;
    };
    match op {
        0 => {
            sc.init_options(a1 != 0, env_ref);
            0
        }
        2 => {
            sc.service_work(env_ref);
            0
        }
        3 => {
            sc.scan_nzb_dir(a1 != 0, env_ref);
            0
        }
        4 => {
            if env_ref.buf_len < 0 || (env_ref.buf_len > 0 && env_ref.buf_ptr.is_null()) {
                return AS_FAILED;
            }
            let slice = if env_ref.buf_ptr.is_null() || env_ref.buf_len == 0 {
                &[][..]
            } else {
                unsafe {
                    std::slice::from_raw_parts(
                        env_ref.buf_ptr.cast::<u8>(),
                        env_ref.buf_len as usize,
                    )
                }
            };
            sc.add_archive_bytes(slice, env_ref)
        }
        5 => sc.add_archive_tmp_file(env_ref),
        6 => sc.add_external_file(env_ref),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_next_pass_and_extensions() {
        assert_eq!(next_pass(60, 5, true, 0), (5, 1));
        assert_eq!(next_pass(60, 5, true, 3), (60, 0));
        assert!(can_process_extension(b"movie.nzb"));
        assert!(!can_process_extension(b"movie.nzb.queued"));
        assert!(!can_process_extension(b"movie.nzb.error"));
        assert!(!can_process_extension(b"movie.nzb.processed"));
    }

    #[test]
    fn test_normalize_external_name_and_xml_prefix() {
        assert!(is_nzb_xml_prefix(b"<?xml version=\"1.0\"?><nzb></nzb>"));
        assert_eq!(
            normalize_external_nzb_name(b"release", b".release", true),
            b"_.release.nzb"
        );
        assert!(is_absolute_nzb_name(b"/tmp/test.nzb"));
        assert!(is_absolute_nzb_name(b"C:\\temp\\test.nzb"));
        assert!(!is_absolute_nzb_name(b"[1/10] - Title.nzb"));
    }

    #[test]
    fn test_scanner_engine_external_and_archive_end_to_end() {
        let base = std::env::temp_dir().join(format!(
            "nzbget-rs-scanner-test-{}-{}",
            std::process::id(),
            now_unix_secs()
        ));
        let nzb_dir = base.join("nzb");
        let temp_dir = base.join("tmp");
        std::fs::create_dir_all(temp_dir.join("_unpack").join("stale")).unwrap();
        std::fs::create_dir_all(&nzb_dir).unwrap();

        let nzb_dir_c = CString::new(crate::fsops::path_to_bytes(&nzb_dir)).unwrap();
        let temp_dir_c = CString::new(crate::fsops::path_to_bytes(&temp_dir)).unwrap();

        struct MockHost {
            scanner: Arc<NzbgetRsScanner>,
            nzb_dir_c: CString,
            temp_dir_c: CString,
            added_count: AtomicI32,
            last_cat: Mutex<String>,
        }

        unsafe extern "C" fn wake_cb(ctx: *mut c_void) {
            let host = unsafe { &*ctx.cast::<MockHost>() };
            let scanner = host.scanner.clone();
            let nzb_c = host.nzb_dir_c.clone();
            let tmp_c = host.temp_dir_c.clone();
            let ctx_usize = ctx as usize;
            std::thread::spawn(move || {
                let mut env: NzbgetRsScannerEnv = unsafe { std::mem::zeroed() };
                env.ctx = ctx_usize as *mut c_void;
                env.nzb_dir = nzb_c.as_ptr();
                env.temp_dir = tmp_c.as_ptr();
                env.nzb_dir_interval = 5;
                env.nzb_dir_file_age = 0;
                env.archive_scan = 1;
                env.archive_action = 0;
                env.queue_loaded = 1;
                env.path_sep = b'/' as c_char;
                env.parse_nzb_cb = Some(parse_cb);
                env.commit_nzb_cb = Some(commit_cb);
                scanner.service_work(&env);
            });
        }

        unsafe extern "C" fn parse_cb(
            _ctx: *mut c_void,
            _filename: *const c_char,
            _category: *const c_char,
            out_cat: *mut *const c_char,
        ) -> c_int {
            unsafe { *out_cat = c"movies".as_ptr() };
            1
        }

        unsafe extern "C" fn commit_cb(
            ctx: *mut c_void,
            ok: c_int,
            _queued: *const c_char,
            auto_cat: *const c_char,
            has_auto: c_int,
            _nzb_name: *const c_char,
            _prio: c_int,
            _dupe_key: *const c_char,
            _dupe_score: c_int,
            _dupe_mode: c_int,
            _add_top: c_int,
            _add_paused: c_int,
            _url_info: *mut c_void,
        ) -> c_int {
            if ok == 0 {
                return 0;
            }
            let host = unsafe { &*ctx.cast::<MockHost>() };
            if has_auto != 0 && !auto_cat.is_null() {
                if let Ok(s) = unsafe { CStr::from_ptr(auto_cat) }.to_str() {
                    *host.last_cat.lock().unwrap() = s.to_string();
                }
            }
            host.added_count.fetch_add(1, Ordering::SeqCst) + 100
        }

        let scanner = Arc::new(NzbgetRsScanner::new());
        let mut host = Box::new(MockHost {
            scanner: scanner.clone(),
            nzb_dir_c: nzb_dir_c.clone(),
            temp_dir_c: temp_dir_c.clone(),
            added_count: AtomicI32::new(0),
            last_cat: Mutex::new(String::new()),
        });

        let mut env: NzbgetRsScannerEnv = unsafe { std::mem::zeroed() };
        env.ctx = (&mut *host as *mut MockHost).cast();
        env.nzb_dir = nzb_dir_c.as_ptr();
        env.temp_dir = temp_dir_c.as_ptr();
        env.path_sep = b'/' as c_char;
        env.wake_cb = Some(wake_cb);
        env.parse_nzb_cb = Some(parse_cb);
        env.commit_nzb_cb = Some(commit_cb);

        scanner.init_options(false, &env);
        assert!(!temp_dir.join("_unpack").exists());
        assert_eq!(scanner.service_interval(5), 1);
        assert_eq!(scanner.service_interval(0), -1);

        let xml = b"<?xml version=\"1.0\"?><nzb></nzb>";
        let name_c = c"Show.S01E01";
        let mut out_id: c_int = 0;
        env.nzb_name = name_c.as_ptr();
        env.category = c"".as_ptr();
        env.dupe_key = c"dk".as_ptr();
        env.auto_category = 1;
        env.buf_ptr = xml.as_ptr().cast();
        env.buf_len = xml.len() as c_int;
        env.out_nzb_id = &mut out_id;

        let status = scanner.add_external_file(&env);
        assert_eq!(status, AS_SUCCESS);
        assert_eq!(out_id, 100);
        assert_eq!(host.last_cat.lock().unwrap().as_str(), "movies");
        assert!(nzb_dir.join("Show.S01E01.nzb.queued").exists());

        let _ = std::fs::remove_dir_all(&base);
    }
}
