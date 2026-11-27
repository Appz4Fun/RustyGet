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

use crate::queuesubsys_host::NzbRef;
use crate::cstr;
use crate::options::OptStr;
use crate::status::DeleteStatus;
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

/// What the scanner asks of its owner while it scans and queues files. The
/// daemon's host is [`NativeScannerHost`]; tests supply their own. Each
/// default is what the scanner did when the owner had nothing to add.
pub(crate) trait ScannerHost: Send + Sync {
    /// Wakes the scanner's service thread. False lets the scanner wake the
    /// daemon's service coordinator itself.
    fn wake(&self) -> bool {
        false
    }

    /// Starts a new file's queue parameters.
    fn clear_work_params(&self) {}

    /// Sets one queue parameter of the file being added.
    fn set_work_param(&self, _name: &[u8], _value: &[u8]) {}

    /// Sets the post-processing parameters from the file's category.
    fn set_work_category(&self, _category: &[u8]) {}

    /// Parses an NZB file. Returns whether it parsed and the category the
    /// file itself names (empty for none).
    fn parse_nzb(&self, _filename: &[u8], _category: &[u8]) -> (bool, Vec<u8>) {
        (false, Vec::new())
    }

    /// Adds the last parsed NZB to the queue. Returns its id, or 0.
    fn commit_nzb(&self, _nzb: &CommitNzb<'_>) -> c_int {
        0
    }
}

/// The settings [`ScannerHost::commit_nzb`] applies to a parsed NZB.
pub(crate) struct CommitNzb<'a> {
    /// Whether parsing and renaming succeeded.
    pub ok: bool,
    /// The file's name after renaming to `.queued` or `.error`.
    pub queued_filename: &'a [u8],
    /// The category auto-detection settled on, if any.
    pub auto_category: Option<&'a CStr>,
    pub nzb_name: &'a [u8],
    pub priority: c_int,
    pub dupe_key: &'a [u8],
    pub dupe_score: c_int,
    pub dupe_mode: c_int,
    pub add_top: bool,
    pub add_paused: bool,
    /// The URL queue entry this file came from (0 for none).
    pub url_info: usize,
}

#[derive(Clone)]
pub struct NzbgetRsScannerEnv {
    pub(crate) host: Option<Arc<dyn ScannerHost>>,
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
}

impl NzbgetRsScannerEnv {
    /// No host, null strings and zero settings.
    pub(crate) fn empty() -> Self {
        Self {
            host: None,
            nzb_dir: std::ptr::null(),
            temp_dir: std::ptr::null(),
            filename: std::ptr::null(),
            nzb_name: std::ptr::null(),
            category: std::ptr::null(),
            dupe_key: std::ptr::null(),
            buf_ptr: std::ptr::null(),
            buf_len: 0,
            priority: 0,
            dupe_score: 0,
            dupe_mode: 0,
            auto_category: 0,
            add_top: 0,
            add_paused: 0,
            nzb_dir_interval: 0,
            nzb_dir_file_age: 0,
            archive_scan: 0,
            archive_action: 0,
            queue_loaded: 0,
            pause_scan: 0,
            path_sep: 0,
            parameters: std::ptr::null_mut(),
            url_info: std::ptr::null_mut(),
            out_nzb_id: std::ptr::null_mut(),
        }
    }
}

unsafe impl Send for NzbgetRsScannerEnv {}
unsafe impl Sync for NzbgetRsScannerEnv {}

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
    /// The adding thread is in a `statesaver::DeferWaits` scope: the saves of this
    /// add go to it instead of being waited for here.
    handoff: Option<crate::statesaver::Handoff>,
}

struct ScannerInner {
    file_list: Vec<ScanFileData>,
    queue_list: Vec<ScanQueueData>,
    pass: i32,
    scan_script: bool,
    /// `.nzb` files dropped into NzbDir this pass, held until the pass ends
    /// so copies of one release go in together as one fleet.
    fleet_pending: Vec<PendingCopy>,
    /// Fleets started from NzbDir lately, by (category, DupeKey), with the
    /// time: a copy of the release that lands within [`FLEET_WINDOW_SEC`]
    /// joins its fleet.
    recent_fleets: Vec<(Vec<u8>, Vec<u8>, i64)>,
}

/// A copy of a release dropped into NzbDir, waiting for the end of the pass.
struct PendingCopy {
    base_filename: Vec<u8>,
    full_filename: Vec<u8>,
    category: Vec<u8>,
    key: Vec<u8>,
    /// When it was first held back for a sibling still waiting its
    /// NzbDirFileAge ([`single_waits_for_sibling`]); 0 if never.
    held_since: i64,
}

/// Copies of one release that land in NzbDir within this many seconds of a
/// fleet started from there join that fleet.
pub const FLEET_WINDOW_SEC: i64 = 10;
/// The time `appendfleet` may take to check the copies of a fleet from NzbDir.
const FLEET_TIMEOUT_SEC: c_int = 60;

/// The copies of `pending` that go in as one fleet, by index, grouped by
/// category and DupeKey (`dupes:<normalized name>`), in the order they were
/// found: groups of two or more, and any copy whose (category, key) is in
/// `recent`. The others are added on their own.
pub fn plan_fleets(
    pending: &[(Vec<u8>, Vec<u8>)],
    recent: &[(Vec<u8>, Vec<u8>)],
) -> (Vec<Vec<usize>>, Vec<usize>) {
    let mut groups: Vec<((Vec<u8>, Vec<u8>), Vec<usize>)> = Vec::new();
    for (i, (cat, key)) in pending.iter().enumerate() {
        match groups
            .iter_mut()
            .find(|((c, k), _)| c == cat && k.eq_ignore_ascii_case(key))
        {
            Some((_, members)) => members.push(i),
            None => groups.push(((cat.clone(), key.clone()), vec![i])),
        }
    }
    let mut fleets = Vec::new();
    let mut single = Vec::new();
    for ((cat, key), members) in groups {
        let joins_recent = recent
            .iter()
            .any(|(c, k)| *c == cat && k.eq_ignore_ascii_case(&key));
        if members.len() >= 2 || joins_recent {
            fleets.push(members);
        } else {
            single.extend(members);
        }
    }
    single.sort_unstable();
    (fleets, single)
}

/// Whether a copy alone in its pass waits for the next pass: another `.nzb` of
/// the same release (`key`) in the same directory was seen but is still
/// waiting its NzbDirFileAge. The scanner stamps a file when a pass first sees
/// it, in whole seconds, so files dropped together can become ready one pass
/// apart; without the wait the first is added alone and the rest form a fleet
/// without it. `waiting` lists the files seen and not ready yet.
pub fn single_waits_for_sibling(pending_full: &[u8], key: &[u8], waiting: &[&[u8]]) -> bool {
    let split = |f: &[u8]| f.iter().rposition(|&b| b == b'/' || b == b'\\').map_or(0, |i| i + 1);
    let cut = split(pending_full);
    let dir = &pending_full[..cut];
    waiting.iter().any(|&f| {
        if f == pending_full {
            return false;
        }
        let c = split(f);
        if &f[..c] != dir {
            return false;
        }
        let base = &f[c..];
        let is_nzb = base.len() > 4 && base[base.len() - 4..].eq_ignore_ascii_case(b".nzb");
        is_nzb && crate::releasename::group_key(base).eq_ignore_ascii_case(key)
    })
}

pub struct NzbgetRsScanner {
    inner: Mutex<ScannerInner>,
    host_env: Mutex<Option<NzbgetRsScannerEnv>>,
    requested_nzb_dir_scan: AtomicBool,
    scanning: AtomicBool,
    /// Notified when a pass ends, for a caller waiting for its file (`scan_nzb_dir`).
    scan_done: (Mutex<()>, std::sync::Condvar),
    nzb_dir_interval: AtomicI32,
    #[cfg(unix)]
    work_params: Mutex<Vec<(CString, CString)>>,
    #[cfg(unix)]
    active_parsed: Mutex<Option<crate::nzbfile::ParsedMaterializedNzb>>,
}

static ACTIVE_SCANNER: std::sync::OnceLock<Mutex<Option<Arc<NzbgetRsScanner>>>> =
    std::sync::OnceLock::new();

fn active_scanner_slot() -> &'static Mutex<Option<Arc<NzbgetRsScanner>>> {
    ACTIVE_SCANNER.get_or_init(|| Mutex::new(None))
}

pub(crate) fn register_active_scanner(scanner: Arc<NzbgetRsScanner>) {
    *active_scanner_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(scanner);
}

