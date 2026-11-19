//! The download queue: the queue and history lists, the queue lock, and the
//! daemon's queue, which is the current one. The C++ `DownloadQueue` object is this allocation
//! (host op 25 creates it), so a `DownloadQueue*` is a `*mut Queue`.
//!
//! The queue owns its collections and the history its entries. Both are
//! freed here: a collection with its post job and files, a history entry
//! after its inner record, as the C++ destructors did.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};

use crate::qdup::{nzbget_rs_dup_free, nzbget_rs_dup_new, nzbget_rs_dup_set, Dup};
use crate::qhist::{
    field as hfield, host_hist_summary, nzbget_rs_hist_free, nzbget_rs_hist_get,
    nzbget_rs_hist_new, nzbget_rs_hist_set, Hist, KIND_DUP, KIND_NZB, KIND_URL,
};
use crate::qlist::{nzbget_rs_ptrlist_free, nzbget_rs_ptrlist_new, nzbget_rs_ptrlist_op, PtrList};
use crate::qnzb::{nzbget_rs_nzb_text, NzbScalars};

/// Deletes a collection (null is ignored), as `delete NzbInfo*` did: its
/// post job, then its file list (which frees its files), then the
/// collection itself.
pub fn drop_nzb(p: usize) {
    if p == 0 {
        return;
    }
    let n = p as *mut NzbScalars;
    unsafe {
        let post = crate::qnzb::nzbget_rs_nzb_slot(n, 1, 1, 0);
        crate::qnzb::nzbget_rs_post_free(post as *mut crate::qnzb::Post);
        let files = crate::qnzb::nzbget_rs_nzb_slot(n, 0, 1, 0);
        nzbget_rs_ptrlist_free(files as *mut PtrList);
        crate::qnzb::nzbget_rs_nzb_scalars_free(n);
    }
}

unsafe extern "C" fn nzb_drop_fn(p: usize) {
    drop_nzb(p);
}

/// Frees a history entry and its inner record (the C++ `~HistoryInfo` and
/// its operator delete).
unsafe extern "C" fn hist_drop_fn(p: usize) {
    let h = p as *mut Hist;
    let kind = unsafe { nzbget_rs_hist_get(h, hfield::KIND) } as i32;
    let info = unsafe { nzbget_rs_hist_get(h, hfield::INFO) } as usize;
    if (kind == KIND_NZB || kind == KIND_URL) && info != 0 {
        drop_nzb(info);
    } else if kind == KIND_DUP && info != 0 {
        unsafe { nzbget_rs_dup_free(info as *mut Dup) };
    }
    unsafe { nzbget_rs_hist_free(h) };
}

/// A lock that is taken and released in separate calls (the C++ guard).
#[derive(Default)]
struct RawLock {
    held: Mutex<bool>,
    cv: Condvar,
}

impl RawLock {
    fn state(&self) -> MutexGuard<'_, bool> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock(&self) {
        let mut held = self.state();
        while *held {
            held = self.cv.wait(held).unwrap_or_else(|e| e.into_inner());
        }
        *held = true;
    }

    fn unlock(&self) {
        *self.state() = false;
        self.cv.notify_one();
    }
}

pub struct Queue {
    queue: *mut PtrList,
    history: *mut PtrList,
    lock: RawLock,
}

unsafe impl Send for Queue {}
unsafe impl Sync for Queue {}

impl Queue {
    fn queue(&self) -> *const PtrList {
        self.queue
    }

    fn history(&self) -> *const PtrList {
        self.history
    }
}

/// The daemon's queue (host op 25), also the current one; 0 when none.
static DAEMON: AtomicUsize = AtomicUsize::new(0);

fn alloc_queue() -> *mut Queue {
    Box::into_raw(Box::new(Queue {
        queue: nzbget_rs_ptrlist_new(nzb_drop_fn),
        history: nzbget_rs_ptrlist_new(hist_drop_fn),
        lock: RawLock::default(),
    }))
}

