//! A queued file's data, which was the C++ `FileInfo`'s members, now owned by
//! Rust.
//!
//! The C++ `FileInfo` is a thin owner of one [`File`] and reaches every field
//! through the C ABI below, so the queue model's C++ code and host operations
//! 40-45 keep their meaning while the storage lives here. The file owns its
//! [`crate::qarticle::ArticleList`].
//!
//! Locking matches the C++ class: plain fields are guarded by the
//! download-queue lock (here also by the file's own mutex, so Rust never forms
//! aliasing references), the cached-article count and decoded size are atomic,
//! and the output-file lock is a separate lock held across host callbacks.

use crate::options::OptStr;
use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};

use crate::qarticle::{nzbget_rs_artlist_free, nzbget_rs_artlist_new, ArticleList};

static ID_GEN: AtomicI32 = AtomicI32::new(0);
static ID_MAX: AtomicI32 = AtomicI32::new(0);

/// Integer fields, by the id the C ABI uses.
pub mod field {
    pub const ID: i32 = 0;
    pub const SIZE: i32 = 1;
    pub const REMAINING_SIZE: i32 = 2;
    pub const SUCCESS_SIZE: i32 = 3;
    pub const FAILED_SIZE: i32 = 4;
    pub const MISSED_SIZE: i32 = 5;
    pub const TOTAL_ARTICLES: i32 = 6;
    pub const MISSED_ARTICLES: i32 = 7;
    pub const FAILED_ARTICLES: i32 = 8;
    pub const SUCCESS_ARTICLES: i32 = 9;
    pub const DUPE_RECOVERED_ARTICLES: i32 = 10;
    pub const DUPE_ATTEMPTED_ARTICLES: i32 = 11;
    pub const DUPE_CUTOVER: i32 = 12;
    pub const DUPE_NO_SOURCE_LOGGED: i32 = 13;
    pub const FIRST_CONTENT: i32 = 14;
    pub const DUPE_LEAD_DONOR_ID: i32 = 15;
    pub const DUPE_LEAD_FAILURES: i32 = 16;
    pub const DUPE_LEAD_SWITCHES: i32 = 17;
    pub const TIME: i32 = 18;
    pub const PAUSED: i32 = 19;
    pub const DELETED: i32 = 20;
    pub const FILENAME_CONFIRMED: i32 = 21;
    pub const PAR_FILE: i32 = 22;
    pub const COMPLETED_ARTICLES: i32 = 23;
    pub const OUTPUT_INITIALIZED: i32 = 24;
    pub const EXTRA_PRIORITY: i32 = 25;
    pub const ACTIVE_DOWNLOADS: i32 = 26;
    pub const DUPE_DELETED: i32 = 27;
    pub const CACHED_ARTICLES: i32 = 28;
    pub const PARTIAL_CHANGED: i32 = 29;
    pub const FORCE_DIRECT_WRITE: i32 = 30;
    pub const PARTIAL_STATE: i32 = 31;
    pub const CRC: i32 = 32;
    pub const FLUSH_LOCKED: i32 = 33;
    pub const DECODED_FILE_SIZE: i32 = 34;
    pub const NZB: i32 = 35;
}

/// String fields, by id.
pub mod text {
    pub const SUBJECT: i32 = 0;
    pub const FILENAME: i32 = 1;
    pub const ORIGNAME: i32 = 2;
    pub const OUTPUT_FILENAME: i32 = 3;
    pub const HASH16K: i32 = 4;
    pub const PAR_SET_ID: i32 = 5;
    pub const HARD_LINK_PATH: i32 = 6;
    pub const COUNT: usize = 7;
}

struct Data {
    ints: [i64; 36],
    texts: [CString; text::COUNT],
    groups: Vec<CString>,
    // (server id, success, failed)
    server_stats: Vec<[i32; 3]>,
}

impl Default for Data {
    fn default() -> Self {
        Self {
            ints: [0; 36],
            texts: Default::default(),
            groups: Vec::new(),
            server_stats: Vec::new(),
        }
    }
}

/// One queued file.
pub struct File {
    articles: *mut ArticleList,
    cached_articles: AtomicI32,
    decoded_file_size: AtomicI64,
    output_locked: Mutex<bool>,
    output_cv: Condvar,
    d: Mutex<Data>,
}

// The article list is owned and only freed in Drop; access follows the queue
// lock as in the C++ code.
unsafe impl Send for File {}
unsafe impl Sync for File {}

impl Drop for File {
    fn drop(&mut self) {
        unsafe { nzbget_rs_artlist_free(self.articles) };
    }
}

fn cstring(p: *const c_char) -> CString {
    if p.is_null() {
        CString::default()
    } else {
        unsafe { CStr::from_ptr(p) }.to_owned()
    }
}

/// Applies a server-stat operation (0 set, 1 add, 2 subtract) to `list`.
fn stat_op(list: &mut Vec<[i32; 3]>, server_id: i32, success: i32, failed: i32, op: i32) {
    let i = match list.iter().position(|s| s[0] == server_id) {
        Some(i) => i,
        None => {
            list.push([server_id, 0, 0]);
            list.len() - 1
        }
    };
    let (mut s, mut f) = (list[i][1], list[i][2]);
    unsafe { crate::ffi::nzbget_rs_server_stat_apply_op(op, success, failed, &mut s, &mut f) };
    list[i][1] = s;
    list[i][2] = f;
}

impl File {
    fn new(id: i32) -> Self {
        // < 0: storage for a host object whose constructor sets the id
        let id = match id {
            0 => ID_GEN.fetch_add(1, Ordering::SeqCst) + 1,
            i if i < 0 => 0,
            i => i,
        };
        let f = Self {
            articles: nzbget_rs_artlist_new(),
            cached_articles: AtomicI32::new(0),
            decoded_file_size: AtomicI64::new(0),
            output_locked: Mutex::new(false),
            output_cv: Condvar::new(),
            d: Mutex::new(Data::default()),
        };
        f.lock().ints[field::ID as usize] = i64::from(id);
        f
    }