pub(crate) fn active_scanner() -> Option<Arc<NzbgetRsScanner>> {
    active_scanner_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

struct ScannerRuntimeOpts {
    nzb_dir: Vec<u8>,
    temp_dir: Vec<u8>,
    nzb_dir_interval: i32,
    nzb_dir_file_age: i32,
    archive_scan: bool,
    archive_action: i32,
    queue_loaded: bool,
    pause_scan: bool,
    sep: u8,
}

fn resolve_runtime_opts(env: &NzbgetRsScannerEnv) -> ScannerRuntimeOpts {
    if !env.nzb_dir.is_null() {
        return ScannerRuntimeOpts {
            nzb_dir: unsafe { cstr::bytes(env.nzb_dir) }.to_vec(),
            temp_dir: unsafe { cstr::bytes(env.temp_dir) }.to_vec(),
            nzb_dir_interval: env.nzb_dir_interval,
            nzb_dir_file_age: env.nzb_dir_file_age,
            archive_scan: env.archive_scan != 0,
            archive_action: env.archive_action,
            queue_loaded: env.queue_loaded != 0,
            pause_scan: env.pause_scan != 0,
            sep: if env.path_sep == 0 {
                b'/'
            } else {
                env.path_sep as u8
            },
        };
    }
    let opts = crate::options::active_options();
    ScannerRuntimeOpts {
        nzb_dir: opts
            .as_deref()
            .map_or_else(Vec::new, |o| o.str_bytes(OptStr::NzbDir).to_vec()),
        temp_dir: opts
            .as_deref()
            .map_or_else(Vec::new, |o| o.str_bytes(OptStr::TempDir).to_vec()),
        nzb_dir_interval: opts.as_deref().map_or(0, |o| o.scalars().nzb_dir_interval),
        nzb_dir_file_age: opts.as_deref().map_or(0, |o| o.scalars().nzb_dir_file_age),
        archive_scan: opts
            .as_deref()
            .is_some_and(|o| o.scalars().nzb_dir_archive_scan != 0),
        archive_action: opts
            .as_deref()
            .map_or(0, |o| o.scalars().nzb_dir_archive_action),
        queue_loaded: crate::ffi::is_queue_loaded(),
        pause_scan: crate::ffi::active_workstate().is_some_and(|ws| ws.pause_scan()),
        sep: crate::paths::PATH_SEPARATOR as u8,
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
    if env.nzb_dir.is_null() {
        #[cfg(unix)]
        {
            let active_kind = match kind {
                LOG_DEBUG => 3,
                LOG_DETAIL => 4,
                LOG_INFO => 0,
                LOG_WARN => 1,
                _ => 2,
            };
            crate::log::log_active_bytes(active_kind, msg);
        }
    }
}

fn join_sep(dir: &[u8], sep: u8, leaf: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dir.len() + 1 + leaf.len());
    out.extend_from_slice(dir);
    out.push(sep);
    out.extend_from_slice(leaf);
    out
}

fn find_matched_category(env: &NzbgetRsScannerEnv, cat_c: &CStr) -> Option<Vec<u8>> {
    if env.nzb_dir.is_null() {
        if let Some(opts) = crate::options::active_options() {
            if let Some(cat) = opts.find_category(cat_c, true) {
                return Some(cat.name.to_bytes().to_vec());
            }
        }
    }
    None
}

fn resolve_category(env: &NzbgetRsScannerEnv, category: &[u8], filename: &[u8]) -> Vec<u8> {
    let cat_c = to_cstr_sanitized(category);
    if let Some(matched) = find_matched_category(env, &cat_c) {
        if matched.as_slice() != category {
            let mut msg = b"Category '".to_vec();
            msg.extend_from_slice(category);
            msg.extend_from_slice(b"' matched to '");
            msg.extend_from_slice(&matched);
            msg.extend_from_slice(b"' for '");
            msg.extend_from_slice(filename);
            msg.push(b'\'');
            emit_log(env, LOG_DETAIL, &msg);
            return matched;
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

/// The last "Could not scan" warning about a scan folder: the same one is not
/// repeated on every scan (a NzbDir that does not exist yet), only after the
/// folder could be read again or the reason changed.
static LAST_SCAN_WARNING: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

/// True when `msg` should be logged as a warning: it differs from the last one.
fn scan_warning_is_new(msg: &[u8]) -> bool {
    let mut last = LAST_SCAN_WARNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if last.as_slice() == msg {
        return false;
    }
    last.clear();
    last.extend_from_slice(msg);
    true
}

fn clear_scan_warning() {
    LAST_SCAN_WARNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
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
        Ok(rd) => {
            clear_scan_warning();
            rd
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return archives,
        Err(e) => {
            let code = e.raw_os_error().unwrap_or(1);
            let err_bytes = crate::fsops::error_message(code);
            let mut msg = b"Could not scan ".to_vec();
            msg.extend_from_slice(crate::fsops::path_to_bytes(dir));
            msg.extend_from_slice(b" for archives: ");
            msg.extend_from_slice(&err_bytes);
            let kind = if scan_warning_is_new(&msg) { LOG_WARN } else { LOG_DEBUG };
            emit_log(env, kind, &msg);
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

    let (mut ok, detected_cat) = match &env.host {
        Some(host) => host.parse_nzb(
            to_cstr_sanitized(filename).to_bytes(),
            to_cstr_sanitized(category).to_bytes(),
        ),
        None => (false, Vec::new()),
    };
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
            if let Some(matched) = find_matched_category(env, &det_c) {
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
        if let Some(host) = &env.host {
            host.set_work_category(to_cstr_sanitized(&pp_cat).to_bytes());
        }
    }

    let nzb_id = match &env.host {
        Some(host) => host.commit_nzb(&CommitNzb {
            ok,
            queued_filename: to_cstr_sanitized(&bakname2).to_bytes(),
            auto_category: auto_resolved_cat.as_deref(),
            nzb_name: to_cstr_sanitized(nzb_name).to_bytes(),
            priority,
            dupe_key: to_cstr_sanitized(dupe_key).to_bytes(),
            dupe_score,
            dupe_mode,
            add_top,
            add_paused,
            url_info: url_info as usize,
        }),
        None => 0,
    };
    (ok, nzb_id)
}

/// The DupeKey a file dropped into NzbDir is grouped by, when the duplicate
/// check is on and the file is a plain `.nzb` whose name names a release.
fn fleet_key_for(base_filename: &[u8], extension: &[u8]) -> Option<Vec<u8>> {
    if !extension.eq_ignore_ascii_case(b".nzb") {
        return None;
    }
    let dupe_check =
        crate::options::active_options().is_some_and(|o| o.scalars().dupe_check != 0);
    if !dupe_check {
        return None;
    }
    let key = crate::releasename::group_key(base_filename);
    (!key.is_empty()).then_some(key)
}

/// One copy of a fleet reply, for the log: its name, nzb id, status and
/// measured alive share (percent, `-1` unverified).
pub type FleetCopy = (Vec<u8>, c_int, Vec<u8>, c_int);

/// Collects what `appendfleet` replies, for the log.
#[derive(Default)]
struct FleetReply {
    chosen: c_int,
    reason: Vec<u8>,
    added: Vec<FleetCopy>,
}

impl crate::xmlrpc_format::RpcEmit for FleetReply {
    fn fleet_result(
        &mut self,
        chosen: c_int,
        _complete: bool,
        reason: &CStr,
        members: &[crate::xmlrpc_format::NzbgetRsRpcFleetMember<'_>],
    ) {
        self.chosen = chosen;
        self.reason = reason.to_bytes().to_vec();
        self.added = members
            .iter()
            .map(|m| (m.name.to_bytes().to_vec(), m.nzb_id, m.status.to_bytes().to_vec(), m.alive))
            .collect();
    }
}

/// The hidden file beside `nzb` that holds the priority of a fleet member
/// written back to NzbDir (see [`crate::dupesearch_coord::keep_members_in`]):
/// `.<file name>.priority`. The scan skips hidden files.
pub fn priority_sidecar(nzb: &Path) -> PathBuf {
    let mut leaf = std::ffi::OsString::from(".");
    leaf.push(nzb.file_name().unwrap_or_default());
    leaf.push(".priority");
    nzb.with_file_name(leaf)
}

/// The priority kept for `nzb` ([`priority_sidecar`]), its file removed.
pub fn take_kept_priority(nzb: &Path) -> Option<c_int> {
    let side = priority_sidecar(nzb);
    let text = std::fs::read(&side).ok()?;
    let _ = std::fs::remove_file(&side);
    std::str::from_utf8(&text).ok()?.trim().parse().ok()
}

/// Puts `priority` back beside each of `files` for the next scan.
fn keep_priority(files: &[Vec<u8>], priority: c_int) {
    if priority != 0 {
        for f in files {
            let _ = std::fs::write(priority_sidecar(crate::fsops::path(f)), priority.to_string());
        }
    }
}

/// Longest line [`fleet_summary_lines`] writes: a log line is cut at 1023
/// bytes, which lost the outcome of the later copies of a big fleet.
pub const FLEET_SUMMARY_LINE_MAX: usize = 900;

/// The log lines summing up a fleet sent from NzbDir (B4): a first line with
/// what downloads and how many copies ended how, then the copies with their
/// outcome over as many lines as it takes, none over
/// [`FLEET_SUMMARY_LINE_MAX`] (a name longer than that alone is cut).
/// B2b: how the summary names a copy's outcome (its group in the counts, and
/// its word in the list), with the same honest wording as its parameters and
/// the web UI: a copy with any alive article is never DEAD (F3, F15). A
/// backup under the healthy share (97.5%) is a weak one, with its share; a
/// copy the fleet dropped while some of it is alive says so instead of DEAD.
pub fn fleet_copy_outcome(status: &[u8], alive: c_int) -> (Vec<u8>, Vec<u8>) {
    let healthy = crate::dupesearch_nzb::FLEET_HEALTHY_ALIVE_PERMILLE;
    if status == b"BACKUP" && alive >= 0 && alive * 10 < healthy {
        (b"BACKUP (weak)".to_vec(), format!("BACKUP (weak, {alive}% alive)").into_bytes())
    } else if (status == b"DEAD" || status == b"WEAK") && alive > 0 {
        (b"too weak".to_vec(), format!("too weak ({alive}% alive)").into_bytes())
    } else {
        (status.to_vec(), status.to_vec())
    }
}

pub fn fleet_summary_lines(
    key: &[u8],
    reason: &[u8],
    chosen: c_int,
    added: &[FleetCopy],
) -> Vec<Vec<u8>> {
    let mut head = b"Fleet ".to_vec();
    head.extend_from_slice(key);
    head.extend_from_slice(b" from NzbDir: ");
    let mut said = Vec::new();
    if chosen > 0 {
        said.push(format!("downloading #{chosen}").into_bytes());
    }
    if !reason.is_empty() {
        said.push(reason.to_vec());
    }
    if said.is_empty() {
        said.push(b"nothing queued".to_vec());
    }
    head.extend_from_slice(&said.join(&b", "[..]));
    let mut counts: Vec<(Vec<u8>, usize)> = Vec::new();
    for (_, _, status, alive) in added {
        let (group, _) = fleet_copy_outcome(status, *alive);
        match counts.iter_mut().find(|(s, _)| *s == group) {
            Some((_, n)) => *n += 1,
            None => counts.push((group, 1)),
        }
    }
    head.extend_from_slice(format!("; {} cop(ies)", added.len()).as_bytes());
    for (i, (status, n)) in counts.iter().enumerate() {
        head.extend_from_slice(if i == 0 { b": " } else { b", " });
        head.extend_from_slice(format!("{n} ").as_bytes());
        head.extend_from_slice(status);
    }
    truncate_utf8(&mut head, FLEET_SUMMARY_LINE_MAX);
    let mut lines = vec![head];
    let mut prefix = b"Fleet ".to_vec();
    prefix.extend_from_slice(key);
    prefix.extend_from_slice(b" from NzbDir, copies: ");
    let mut line = prefix.clone();
    for (name, id, status, alive) in added {
        let mut item = crate::releasename::log_name(name);
        if item.is_empty() {
            item.extend_from_slice(b"(no name)");
        }
        item.push(b' ');
        item.extend_from_slice(&fleet_copy_outcome(status, *alive).1);
        if *id > 0 {
            item.extend_from_slice(format!(" #{id}").as_bytes());
        }
        let sep = if line.len() > prefix.len() { 2 } else { 0 };
        if line.len() > prefix.len() && line.len() + sep + item.len() > FLEET_SUMMARY_LINE_MAX {
            lines.push(std::mem::replace(&mut line, prefix.clone()));
        }
        if line.len() > prefix.len() {
            line.extend_from_slice(b"; ");
        }
        line.extend_from_slice(&item);
        truncate_utf8(&mut line, FLEET_SUMMARY_LINE_MAX);
    }
    if line.len() > prefix.len() {
        lines.push(line);
    }
    lines
}

/// `v` cut to at most `max` bytes, never inside a UTF-8 sequence (names are
/// bytes and may not be UTF-8: only continuation bytes are backed over).
fn truncate_utf8(v: &mut Vec<u8>, max: usize) {
    if v.len() <= max {
        return;
    }
    let mut end = max;
    while end > 0 && (v[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    v.truncate(end);
}

/// Whether a fleet call from NzbDir did nothing with its copies, so they go
/// back for the next scan: nothing was added and the reply says the key was
/// busy, the program stops, or no member came through.
pub fn fleet_never_ran(reason: &[u8], any_added: bool) -> bool {
    !any_added && matches!(reason, b"KEY_BUSY" | b"SHUTDOWN" | b"NO_MEMBERS" | b"")
}

/// Sends copies dropped into NzbDir to `appendfleet` on a thread of its own
/// (it adds each copy back through this scanner, which must not wait for
/// itself). The file of each copy that was added is removed afterwards: the
/// fleet wrote its own copy into NzbDir.
fn spawn_nzbdir_fleet(
    key: Vec<u8>,
    category: Vec<u8>,
    priority: c_int,
    copies: Vec<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)>,
    env: NzbgetRsScannerEnv,
) {
    let env_for_spawn_failure = env.clone();
    let key_for_spawn_failure = key.clone();
    // counted like the donor walkers, so shutdown waits for it
    #[cfg(unix)]
    let walker = crate::dupesearch_nzb::DonorWalkerGuard::enter();
    // (renamed .queued file, original name): put back for the next scan when
    // the fleet never ran, so no copy is stranded
    let restore: Vec<(Vec<u8>, Vec<u8>)> = copies.iter().map(|c| (c.2.clone(), c.3.clone())).collect();
    let spawned = std::thread::Builder::new()
        .name("nzbdir-fleet".to_owned())
        .spawn(move || {
            #[cfg(unix)]
            let _walker = walker;
            #[cfg(unix)]
            {
                let key_c = to_cstr_sanitized(&key);
                let cat_c = to_cstr_sanitized(&category);
                let names: Vec<CString> = copies.iter().map(|c| to_cstr_sanitized(&c.0)).collect();
                let req: Vec<crate::xmlrpc_format::NzbgetRsRpcFleetReqMember> = copies
                    .iter()
                    .zip(&names)
                    .map(|(c, n)| crate::xmlrpc_format::NzbgetRsRpcFleetReqMember {
                        name: n.as_ptr(),
                        is_url: 0,
                        data: c.1.as_ptr().cast(),
                        data_len: c.1.len(),
                    })
                    .collect();
                let mut reply = FleetReply::default();
                unsafe {
                    crate::dupesearch_coord::fleet_append(
                        std::ptr::null_mut(),
                        crate::dupesearch_coord::native_dupe_host,
                        key_c.as_ptr(),
                        cat_c.as_ptr(),
                        priority,
                        FLEET_TIMEOUT_SEC,
                        req.as_ptr(),
                        req.len(),
                        &mut reply,
                    );
                }
                for (name, id, _, _) in &reply.added {
                    if *id > 0 {
                        if let Some(c) = copies.iter().find(|c| c.0 == *name) {
                            let _ = std::fs::remove_file(crate::fsops::path(&c.2));
                        }
                    }
                }
                if fleet_never_ran(&reply.reason, reply.added.iter().any(|(_, id, _, _)| *id > 0)) {
                    // the key was busy or the program stops: the next scan
                    // sends these copies again
                    for c in &copies {
                        if let Err(err) = std::fs::rename(crate::fsops::path(&c.2), crate::fsops::path(&c.3)) {
                            let mut msg = b"Could not put ".to_vec();
                            msg.extend_from_slice(&crate::releasename::log_name(&c.2));
                            msg.extend_from_slice(format!(" back for the next scan: {err}").as_bytes());
                            emit_log(&env, LOG_WARN, &msg);
                        }
                    }
                    keep_priority(&copies.iter().map(|c| c.3.clone()).collect::<Vec<_>>(), priority);
                }
                for msg in fleet_summary_lines(&key, &reply.reason, reply.chosen, &reply.added) {
                    emit_log(&env, LOG_INFO, &msg);
                }
            }
            #[cfg(not(unix))]
            {
                let _ = (key, category, priority, copies, env);
            }
        });
    if let Err(err) = spawned {
        let mut msg = b"Fleet ".to_vec();
        msg.extend_from_slice(&key_for_spawn_failure);
        msg.extend_from_slice(
            format!(" from NzbDir: its check could not start ({err}), its copies stay for the next scan").as_bytes(),
        );
        emit_log(&env_for_spawn_failure, LOG_WARN, &msg);
        for (queued, original) in &restore {
            if let Err(err) = std::fs::rename(crate::fsops::path(queued), crate::fsops::path(original)) {
                let mut msg = b"Could not put ".to_vec();
                msg.extend_from_slice(&crate::releasename::log_name(queued));
                msg.extend_from_slice(format!(" back for the next scan: {err}").as_bytes());
                emit_log(&env_for_spawn_failure, LOG_WARN, &msg);
            }
        }
        keep_priority(&restore.iter().map(|r| r.1.clone()).collect::<Vec<_>>(), priority);
    }
}

impl NzbgetRsScanner {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ScannerInner {
                file_list: Vec::new(),
                queue_list: Vec::new(),
                pass: 0,
                scan_script: false,
                fleet_pending: Vec::new(),
                recent_fleets: Vec::new(),
            }),
            host_env: Mutex::new(None),
            requested_nzb_dir_scan: AtomicBool::new(false),
            scanning: AtomicBool::new(false),
            scan_done: (Mutex::new(()), std::sync::Condvar::new()),
            nzb_dir_interval: AtomicI32::new(0),
            #[cfg(unix)]
            work_params: Mutex::new(Vec::new()),
            #[cfg(unix)]
            active_parsed: Mutex::new(None),
        }
    }

    /// The env the daemon stored for this scanner.
    fn stored_env(&self) -> Option<NzbgetRsScannerEnv> {
        self.host_env.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn init_options(&self, has_scan_script: bool, env: &NzbgetRsScannerEnv) {
        self.nzb_dir_interval.store(1, Ordering::SeqCst);
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.scan_script = has_scan_script;
        }
        let rt = resolve_runtime_opts(env);
        cleanup_stale_unpack_dir(&rt.temp_dir, env);
    }

    pub(crate) fn init_options_active(&self) {
        let env = self
            .stored_env()
            .unwrap_or_else(NzbgetRsScannerEnv::empty);
        self.init_options(false, &env);
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

    pub(crate) fn service_interval_active(&self) -> i32 {
        let opt_iv = crate::options::active_options().map_or(0, |o| o.scalars().nzb_dir_interval);
        self.service_interval(opt_iv)
    }

    fn scan_nzb_dir(&self, sync_mode: bool, env: &NzbgetRsScannerEnv) {
        {
            let _guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            self.scanning.store(true, Ordering::SeqCst);
            self.requested_nzb_dir_scan.store(true, Ordering::SeqCst);
            let woken = env.host.as_ref().is_some_and(|h| h.wake());
            if !woken && env.nzb_dir.is_null() {
                crate::scheduler::wake_active_service_coord();
            }
        }
        // woken when the pass ends (a fleet adds its members one after another: a
        // fixed 100 ms nap per member added seconds); the timeout stays a fallback
        let (lock, cv) = &self.scan_done;
        let mut g = lock.lock().unwrap_or_else(|e| e.into_inner());
        while sync_mode
            && (self.scanning.load(Ordering::SeqCst)
                || self.requested_nzb_dir_scan.load(Ordering::SeqCst))
        {
            g = cv
                .wait_timeout(g, Duration::from_millis(100))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    pub(crate) fn scan_nzb_dir_active(&self, sync_mode: bool) {
        let env = self
            .stored_env()
            .unwrap_or_else(NzbgetRsScannerEnv::empty);
        self.scan_nzb_dir(sync_mode, &env);
    }

    pub(crate) fn service_work_active(&self) {
        let env = self
            .stored_env()
            .unwrap_or_else(NzbgetRsScannerEnv::empty);
        self.service_work(&env);
    }

    fn service_work(&self, env: &NzbgetRsScannerEnv) {
        emit_log(env, LOG_DEBUG, b"Scanner service work");

        let rt = resolve_runtime_opts(env);
        if !rt.queue_loaded {
            return;
        }

        self.nzb_dir_interval
            .store(rt.nzb_dir_interval, Ordering::SeqCst);

        if rt.pause_scan && !self.requested_nzb_dir_scan.load(Ordering::SeqCst) {
            return;
        }

        emit_log(env, LOG_DEBUG, b"Scanner service work: doing work");

        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        let check_stat = !self.requested_nzb_dir_scan.swap(false, Ordering::SeqCst);
        self.scanning.store(true, Ordering::SeqCst);

        let nzb_dir = rt.nzb_dir.as_slice();
        let temp_dir = rt.temp_dir.as_slice();
        let sep = rt.sep;

        if rt.archive_scan {
            let nzb_dir_path = crate::fsops::path(nzb_dir);
            let archives = find_archives(
                nzb_dir_path,
                check_stat,
                rt.nzb_dir_file_age,
                &mut inner.file_list,
                env,
            );
            for archive in &archives {
                let parent = archive.parent().unwrap_or(nzb_dir_path);
                let _ = process_archive(archive, parent, temp_dir, nzb_dir, rt.archive_action, env);
            }
        }

        Self::check_incoming_nzbs(
            &mut inner,
            nzb_dir,
            b"",
            check_stat,
            sep,
            rt.nzb_dir_file_age,
            env,
            0,
        );
        if !check_stat && inner.scan_script {
            Self::check_incoming_nzbs(
                &mut inner,
                nzb_dir,
                b"",
                check_stat,
                sep,
                rt.nzb_dir_file_age,
                env,
                0,
            );
        }

        Self::flush_fleet_pending(&mut inner, env);

        self.scanning.store(false, Ordering::SeqCst);
        {
            let (lock, cv) = &self.scan_done;
            let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
            cv.notify_all();
        }

        let (next_iv, next_p) = next_pass(
            rt.nzb_dir_interval,
            rt.nzb_dir_file_age,
            inner.scan_script,
            inner.pass,
        );
        inner.pass = next_p;
        self.nzb_dir_interval.store(next_iv, Ordering::SeqCst);

        drop_old_files(
            &mut inner.file_list,
            rt.nzb_dir_interval,
            rt.nzb_dir_file_age,
            env,
        );
        inner.queue_list.clear();
    }

    #[allow(clippy::too_many_arguments)]
    fn check_incoming_nzbs(
        inner: &mut ScannerInner,
        directory: &[u8],
        category: &[u8],
        check_stat: bool,
        sep: u8,
        file_age: i32,
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
            let is_dir = crate::fsops::directory_exists(&full_filename, &crate::fsops::posix_stat);
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
                    file_age,
                    env,
                    depth + 1,
                );
            } else if can_process_file(&mut inner.file_list, &full_filename, check_stat, file_age) {
                Self::process_incoming_file(
                    inner,
                    &filename,
                    &full_filename,
                    category,
                    env,
                    true,
                );
            }
        }
    }

    fn process_incoming_file(
        inner: &mut ScannerInner,
        base_filename: &[u8],
        full_filename: &[u8],
        category: &[u8],
        env: &NzbgetRsScannerEnv,
        defer_fleet: bool,
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

        let matched_idx = inner
            .queue_list
            .iter()
            .rposition(|q| q.filename == full_filename);

        // A file dropped into NzbDir (not one the API wrote there) waits for the
        // end of the pass, so copies of one release go in together as a fleet.
        if defer_fleet && matched_idx.is_none() && !inner.scan_script {
            if let Some(key) = fleet_key_for(base_filename, extension) {
                if !inner.fleet_pending.iter().any(|p| p.full_filename == full_filename) {
                    inner.fleet_pending.push(PendingCopy {
                        base_filename: base_filename.to_vec(),
                        full_filename: full_filename.to_vec(),
                        category: category.to_vec(),
                        key,
                        held_since: 0,
                    });
                }
                return;
            }
        }

        // an add whose caller collects its saves: they go to it, not waited for here
        let _defer = matched_idx
            .and_then(|idx| inner.queue_list[idx].handoff.clone())
            .map(crate::statesaver::DeferWaits::handing_to);
        if let Some(host) = &env.host {
            host.clear_work_params();
        }
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
            if let Some(host) = &env.host {
                for (pname, pval) in &q.params {
                    host.set_work_param(pname.to_bytes(), pval.to_bytes());
                }
            }
        }

        if matched_idx.is_none() {
            // a fleet member written back to NzbDir keeps its priority
            if let Some(kept) = take_kept_priority(crate::fsops::path(full_filename)) {
                priority = kept;
            }
        }
        if let Some(host) = &env.host {
            host.set_work_category(to_cstr_sanitized(&nzb_category).to_bytes());
        }

        let mut exists = true;
        let ran_scan_script =
            inner.scan_script && !extension.eq_ignore_ascii_case(b".nzb_processed");
        if ran_scan_script {
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

    /// Adds the files dropped into NzbDir this pass ([`ScannerInner::fleet_pending`]):
    /// copies of one release (the same category and normalized name, see
    /// [`plan_fleets`]) go in as one fleet through `appendfleet`, which checks
    /// every copy, queues the best and keeps the rest as labelled backups; the
    /// others are added on their own as before.
    fn flush_fleet_pending(inner: &mut ScannerInner, env: &NzbgetRsScannerEnv) {
        let now = now_unix_secs();
        inner
            .recent_fleets
            .retain(|(_, _, t)| now - *t <= FLEET_WINDOW_SEC && *t <= now);
        let pending = std::mem::take(&mut inner.fleet_pending);
        if pending.is_empty() {
            return;
        }
        let keys: Vec<(Vec<u8>, Vec<u8>)> = pending
            .iter()
            .map(|p| (p.category.clone(), p.key.clone()))
            .collect();
        let recent: Vec<(Vec<u8>, Vec<u8>)> = inner
            .recent_fleets
            .iter()
            .map(|(c, k, _)| (c.clone(), k.clone()))
            .collect();
        let (fleets, single) = plan_fleets(&keys, &recent);
        // a copy alone in its pass waits (up to FLEET_WINDOW_SEC) while a file
        // of its release next to it still waits its NzbDirFileAge
        let waiting: Vec<Vec<u8>> = inner
            .file_list
            .iter()
            .filter(|f| std::fs::metadata(crate::fsops::path(&f.filename)).is_ok_and(|m| m.is_file()))
            .map(|f| f.filename.clone())
            .collect();
        let waiting_refs: Vec<&[u8]> = waiting.iter().map(Vec::as_slice).collect();
        let mut held: Vec<PendingCopy> = Vec::new();
        for i in single {
            let p = &pending[i];
            let within = p.held_since == 0 || (now >= p.held_since && now - p.held_since <= FLEET_WINDOW_SEC);
            if within && single_waits_for_sibling(&p.full_filename, &p.key, &waiting_refs) {
                held.push(PendingCopy {
                    base_filename: p.base_filename.clone(),
                    full_filename: p.full_filename.clone(),
                    category: p.category.clone(),
                    key: p.key.clone(),
                    held_since: if p.held_since == 0 { now } else { p.held_since },
                });
                continue;
            }
            Self::process_incoming_file(
                inner,
                &p.base_filename,
                &p.full_filename,
                &p.category,
                env,
                false,
            );
        }
        for mut members in fleets {
            // the shortest name first (the copy without a suffix), then by name:
            // the order a directory lists its files in differs between systems
            members.sort_by(|&a, &b| {
                let (x, y) = (&pending[a].base_filename, &pending[b].base_filename);
                x.len().cmp(&y.len()).then_with(|| x.cmp(y))
            });
            let first = &pending[members[0]];
            let (category, key) = (first.category.clone(), first.key.clone());
            let mut copies: Vec<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> = Vec::new();
            // the highest priority kept for a member written back to NzbDir
            let mut priority: Option<c_int> = None;
            for &i in &members {
                let p = &pending[i];
                let Ok(data) = std::fs::read(crate::fsops::path(&p.full_filename)) else {
                    continue;
                };
                if let Some(kept) = take_kept_priority(crate::fsops::path(&p.full_filename)) {
                    priority = Some(priority.map_or(kept, |p| p.max(kept)));
                }
                // renamed as any file it adds is, so the next pass leaves it be
                let mut queued_name = Vec::new();
                let renamed = crate::fsdelete::rename_bak(
                    &mut crate::fsops::PosixFsOps,
                    &p.full_filename,
                    b"queued",
                    false,
                    &mut queued_name,
                )
                .unwrap_or(false);
                if !renamed {
                    continue;
                }
                copies.push((p.base_filename.clone(), data, queued_name, p.full_filename.clone()));
            }
            if copies.is_empty() {
                continue;
            }
            inner.recent_fleets.retain(|(c, k, _)| !(*c == category && k.eq_ignore_ascii_case(&key)));
            inner.recent_fleets.push((category.clone(), key.clone(), now));
            let mut msg = format!("Adding {} cop(ies) of ", copies.len()).into_bytes();
            msg.extend_from_slice(&key[crate::releasename::GROUP_KEY_PREFIX.len().min(key.len())..]);
            msg.extend_from_slice(b" from NzbDir as one fleet under DupeKey ");
            msg.extend_from_slice(&key);
            emit_log(env, LOG_INFO, &msg);
            let use_cat = resolve_category(env, &category, &copies[0].0);
            spawn_nzbdir_fleet(key, use_cat, priority.unwrap_or(0), copies, env.clone());
        }
        inner.fleet_pending.extend(held);
    }

    fn add_archive_bytes_with_params(
        &self,
        buf: &[u8],
        env: &NzbgetRsScannerEnv,
        captured_params: Vec<(CString, CString)>,
    ) -> c_int {
        let filename = unsafe { cstr::bytes(env.filename) };
        if filename.is_empty() {
            emit_log(
                env,
                LOG_ERROR,
                b"Failed to add the archive to the download queue: Archive filename cannot be empty",
            );
            return AS_FAILED;
        }
        let rt = resolve_runtime_opts(env);
        let temp_dir = rt.temp_dir.as_slice();
        if temp_dir.is_empty() {
            let mut msg = b"Failed to create file '".to_vec();
            msg.extend_from_slice(filename);
            msg.extend_from_slice(b"': 'TempDir' is required and cannot be empty");
            emit_log(env, LOG_ERROR, &msg);
            return AS_FAILED;
        }
        let nzb_dir = rt.nzb_dir.as_slice();
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
                    let err_bytes = crate::fsops::error_message(e.raw_os_error().unwrap_or(1));
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

        if !rt.archive_scan {
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

        let category_bytes = unsafe { cstr::bytes(env.category) };
        let dupe_key_bytes = unsafe { cstr::bytes(env.dupe_key) };
        let slots: Vec<ResultSlot>;
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let Some(nzb_files) = process_archive(
                &archive_file_path,
                crate::fsops::path(nzb_dir),
                temp_dir,
                nzb_dir,
                rt.archive_action,
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
                    url_info: if index == 0 { env.url_info as usize } else { 0 },
                    result_slot: Some(slots[index].clone()),
                    handoff: None,
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

    fn add_archive_tmp_file_with_params(
        &self,
        env: &NzbgetRsScannerEnv,
        captured_params: Vec<(CString, CString)>,
    ) -> c_int {
        let tmp_bytes = unsafe { cstr::bytes(env.nzb_name) };
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
        self.add_archive_bytes_with_params(&buf, env, captured_params)
    }

    #[cfg(test)]
    fn add_external_file(&self, env: &NzbgetRsScannerEnv) -> c_int {
        self.add_external_with_params(env, Vec::new())
    }

    fn add_external_with_params(
        &self,
        env: &NzbgetRsScannerEnv,
        captured_params: Vec<(CString, CString)>,
    ) -> c_int {
        let rt = resolve_runtime_opts(env);
        let sep = rt.sep;
        let nzb_dir = rt.nzb_dir.as_slice();
        let temp_dir = rt.temp_dir.as_slice();
        let nzb_name = unsafe { cstr::bytes(env.nzb_name) };
        let category = unsafe { cstr::bytes(env.category) };
        let dupe_key = unsafe { cstr::bytes(env.dupe_key) };
        let has_disk_file = !env.filename.is_null();
        let buf_slice: &[u8] = if env.buf_ptr.is_null() || env.buf_len <= 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(env.buf_ptr.cast::<u8>(), env.buf_len as usize) }
        };
        let slot: ResultSlot = Arc::new(Mutex::new((AS_SKIPPED, None)));
        let handoff: Option<crate::statesaver::Handoff> =
            crate::statesaver::deferring().then(|| Arc::new(Mutex::new(Vec::new())));

        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let (temp_file_name, is_nzb) = if has_disk_file {
                (unsafe { cstr::bytes(env.filename) }.to_vec(), false)
            } else {
                let mut num = SCANNER_ID_GEN
                    .fetch_add(1, Ordering::Relaxed)
                    .wrapping_add(1);
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
                handoff: handoff.clone(),
            });
            self.requested_nzb_dir_scan.store(true, Ordering::SeqCst);
        }

        self.scan_nzb_dir(true, env);
        if let Some(h) = handoff {
            let tickets = std::mem::take(&mut *h.lock().unwrap_or_else(|e| e.into_inner()));
            crate::statesaver::adopt(tickets);
        }

        let (status, nzb_id_opt) = *slot.lock().unwrap_or_else(|e| e.into_inner());
        if !env.out_nzb_id.is_null() {
            if let Some(id) = nzb_id_opt {
                unsafe { *env.out_nzb_id = id };
            }
        }
        status
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_external_bytes_active(
        &self,
        nzb_name: &[u8],
        category: &[u8],
        auto_category: bool,
        priority: c_int,
        dupe_key: &[u8],
        dupe_score: c_int,
        dupe_mode: c_int,
        params: &[(&[u8], &[u8])],
        add_top: bool,
        add_paused: bool,
        data: &[u8],
    ) -> (c_int, c_int) {
        let Some(mut env) = self.stored_env() else {
            return (AS_FAILED, 0);
        };
        let name_c = to_cstr_sanitized(nzb_name);
        let cat_c = to_cstr_sanitized(category);
        let dk_c = to_cstr_sanitized(dupe_key);
        let mut out_id: c_int = -1;
        env.filename = std::ptr::null();
        env.nzb_name = name_c.as_ptr();
        env.category = cat_c.as_ptr();
        env.dupe_key = dk_c.as_ptr();
        env.priority = priority;
        env.dupe_score = dupe_score;
        env.dupe_mode = dupe_mode;
        env.auto_category = c_int::from(auto_category);
        env.add_top = c_int::from(add_top);
        env.add_paused = c_int::from(add_paused);
        env.parameters = std::ptr::null_mut();
        env.url_info = std::ptr::null_mut();
        env.buf_ptr = if data.is_empty() {
            std::ptr::null()
        } else {
            data.as_ptr().cast()
        };
        env.buf_len = data.len() as c_int;
        env.out_nzb_id = &mut out_id;
        let captured_params: Vec<(CString, CString)> = params
            .iter()
            .map(|&(k, v)| (to_cstr_sanitized(k), to_cstr_sanitized(v)))
            .collect();
        let st = self.add_external_with_params(&env, captured_params);
        (st, out_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_archive_bytes_active(
        &self,
        filename: &[u8],
        category: &[u8],
        auto_category: bool,
        priority: c_int,
        dupe_key: &[u8],
        dupe_score: c_int,
        dupe_mode: c_int,
        params: &[(&[u8], &[u8])],
        add_top: bool,
        add_paused: bool,
        data: &[u8],
    ) -> c_int {
        let Some(mut env) = self.stored_env() else {
            return AS_FAILED;
        };
        let fn_c = to_cstr_sanitized(filename);
        let cat_c = to_cstr_sanitized(category);
        let dk_c = to_cstr_sanitized(dupe_key);
        env.filename = fn_c.as_ptr();
        env.nzb_name = std::ptr::null();
        env.category = cat_c.as_ptr();
        env.dupe_key = dk_c.as_ptr();
        env.priority = priority;
        env.dupe_score = dupe_score;
        env.dupe_mode = dupe_mode;
        env.auto_category = c_int::from(auto_category);
        env.add_top = c_int::from(add_top);
        env.add_paused = c_int::from(add_paused);
        env.parameters = std::ptr::null_mut();
        env.url_info = std::ptr::null_mut();
        env.buf_ptr = if data.is_empty() {
            std::ptr::null()
        } else {
            data.as_ptr().cast()
        };
        env.buf_len = data.len() as c_int;
        env.out_nzb_id = std::ptr::null_mut();
        let captured_params: Vec<(CString, CString)> = params
            .iter()
            .map(|&(k, v)| (to_cstr_sanitized(k), to_cstr_sanitized(v)))
            .collect();
        self.add_archive_bytes_with_params(data, &env, captured_params)
    }

    #[cfg(unix)]
    fn init_work_params_pp(&self, category: &[u8]) {
        let mut wp = self.work_params.lock().unwrap_or_else(|e| e.into_inner());
        if !wp
            .iter()
            .any(|(k, _)| eq_ignore_ascii_case(k.as_bytes(), b"*Unpack:"))
        {
            let v: &[u8] = if category_unpack_enabled(category) {
                b"yes"
            } else {
                b"no"
            };
            set_param_list(&mut wp, b"*Unpack:", Some(v));
        }
    }
}

impl Default for NzbgetRsScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether unpacking is enabled for `category` (the category's `Unpack` option, or
/// the global `Unpack` option when the category is empty or unknown). Used to
/// default the `*Unpack:` post-processing parameter of new downloads.
#[cfg(unix)]
fn category_unpack_enabled(category: &[u8]) -> bool {
    let Some(opts) = crate::options::active_options() else {
        return false;
    };
    let mut unpack = opts.scalars().unpack != 0;
    if !category.is_empty() {
        if let Ok(cat_c) = CString::new(category) {
            if let Some(cat) = opts.find_category(&cat_c, false) {
                unpack = cat.unpack;
            }
        }
    }
    unpack
}

#[cfg(unix)]
fn set_param_list(list: &mut Vec<(CString, CString)>, name: &[u8], val: Option<&[u8]>) {
    let empty_val = val.is_none_or(|v| v.is_empty());
    let pos = list
        .iter()
        .position(|(k, _)| eq_ignore_ascii_case(k.as_bytes(), name));
    if empty_val {
        if let Some(i) = pos {
            list.remove(i);
        }
    } else if let Some(v) = val {
        let vc = to_cstr_sanitized(v);
        if let Some(i) = pos {
            list[i].1 = vc;
        } else {
            list.push((to_cstr_sanitized(name), vc));
        }
    }
}

/// The daemon's [`ScannerHost`]: parses and queues through the queue host,
/// keeping the work parameters and the parsed NZB on its scanner.
#[cfg(unix)]
struct NativeScannerHost(std::sync::Weak<NzbgetRsScanner>);

#[cfg(unix)]
impl ScannerHost for NativeScannerHost {
    fn clear_work_params(&self) {
        if let Some(sc) = self.0.upgrade() {
            sc.work_params.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
    }

    fn set_work_param(&self, name: &[u8], value: &[u8]) {
        if let Some(sc) = self.0.upgrade() {
            let mut wp = sc.work_params.lock().unwrap_or_else(|e| e.into_inner());
            set_param_list(&mut wp, name, Some(value));
        }
    }

    fn set_work_category(&self, category: &[u8]) {
        if let Some(sc) = self.0.upgrade() {
            sc.init_work_params_pp(category);
        }
    }

    fn parse_nzb(&self, filename: &[u8], category: &[u8]) -> (bool, Vec<u8>) {
        match self.0.upgrade() {
            Some(sc) => native_parse_nzb(&sc, filename, category),
            None => (false, Vec::new()),
        }
    }

    fn commit_nzb(&self, nzb: &CommitNzb<'_>) -> c_int {
        match self.0.upgrade() {
            Some(sc) => native_commit_nzb(&sc, nzb),
            None => 0,
        }
    }
}

#[cfg(unix)]
fn native_parse_nzb(scanner: &NzbgetRsScanner, filename: &[u8], category: &[u8]) -> (bool, Vec<u8>) {
    let Some(host) = crate::ffi::queue_host() else {
        return (false, Vec::new());
    };
    if let Some(prev) = scanner
        .active_parsed
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        host.destroy_nzb_uptr(prev.uptr_addr);
    }
    let parsed = crate::nzbfile::parse_and_materialize_nzb(&host, filename, category);
    let ok = parsed.ok;
    let detected = to_cstr_sanitized(&parsed.category_from_file).to_bytes().to_vec();
    *scanner
        .active_parsed
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(parsed);
    (ok, detected)
}

#[cfg(unix)]
fn native_commit_nzb(scanner: &NzbgetRsScanner, nzb: &CommitNzb<'_>) -> c_int {
    use crate::queuesubsys_host::{
        FILE_INT_PAUSED, MK_DETAIL, MK_WARNING, NZB_ACT_BUILD_DEST_DIR, NZB_INT_DESIRED_SERVER_ID,
        NZB_INT_DUPE_HINT, NZB_INT_DUPE_MODE, NZB_INT_DUPE_SCORE, NZB_INT_FEED_ID,
        NZB_INT_PRIORITY, NZB_INT_SKIP_DISK_WRITE, NZB_INT_SKIP_SCRIPT_PROCESSING,
        NZB_STR_CATEGORY, NZB_STR_DUPE_KEY, NZB_STR_FILENAME, NZB_STR_NAME,
        NZB_STR_QUEUED_FILENAME, NZB_STR_URL,
    };
    let Some(host) = crate::ffi::queue_host() else {
        return 0;
    };
    let Some(parsed) = scanner
        .active_parsed
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    else {
        return 0;
    };
    struct UptrGuard<'a>(&'a crate::queuesubsys_host::QueueSubsysHost, usize);
    impl Drop for UptrGuard<'_> {
        fn drop(&mut self) {
            self.0.destroy_nzb_uptr(self.1);
        }
    }
    let _uptr_guard = UptrGuard(&host, parsed.uptr_addr);
    let nzb_ptr = parsed.nzb_ptr;
    if nzb_ptr.is_none() {
        return 0;
    }

    host.nzb_set_str(nzb_ptr, NZB_STR_QUEUED_FILENAME, nzb.queued_filename);
    if let Some(auto_cat) = nzb.auto_category {
        host.nzb_set_str(nzb_ptr, NZB_STR_CATEGORY, auto_cat.to_bytes());
    }
    let name_bytes = nzb.nzb_name;
    if !name_bytes.is_empty() {
        host.nzb_set_str_opt(nzb_ptr, NZB_STR_NAME, None);
        host.nzb_set_str(nzb_ptr, NZB_STR_FILENAME, name_bytes);
        host.nzb_action(nzb_ptr, NZB_ACT_BUILD_DEST_DIR);
    }
    if nzb.ok {
        let cur_name = host.nzb_snap(nzb_ptr, false).name;
        for mf in &parsed.files {
            let sev = classify_missed_articles(mf.missed_articles, mf.total_articles, mf.cut_short);
            if sev > 0 {
                let fs = host.file_snap(mf.file_ptr);
                let mut msg = b"Collection ".to_vec();
                msg.extend_from_slice(&cur_name);
                msg.extend_from_slice(
                    format!(
                        " lists only {} of the {} articles of ",
                        mf.total_articles - mf.missed_articles,
                        mf.total_articles
                    )
                    .as_bytes(),
                );
                msg.extend_from_slice(&fs.filename);
                msg.extend_from_slice(b": the missing ones count as failed");
                host.nzb_print_message(
                    nzb_ptr,
                    if sev == 2 { MK_WARNING } else { MK_DETAIL },
                    &msg,
                );
            }
        }
    }
    host.nzb_set_str(nzb_ptr, NZB_STR_DUPE_KEY, nzb.dupe_key);
    host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_SCORE, nzb.dupe_score);
    host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_MODE, nzb.dupe_mode);
    host.nzb_set_int(nzb_ptr, NZB_INT_PRIORITY, nzb.priority);
    let url_ptr = NzbRef::from_addr(nzb.url_info);
    if url_ptr.is_some() {
        let us = host.nzb_snap(url_ptr, false);
        host.nzb_set_str(nzb_ptr, NZB_STR_URL, &us.url);
        host.nzb_set_url_status(nzb_ptr, us.url_status);
        host.nzb_set_int(nzb_ptr, NZB_INT_FEED_ID, us.feed_id);
        host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_HINT, us.dupe_hint);
        host.nzb_set_int(nzb_ptr, NZB_INT_DESIRED_SERVER_ID, us.desired_server_id);
        host.nzb_set_int(
            nzb_ptr,
            NZB_INT_SKIP_SCRIPT_PROCESSING,
            i32::from(us.skip_script_processing),
        );
        host.nzb_set_int(
            nzb_ptr,
            NZB_INT_SKIP_DISK_WRITE,
            i32::from(us.skip_disk_write),
        );
    }
    if !parsed.password.is_empty() {
        host.nzb_set_param(nzb_ptr, b"*Unpack:Password", Some(&parsed.password));
    }
    let wp = scanner
        .work_params
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    for (k, v) in &wp {
        host.nzb_set_param(nzb_ptr, k.as_bytes(), Some(v.as_bytes()));
    }
    let paused_val = i32::from(nzb.add_paused);
    for mf in &parsed.files {
        host.file_set_int(mf.file_ptr, FILE_INT_PAUSED, paused_val);
    }

    let added = if nzb.ok {
        crate::ffi::active_queue_coord_and_host().map_or(NzbRef::NONE, |(qc, qh)| {
            qc.add_nzb_file_to_queue(&qh, parsed.uptr_addr, url_ptr, nzb.add_top)
        })
    } else if url_ptr.is_some() {
        host.nzb_copy_cached_messages_silent(url_ptr, nzb_ptr);
        NzbRef::NONE
    } else {
        host.nzb_set_delete_status(nzb_ptr, DeleteStatus::Scan);
        crate::ffi::active_queue_coord_and_host().map_or(NzbRef::NONE, |(qc, qh)| {
            qc.add_nzb_file_to_queue(&qh, parsed.uptr_addr, url_ptr, nzb.add_top)
        })
    };
    if added.is_some() {
        host.nzb_snap(added, false).id
    } else {
        0
    }
}

#[cfg(unix)]
pub(crate) fn create_daemon_scanner() {
    let scanner = Arc::new(NzbgetRsScanner::new());
    let mut env = NzbgetRsScannerEnv::empty();
    env.host = Some(Arc::new(NativeScannerHost(Arc::downgrade(&scanner))));
    *scanner.host_env.lock().unwrap_or_else(|e| e.into_inner()) = Some(env);
    register_active_scanner(scanner);
}

#[cfg(unix)]
pub(crate) fn shutdown_daemon_scanner() {
    let taken = active_scanner_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(sc) = taken {
        if let Some(prev) = sc
            .active_parsed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            if let Some(host) = crate::ffi::queue_host() {
                host.destroy_nzb_uptr(prev.uptr_addr);
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn add_url_file_active(
    url_ptr: NzbRef,
    is_archive: bool,
    eff_name: &[u8],
    eff_cat: &[u8],
    out_file: &[u8],
) -> i32 {
    let (Some(sc), Some(host)) = (active_scanner(), crate::ffi::queue_host()) else {
        return AS_FAILED;
    };
    let Some(mut env) = sc.stored_env() else {
        return AS_FAILED;
    };
    let snap = host.nzb_snap(url_ptr, false);
    let captured_params: Vec<_> = host
        .nzb_params(url_ptr)
        .iter()
        .map(|(k, v)| (to_cstr_sanitized(k), to_cstr_sanitized(v)))
        .collect();
    let name_c = to_cstr_sanitized(eff_name);
    let cat_c = to_cstr_sanitized(eff_cat);
    let out_c = to_cstr_sanitized(out_file);
    let dk_c = to_cstr_sanitized(&snap.dupe_key);
    env.category = cat_c.as_ptr();
    env.dupe_key = dk_c.as_ptr();
    env.priority = snap.priority;
    env.dupe_score = snap.dupe_score;
    env.dupe_mode = snap.dupe_mode.into();
    env.auto_category = c_int::from(snap.auto_category);
    env.add_top = 0;
    env.add_paused = c_int::from(snap.add_url_paused);
    env.parameters = std::ptr::null_mut();
    env.url_info = url_ptr.addr() as *mut c_void;
    env.buf_ptr = std::ptr::null();
    env.buf_len = 0;
    env.out_nzb_id = std::ptr::null_mut();
    if is_archive {
        env.filename = name_c.as_ptr();
        env.nzb_name = out_c.as_ptr();
        sc.add_archive_tmp_file_with_params(&env, captured_params)
    } else {
        env.filename = out_c.as_ptr();
        env.nzb_name = name_c.as_ptr();
        sc.add_external_with_params(&env, captured_params)
    }
}

#[cfg(unix)]
pub(crate) unsafe fn add_cli_nzb_file_active(
    category_ptr: *const c_char,
    filename_ptr: *const c_char,
) -> bool {
    let (Some(host), Some((qc, qh))) = (
        crate::ffi::queue_host(),
        crate::ffi::active_queue_coord_and_host(),
    ) else {
        return false;
    };
    let cat_bytes = unsafe { cstr::bytes(category_ptr) };
    let fn_bytes = unsafe { cstr::bytes(filename_ptr) };
    let parsed = crate::nzbfile::parse_and_materialize_nzb(&host, fn_bytes, cat_bytes);
    struct UptrGuard<'a>(&'a crate::queuesubsys_host::QueueSubsysHost, usize);
    impl Drop for UptrGuard<'_> {
        fn drop(&mut self) {
            self.0.destroy_nzb_uptr(self.1);
        }
    }
    let _uptr_guard = UptrGuard(&host, parsed.uptr_addr);
    if !parsed.ok || parsed.nzb_ptr.is_none() {
        let disp = if filename_ptr.is_null() {
            c"N/A".as_ptr()
        } else {
            filename_ptr
        };
        libc::printf(c"Parsing NZB-document %s failed\n\n".as_ptr(), disp);
    } else {
        let nzb_ptr = parsed.nzb_ptr;
        if !parsed.password.is_empty() {
            host.nzb_set_param(nzb_ptr, b"*Unpack:Password", Some(&parsed.password));
        }
        if host.nzb_get_param(nzb_ptr, b"*Unpack:").is_none() {
            let v: &[u8] = if category_unpack_enabled(cat_bytes) {
                b"yes"
            } else {
                b"no"
            };
            host.nzb_set_param(nzb_ptr, b"*Unpack:", Some(v));
        }
        qc.add_nzb_file_to_queue(&qh, parsed.uptr_addr, NzbRef::NONE, false);
    }
    true
}

#[cfg(test)]
mod tests {
    /// B4: the summary of a fleet from NzbDir names what downloads first
    /// (never an empty reason) and lists every copy over lines short enough
    /// for the log, instead of one line cut at 1023 bytes.
    #[test]
    fn a_fleet_summary_is_split_never_cut() {
        let key = b"dupes:dune.part.two.2024.uhd.bluray.2160p.truehd.atmos.7.1.dv.hevc.remux.framestor";
        let added: Vec<super::FleetCopy> = (0..17)
            .map(|i| {
                (
                    format!("Dune.Part.Two.2024.UHD.BluRay.2160p.TrueHD.Atmos.7.1.DV.HEVC.REMUX-FraMeSToR_{i}.nzb").into_bytes(),
                    if i < 14 { 460 + i } else { 0 },
                    if i == 0 { b"QUEUED".to_vec() } else if i < 14 { b"BACKUP".to_vec() } else { b"SAME_POSTING".to_vec() },
                    if i < 14 { 100 } else { -1 },
                )
            })
            .collect();
        let lines = super::fleet_summary_lines(key, b"", 460, &added);
        let first = String::from_utf8_lossy(&lines[0]).to_string();
        assert!(first.contains("from NzbDir: downloading #460; 17 cop(ies): 1 QUEUED, 13 BACKUP, 3 SAME_POSTING"), "{first}");
        assert!(!first.contains(": ,"), "{first}");
        assert!(lines.len() > 2);
        assert!(lines.iter().all(|l| l.len() <= super::FLEET_SUMMARY_LINE_MAX));
        let all = lines.iter().map(|l| String::from_utf8_lossy(l).to_string()).collect::<Vec<_>>().join("\n");
        for i in 0..17 {
            assert!(all.contains(&format!("FraMeSToR_{i}.nzb ")), "copy {i} listed");
        }
        assert!(all.contains("FraMeSToR_16.nzb SAME_POSTING"));
        // a name longer than a line is cut, never inside a UTF-8 character
        let long = ("é".repeat(600) + ".nzb").into_bytes();
        let cut = super::fleet_summary_lines(key, b"", 9, &[(long, 9, b"BACKUP".to_vec(), 100)]);
        assert!(cut.iter().all(|l| std::str::from_utf8(l).is_ok() && l.len() <= super::FLEET_SUMMARY_LINE_MAX));
        let none = super::fleet_summary_lines(key, b"KEY_BUSY", 0, &[]);
        assert_eq!(none.len(), 1);
        assert!(String::from_utf8_lossy(&none[0]).contains("from NzbDir: KEY_BUSY; 0 cop(ies)"));
    }

    /// B2b (6790, 2026-11-25): the summary called weak donors DEAD (_3 29%,
    /// _5 6%, _13 5%) while their parameters and the web UI said N% alive.
    /// Any alive article makes a donor: DEAD only for 0%; a backup under the
    /// healthy share is "BACKUP (weak, N% alive)", a healthy or unverified one
    /// plain BACKUP; a copy with no name still shows as one.
    #[test]
    fn a_fleet_summary_calls_weak_donors_weak_never_dead() {
        let key = b"dupes:dune.part.two.2024";
        let copy = |n: &str, id: c_int, status: &[u8], alive: c_int| -> super::FleetCopy {
            (n.as_bytes().to_vec(), id, status.to_vec(), alive)
        };
        let added = vec![
            copy("Dune_12.nzb", 1795, b"QUEUED", 100),
            copy("Dune_1.nzb", 1786, b"BACKUP", 99),
            copy("Dune_3.nzb", 1787, b"BACKUP", 29),
            copy("Dune_5.nzb", 1788, b"BACKUP", 6),
            copy("Dune_13.nzb", 1796, b"BACKUP", 5),
            copy("Dune_7.nzb", 1790, b"DEAD", 0),
            copy("Dune_8.nzb", 1791, b"BACKUP", -1),
            copy("Dune_9.nzb", 0, b"WEAK", 20),
            copy("", 1799, b"BACKUP", 100),
        ];
        let lines = super::fleet_summary_lines(key, b"", 1795, &added);
        let first = String::from_utf8_lossy(&lines[0]).to_string();
        assert!(
            first.contains("9 cop(ies): 1 QUEUED, 3 BACKUP, 3 BACKUP (weak), 1 DEAD, 1 too weak"),
            "{first}"
        );
        let all = lines[1..].iter().map(|l| String::from_utf8_lossy(l).to_string()).collect::<Vec<_>>().join("\n");
        assert!(all.contains("Dune_3.nzb BACKUP (weak, 29% alive) #1787"), "{all}");
        assert!(all.contains("Dune_5.nzb BACKUP (weak, 6% alive) #1788"), "{all}");
        assert!(all.contains("Dune_13.nzb BACKUP (weak, 5% alive) #1796"), "{all}");
        assert!(all.contains("Dune_7.nzb DEAD #1790"), "{all}");
        assert!(all.contains("Dune_8.nzb BACKUP #1791"), "{all}");
        assert!(all.contains("Dune_9.nzb too weak (20% alive)"), "{all}");
        assert!(all.contains("Dune_1.nzb BACKUP #1786"), "{all}");
        assert!(all.contains("(no name) BACKUP #1799"), "{all}");
        assert!(!all.contains("copies: ;") && !all.contains(";  "), "{all}");
        // the reply's own status: DEAD only for 0% alive
        assert_eq!(crate::dupesearch_coord::fleet_added_status(true, 29).0, b"BACKUP");
        assert!(String::from_utf8_lossy(&crate::dupesearch_coord::fleet_added_status(true, 29).1).contains("29%"));
        assert_eq!(crate::dupesearch_coord::fleet_added_status(true, 0).0, b"DEAD");
        assert_eq!(crate::dupesearch_coord::fleet_added_status(false, 99).0, b"BACKUP");
        // not queued because no copy of the fleet can complete: WEAK, never DEAD
        assert_eq!(crate::dupesearch_coord::fleet_unqueued_status(20).0, b"WEAK");
        assert_eq!(crate::dupesearch_coord::fleet_unqueued_status(0).0, b"DEAD");
    }
    #[test]
    fn a_scan_folder_that_cannot_be_read_is_warned_about_once_until_it_works() {
        let a = b"Could not scan /nonexistent/rg/nzb for archives: No such file or directory";
        let b = b"Could not scan /other/nzb for archives: No such file or directory";
        super::clear_scan_warning();
        assert!(super::scan_warning_is_new(a));
        assert!(!super::scan_warning_is_new(a));
        assert!(!super::scan_warning_is_new(a));
        assert!(super::scan_warning_is_new(b), "another folder or reason is new");
        assert!(super::scan_warning_is_new(a));
        super::clear_scan_warning();
        assert!(super::scan_warning_is_new(a), "after a good scan it is warned about again");
        super::clear_scan_warning();
    }

    use super::*;

    #[test]
    fn nzbdir_copies_of_one_release_form_one_fleet() {
        let k = |c: &str, n: &str| (c.as_bytes().to_vec(), crate::releasename::group_key(n.as_bytes()));
        let base = "Dune.Part.Two.2024.UHD.BluRay.2160p.TrueHD.Atmos.7.1.DV.HEVC.REMUX-FraMeSToR";
        let other = "Other.Show.S01E01.1080p.WEB-GRP";
        let pending = vec![
            k("", &format!("{base}.nzb")),
            k("", &format!("{other}.nzb")),
            k("", &format!("{base} (1).nzb")),
            k("", &format!("{base}_3.nzb")),
            k("movies", &format!("{base}_4.nzb")),
        ];
        let (fleets, single) = plan_fleets(&pending, &[]);
        assert_eq!(fleets, vec![vec![0, 2, 3]]);
        assert_eq!(single, vec![1, 4]);
        // a copy that lands within the window joins the fleet started before
        let recent = vec![k("movies", &format!("{base}.nzb"))];
        let (fleets, single) = plan_fleets(&pending[4..], &recent);
        assert_eq!(fleets, vec![vec![0]]);
        assert!(single.is_empty());
        assert_eq!(fleet_key_for(b"X.nzb", b".zip"), None);
        // a copy alone in its pass waits for a file of its release next to it
        // that the scanner saw a pass later (still waiting its NzbDirFileAge)
        let key = crate::releasename::group_key(format!("{base}.nzb").as_bytes());
        let first = format!("/nzb/{base}.nzb");
        let late = format!("/nzb/{base}_1.nzb");
        let elsewhere = format!("/nzb/movies/{base}_2.nzb");
        let unrelated = format!("/nzb/{other}.nzb");
        let waits = |w: &[&str]| {
            let w: Vec<&[u8]> = w.iter().map(|s| s.as_bytes()).collect();
            single_waits_for_sibling(first.as_bytes(), &key, &w)
        };
        assert!(waits(&[&first, &late]));
        assert!(!waits(&[&first]));
        assert!(!waits(&[&elsewhere, &unrelated]));
        assert!(!waits(&[&format!("/nzb/{base}_1.nzb.queued")]));
        // finding 8: a busy key or a stop puts the copies back for the next scan
        assert!(fleet_never_ran(b"KEY_BUSY", false));
        assert!(fleet_never_ran(b"SHUTDOWN", false));
        // review: copies a grouped fleet already added held before the program
        // stopped are reported as added, so they are never written back
        assert!(!fleet_never_ran(b"SHUTDOWN", true));
        assert!(fleet_never_ran(b"", false));
        assert!(!fleet_never_ran(b"KEY_BUSY", true));
        assert!(!fleet_never_ran(b"ALL_DEAD", false));
    }

    /// A fleet member's priority kept beside its file is hidden from the scan
    /// and read once.
    #[test]
    fn a_kept_priority_is_hidden_and_read_once() {
        let nzb = Path::new("/nzb/Movies/Movie.2024-GRP (2).nzb");
        let side = priority_sidecar(nzb);
        assert_eq!(side, Path::new("/nzb/Movies/.Movie.2024-GRP (2).nzb.priority"));
        let leaf = side.file_name().unwrap().as_encoded_bytes();
        assert!(should_skip_item(leaf, b"processed", b"broken"));
        let dir = std::env::temp_dir().join(format!("rg-prio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("A.nzb");
        keep_priority(&[f.to_string_lossy().into_owned().into_bytes()], 100);
        assert_eq!(take_kept_priority(&f), Some(100));
        assert_eq!(take_kept_priority(&f), None);
        keep_priority(&[f.to_string_lossy().into_owned().into_bytes()], 0);
        assert!(!priority_sidecar(&f).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

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

        /// What the test reads back, shared by both mock hosts.
        struct MockState {
            scanner: Arc<NzbgetRsScanner>,
            nzb_dir_c: CString,
            temp_dir_c: CString,
            added_count: AtomicI32,
            last_cat: Mutex<String>,
        }

        /// `wakes`: the caller's host runs a service pass on a new thread
        /// with a host that does not wake.
        struct MockHost {
            state: Arc<MockState>,
            wakes: bool,
        }

        impl ScannerHost for MockHost {
            fn wake(&self) -> bool {
                if !self.wakes {
                    return false;
                }
                let state = self.state.clone();
                std::thread::spawn(move || {
                    let mut env = NzbgetRsScannerEnv::empty();
                    env.host = Some(Arc::new(MockHost {
                        state: state.clone(),
                        wakes: false,
                    }));
                    env.nzb_dir = state.nzb_dir_c.as_ptr();
                    env.temp_dir = state.temp_dir_c.as_ptr();
                    env.nzb_dir_interval = 5;
                    env.nzb_dir_file_age = 0;
                    env.archive_scan = 1;
                    env.archive_action = 0;
                    env.queue_loaded = 1;
                    env.path_sep = b'/' as c_char;
                    state.scanner.service_work(&env);
                });
                true
            }

            fn parse_nzb(&self, _filename: &[u8], _category: &[u8]) -> (bool, Vec<u8>) {
                (true, b"movies".to_vec())
            }

            fn commit_nzb(&self, nzb: &CommitNzb<'_>) -> c_int {
                if !nzb.ok {
                    return 0;
                }
                if let Some(cat) = nzb.auto_category {
                    if let Ok(s) = cat.to_str() {
                        *self.state.last_cat.lock().unwrap() = s.to_string();
                    }
                }
                self.state.added_count.fetch_add(1, Ordering::SeqCst) + 100
            }
        }

        let scanner = Arc::new(NzbgetRsScanner::new());
        let state = Arc::new(MockState {
            scanner: scanner.clone(),
            nzb_dir_c: nzb_dir_c.clone(),
            temp_dir_c: temp_dir_c.clone(),
            added_count: AtomicI32::new(0),
            last_cat: Mutex::new(String::new()),
        });

        let mut env = NzbgetRsScannerEnv::empty();
        env.host = Some(Arc::new(MockHost {
            state: state.clone(),
            wakes: true,
        }));
        env.nzb_dir = nzb_dir_c.as_ptr();
        env.temp_dir = temp_dir_c.as_ptr();
        env.path_sep = b'/' as c_char;

        scanner.init_options(false, &env);
        assert!(!temp_dir.join("_unpack").exists());
        assert_eq!(scanner.service_interval(5), 1);
        assert_eq!(scanner.service_interval(0), -1);

        let xml = b"<?xml version=\"1.0\"?><nzb></nzb>";
        let name_c = c"Show.S01E01";
        let mut out_id: c_int = -1;
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
        assert_eq!(state.last_cat.lock().unwrap().as_str(), "movies");
        assert!(nzb_dir.join("Show.S01E01.nzb.queued").exists());

        let _ = std::fs::remove_dir_all(&base);
    }
}