unsafe fn free_queue(q: *mut Queue) {
    if q.is_null() {
        return;
    }
    let q = unsafe { Box::from_raw(q) };
    // as ~DownloadQueue: the history first, then the queue
    unsafe {
        nzbget_rs_ptrlist_free(q.history);
        nzbget_rs_ptrlist_free(q.queue);
    }
}

fn resolve(key: usize) -> *const Queue {
    key as *const Queue
}

fn current() -> *const Queue {
    DAEMON.load(Ordering::SeqCst) as *const Queue
}

/// Queue operations for the host's `DownloadQueue` (`q` a queue, or 0):
///  1 its queue list; 2 its history list; 3 lock; 4 unlock; 9 the current
///  (daemon) queue.
pub extern "C" fn nzbget_rs_queue_op(q: usize, op: c_int) -> usize {
    let queue = || unsafe { resolve(q).as_ref() };
    match op {
        1 => queue().map_or(0, |q| q.queue() as usize),
        2 => queue().map_or(0, |q| q.history() as usize),
        3 => {
            if let Some(q) = queue() {
                q.lock.lock();
            }
            0
        }
        4 => {
            if let Some(q) = queue() {
                q.lock.unlock();
            }
            0
        }
        9 => current() as usize,
        _ => 0,
    }
}

fn list_op(l: *const PtrList, op: c_int, a: i64, p: usize) -> i64 {
    if l.is_null() {
        return 0;
    }
    unsafe { nzbget_rs_ptrlist_op(l, op, a, p, std::ptr::null(), std::ptr::null()) }
}

fn list_len(l: *const PtrList) -> usize {
    list_op(l, 0, 0, 0) as usize
}

fn list_at(l: *const PtrList, i: usize) -> usize {
    list_op(l, 1, i as i64, 0) as usize
}

/// A new collection, as `new NzbInfo` (the next id, an empty file list).
pub fn new_nzb() -> usize {
    let n = crate::qnzb::nzbget_rs_nzb_scalars_new();
    unsafe { crate::qnzb::nzbget_rs_nzb_init(n) };
    n as usize
}

/// Host op 22 sub-op 3 hands out an owning handle (a C++
/// `std::unique_ptr<NzbInfo>*` before): a box holding the collection, or 0
/// once it was moved out.
fn handle_new(nzb: usize) -> usize {
    Box::into_raw(Box::new(nzb)) as usize
}

unsafe fn handle_take(h: usize) -> usize {
    if h == 0 {
        return 0;
    }
    std::mem::take(unsafe { &mut *(h as *mut usize) })
}

unsafe fn handle_free(h: usize) {
    if h != 0 {
        let nzb = *unsafe { Box::from_raw(h as *mut usize) };
        drop_nzb(nzb);
    }
}

fn hist_new_nzb(nzb: usize, time: i64) -> usize {
    let h = nzbget_rs_hist_new();
    let kind = if unsafe { (*(nzb as *const NzbScalars)).kind } == 0 {
        KIND_NZB
    } else {
        KIND_URL
    };
    unsafe {
        nzbget_rs_hist_set(h, hfield::INFO, nzb as i64);
        nzbget_rs_hist_set(h, hfield::KIND, i64::from(kind));
        nzbget_rs_hist_set(h, hfield::TIME, time);
    }
    h as usize
}

fn hist_new_dup(dup: *mut Dup, time: i64) -> usize {
    let h = nzbget_rs_hist_new();
    unsafe {
        nzbget_rs_hist_set(h, hfield::INFO, dup as i64);
        nzbget_rs_hist_set(h, hfield::KIND, i64::from(KIND_DUP));
        nzbget_rs_hist_set(h, hfield::TIME, time);
    }
    h as usize
}

/// `DupInfo::SetId`: the id also raises the collection id floor.
unsafe fn dup_set_id(d: *const Dup, id: i32) {
    unsafe { nzbget_rs_dup_set(d, 5, i64::from(id), std::ptr::null()) };
    crate::qnzb::nzbget_rs_nzb_ids(3, id);
}

fn now() -> i64 {
    unsafe { libc::time(std::ptr::null_mut()) as i64 }
}