    fn lock(&self) -> MutexGuard<'_, Data> {
        self.d.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn get(&self, id: i32) -> i64 {
        match id {
            field::CACHED_ARTICLES => i64::from(self.cached_articles.load(Ordering::SeqCst)),
            field::DECODED_FILE_SIZE => self.decoded_file_size.load(Ordering::SeqCst),
            0..=35 => self.lock().ints[id as usize],
            _ => 0,
        }
    }

    fn set(&self, id: i32, v: i64) {
        match id {
            field::CACHED_ARTICLES => self.cached_articles.store(v as i32, Ordering::SeqCst),
            field::DECODED_FILE_SIZE => self.decoded_file_size.store(v, Ordering::SeqCst),
            field::ID => {
                self.lock().ints[0] = v;
                ID_MAX.fetch_max(v as i32, Ordering::SeqCst);
            }
            field::SIZE => {
                // setting the size also resets what remains (as C++ SetSize did)
                let mut d = self.lock();
                d.ints[field::SIZE as usize] = v;
                d.ints[field::REMAINING_SIZE as usize] = v;
            }
            0..=35 => self.lock().ints[id as usize] = v,
            _ => {}
        }
    }

    /// Takes the output-file lock when the file has active downloads, as the
    /// C++ guard did (its mutex existed only while downloads were active).
    /// Returns whether it was taken.
    fn output_lock(&self) -> bool {
        if self.get(field::ACTIVE_DOWNLOADS) <= 0 {
            return false;
        }
        let mut locked = self.output_locked.lock().unwrap_or_else(|e| e.into_inner());
        while *locked {
            locked = self
                .output_cv
                .wait(locked)
                .unwrap_or_else(|e| e.into_inner());
        }
        *locked = true;
        true
    }

    fn output_unlock(&self) {
        *self.output_locked.lock().unwrap_or_else(|e| e.into_inner()) = false;
        self.output_cv.notify_one();
    }
}

/// A new file with `id` (0: the next generated id).
pub extern "C" fn nzbget_rs_file_new(id: c_int) -> *mut File {
    Box::into_raw(Box::new(File::new(id)))
}

/// # Safety
/// `f` is null or from `nzbget_rs_file_new`, freed once.
pub unsafe extern "C" fn nzbget_rs_file_free(f: *mut File) {
    if !f.is_null() {
        drop(unsafe { Box::from_raw(f) });
    }
}

/// Id generation shared by all files: 0 last generated id, 1 reset to the
/// highest id seen, 2 reset to zero, 3 raise the floor to `v`, 4 generate
/// the next id.
pub extern "C" fn nzbget_rs_file_ids(op: c_int, v: c_int) -> c_int {
    match op {
        0 => ID_GEN.load(Ordering::SeqCst),
        1 => {
            ID_GEN.store(ID_MAX.load(Ordering::SeqCst), Ordering::SeqCst);
            0
        }
        2 => {
            ID_GEN.store(0, Ordering::SeqCst);
            ID_MAX.store(0, Ordering::SeqCst);
            0
        }
        3 => {
            ID_MAX.fetch_max(v, Ordering::SeqCst);
            0
        }
        4 => ID_GEN.fetch_add(1, Ordering::SeqCst) + 1,
        _ => 0,
    }
}

/// # Safety
/// `f` is a live file.
pub unsafe extern "C" fn nzbget_rs_file_get(f: *const File, id: c_int) -> i64 {
    unsafe { &*f }.get(id)
}

/// # Safety
/// `f` is a live file.
pub unsafe extern "C" fn nzbget_rs_file_set(f: *const File, id: c_int, v: i64) {
    unsafe { &*f }.set(id, v);
}

/// The text field `id`; the pointer stays valid until that field changes.
///
/// # Safety
/// `f` is a live file.
pub unsafe extern "C" fn nzbget_rs_file_text(f: *const File, id: c_int) -> *const c_char {
    let d = unsafe { &*f }.lock();
    d.texts.get(id as usize).map_or(ptr::null(), |s| s.as_ptr())
}

/// Sets text field `id` (null stores an empty string).
///
/// # Safety
/// `f` is a live file; `s` is null or NUL-terminated.
pub unsafe extern "C" fn nzbget_rs_file_set_text(f: *const File, id: c_int, s: *const c_char) {
    if let Some(slot) = unsafe { &*f }.lock().texts.get_mut(id as usize) {
        *slot = cstring(s);
    }
}

/// The file's article list (owned by the file).
///
/// # Safety
/// `f` is a live file.
pub unsafe extern "C" fn nzbget_rs_file_articles(f: *const File) -> *mut ArticleList {
    unsafe { &*f }.articles
}

/// Collections: 0 group count, 1 group `a2` (into `strs[0]`), 2 append group
/// `s1`; 10 server-stat count, 11 server stat `a2` into `ints[0..3]`, 12
/// stat op `a2` (0 set, 1 add, 2 subtract) with `ints[0..3]`, 13 clear stats;
/// 20 take the output lock (returns 1 when taken), 21 release it.
///
/// # Safety
/// `f` is a live file; output arrays are large enough for the operation.
pub unsafe extern "C" fn nzbget_rs_file_op(
    f: *const File,
    op: c_int,
    a2: c_int,
    s1: *const c_char,
    ints: *mut c_int,
    strs: *mut *const c_char,
) -> i64 {
    let file = unsafe { &*f };
    match op {
        0 => file.lock().groups.len() as i64,
        1 => {
            let d = file.lock();
            unsafe {
                *strs = d
                    .groups
                    .get(a2 as usize)
                    .map_or(ptr::null(), |s| s.as_ptr())
            };
            0
        }
        2 => {
            file.lock().groups.push(cstring(s1));
            0
        }
        10 => file.lock().server_stats.len() as i64,
        11 => {
            let d = file.lock();
            let Some(s) = d.server_stats.get(a2 as usize) else {
                return 0;
            };
            unsafe {
                *ints = s[0];
                *ints.add(1) = s[1];
                *ints.add(2) = s[2];
            }
            0
        }
        12 => {
            let (id, ok, bad) = unsafe { (*ints, *ints.add(1), *ints.add(2)) };
            stat_op(&mut file.lock().server_stats, id, ok, bad, a2);
            0
        }
        13 => {
            file.lock().server_stats.clear();
            0
        }
        20 => i64::from(file.output_lock()),
        21 => {
            file.output_unlock();
            0
        }
        _ => 0,
    }
}

