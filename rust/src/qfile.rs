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
#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_file_new(id: c_int) -> *mut File {
    Box::into_raw(Box::new(File::new(id)))
}

/// # Safety
/// `f` is null or from `nzbget_rs_file_new`, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_file_free(f: *mut File) {
    if !f.is_null() {
        drop(unsafe { Box::from_raw(f) });
    }
}

/// Id generation shared by all files: 0 last generated id, 1 reset to the
/// highest id seen, 2 reset to zero, 3 raise the floor to `v`, 4 generate
/// the next id.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_file_get(f: *const File, id: c_int) -> i64 {
    unsafe { &*f }.get(id)
}

/// # Safety
/// `f` is a live file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_file_set(f: *const File, id: c_int, v: i64) {
    unsafe { &*f }.set(id, v);
}

/// The text field `id`; the pointer stays valid until that field changes.
///
/// # Safety
/// `f` is a live file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_file_text(f: *const File, id: c_int) -> *const c_char {
    let d = unsafe { &*f }.lock();
    d.texts.get(id as usize).map_or(ptr::null(), |s| s.as_ptr())
}

/// Sets text field `id` (null stores an empty string).
///
/// # Safety
/// `f` is a live file; `s` is null or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_file_set_text(f: *const File, id: c_int, s: *const c_char) {
    if let Some(slot) = unsafe { &*f }.lock().texts.get_mut(id as usize) {
        *slot = cstring(s);
    }
}

/// The file's article list (owned by the file).
///
/// # Safety
/// `f` is a live file.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