fn hist_id(h: usize) -> i32 {
    let mut ints = [0 as c_int; 2];
    unsafe { host_hist_summary(h as *const Hist, ints.as_mut_ptr(), std::ptr::null_mut()) };
    ints[1]
}

/// Host op 20: runs `f(ctx, queue)` holding the current queue's lock.
///
/// # Safety
/// `f` is a callable `void(void*, uintptr_t)`.
pub unsafe fn host_locked(f: usize, ctx: usize) -> usize {
    if f == 0 {
        return 0;
    }
    let f: unsafe extern "C" fn(*mut c_void, usize) = unsafe { std::mem::transmute(f) };
    let q = current();
    struct Unlock(*const Queue);
    impl Drop for Unlock {
        fn drop(&mut self) {
            if let Some(q) = unsafe { self.0.as_ref() } {
                q.lock.unlock();
            }
        }
    }
    if let Some(q) = unsafe { q.as_ref() } {
        q.lock.lock();
    }
    let _unlock = Unlock(q);
    unsafe { f(ctx as *mut c_void, q as usize) };
    0
}

/// Host op 21: the queue's length (a1 < 0) or its item at a1.
pub fn host_queue_at(key: usize, a1: i32) -> usize {
    let Some(q) = (unsafe { resolve(key).as_ref() }) else {
        return 0;
    };
    if a1 < 0 {
        list_len(q.queue())
    } else {
        list_at(q.queue(), a1 as usize)
    }
}

/// Host op 22: queue edits and id generation (sub-ops in
/// `QueueSubsysHost`: 0 find, 1 remove and delete, 2 insert a handle's
/// collection, 3 new handle, 4 add a handle's collection and free the
/// handle, 5 reorder, 6 free a handle, 7 id generator state).
///
/// # Safety
/// Pointers are as the host op documents them.
pub unsafe fn host_queue_op(
    a1: i32,
    a2: i32,
    p1: usize,
    p2: usize,
    ints: *mut c_int,
    i64s: *mut i64,
) -> usize {
    use crate::qfile::nzbget_rs_file_ids;
    use crate::qnzb::nzbget_rs_nzb_ids;
    match a1 {
        7 => {
            if a2 == 0 {
                unsafe {
                    *ints = nzbget_rs_nzb_ids(0, 0);
                    *ints.add(1) = nzbget_rs_file_ids(0, 0);
                }
            } else {
                let (nzb, file) = unsafe { (*ints, *ints.add(1)) };
                if nzb >= 0 {
                    nzbget_rs_nzb_ids(3, nzb);
                }
                if file >= 0 {
                    nzbget_rs_file_ids(3, file);
                }
                nzbget_rs_nzb_ids(1, 0);
                nzbget_rs_file_ids(1, 0);
            }
            return 0;
        }
        3 => {
            let nzb = new_nzb();
            unsafe { *i64s = nzb as i64 };
            return handle_new(nzb);
        }
        6 => {
            unsafe { handle_free(p2) };
            return 0;
        }
        _ => {}
    }
    let Some(q) = (unsafe { resolve(p1).as_ref() }) else {
        return 0;
    };
    let list = q.queue();
    match a1 {
        0 => (0..list_len(list))
            .map(|i| list_at(list, i))
            .find(|&n| n != 0 && unsafe { (*(n as *const NzbScalars)).id } == a2)
            .unwrap_or(0),
        1 => {
            // the C++ Remove returned the owner, which was dropped at once
            let gone = list_op(list, 6, 0, p2) as usize;
            drop_nzb(gone);
            0
        }
        2 => {
            let nzb = unsafe { handle_take(p2) };
            if nzb != 0 {
                let url = unsafe { *i64s } as usize;
                if url != 0 {
                    let at = list_op(list, 2, 0, url);
                    list_op(list, 5, at, nzb);
                } else {
                    list_op(list, if a2 != 0 { 4 } else { 3 }, 0, nzb);
                }
            }
            0
        }
        4 => {
            if p2 != 0 {
                let nzb = unsafe { handle_take(p2) };
                if nzb != 0 {
                    list_op(list, 3, 0, nzb);
                }
                unsafe { handle_free(p2) };
            }
            0
        }
        5 => {
            if p2 != 0 && a2 >= 0 {
                unsafe {
                    nzbget_rs_ptrlist_op(
                        list,
                        10,
                        i64::from(a2),
                        0,
                        p2 as *const usize,
                        std::ptr::null(),
                    )
                };
            }
            0
        }
        _ => 0,
    }
}

