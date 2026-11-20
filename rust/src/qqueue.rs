//! The download queue: the queue and history lists, the queue lock, and the
//! daemon's queue, which is the current one. The C++ `DownloadQueue` object is this allocation
//! (`daemon_queue_init` creates it), so a `DownloadQueue*` is a `*mut Queue`.
//!
//! The queue owns its collections and the history its entries. Both are
//! freed here: a collection with its post job and files, a history entry
//! after its inner record, as the C++ destructors did.

use std::ffi::{c_char, c_int, CStr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};

use crate::qdup::{Dup, DupEdit};
use crate::qhist::{
    field as hfield, host_hist_summary, nzbget_rs_hist_free, nzbget_rs_hist_get,
    nzbget_rs_hist_new, nzbget_rs_hist_set, Hist, KIND_DUP, KIND_NZB, KIND_URL,
};
use crate::qlist::{nzbget_rs_ptrlist_free, nzbget_rs_ptrlist_new, nzbget_rs_ptrlist_op, PtrList};
use crate::qnzb::{nzbget_rs_nzb_text, NzbScalars};
use crate::queuesubsys_host::read_cstr;

/// Deletes a collection (null is ignored), as `delete NzbInfo*` did: its
/// post job, then its file list (which frees its files), then the
/// collection itself.
pub fn drop_nzb(p: usize) {
    if p == 0 {
        return;
    }
    let n = p as *mut NzbScalars;
    unsafe {
        let post = crate::qnzb::swap_slot(n, crate::qnzb::Slot::Post, 0);
        crate::qnzb::nzbget_rs_post_free(post as *mut crate::qnzb::Post);
        let files = crate::qnzb::swap_slot(n, crate::qnzb::Slot::FileList, 0);
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
        unsafe { Dup::free(info as *mut Dup) };
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

/// The daemon's queue, also the current one; 0 when none.
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

/// Holds the current (daemon) queue's lock while it lives, as the C++
/// `GuardedDownloadQueue` did. Takes no lock when there is no current queue.
pub(crate) struct CurrentQueueLock(*const Queue);

impl CurrentQueueLock {
    pub(crate) fn acquire() -> Self {
        let q = current();
        if let Some(q) = unsafe { q.as_ref() } {
            q.lock.lock();
        }
        Self(q)
    }

    /// The locked queue's handle, or 0 when there is no current queue.
    pub(crate) fn key(&self) -> usize {
        self.0 as usize
    }
}

impl Drop for CurrentQueueLock {
    fn drop(&mut self) {
        if let Some(q) = unsafe { self.0.as_ref() } {
            q.lock.unlock();
        }
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

/// `new_nzb_handle` hands out an owning handle (a C++
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
fn dup_set_id(d: &Dup, id: i32) {
    d.edit(DupEdit::Id(id));
    crate::idgen::NZB_IDS.raise_floor(id);
}

fn now() -> i64 {
    unsafe { libc::time(std::ptr::null_mut()) as i64 }
}

fn hist_id(h: usize) -> i32 {
    let mut ints = [0 as c_int; 2];
    unsafe { host_hist_summary(h as *const Hist, ints.as_mut_ptr(), std::ptr::null_mut()) };
    ints[1]
}

/// Runs `f` with the current queue's key (0 when there is none), holding
/// its lock until `f` returns. The lock is not reentrant.
pub fn with_current_locked<R>(f: impl FnOnce(usize) -> R) -> R {
    let lock = CurrentQueueLock::acquire();
    f(lock.key())
}

fn queue_ref<'a>(key: usize) -> Option<&'a Queue> {
    unsafe { resolve(key).as_ref() }
}

/// The number of collections in queue `key`.
pub fn queue_len(key: usize) -> usize {
    queue_ref(key).map_or(0, |q| list_len(q.queue()))
}

/// The collection at `idx` in queue `key`, or 0.
pub fn queue_at(key: usize, idx: usize) -> usize {
    queue_ref(key).map_or(0, |q| list_at(q.queue(), idx))
}

/// The queued collection with `id`, or 0.
pub fn queue_find_by_id(key: usize, id: i32) -> usize {
    let Some(q) = queue_ref(key) else {
        return 0;
    };
    let list = q.queue();
    (0..list_len(list))
        .map(|i| list_at(list, i))
        .find(|&n| n != 0 && unsafe { (*(n as *const NzbScalars)).id } == id)
        .unwrap_or(0)
}

/// Removes `nzb` from queue `key` and frees it.
pub fn queue_remove(key: usize, nzb: usize) {
    if let Some(q) = queue_ref(key) {
        // the C++ Remove returned the owner, which was dropped at once
        let gone = list_op(q.queue(), 6, 0, nzb) as usize;
        drop_nzb(gone);
    }
}

/// Moves a handle's collection into queue `key`: before `url` when it is
/// not 0, else at the front (`add_first`) or the back. The handle stays
/// allocated (empty) for `free_nzb_handle`.
///
/// # Safety
/// `handle` is 0 or a live handle from `new_nzb_handle`.
pub unsafe fn queue_insert_handle(key: usize, handle: usize, url: usize, add_first: bool) {
    let Some(q) = queue_ref(key) else {
        return;
    };
    let list = q.queue();
    let nzb = unsafe { handle_take(handle) };
    if nzb != 0 {
        if url != 0 {
            let at = list_op(list, 2, 0, url);
            list_op(list, 5, at, nzb);
        } else {
            list_op(list, if add_first { 4 } else { 3 }, 0, nzb);
        }
    }
}

/// A new collection in an owning handle: (handle, collection).
pub fn new_nzb_handle() -> (usize, usize) {
    let nzb = new_nzb();
    (handle_new(nzb), nzb)
}

/// Adds a handle's collection at the back of queue `key` and frees the
/// handle.
///
/// # Safety
/// `handle` is 0 or a live handle from `new_nzb_handle`.
pub unsafe fn queue_add_handle(key: usize, handle: usize) {
    let Some(q) = queue_ref(key) else {
        return;
    };
    if handle != 0 {
        let nzb = unsafe { handle_take(handle) };
        if nzb != 0 {
            list_op(q.queue(), 3, 0, nzb);
        }
        unsafe { handle_free(handle) };
    }
}

/// Puts queue `key` in `order` (the same collections, reordered).
pub fn queue_reorder(key: usize, order: &[usize]) {
    let Some(q) = queue_ref(key) else {
        return;
    };
    if !order.is_empty() {
        unsafe {
            nzbget_rs_ptrlist_op(
                q.queue(),
                10,
                order.len() as i64,
                0,
                order.as_ptr(),
                std::ptr::null(),
            )
        };
    }
}

/// Frees a handle and the collection still in it.
///
/// # Safety
/// `handle` is 0 or a live handle from `new_nzb_handle`.
pub unsafe fn free_nzb_handle(handle: usize) {
    unsafe { handle_free(handle) };
}

/// The last collection and file ids handed out.
pub fn id_gen_last() -> (i32, i32) {
    use crate::idgen::{FILE_IDS, NZB_IDS};
    (NZB_IDS.last(), FILE_IDS.last())
}

/// Raises the id floors (a negative id leaves its floor) and resets both
/// generators to them.
pub fn id_gen_raise_and_reset(nzb: i32, file: i32) {
    use crate::idgen::{FILE_IDS, NZB_IDS};
    if nzb >= 0 {
        NZB_IDS.raise_floor(nzb);
    }
    if file >= 0 {
        FILE_IDS.raise_floor(file);
    }
    NZB_IDS.reset_to_floor();
    FILE_IDS.reset_to_floor();
}

/// Pushes a handle's collection to the history front of queue `key` (or
/// frees it when there is no queue), then frees the handle.
///
/// # Safety
/// `handle` is 0 or a live handle from `new_nzb_handle`.
pub unsafe fn history_push_handle(key: usize, handle: usize, time: i32) {
    let nzb = unsafe { handle_take(handle) };
    if nzb != 0 {
        if let Some(q) = queue_ref(key) {
            list_op(q.history(), 3, 0, hist_new_nzb(nzb, i64::from(time)));
        } else {
            drop_nzb(nzb);
        }
    }
    unsafe { handle_free(handle) };
}

/// A duplicate record's fields, as `history_push_dup` stores them.
#[derive(Clone, Copy)]
pub struct DupRecord<'a> {
    pub id: i32,
    pub time: i32,
    pub status: i32,
    pub full_hash: u32,
    pub filtered_hash: u32,
    pub dupe_score: i32,
    pub dupe_mode: i32,
    pub size: i64,
    pub name: &'a CStr,
    pub dupe_key: &'a CStr,
}

/// Pushes a duplicate record to the history front of queue `key`.
pub fn history_push_dup(key: usize, rec: &DupRecord<'_>) {
    let Some(q) = queue_ref(key) else {
        return;
    };
    let d = Dup::alloc();
    let r = unsafe { &*d };
    for edit in [
        DupEdit::Status(rec.status.into()),
        DupEdit::FullHash(rec.full_hash),
        DupEdit::FilteredHash(rec.filtered_hash),
        DupEdit::Size(rec.size),
        DupEdit::Score(rec.dupe_score),
        DupEdit::Mode(rec.dupe_mode.into()),
        DupEdit::Name(rec.name.to_bytes()),
        DupEdit::DupeKey(rec.dupe_key.to_bytes()),
    ] {
        r.edit(edit);
    }
    dup_set_id(r, rec.id);
    list_op(q.history(), 3, 0, hist_new_dup(d, i64::from(rec.time)));
}

/// Moves queued `nzb` to the history front of queue `key`.
pub fn history_move_queue_item(key: usize, nzb: usize) {
    let Some(q) = queue_ref(key) else {
        return;
    };
    let nzb = list_op(q.queue(), 6, 0, nzb) as usize;
    if nzb != 0 {
        list_op(q.history(), 4, 0, hist_new_nzb(nzb, now()));
    }
}

/// Replaces history entry `old` (at reverse index `rindex`) with a
/// duplicate record of its collection, with `status`.
///
/// # Safety
/// `old` is a live history entry holding a collection.
pub unsafe fn history_replace_with_dup(key: usize, old: usize, rindex: i32, status: i32) {
    let Some(q) = queue_ref(key) else {
        return;
    };
    let hist = q.history();
    let old = old as *const Hist;
    let n = unsafe { nzbget_rs_hist_get(old, hfield::INFO) } as *const NzbScalars;
    let d = Dup::alloc();
    let r = unsafe { &*d };
    let s = unsafe { &*n };
    dup_set_id(r, s.id);
    let (name, dupe_key) = unsafe {
        (
            read_cstr(nzbget_rs_nzb_text(n, 2)),
            read_cstr(nzbget_rs_nzb_text(n, 7)),
        )
    };
    for edit in [
        DupEdit::Name(&name),
        DupEdit::DupeKey(&dupe_key),
        DupEdit::Score(s.dupeScore),
        DupEdit::Mode(s.dupeMode.into()),
        DupEdit::Size(s.size),
        DupEdit::FullHash(s.fullContentHash),
        DupEdit::FilteredHash(s.filteredContentHash),
        DupEdit::Status(status.into()),
    ] {
        r.edit(edit);
    }
    let time = unsafe { nzbget_rs_hist_get(old, hfield::TIME) };
    let new = hist_new_dup(d, time);
    let at = (list_len(hist) as i64) - 1 - i64::from(rindex);
    if list_op(hist, 1, at, 0) != 0 {
        list_op(hist, 8, at, new);
    } else {
        unsafe { hist_drop_fn(new) };
    }
}

/// Erases (and frees) the history entry at `idx`.
pub fn history_erase_at(key: usize, idx: i32) {
    if let Some(q) = queue_ref(key) {
        list_op(q.history(), 7, i64::from(idx), 0);
    }
}

/// Returns entry `hi`'s collection to the queue front and erases the entry
/// at `idx`; the collection, or 0.
///
/// # Safety
/// `hi` is a live history entry holding a collection.
pub unsafe fn history_return_to_queue(key: usize, hi: usize, idx: i32) -> usize {
    let Some(q) = queue_ref(key) else {
        return 0;
    };
    let h = hi as *const Hist;
    let nzb = unsafe { nzbget_rs_hist_get(h, hfield::INFO) } as usize;
    unsafe { nzbget_rs_hist_set(h, hfield::INFO, 0) };
    list_op(q.queue(), 4, 0, nzb);
    list_op(q.history(), 7, i64::from(idx), 0);
    nzb
}

/// The number of history entries in queue `key`.
pub fn history_len(key: usize) -> usize {
    queue_ref(key).map_or(0, |q| list_len(q.history()))
}

/// The history entry with `id`, or 0.
pub fn history_find_by_id(key: usize, id: i32) -> usize {
    let Some(q) = queue_ref(key) else {
        return 0;
    };
    let hist = q.history();
    (0..list_len(hist))
        .map(|i| list_at(hist, i))
        .find(|&h| h != 0 && hist_id(h) == id)
        .unwrap_or(0)
}

/// A history entry with its summary.
pub struct HistoryEntry {
    pub hi: usize,
    pub kind: i32,
    pub id: i32,
    pub inner: usize,
    pub time: i64,
    /// The entry's name, owned by the entry (null when it has none).
    pub name: *const c_char,
}

/// The history entry at `idx` with its summary, or None.
pub fn history_at(key: usize, idx: usize) -> Option<HistoryEntry> {
    let q = queue_ref(key)?;
    let h = list_at(q.history(), idx);
    if h == 0 {
        return None;
    }
    let mut ints = [0 as c_int; 2];
    let mut strs = [std::ptr::null::<c_char>(); 1];
    let inner =
        unsafe { host_hist_summary(h as *const Hist, ints.as_mut_ptr(), strs.as_mut_ptr()) };
    Some(HistoryEntry {
        hi: h,
        kind: ints[0],
        id: ints[1],
        inner,
        time: unsafe { nzbget_rs_hist_get(h as *const Hist, hfield::TIME) },
        name: strs[0],
    })
}

/// The daemon's queue, or 0.
pub fn daemon_queue() -> usize {
    DAEMON.load(Ordering::SeqCst)
}

/// Creates the daemon's queue anew (freeing the old one) and makes it
/// current; it is not loaded until marked so again.
pub fn daemon_queue_init() {
    let old = DAEMON.swap(alloc_queue() as usize, Ordering::SeqCst);
    unsafe { free_queue(old as *mut Queue) };
    crate::ffi::set_queue_loaded(false);
}

/// Marks the daemon's queue loaded.
pub fn daemon_queue_loaded() {
    crate::ffi::set_queue_loaded(true);
}

/// Leaves no current queue and frees the daemon's.
pub fn daemon_queue_final() {
    let old = DAEMON.swap(0, Ordering::SeqCst);
    crate::ffi::set_queue_loaded(false);
    unsafe { free_queue(old as *mut Queue) };
}

/// A message for collection `nzb` (0: none), printed to the log and kept
/// with it. `add` is `AddMessage`; else `PrintMessage`, which first cuts
/// the text to 1023 bytes. Text after a NUL is ignored.
///
/// # Safety
/// `nzb` is 0 or a live collection.
pub unsafe fn nzb_message(nzb: usize, kind: i32, add: bool, msg: &[u8]) {
    if nzb == 0 {
        return;
    }
    let full = &msg[..msg.iter().position(|&b| b == 0).unwrap_or(msg.len())];
    let text = if add {
        full
    } else {
        &full[..full.len().min(1023)]
    };
    // detail()/info()/warn()/error() cut at 1023 bytes; debug() printed
    // nothing without DEBUG
    if kind != crate::log::MK_DEBUG || crate::ffi::build_debug_enabled() {
        crate::log::log_active_bytes(kind, &text[..text.len().min(1023)]);
    }
    let log_buffer = crate::options::active_options().map_or(0, |o| o.scalars().log_buffer);
    let kept = std::ffi::CString::new(text).unwrap_or_default();
    crate::qnzb::add_message(
        crate::queuesubsys_host::NzbRef::from_addr(nzb),
        kind,
        kept,
        i64::from(now() as c_int),
        log_buffer,
    );
}

/// Starts a collection's live-repair job.
///
/// # Safety
/// `nzb` is 0 or a live collection.
pub unsafe fn start_live_repair(nzb: usize) {
    use crate::jobthread::JobHandle;
    use crate::queuesubsys_host::PostRef;
    if nzb == 0 {
        return;
    }
    let n = nzb as *const NzbScalars;
    let job = JobHandle::new_post(10, PostRef::NONE, unsafe { (*n).id }, true);
    crate::qnzb::swap_slot(n, crate::qnzb::Slot::LiveRepairJob, job.addr());
    if !job.start() {
        unsafe {
            crate::qnzb::swap_slot(n, crate::qnzb::Slot::LiveRepairJob, 0);
            job.free(false);
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

/// The TLS cipher service's JSON reply: `action` 0 starts (on server
/// `arg` when it is not empty), 1 reports job `arg`'s status, else cancels
/// job `arg`.
pub fn tls_cipher(action: i32, arg: &[u8]) -> Vec<u8> {
    let available = !crate::ffi::build_tls_disabled()
        && (cfg!(target_os = "macos") || cfg!(target_os = "linux"));
    if !available {
        let msg = r#"{"Available":false,"State":"unavailable","Message":"TLS cipher optimization requires a TLS-enabled Rust backend and a supported atomic configuration adapter."}"#;
        return msg.as_bytes().to_vec();
    }
    let arg = &arg[..arg.iter().position(|&b| b == 0).unwrap_or(arg.len())];
    if action == 0 {
        if arg.is_empty() {
            unsafe { crate::ffi::nzbget_rs_tls_cipher_start(std::ptr::null()) }
        } else {
            crate::ffi::nzbget_rs_tls_cipher_start_server(atoi(arg))
        }
    } else {
        let arg = std::ffi::CString::new(arg).unwrap_or_default();
        if action == 1 {
            unsafe { crate::ffi::nzbget_rs_tls_cipher_status(arg.as_ptr()) }
        } else {
            unsafe { crate::ffi::nzbget_rs_tls_cipher_cancel(arg.as_ptr()) }
        }
    }
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
        let queue = unsafe { &*(q as *const Queue) };
        let (ql, hl) = (queue.queue(), queue.history());
        assert!(!ql.is_null() && !hl.is_null() && ql != hl);
        unsafe { free_queue(q as *mut Queue) };
    }

    #[test]
    fn queue_ops_insert_find_remove_reorder() {
        let _s = serial();
        let q = alloc_queue() as usize;
        let made: Vec<(usize, usize)> = (0..3).map(|_| new_nzb_handle()).collect();
        let ids: Vec<i32> = made.iter().map(|&(_, n)| nzb_id(n)).collect();
        assert!(ids[0] < ids[1] && ids[1] < ids[2]);
        // add at the back, at the front, then before the first
        unsafe {
            queue_insert_handle(q, made[0].0, 0, false);
            queue_insert_handle(q, made[1].0, 0, true);
            queue_insert_handle(q, made[2].0, made[1].1, false);
            // moved-out handles are freed without touching the collections
            for &(h, _) in &made {
                assert_eq!(*(h as *const usize), 0);
                free_nzb_handle(h);
            }
        }
        let order: Vec<usize> = (0..queue_len(q)).map(|i| queue_at(q, i)).collect();
        assert_eq!(order, vec![made[2].1, made[1].1, made[0].1]);
        assert_eq!(queue_find_by_id(q, ids[1]), made[1].1);
        assert_eq!(queue_find_by_id(q, -5), 0);
        let rev: Vec<usize> = order.iter().rev().copied().collect();
        queue_reorder(q, &rev);
        assert_eq!(queue_at(q, 0), made[0].1);
        queue_remove(q, made[0].1);
        assert_eq!(queue_len(q), 2);
        assert_eq!(queue_find_by_id(q, ids[0]), 0);
        unsafe { free_queue(q as *mut Queue) };
    }

    #[test]
    fn id_generator_state_round_trips() {
        let _s = serial();
        let (base, _) = id_gen_last();
        id_gen_raise_and_reset(base + 1_000_000, -1);
        assert_eq!(id_gen_last().0, base + 1_000_000);
        let n = new_nzb();
        assert_eq!(nzb_id(n), base + 1_000_001);
        drop_nzb(n);
    }

    #[test]
    fn history_ops() {
        let _s = serial();
        let q = alloc_queue() as usize;
        // push a handle's collection, and a duplicate record
        let (h, nzb) = new_nzb_handle();
        let id = nzb_id(nzb);
        unsafe { history_push_handle(q, h, 1234) };
        history_push_dup(
            q,
            &DupRecord {
                id: 777,
                time: 99,
                status: 2,
                full_hash: 5,
                filtered_hash: 6,
                dupe_score: 10,
                dupe_mode: 1,
                size: 4096,
                name: c"dup name",
                dupe_key: c"key",
            },
        );
        assert_eq!(history_len(q), 2);
        let first = history_at(q, 0).unwrap();
        assert_eq!(
            (first.kind, first.id, first.inner, first.time),
            (KIND_NZB, nzb_id(nzb), nzb, 1234)
        );
        let second = history_at(q, 1).unwrap();
        assert_eq!((second.kind, second.id, second.time), (KIND_DUP, 777, 99));
        assert_eq!(unsafe { CStr::from_ptr(second.name) }, c"dup name");
        assert_eq!(history_find_by_id(q, 777), second.hi);
        assert_eq!(history_find_by_id(q, nzb_id(nzb)), first.hi);
        // replace the collection entry (reverse index 1) with a dup record
        unsafe { history_replace_with_dup(q, first.hi, 1, 3) };
        let e = history_at(q, 0).unwrap();
        assert_eq!((e.kind, e.id, e.time), (KIND_DUP, id, 1234));
        // a queued collection moves to the history front, then back
        let (h2, n2) = new_nzb_handle();
        unsafe { queue_add_handle(q, h2) };
        history_move_queue_item(q, n2);
        assert_eq!(queue_len(q), 0);
        let front = history_at(q, 0).unwrap();
        assert_eq!(front.inner, n2);
        assert_eq!(unsafe { history_return_to_queue(q, front.hi, 0) }, n2);
        assert_eq!(queue_at(q, 0), n2);
        assert_eq!(history_len(q), 2);
        history_erase_at(q, 0);
        assert_eq!(history_len(q), 1);
        unsafe { free_queue(q as *mut Queue) };
    }

    #[test]
    fn locked_closure_runs_under_the_current_queue_lock() {
        let _s = serial();
        assert_eq!(daemon_queue(), 0);
        daemon_queue_init();
        let q = daemon_queue();
        assert_eq!(current() as usize, q);
        let seen = with_current_locked(|key| {
            // the lock is held while the closure runs
            assert!(*unsafe { &*(key as *const Queue) }.lock.state());
            key
        });
        assert_eq!(seen, q);
        assert!(!*unsafe { &*(q as *const Queue) }.lock.state());
        daemon_queue_loaded();
        assert!(crate::ffi::is_queue_loaded());
        daemon_queue_final();
        assert_eq!(daemon_queue(), 0);
        assert_eq!(current() as usize, 0);
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