/// Applies a file's server stats to collection `nzb`'s totals (which 0)
/// or current list (1) with `op` (0 set, 1 add, 2 subtract), as
/// NzbInfo::AddFileStats did.
///
/// # Safety
/// `nzb` is a live collection and `f` a live file.
pub(crate) unsafe fn add_file_stats(nzb: usize, f: *const File, which: i32, op: i32) {
    let stats = unsafe { &*f }.lock().server_stats.clone();
    for mut v in stats {
        unsafe {
            crate::qnzb::nzbget_rs_nzb_coll(
                nzb as *const crate::qnzb::NzbScalars,
                44 + which,
                op,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                v.as_mut_ptr(),
                ptr::null_mut(),
            )
        };
    }
}

/// Applies a queue-host file setter (host op 42 sub-op `a1`) as the C++
/// FileInfo did; `p2` is the collection where the op names one.
///
/// # Safety
/// `f` is a live file; `p2` a live collection for 53 and 59; the caller
/// holds the queue lock.
pub(crate) unsafe fn host_file_set(
    f: *const File,
    a1: i32,
    a2: i32,
    p2: usize,
    s1: *const c_char,
    i64s: *const i64,
) {
    use field::*;
    if f.is_null() {
        return;
    }
    let file = unsafe { &*f };
    let flag = i64::from(a2 != 0);
    let v64 = || unsafe { *i64s };
    let nzb = file.get(NZB) as usize as *mut crate::qnzb::NzbScalars;
    // simple setters: (sub-op, field, value)
    let simple = match a1 {
        1 => Some((DELETED, flag)),
        2 => Some((DUPE_DELETED, flag)),
        3 => Some((FILENAME_CONFIRMED, flag)),
        4 => Some((FIRST_CONTENT, i64::from(a2))),
        6 => Some((FORCE_DIRECT_WRITE, flag)),
        7 => Some((OUTPUT_INITIALIZED, flag)),
        8 => Some((PARTIAL_STATE, i64::from(a2))),
        9 => Some((PARTIAL_CHANGED, flag)),
        10 => Some((DUPE_CUTOVER, flag)),
        11 => Some((DUPE_NO_SOURCE_LOGGED, flag)),
        12 => Some((DUPE_LEAD_DONOR_ID, i64::from(a2))),
        13 => Some((DUPE_LEAD_FAILURES, i64::from(a2))),
        14 => Some((DUPE_LEAD_SWITCHES, i64::from(a2))),
        15 => Some((DUPE_RECOVERED_ARTICLES, i64::from(a2))),
        16 => Some((DUPE_ATTEMPTED_ARTICLES, i64::from(a2))),
        17 => Some((COMPLETED_ARTICLES, i64::from(a2))),
        18 => Some((SUCCESS_ARTICLES, i64::from(a2))),
        19 => Some((FAILED_ARTICLES, i64::from(a2))),
        20 => Some((ACTIVE_DOWNLOADS, i64::from(a2))),
        21 => Some((CACHED_ARTICLES, i64::from(a2))),
        22 => Some((PAR_FILE, flag)),
        30 => Some((REMAINING_SIZE, v64())),
        31 => Some((SUCCESS_SIZE, v64())),
        32 => Some((FAILED_SIZE, v64())),
        33 => Some((DECODED_FILE_SIZE, v64())),
        58 => Some((NZB, p2 as i64)),
        62 => Some((TIME, v64())),
        63 => Some((SIZE, v64())),
        64 => Some((MISSED_SIZE, v64())),
        65 => Some((TOTAL_ARTICLES, i64::from(a2))),
        66 => Some((MISSED_ARTICLES, i64::from(a2))),
        _ => None,
    };
    if let Some((id, v)) = simple {
        file.set(id, v);
        return;
    }
    let set_text = |id: i32, v: *const c_char| unsafe { nzbget_rs_file_set_text(f, id, v) };
    // the collection's copy of a file's server stats (NzbInfo::AddFileStats)
    let add_stats = |which: i32, op: i32| unsafe { add_file_stats(p2, f, which, op) };
    match a1 {
        0 => {
            let paused = a2 != 0;
            if (file.get(PAUSED) != 0) != paused && !nzb.is_null() {
                let remaining = file.get(REMAINING_SIZE);
                let n = unsafe { &mut *nzb };
                n.pausedFileCount += if paused { 1 } else { -1 };
                n.pausedSize += if paused { remaining } else { -remaining };
            }
            file.set(PAUSED, i64::from(paused));
        }
        5 => {
            let extra = a2 != 0;
            if (file.get(EXTRA_PRIORITY) != 0) != extra && !nzb.is_null() {
                unsafe { &mut *nzb }.extraPriority += if extra { 1 } else { -1 };
            }
            file.set(EXTRA_PRIORITY, i64::from(extra));
        }
        40 => set_text(text::FILENAME, s1),
        41 => {
            // the output name changes under the output lock while downloads run
            let taken = file.output_lock();
            set_text(text::OUTPUT_FILENAME, s1);
            if taken {
                file.output_unlock();
            }
        }
        42 => set_text(text::ORIGNAME, s1),
        43 => set_text(text::HASH16K, s1),
        44 => set_text(text::PAR_SET_ID, s1),
        45 => set_text(
            text::HARD_LINK_PATH,
            if s1.is_null() { c"".as_ptr() } else { s1 },
        ),
        50 => {
            let name = file.lock().texts[text::FILENAME as usize]
                .as_bytes()
                .to_vec();
            let valid = crate::paths::make_valid_filename(&name, false);
            let valid = CString::new(valid).unwrap_or_default();
            set_text(text::FILENAME, valid.as_ptr());
        }
        51 | 52 | 54 => unsafe {
            let op = match a1 {
                51 => 101,
                52 => 102,
                _ => 103,
            };
            crate::qarticle::nzbget_rs_article_host(
                file.articles,
                ptr::null_mut(),
                op,
                0,
                0,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        },
        53 => {
            add_stats(1, 2);
            file.lock().server_stats.clear();
        }
        55 => {
            let taken = file.output_lock();
            unsafe {
                crate::qarticle::nzbget_rs_article_host(
                    file.articles,
                    ptr::null_mut(),
                    104,
                    0,
                    p2,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if taken {
                file.output_unlock();
            }
        }
        59 => add_stats(0, 2),
        61 => set_text(text::SUBJECT, s1),
        _ => {}
    }
}

fn art(
    list: *mut ArticleList,
    article: usize,
    op: c_int,
    a2: c_int,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    unsafe {
        crate::qarticle::nzbget_rs_article_host(
            list,
            article as *mut crate::qarticle::Article,
            op,
            a2,
            0,
            ptr::null(),
            ints,
            i64s,
            strs,
        )
    }
}

thread_local! {
    // the output name handed back by host op 44 sub-op 4 (valid until the
    // next call on this thread, as the C++ thread_local string was)
    static OUT_NAME: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The article cache's file ops (host op 44), as the C++ host did them.
/// Returns what that op returned.
///
/// # Safety
/// `f` is a live file; `p2` is a live article for 1, 2 and 6 (a2 != 0), the
/// callback context for 5; output arrays are large enough for the op.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn host_cache_op(
    f: *const File,
    a1: i32,
    a2: i32,
    p2: usize,
    s1: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    use field::*;
    if f.is_null() {
        return 0;
    }
    let file = unsafe { &*f };
    let null_i = ptr::null_mut();
    let null_l = ptr::null_mut();
    let null_s = ptr::null_mut();
    let nzb = file.get(NZB) as usize as *const crate::qnzb::NzbScalars;
    let opts = crate::options::active_options();
    let skip_write = || {
        let nzb_skip = !nzb.is_null() && unsafe { (*nzb).skipDiskWrite };
        nzb_skip || opts.as_ref().is_some_and(|o| o.scalars().skip_write != 0)
    };
    // take the flush lock unless it is already taken
    let try_flush_lock = || {
        let mut d = file.lock();
        if d.ints[FLUSH_LOCKED as usize] != 0 {
            false
        } else {
            d.ints[FLUSH_LOCKED as usize] = 1;
            true
        }
    };
    match a1 {
        // copy the article's cached data out
        1 => {
            let data = art(null_l, p2, 110, 0, null_i, null_l.cast(), null_s) as *const u8;
            if file.get(FLUSH_LOCKED) == 0 && !data.is_null() && a2 > 0 {
                unsafe { ptr::copy_nonoverlapping(data, *i64s as usize as *mut u8, a2 as usize) };
                1
            } else {
                0
            }
        }
        // drop the article's cached data once it is written
        2 => {
            let result = art(null_l, p2, 114, 0, null_i, null_l.cast(), null_s) as *const c_char;
            if art(null_l, p2, 117, 0, null_i, null_l.cast(), null_s) > 0 && !result.is_null() {
                unsafe { libc::unlink(result) };
            }
            if art(null_l, p2, 110, 0, null_i, null_l.cast(), null_s) == 0
                || file.get(FLUSH_LOCKED) != 0
            {
                return 0;
            }
            art(null_l, p2, 111, 0, null_i, null_l.cast(), null_s);
            file.set(CACHED_ARTICLES, file.get(CACHED_ARTICLES) - 1);
            1
        }
        3 => {
            if a2 != 0 {
                usize::from(try_flush_lock())
            } else {
                file.set(FLUSH_LOCKED, 0);
                1
            }
        }
        // where and how the article writer should write
        4 => {
            let (out_init, name) = {
                let taken = file.output_lock();
                let r = {
                    let d = file.lock();
                    (
                        d.ints[OUTPUT_INITIALIZED as usize] != 0,
                        d.texts[text::OUTPUT_FILENAME as usize].as_bytes().to_vec(),
                    )
                };
                if taken {
                    file.output_unlock();
                }
                r
            };
            let direct = opts.as_ref().is_some_and(|o| o.scalars().direct_write != 0)
                || file.get(FORCE_DIRECT_WRITE) != 0;
            unsafe {
                *ints = i32::from(direct && out_init);
                *ints.add(1) = i32::from(skip_write());
                *i64s = name.len() as i64;
                *i64s.add(1) = nzb as usize as i64;
            }
            OUT_NAME.with(|n| {
                let mut n = n.borrow_mut();
                *n = name;
                unsafe { *strs = n.as_ptr().cast() };
            });
            1
        }
        // hand every cached segment to the writer callback, under the flush lock
        5 => {
            if s1.is_null() || !try_flush_lock() {
                return 0;
            }
            type PushSeg =
                unsafe extern "C" fn(usize, usize, i64, c_int, *const c_char, *const c_char);
            let push: PushSeg = unsafe { std::mem::transmute::<*const c_char, PushSeg>(s1) };
            let list = file.articles;
            let n = art(list, 0, 100, 0, null_i, null_l.cast(), null_s);
            for i in 0..n {
                let pa = art(list, 0, 106, i as c_int, null_i, null_l.cast(), null_s);
                let mut ai = [0 as c_int; 5];
                let mut ao = [0i64; 1];
                let mut as_ = [ptr::null::<c_char>(); 2];
                art(
                    ptr::null_mut(),
                    pa,
                    120,
                    0,
                    ai.as_mut_ptr(),
                    ao.as_mut_ptr(),
                    as_.as_mut_ptr(),
                );
                if !as_[0].is_null() {
                    unsafe { push(p2, pa, ao[0], ai[1], as_[0], as_[1]) };
                }
            }
            1
        }
        6 => {
            if a2 == 0 {
                let parking = !nzb.is_null() && unsafe { (*nzb).parking };
                return usize::from(file.get(DELETED) != 0 && !parking);
            }
            art(null_l, p2, 111, 0, null_i, null_l.cast(), null_s);
            0
        }
        7 => {
            file.set(CACHED_ARTICLES, file.get(CACHED_ARTICLES) - i64::from(a2));
            file.set(FLUSH_LOCKED, 0);
            unsafe { *ints = i32::from(skip_write()) };
            0
        }
        _ => 0,
    }
}

thread_local! {
    // the names handed back by host op 45 sub-op 9/0 (valid until the next
    // call on this thread, as the C++ thread_local strings were)
    static WRITER_NAMES: std::cell::RefCell<[CString; 4]> = std::cell::RefCell::new(Default::default());
}

/// Holds a file's output lock while it lives (the C++ `FileOutputGuard`).
struct OutputGuard<'a>(&'a File, bool);

impl<'a> OutputGuard<'a> {
    fn take(file: &'a File) -> Self {
        Self(file, file.output_lock())
    }
}

impl Drop for OutputGuard<'_> {
    fn drop(&mut self) {
        if self.1 {
            self.0.output_unlock();
        }
    }
}

/// The article writer's and downloader's file ops (host op 45), as the C++
/// host did them. Returns what that op returned.
///
/// # Safety
/// `f` is a live file; `p2` is a live article for the article-based
/// sub-ops, a `void(void*)` callback for 0 (context in `i64s[0]`), an
/// article index for 9/4 and 9/5, a count for 9/6 and a CRC for 9/8; output
/// arrays are large enough for the op.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn host_output_op(
    f: *const File,
    a1: i32,
    a2: i32,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    use field::*;
    if f.is_null() {
        return 0;
    }
    let file = unsafe { &*f };
    let art_op = |list: *mut ArticleList,
                  article: usize,
                  op: c_int,
                  a2: c_int,
                  s1: *const c_char,
                  ints: *mut c_int,
                  i64s: *mut i64,
                  strs: *mut *const c_char| unsafe {
        crate::qarticle::nzbget_rs_article_host(
            list,
            article as *mut crate::qarticle::Article,
            op,
            a2,
            0,
            s1,
            ints,
            i64s,
            strs,
        )
    };
    let (null_i, null_l, null_s) = (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
    let nzb = file.get(NZB) as usize as *const crate::qnzb::NzbScalars;
    let nzb_text = |id: c_int| {
        if nzb.is_null() {
            ptr::null()
        } else {
            unsafe { crate::qnzb::nzbget_rs_nzb_text(nzb, id) }
        }
    };
    let text = |id: i32| unsafe { nzbget_rs_file_text(f, id) };
    let set_text = |id: i32, v: *const c_char| unsafe { nzbget_rs_file_set_text(f, id, v) };
    let or_empty = |s: *const c_char| if s.is_null() { c"".as_ptr() } else { s };
    let opts = crate::options::active_options();
    let direct_write = opts.as_ref().is_some_and(|o| o.scalars().direct_write != 0);
    let inter_dir_empty = opts.as_ref().is_none_or(|o| o.str_bytes(OptStr::InterDir).is_empty());
    let hard_linked = || file.lock().texts[text::HARD_LINK_PATH as usize].as_bytes() != b"";
    let flag = |b: bool| c_int::from(b);
    match a1 {
        // run the callback under the output lock (taken while downloads run)
        0 => {
            let _g = OutputGuard::take(file);
            let cb: unsafe extern "C" fn(*mut std::ffi::c_void) =
                unsafe { std::mem::transmute::<usize, _>(p2) };
            unsafe { cb(*i64s as usize as *mut std::ffi::c_void) };
            0
        }
        1 => {
            unsafe {
                *ints = file.get(ID) as c_int;
                *ints.add(1) = art_op(
                    null_l,
                    p2,
                    118,
                    0,
                    ptr::null(),
                    null_i,
                    null_l.cast(),
                    null_s,
                ) as c_int;
                *ints.add(2) = flag(file.get(FORCE_DIRECT_WRITE) != 0);
                *ints.add(3) = flag(file.get(OUTPUT_INITIALIZED) != 0);
                *strs = nzb_text(3);
                *strs.add(1) = text(text::OUTPUT_FILENAME);
                *strs.add(2) = text(text::FILENAME);
            }
            0
        }
        2 => {
            let result = art_op(null_l, p2, 115, 0, s1, null_i, null_l.cast(), null_s);
            if !s2.is_null() {
                set_text(text::OUTPUT_FILENAME, s2);
            }
            unsafe { *strs = result as *const c_char };
            0
        }
        3 => {
            art_op(
                null_l,
                p2,
                116,
                a2,
                ptr::null(),
                null_i,
                null_l.cast(),
                null_s,
            );
            0
        }
        4 => {
            let mut range = [0i64; 2];
            unsafe {
                *ints = art_op(
                    null_l,
                    p2,
                    117,
                    0,
                    ptr::null(),
                    null_i,
                    null_l.cast(),
                    null_s,
                ) as c_int;
            }
            art_op(
                null_l,
                p2,
                119,
                0,
                ptr::null(),
                null_i,
                range.as_mut_ptr(),
                null_s,
            );
            unsafe {
                *i64s = file.get(DECODED_FILE_SIZE);
                *i64s.add(1) = range[0];
                *i64s.add(2) = range[1];
            }
            0
        }
        5 => {
            let force = || file.get(FORCE_DIRECT_WRITE) != 0;
            let out_init = {
                let _g = OutputGuard::take(file);
                let out_init = file.get(OUTPUT_INITIALIZED) != 0;
                if !direct_write && !force() {
                    file.set(OUTPUT_INITIALIZED, 1);
                }
                out_init
            };
            let mut is_own = false;
            if direct_write || force() {
                let _g = OutputGuard::take(file);
                is_own = force() || {
                    let out = file.lock().texts[text::OUTPUT_FILENAME as usize].clone();
                    !s1.is_null()
                        && !out.as_bytes().is_empty()
                        && out.as_c_str() == unsafe { CStr::from_ptr(s1) }
                };
            }
            unsafe {
                *ints = flag(out_init);
                *ints.add(1) = flag(is_own);
            }
            0
        }
        6 => {
            file.set(OUTPUT_INITIALIZED, 1);
            0
        }
        7 => {
            if a2 == 0 {
                art_op(null_l, p2, 112, 0, ptr::null(), ints, i64s, null_s);
                file.set(CACHED_ARTICLES, file.get(CACHED_ARTICLES) + 1);
            } else {
                art_op(null_l, p2, 113, 0, ptr::null(), ints, i64s, null_s);
            }
            0
        }
        8 => {
            set_text(text::OUTPUT_FILENAME, or_empty(s1));
            0
        }
        9 => match a2 {
            0 => {
                let cstr = |p: *const c_char| {
                    if p.is_null() {
                        CString::default()
                    } else {
                        unsafe { CStr::from_ptr(p) }.to_owned()
                    }
                };
                let (n_name, n_dest, f_name) = {
                    let _g = crate::qqueue::CurrentQueueLock::acquire();
                    (
                        cstr(nzb_text(2)),
                        cstr(nzb_text(3)),
                        cstr(text(text::FILENAME)),
                    )
                };
                let f_out = file.lock().texts[text::OUTPUT_FILENAME as usize].clone();
                let skip_write = (!nzb.is_null() && unsafe { (*nzb).skipDiskWrite })
                    || opts.as_ref().is_some_and(|o| o.scalars().skip_write != 0);
                unsafe {
                    *ints = flag(direct_write);
                    *ints.add(1) = flag(file.get(FORCE_DIRECT_WRITE) != 0);
                    *ints.add(2) = flag(file.get(OUTPUT_INITIALIZED) != 0);
                    *ints.add(3) = file.get(CACHED_ARTICLES) as c_int;
                    *ints.add(4) = file.get(SUCCESS_ARTICLES) as c_int;
                    *ints.add(5) = file.get(FAILED_ARTICLES) as c_int;
                    *ints.add(6) = file.get(MISSED_ARTICLES) as c_int;
                    *ints.add(7) = file.get(TOTAL_ARTICLES) as c_int;
                    *ints.add(8) = flag(hard_linked());
                    *ints.add(9) = flag(inter_dir_empty);
                    *ints.add(10) =
                        flag(opts.as_ref().is_some_and(|o| o.scalars().raw_article != 0));
                    *ints.add(11) = flag(skip_write);
                    *ints.add(12) = opts.as_ref().map_or(0, |o| o.scalars().write_buffer);
                    *ints.add(15) = art_op(
                        file.articles,
                        0,
                        100,
                        0,
                        ptr::null(),
                        null_i,
                        null_l.cast(),
                        null_s,
                    ) as c_int;
                }
                WRITER_NAMES.with(|names| {
                    let mut names = names.borrow_mut();
                    *names = [n_name, n_dest, f_name, f_out];
                    for (i, s) in names.iter().enumerate() {
                        unsafe { *strs.add(i) = s.as_ptr() };
                    }
                });
                nzb as usize
            }
            2 => {
                let _g = crate::qqueue::CurrentQueueLock::acquire();
                file.set(CRC, 0);
                set_text(text::OUTPUT_FILENAME, c"".as_ptr());
                0
            }
            4 => {
                let pa = art_op(
                    file.articles,
                    0,
                    106,
                    p2 as c_int,
                    ptr::null(),
                    null_i,
                    null_l.cast(),
                    null_s,
                );
                if pa == 0 {
                    0
                } else {
                    art_op(null_l, pa, 120, 0, ptr::null(), ints, i64s, strs)
                }
            }
            5 => {
                let pa = art_op(
                    file.articles,
                    0,
                    106,
                    p2 as c_int,
                    ptr::null(),
                    null_i,
                    null_l.cast(),
                    null_s,
                );
                art_op(
                    null_l,
                    pa,
                    111,
                    0,
                    ptr::null(),
                    null_i,
                    null_l.cast(),
                    null_s,
                );
                0
            }
            6 => {
                let n = i64::from(p2 as c_int);
                file.set(
                    FAILED_ARTICLES,
                    i64::from(file.get(FAILED_ARTICLES) as c_int) + n,
                );
                file.set(
                    SUCCESS_ARTICLES,
                    i64::from(file.get(SUCCESS_ARTICLES) as c_int) - n,
                );
                0
            }
            8 => {
                file.set(CRC, i64::from(p2 as u32));
                set_text(text::OUTPUT_FILENAME, or_empty(s1));
                unsafe {
                    *ints = flag(hard_linked());
                    *ints.add(1) = flag(inter_dir_empty);
                    *strs = text(text::FILENAME);
                    *strs.add(1) = text(text::OUTPUT_FILENAME);
                    *strs.add(2) = nzb_text(3);
                }
                nzb as usize
            }
            _ => 0,
        },
        // add a server's stats to the file and to its collection
        10 => {
            unsafe { nzbget_rs_file_op(f, 12, 1, ptr::null(), ints, ptr::null_mut()) };
            if !nzb.is_null() {
                unsafe {
                    crate::qnzb::nzbget_rs_nzb_coll(
                        nzb,
                        45,
                        1,
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        ints,
                        ptr::null_mut(),
                    )
                };
            }
            0
        }
        _ => 0,
    }
}

/// What the queue host reported for a file (host op 40), read directly:
/// `ints` and `i64s` in that op's order, the seven texts (filename, subject,
/// output filename, hard-link path, origname, hash16k, par set id) when
/// asked; returns the article count.
///
/// # Safety
/// `f` is a live file.
pub(crate) unsafe fn host_snapshot(
    f: *const File,
    ints: &mut [i32; 28],
    i64s: &mut [i64; 8],
    texts: Option<&mut [Vec<u8>; 7]>,
) -> usize {
    use field::*;
    let file = unsafe { &*f };
    let g = |id| file.get(id);
    *ints = [
        g(ID) as i32,
        g(PAUSED) as i32,
        g(DELETED) as i32,
        g(DUPE_DELETED) as i32,
        g(PAR_FILE) as i32,
        g(FILENAME_CONFIRMED) as i32,
        g(FIRST_CONTENT) as i32,
        g(EXTRA_PRIORITY) as i32,
        g(FORCE_DIRECT_WRITE) as i32,
        g(OUTPUT_INITIALIZED) as i32,
        0,
        g(PARTIAL_STATE) as i32,
        g(PARTIAL_CHANGED) as i32,
        g(DUPE_CUTOVER) as i32,
        g(DUPE_NO_SOURCE_LOGGED) as i32,
        g(DUPE_LEAD_DONOR_ID) as i32,
        g(DUPE_LEAD_FAILURES) as i32,
        g(DUPE_LEAD_SWITCHES) as i32,
        g(DUPE_RECOVERED_ARTICLES) as i32,
        g(DUPE_ATTEMPTED_ARTICLES) as i32,
        g(TOTAL_ARTICLES) as i32,
        g(MISSED_ARTICLES) as i32,
        g(COMPLETED_ARTICLES) as i32,
        g(SUCCESS_ARTICLES) as i32,
        g(FAILED_ARTICLES) as i32,
        g(ACTIVE_DOWNLOADS) as i32,
        g(CACHED_ARTICLES) as i32,
        g(CRC) as u32 as i32,
    ];
    *i64s = [
        g(SIZE),
        g(REMAINING_SIZE),
        g(MISSED_SIZE),
        g(SUCCESS_SIZE),
        g(FAILED_SIZE),
        g(DECODED_FILE_SIZE),
        g(NZB),
        g(TIME),
    ];
    {
        let d = file.lock();
        ints[10] = i32::from(!d.texts[text::HARD_LINK_PATH as usize].is_empty());
    }
    if let Some(out) = texts {
        // the output name changes under the output lock while downloads run
        let taken = file.output_lock();
        {
            let d = file.lock();
            for (slot, id) in out.iter_mut().zip([
                text::FILENAME,
                text::SUBJECT,
                text::OUTPUT_FILENAME,
                text::HARD_LINK_PATH,
                text::ORIGNAME,
                text::HASH16K,
                text::PAR_SET_ID,
            ]) {
                *slot = d.texts[id as usize].as_bytes().to_vec();
            }
        }
        if taken {
            file.output_unlock();
        }
    }
    unsafe {
        crate::qarticle::nzbget_rs_article_host(
            file.articles,
            ptr::null_mut(),
            100,
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn bump(ctx: *mut std::ffi::c_void) {
        unsafe { *ctx.cast::<i32>() += 1 };
    }

    #[test]
    fn output_op_matches_the_cpp_host() {
        let f = nzbget_rs_file_new(0);
        let (ni, nl, ns) = (
            ptr::null_mut::<c_int>(),
            ptr::null_mut::<i64>(),
            ptr::null_mut::<*const c_char>(),
        );
        unsafe {
            // 0: the callback runs (no downloads: no output lock needed)
            let mut hits = 0i32;
            let mut ctx = [(&mut hits as *mut i32) as usize as i64];
            host_output_op(
                f,
                0,
                0,
                bump as *const () as usize,
                ptr::null(),
                ptr::null(),
                ni,
                ctx.as_mut_ptr(),
                ns,
            );
            assert_eq!(hits, 1);
            // 8 and 9/8: the output name, the CRC and what the writer needs back
            let out = CString::new("/inter/x.out.tmp").unwrap();
            host_output_op(f, 8, 0, 0, out.as_ptr(), ptr::null(), ni, nl, ns);
            assert_eq!(
                CStr::from_ptr(nzbget_rs_file_text(f, text::OUTPUT_FILENAME)).to_bytes(),
                b"/inter/x.out.tmp"
            );
            let mut ints = [7 as c_int; 2];
            let mut strs = [ptr::null::<c_char>(); 3];
            let nzb = host_output_op(
                f,
                9,
                8,
                0xdead_beef,
                out.as_ptr(),
                ptr::null(),
                ints.as_mut_ptr(),
                nl,
                strs.as_mut_ptr(),
            );
            assert_eq!(nzb, 0);
            assert_eq!(nzbget_rs_file_get(f, field::CRC), 0xdead_beef);
            assert_eq!(ints, [0, 1]); // not hard-linked; no InterDir without options
            assert_eq!(CStr::from_ptr(strs[1]).to_bytes(), b"/inter/x.out.tmp");
            assert!(strs[2].is_null());
            // 9/6: moves articles from success to failed
            nzbget_rs_file_set(f, field::SUCCESS_ARTICLES, 10);
            nzbget_rs_file_set(f, field::FAILED_ARTICLES, 1);
            host_output_op(f, 9, 6, 3, ptr::null(), ptr::null(), ni, nl, ns);
            assert_eq!(nzbget_rs_file_get(f, field::SUCCESS_ARTICLES), 7);
            assert_eq!(nzbget_rs_file_get(f, field::FAILED_ARTICLES), 4);
            // 5: without direct write the output gets initialized; 6 sets it
            let mut ints = [9 as c_int; 2];
            host_output_op(
                f,
                5,
                0,
                0,
                out.as_ptr(),
                ptr::null(),
                ints.as_mut_ptr(),
                nl,
                ns,
            );
            assert_eq!(ints, [0, 0]);
            assert_eq!(nzbget_rs_file_get(f, field::OUTPUT_INITIALIZED), 1);
            // with forced direct write the output is the file's own
            nzbget_rs_file_set(f, field::FORCE_DIRECT_WRITE, 1);
            host_output_op(
                f,
                5,
                0,
                0,
                out.as_ptr(),
                ptr::null(),
                ints.as_mut_ptr(),
                nl,
                ns,
            );
            assert_eq!(ints, [1, 1]);
            // 10: server stats are added
            let mut stat = [3 as c_int, 5, 2];
            host_output_op(
                f,
                10,
                0,
                0,
                ptr::null(),
                ptr::null(),
                stat.as_mut_ptr(),
                nl,
                ns,
            );
            host_output_op(
                f,
                10,
                0,
                0,
                ptr::null(),
                ptr::null(),
                stat.as_mut_ptr(),
                nl,
                ns,
            );
            let mut got = [0 as c_int; 3];
            nzbget_rs_file_op(f, 11, 0, ptr::null(), got.as_mut_ptr(), ns);
            assert_eq!(got, [3, 10, 4]);
            // 9/2: the CRC and output name reset
            host_output_op(f, 9, 2, 0, ptr::null(), ptr::null(), ni, nl, ns);
            assert_eq!(nzbget_rs_file_get(f, field::CRC), 0);
            assert_eq!(
                CStr::from_ptr(nzbget_rs_file_text(f, text::OUTPUT_FILENAME)).to_bytes(),
                b""
            );
            nzbget_rs_file_free(f);
        }
    }

    #[test]
    fn fields_texts_and_ids() {
        let f = nzbget_rs_file_new(0);
        let id = unsafe { nzbget_rs_file_get(f, field::ID) };
        assert!(id > 0);
        unsafe {
            nzbget_rs_file_set(f, field::SIZE, 1000);
            assert_eq!(nzbget_rs_file_get(f, field::REMAINING_SIZE), 1000);
            nzbget_rs_file_set(f, field::REMAINING_SIZE, 400);
            assert_eq!(nzbget_rs_file_get(f, field::SIZE), 1000);
            nzbget_rs_file_set(f, field::CACHED_ARTICLES, 3);
            assert_eq!(nzbget_rs_file_get(f, field::CACHED_ARTICLES), 3);
            let name = CString::new("a.rar").unwrap();
            nzbget_rs_file_set_text(f, text::FILENAME, name.as_ptr());
            assert_eq!(
                CStr::from_ptr(nzbget_rs_file_text(f, text::FILENAME)).to_bytes(),
                b"a.rar"
            );
            assert_eq!(
                CStr::from_ptr(nzbget_rs_file_text(f, text::ORIGNAME)).to_bytes(),
                b""
            );
            nzbget_rs_file_set(f, field::ID, 9000);
            nzbget_rs_file_free(f);
        }
        nzbget_rs_file_ids(1, 0);
        assert!(nzbget_rs_file_ids(0, 0) >= 9000);
    }

    #[test]
    fn groups_and_server_stats() {
        let f = nzbget_rs_file_new(5);
        unsafe {
            let g = CString::new("alt.binaries.x").unwrap();
            nzbget_rs_file_op(f, 2, 0, g.as_ptr(), ptr::null_mut(), ptr::null_mut());
            assert_eq!(
                nzbget_rs_file_op(f, 0, 0, ptr::null(), ptr::null_mut(), ptr::null_mut()),
                1
            );
            let mut s = [ptr::null(); 1];
            nzbget_rs_file_op(f, 1, 0, ptr::null(), ptr::null_mut(), s.as_mut_ptr());
            assert_eq!(CStr::from_ptr(s[0]).to_bytes(), b"alt.binaries.x");
            let mut ints = [2, 10, 1];
            nzbget_rs_file_op(f, 12, 1, ptr::null(), ints.as_mut_ptr(), ptr::null_mut());
            nzbget_rs_file_op(f, 12, 1, ptr::null(), ints.as_mut_ptr(), ptr::null_mut());
            let mut out = [0; 3];
            nzbget_rs_file_op(f, 11, 0, ptr::null(), out.as_mut_ptr(), ptr::null_mut());
            assert_eq!(out, [2, 20, 2]);
            nzbget_rs_file_free(f);
        }
    }

    #[test]
    fn output_lock_only_while_active() {
        let f = nzbget_rs_file_new(6);
        unsafe {
            assert_eq!(
                nzbget_rs_file_op(f, 20, 0, ptr::null(), ptr::null_mut(), ptr::null_mut()),
                0
            );
            nzbget_rs_file_set(f, field::ACTIVE_DOWNLOADS, 1);
            assert_eq!(
                nzbget_rs_file_op(f, 20, 0, ptr::null(), ptr::null_mut(), ptr::null_mut()),
                1
            );
            nzbget_rs_file_op(f, 21, 0, ptr::null(), ptr::null_mut(), ptr::null_mut());
            assert_eq!(
                nzbget_rs_file_op(f, 20, 0, ptr::null(), ptr::null_mut(), ptr::null_mut()),
                1
            );
            nzbget_rs_file_op(f, 21, 0, ptr::null(), ptr::null_mut(), ptr::null_mut());
            nzbget_rs_file_free(f);
        }
    }
}