/// Host op 23: history (a1 >= 0 the entry at a1 with its summary; -1 count,
/// -2 find by id, -3 move a queued collection to the front of history,
/// -4 replace an entry with a duplicate record, -5 erase, -6 return an
/// entry's collection to the queue front, -7 push a handle's collection,
/// -8 push a duplicate record).
///
/// # Safety
/// Pointers are as the host op documents them.
#[allow(clippy::too_many_arguments)]
pub unsafe fn host_history_op(
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
    let q = unsafe { resolve(p1).as_ref() };
    if a1 == -7 {
        let nzb = unsafe { handle_take(p2) };
        if nzb != 0 {
            if let Some(q) = q {
                list_op(q.history(), 3, 0, hist_new_nzb(nzb, i64::from(a2)));
            } else {
                drop_nzb(nzb);
            }
        }
        unsafe { handle_free(p2) };
        return 0;
    }
    let Some(q) = q else {
        return 0;
    };
    let hist = q.history();
    match a1 {
        -8 => {
            let d = nzbget_rs_dup_new();
            let int = |i: usize| i64::from(unsafe { *ints.add(i) });
            let text = |s: *const c_char| if s.is_null() { c"".as_ptr() } else { s };
            unsafe {
                nzbget_rs_dup_set(d, 2, int(2), std::ptr::null());
                nzbget_rs_dup_set(d, 3, int(3) as u32 as i64, std::ptr::null());
                nzbget_rs_dup_set(d, 4, int(4) as u32 as i64, std::ptr::null());
                nzbget_rs_dup_set(d, 6, *i64s, std::ptr::null());
                nzbget_rs_dup_set(d, 1, int(5), std::ptr::null());
                nzbget_rs_dup_set(d, 0, int(6), std::ptr::null());
                nzbget_rs_dup_set(d, 7, 0, text(s1));
                nzbget_rs_dup_set(d, 8, 0, text(s2));
                dup_set_id(d, *ints);
            }
            list_op(hist, 3, 0, hist_new_dup(d, int(1)));
            0
        }
        -1 => list_len(hist),
        -2 => (0..list_len(hist))
            .map(|i| list_at(hist, i))
            .find(|&h| h != 0 && hist_id(h) == a2)
            .unwrap_or(0),
        -3 => {
            let nzb = list_op(q.queue(), 6, 0, p2) as usize;
            if nzb != 0 {
                list_op(hist, 4, 0, hist_new_nzb(nzb, now()));
            }
            0
        }
        -4 => {
            let old = p2 as *const Hist;
            let n = unsafe { nzbget_rs_hist_get(old, hfield::INFO) } as *const NzbScalars;
            let d = nzbget_rs_dup_new();
            unsafe {
                let s = &*n;
                dup_set_id(d, s.id);
                nzbget_rs_dup_set(d, 7, 0, nzbget_rs_nzb_text(n, 2));
                nzbget_rs_dup_set(d, 8, 0, nzbget_rs_nzb_text(n, 7));
                nzbget_rs_dup_set(d, 1, i64::from(s.dupeScore), std::ptr::null());
                nzbget_rs_dup_set(d, 0, i64::from(s.dupeMode), std::ptr::null());
                nzbget_rs_dup_set(d, 6, s.size, std::ptr::null());
                nzbget_rs_dup_set(d, 3, i64::from(s.fullContentHash), std::ptr::null());
                nzbget_rs_dup_set(d, 4, i64::from(s.filteredContentHash), std::ptr::null());
                nzbget_rs_dup_set(d, 2, i64::from(a2), std::ptr::null());
            }
            let time = unsafe { nzbget_rs_hist_get(old, hfield::TIME) };
            let new = hist_new_dup(d, time);
            let at = (list_len(hist) as i64) - 1 - i64::from(unsafe { *ints });
            if list_op(hist, 1, at, 0) != 0 {
                list_op(hist, 8, at, new);
            } else {
                unsafe { hist_drop_fn(new) };
            }
            0
        }
        -5 => {
            list_op(hist, 7, i64::from(a2), 0);
            0
        }
        -6 => {
            let h = p2 as *const Hist;
            let nzb = unsafe { nzbget_rs_hist_get(h, hfield::INFO) } as usize;
            unsafe { nzbget_rs_hist_set(h, hfield::INFO, 0) };
            list_op(q.queue(), 4, 0, nzb);
            list_op(hist, 7, i64::from(a2), 0);
            nzb
        }
        _ if a1 >= 0 => {
            let h = list_at(hist, a1 as usize);
            if h == 0 {
                return 0;
            }
            let inner = unsafe { host_hist_summary(h as *const Hist, ints, strs) };
            unsafe {
                *i64s = inner as i64;
                *i64s.add(1) = nzbget_rs_hist_get(h as *const Hist, hfield::TIME);
            }
            h
        }
        _ => 0,
    }
}

/// Host op 25: the daemon's queue (0 get, 1 create anew and make current,
/// 2 mark loaded, 3 no current queue and free it).
pub fn host_daemon_queue(a1: i32) -> usize {
    match a1 {
        0 => DAEMON.load(Ordering::SeqCst),
        1 => {
            let old = DAEMON.swap(alloc_queue() as usize, Ordering::SeqCst);
            unsafe { free_queue(old as *mut Queue) };
            // a reload creates the queue anew: not loaded until it is again
            crate::ffi::nzbget_rs_thread_count_op(4);
            0
        }
        2 => {
            crate::ffi::nzbget_rs_thread_count_op(3);
            0
        }
        3 => {
            let old = DAEMON.swap(0, Ordering::SeqCst);
            crate::ffi::nzbget_rs_thread_count_op(4);
            unsafe { free_queue(old as *mut Queue) };
            0
        }
        _ => 0,
    }
}

/// Host op 2: a message for a collection, printed to the log and kept with
/// it (a2 == 1 `AddMessage`, else `PrintMessage`, which first cuts the text
/// to 1023 bytes).
///
/// # Safety
/// `nzb` is 0 or a live collection; `s1` is null or NUL-terminated.
pub unsafe fn host_nzb_message(kind: i32, a2: i32, nzb: usize, s1: *const c_char) {
    if nzb == 0 {
        return;
    }
    let full = if s1.is_null() {
        &[][..]
    } else {
        unsafe { CStr::from_ptr(s1) }.to_bytes()
    };
    let text = if a2 == 1 {
        full
    } else {
        &full[..full.len().min(1023)]
    };
    // detail()/info()/warn()/error() cut at 1023 bytes; debug() printed
    // nothing without DEBUG
    if kind != crate::log::MK_DEBUG || crate::ffi::build_debug_enabled() {
        let logged = std::ffi::CString::new(&text[..text.len().min(1023)]).unwrap_or_default();
        unsafe { crate::log::nzbget_rs_log_active_emit(kind, logged.as_ptr()) };
    }
    let opts = unsafe {
        crate::options::nzbget_rs_options_scalars(crate::options::nzbget_rs_options_active())
            .as_ref()
    };
    let ints = [now() as c_int, opts.map_or(0, |o| o.log_buffer)];
    let kept = std::ffi::CString::new(text).unwrap_or_default();
    unsafe {
        crate::qnzb::nzbget_rs_nzb_msgs(
            nzb as *const NzbScalars,
            0,
            kind,
            std::ptr::null(),
            kept.as_ptr(),
            ints.as_ptr(),
            None,
            0,
        )
    };
}

/// Host op 6 sub-op 5: starts a collection's live-repair job.
///
/// # Safety
/// `nzb` is 0 or a live collection.
pub unsafe fn host_start_live_repair(nzb: usize) {
    use crate::jobthread::{nzbget_rs_job_free, nzbget_rs_job_new_post, nzbget_rs_job_start};
    if nzb == 0 {
        return;
    }
    let n = nzb as *const NzbScalars;
    unsafe {
        let job = nzbget_rs_job_new_post(10, 0, (*n).id, 1);
        crate::qnzb::nzbget_rs_nzb_slot(n, 3, 1, job as usize);
        if nzbget_rs_job_start(job) == 0 {
            crate::qnzb::nzbget_rs_nzb_slot(n, 3, 1, 0);
            nzbget_rs_job_free(job, 0);
        }
    }
}

/// C `atoi`: optional blanks and sign, then digits.
fn atoi(s: &[u8]) -> c_int {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let neg = i < s.len() && s[i] == b'-';
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        i += 1;
    }
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + i64::from(s[i] - b'0')).min(i64::from(u32::MAX) + 1);
        i += 1;
    }
    (if neg { -v } else { v }) as c_int
}

/// Host op 7: the TLS cipher service (a1 0 start, with s1 a server id when
/// not empty; 1 status of job s1; else cancel job s1). The JSON reply goes
/// to `*out` (an `RsBuf`) when `out` is not null. Returns 1.
///
/// # Safety
/// `s1` is null or NUL-terminated; `out` is null or a writable `RsBuf`.
pub unsafe fn host_tls_cipher(a1: i32, out: usize, s1: *const c_char) -> usize {
    use crate::ffi::RsBuf;
    let available = !crate::ffi::build_tls_disabled()
        && (cfg!(target_os = "macos") || cfg!(target_os = "linux"));
    let buf: RsBuf = if !available {
        let msg = r#"{"Available":false,"State":"unavailable","Message":"TLS cipher optimization requires a TLS-enabled Rust backend and a supported atomic configuration adapter."}"#;
        unsafe { crate::ffi::nzbget_rs_alloc_buf(msg.as_ptr().cast(), msg.len()) }
    } else if a1 == 0 {
        let id = (!s1.is_null()).then(|| unsafe { CStr::from_ptr(s1) }.to_bytes());
        match id {
            Some(id) if !id.is_empty() => crate::ffi::nzbget_rs_tls_cipher_start_server(atoi(id)),
            _ => unsafe { crate::ffi::nzbget_rs_tls_cipher_start(std::ptr::null()) },
        }
    } else if a1 == 1 {
        unsafe { crate::ffi::nzbget_rs_tls_cipher_status(s1) }
    } else {
        unsafe {
            crate::ffi::nzbget_rs_tls_cipher_cancel(if s1.is_null() { c"".as_ptr() } else { s1 })
        }
    };
    if out != 0 {
        unsafe { std::ptr::write(out as *mut RsBuf, buf) };
    } else {
        unsafe { crate::ffi::nzbget_rs_free(buf) };
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    // tests share the global current queue and id generators
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn nzb_id(n: usize) -> i32 {
        unsafe { (*(n as *const NzbScalars)).id }
    }

    #[test]
    fn queue_lists_are_distinct_and_free() {
        let q = alloc_queue() as usize;
        let (ql, hl) = (nzbget_rs_queue_op(q, 1), nzbget_rs_queue_op(q, 2));
        assert!(ql != 0 && hl != 0 && ql != hl);
        assert_eq!(nzbget_rs_queue_op(0, 1), 0);
        unsafe { free_queue(q as *mut Queue) };
    }

    #[test]
    fn queue_ops_insert_find_remove_reorder() {
        let _s = serial();
        let q = alloc_queue() as usize;
        let mut made = Vec::new();
        for _ in 0..3 {
            let mut i64s = [0i64];
            let h = unsafe { host_queue_op(3, 0, 0, 0, std::ptr::null_mut(), i64s.as_mut_ptr()) };
            made.push((h, i64s[0] as usize));
        }
        let ids: Vec<i32> = made.iter().map(|&(_, n)| nzb_id(n)).collect();
        assert!(ids[0] < ids[1] && ids[1] < ids[2]);
        // add at the back, at the front, then before the first
        let (ni, nl, ns): (*mut c_int, *mut i64, *mut *const c_char) = (
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        let _ = ns;
        unsafe {
            let mut url = [0i64];
            host_queue_op(2, 0, q, made[0].0, ni, url.as_mut_ptr());
            host_queue_op(2, 1, q, made[1].0, ni, url.as_mut_ptr());
            let mut url = [made[1].1 as i64];
            host_queue_op(2, 0, q, made[2].0, ni, url.as_mut_ptr());
            // moved-out handles are freed without touching the collections
            for &(h, _) in &made {
                assert_eq!(*(h as *const usize), 0);
                host_queue_op(6, 0, 0, h, ni, nl);
            }
        }
        let order: Vec<usize> = (0..host_queue_at(q, -1))
            .map(|i| host_queue_at(q, i as i32))
            .collect();
        assert_eq!(order, vec![made[2].1, made[1].1, made[0].1]);
        assert_eq!(unsafe { host_queue_op(0, ids[1], q, 0, ni, nl) }, made[1].1);
        assert_eq!(unsafe { host_queue_op(0, -5, q, 0, ni, nl) }, 0);
        let rev: Vec<usize> = order.iter().rev().copied().collect();
        unsafe { host_queue_op(5, 3, q, rev.as_ptr() as usize, ni, nl) };
        assert_eq!(host_queue_at(q, 0), made[0].1);
        unsafe { host_queue_op(1, 0, q, made[0].1, ni, nl) };
        assert_eq!(host_queue_at(q, -1), 2);
        assert_eq!(unsafe { host_queue_op(0, ids[0], q, 0, ni, nl) }, 0);
        unsafe { free_queue(q as *mut Queue) };
    }

    #[test]
    fn id_generator_state_round_trips() {
        let _s = serial();
        let nl: *mut i64 = std::ptr::null_mut();
        let mut ints = [0i32; 2];
        unsafe { host_queue_op(7, 0, 0, 0, ints.as_mut_ptr(), nl) };
        let base = ints[0];
        let mut raise = [base + 1_000_000, -1];
        unsafe { host_queue_op(7, 1, 0, 0, raise.as_mut_ptr(), nl) };
        unsafe { host_queue_op(7, 0, 0, 0, ints.as_mut_ptr(), nl) };
        assert_eq!(ints[0], base + 1_000_000);
        let n = new_nzb();
        assert_eq!(nzb_id(n), base + 1_000_001);
        drop_nzb(n);
    }

    #[test]
    fn history_ops() {
        let _s = serial();
        let q = alloc_queue() as usize;
        let (ni, nl, ns): (*mut c_int, *mut i64, *mut *const c_char) = (
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        let _ = ns;
        let (mut ints, mut i64s, mut strs) = ([0i32; 7], [0i64; 2], [std::ptr::null(); 1]);
        // push a handle's collection, and a duplicate record
        let mut tmp = [0i64];
        let h = unsafe { host_queue_op(3, 0, 0, 0, ni, tmp.as_mut_ptr()) };
        let nzb = tmp[0] as usize;
        let id = nzb_id(nzb);
        unsafe {
            host_history_op(
                -7,
                1234,
                q,
                h,
                std::ptr::null(),
                std::ptr::null(),
                ni,
                nl,
                ns,
            )
        };
        let mut dup_ints = [777, 99, 2, 5, 6, 10, 1];
        let mut size = [4096i64];
        unsafe {
            host_history_op(
                -8,
                0,
                q,
                0,
                c"dup name".as_ptr(),
                c"key".as_ptr(),
                dup_ints.as_mut_ptr(),
                size.as_mut_ptr(),
                ns,
            )
        };
        let hist = |a1: i32,
                    a2: i32,
                    p2: usize,
                    ints: &mut [i32],
                    i64s: &mut [i64],
                    strs: &mut [*const c_char]| unsafe {
            host_history_op(
                a1,
                a2,
                q,
                p2,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                strs.as_mut_ptr(),
            )
        };
        assert_eq!(hist(-1, 0, 0, &mut ints, &mut i64s, &mut strs), 2);
        let first = hist(0, 0, 0, &mut ints, &mut i64s, &mut strs);
        assert_eq!(
            (ints[0], ints[1], i64s[0] as usize, i64s[1]),
            (KIND_NZB, nzb_id(nzb), nzb, 1234)
        );
        let second = hist(1, 0, 0, &mut ints, &mut i64s, &mut strs);
        assert_eq!((ints[0], ints[1], i64s[1]), (KIND_DUP, 777, 99));
        assert_eq!(unsafe { CStr::from_ptr(strs[0]) }, c"dup name");
        assert_eq!(hist(-2, 777, 0, &mut ints, &mut i64s, &mut strs), second);
        assert_eq!(
            hist(-2, nzb_id(nzb), 0, &mut ints, &mut i64s, &mut strs),
            first
        );
        // replace the collection entry (reverse index 1) with a dup record
        ints[0] = 1;
        hist(-4, 3, first, &mut ints, &mut i64s, &mut strs);
        hist(0, 0, 0, &mut ints, &mut i64s, &mut strs);
        assert_eq!((ints[0], ints[1], i64s[1]), (KIND_DUP, id, 1234));
        // a queued collection moves to the history front, then back
        let mut tmp = [0i64];
        let h2 = unsafe { host_queue_op(3, 0, 0, 0, ni, tmp.as_mut_ptr()) };
        unsafe { host_queue_op(4, 0, q, h2, ni, nl) };
        let n2 = tmp[0] as usize;
        hist(-3, 0, n2, &mut ints, &mut i64s, &mut strs);
        assert_eq!(host_queue_at(q, -1), 0);
        let front = hist(0, 0, 0, &mut ints, &mut i64s, &mut strs);
        assert_eq!(i64s[0] as usize, n2);
        assert_eq!(hist(-6, 0, front, &mut ints, &mut i64s, &mut strs), n2);
        assert_eq!(host_queue_at(q, 0), n2);
        assert_eq!(hist(-1, 0, 0, &mut ints, &mut i64s, &mut strs), 2);
        hist(-5, 0, 0, &mut ints, &mut i64s, &mut strs);
        assert_eq!(hist(-1, 0, 0, &mut ints, &mut i64s, &mut strs), 1);
        unsafe { free_queue(q as *mut Queue) };
    }

    static CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn locked_cb(ctx: *mut c_void, q: usize) {
        let q = q as *const Queue;
        // the lock is held while the callback runs
        assert!(*unsafe { &*q }.lock.state());
        CALLS.fetch_add(unsafe { *(ctx as *const usize) }, Ordering::SeqCst);
    }

    #[test]
    fn locked_closure_runs_under_the_current_queue_lock() {
        let _s = serial();
        assert_eq!(host_daemon_queue(0), 0);
        host_daemon_queue(1);
        let q = host_daemon_queue(0);
        assert_eq!(nzbget_rs_queue_op(0, 9), q);
        let ctx = 5usize;
        unsafe {
            host_locked(
                locked_cb as *const () as usize,
                &ctx as *const usize as usize,
            )
        };
        assert_eq!(CALLS.load(Ordering::SeqCst), 5);
        assert!(!*unsafe { &*(q as *const Queue) }.lock.state());
        host_daemon_queue(2);
        assert!(crate::ffi::is_queue_loaded());
        host_daemon_queue(3);
        assert_eq!(host_daemon_queue(0), 0);
        assert_eq!(nzbget_rs_queue_op(0, 9), 0);
        assert!(!crate::ffi::is_queue_loaded());
    }

    #[test]
    fn raw_lock_excludes_other_threads() {
        let lock = std::sync::Arc::new(RawLock::default());
        lock.lock();
        let l2 = lock.clone();
        let t = std::thread::spawn(move || {
            l2.lock();
            l2.unlock();
        });
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!t.is_finished());
        lock.unlock();
        t.join().unwrap();
    }

    #[test]
    fn atoi_matches_c() {
        assert_eq!(atoi(b"12"), 12);
        assert_eq!(atoi(b"  -3x"), -3);
        assert_eq!(atoi(b"x1"), 0);
        assert_eq!(atoi(b"+7"), 7);
    }
}
