//! A queued collection's numeric and status fields, which were C++ `NzbInfo`
//! members, in one Rust allocation with C layout.
//!
//! The C++ `NzbInfo` keeps a pointer to it and reads and writes the fields in
//! place (`m_s->field`), under the download-queue lock as before, so its code
//! and the host operations keep their behaviour while the data moves to Rust.
//! The collection's strings live next to them, behind a mutex, reached through
//! `nzbget_rs_nzb_text` / `nzbget_rs_nzb_set_text`. Collections still live in
//! C++ for now.

#![allow(non_snake_case)]

use std::collections::BTreeSet;
use std::ffi::{c_char, c_int, CStr, CString};
use std::sync::Mutex;

/// Field order and types must match `NzbgetRsNzbScalars` in `nzbget_rs.h`.
#[repr(C)]
pub struct NzbScalars {
    pub id: i32,
    pub kind: i32,
    pub fileCount: i32,
    pub parkedFileCount: i32,
    pub size: i64,
    pub remainingSize: i64,
    pub pausedFileCount: i32,
    pub pausedSize: i64,
    pub remainingParCount: i32,
    pub activeDownloads: i32,
    pub successSize: i64,
    pub failedSize: i64,
    pub currentSuccessSize: i64,
    pub currentFailedSize: i64,
    pub parSize: i64,
    pub parSuccessSize: i64,
    pub parFailedSize: i64,
    pub parCurrentSuccessSize: i64,
    pub parCurrentFailedSize: i64,
    pub totalArticles: i32,
    pub successArticles: i32,
    pub failedArticles: i32,
    pub currentSuccessArticles: i32,
    pub currentFailedArticles: i32,
    pub dupeRecoveredArticles: i32,
    pub dupeFailedRun: i32,
    pub dupeRecoveredBytes: i64,
    pub dupeRecoveredHoles: i32,
    pub dupeAttemptedArticles: i32,
    pub dupeUnsourcedArticles: i32,
    pub dupeFailoverChecked: i32,
    pub deadPickProbed: bool,
    pub dupeParDeferState: i32,
    pub dupeNoParNoted: bool,
    pub minTime: i64,
    pub maxTime: i64,
    pub priority: i32,
    pub extraPriority: i32,
    pub directRenameStatus: i32,
    pub parRenameStatus: i32,
    pub rarRenameStatus: i32,
    pub parStatus: i32,
    pub directUnpackStatus: i32,
    pub unpackStatus: i32,
    pub cleanupStatus: i32,
    pub moveStatus: i32,
    pub postUnpackRenamingStatus: i32,
    pub postDownloadRenamingStatus: i32,
    pub deleteStatus: i32,
    pub markStatus: i32,
    pub urlStatus: i32,
    pub extraParBlocks: i32,
    pub addUrlPaused: bool,
    pub deletePaused: bool,
    pub manyDupeFiles: bool,
    pub deleting: bool,
    pub parking: bool,
    pub avoidHistory: bool,
    pub healthPaused: bool,
    pub cleanupDisk: bool,
    pub unpackCleanedUpDisk: bool,
    pub dupeScore: i32,
    pub dupeMode: i32,
    pub dupeHint: i32,
    pub fullContentHash: u32,
    pub filteredContentHash: u32,
    pub idMessageGen: i32,
    pub parRepaired: bool,
    pub unpackTried: bool,
    pub lastUnpackStatus: i32,
    pub downloadedSize: i64,
    pub downloadStartTime: i64,
    pub downloadStartSec: i32,
    pub downloadSec: i32,
    pub postTotalSec: i32,
    pub parSec: i32,
    pub repairSec: i32,
    pub unpackSec: i32,
    pub liveUnpackSec: i32,
    pub liveParSec: i32,
    pub reprocess: bool,
    pub changed: bool,
    pub queueScriptTime: i64,
    pub parFull: bool,
    pub cachedMessageCount: i32,
    pub feedId: i32,
    pub allFirst: bool,
    pub waitingPar: bool,
    pub loadingPar: bool,
    pub desiredServerId: i32,
    pub skipScriptProcessing: bool,
    pub skipDiskWrite: bool,
    pub autoCategory: bool,
}

impl Default for NzbScalars {
    fn default() -> Self {
        Self {
            id: 0,
            kind: 0,
            fileCount: 0,
            parkedFileCount: 0,
            size: 0,
            remainingSize: 0,
            pausedFileCount: 0,
            pausedSize: 0,
            remainingParCount: 0,
            activeDownloads: 0,
            successSize: 0,
            failedSize: 0,
            currentSuccessSize: 0,
            currentFailedSize: 0,
            parSize: 0,
            parSuccessSize: 0,
            parFailedSize: 0,
            parCurrentSuccessSize: 0,
            parCurrentFailedSize: 0,
            totalArticles: 0,
            successArticles: 0,
            failedArticles: 0,
            currentSuccessArticles: 0,
            currentFailedArticles: 0,
            dupeRecoveredArticles: 0,
            dupeFailedRun: 0,
            dupeRecoveredBytes: 0,
            dupeRecoveredHoles: 0,
            dupeAttemptedArticles: 0,
            dupeUnsourcedArticles: 0,
            dupeFailoverChecked: -1,
            deadPickProbed: false,
            dupeParDeferState: 0,
            dupeNoParNoted: false,
            minTime: 0,
            maxTime: 0,
            priority: 0,
            extraPriority: 0,
            directRenameStatus: 0,
            parRenameStatus: 0,
            rarRenameStatus: 0,
            parStatus: 0,
            directUnpackStatus: 0,
            unpackStatus: 0,
            cleanupStatus: 0,
            moveStatus: 0,
            postUnpackRenamingStatus: 0,
            postDownloadRenamingStatus: 0,
            deleteStatus: 0,
            markStatus: 0,
            urlStatus: 0,
            extraParBlocks: 0,
            addUrlPaused: false,
            deletePaused: false,
            manyDupeFiles: false,
            deleting: false,
            parking: false,
            avoidHistory: false,
            healthPaused: false,
            cleanupDisk: false,
            unpackCleanedUpDisk: false,
            dupeScore: 0,
            dupeMode: 0,
            dupeHint: 0,
            fullContentHash: 0,
            filteredContentHash: 0,
            idMessageGen: 0,
            parRepaired: false,
            unpackTried: false,
            lastUnpackStatus: 0,
            downloadedSize: 0,
            downloadStartTime: 0,
            downloadStartSec: 0,
            downloadSec: 0,
            postTotalSec: 0,
            parSec: 0,
            repairSec: 0,
            unpackSec: 0,
            liveUnpackSec: 0,
            liveParSec: 0,
            reprocess: false,
            changed: false,
            queueScriptTime: 0,
            parFull: false,
            cachedMessageCount: 0,
            feedId: 0,
            allFirst: false,
            waitingPar: false,
            loadingPar: false,
            desiredServerId: 0,
            skipScriptProcessing: false,
            skipDiskWrite: false,
            autoCategory: false,
        }
    }
}

/// Text fields, by id: 0 url, 1 filename, 2 name, 3 destination dir, 4 final
/// dir, 5 category, 6 queued filename, 7 dupe key, 8 hard-link path, 9 meta name.
const TEXTS: usize = 10;

/// The Rust allocation behind `NzbgetRsNzbScalars*`: the scalars first (so the
/// C pointer is this struct's address), then the strings. `None` is the C++
/// null string (only the name starts unset).
#[repr(C)]
struct NzbData {
    s: NzbScalars,
    texts: Mutex<[Option<CString>; TEXTS]>,
    coll: Mutex<Coll>,
    files: Mutex<Files>,
    // (id, kind, time, text), oldest first (see `nzbget_rs_nzb_msgs`)
    msgs: Mutex<std::collections::VecDeque<(u32, i32, i64, CString)>>,
    // host handles (see `nzbget_rs_nzb_slot`)
    slots: [std::sync::atomic::AtomicUsize; SLOTS],
}

const SLOTS: usize = 5;

/// A file that finished downloading and left the file list.
struct Completed {
    id: i32,
    // 0 none, 1 success, 2 partial, 3 failure
    status: i32,
    crc: u32,
    par: bool,
    filename: CString,
    origname: CString,
    hash16k: CString,
    par_set_id: CString,
}

/// Byte ranges of a completed file no server could supply, kept for stream
/// repair from a duplicate posting (see option DupeArticleFallback).
struct RepairJob {
    file_id: i32,
    filename: CString,
    decoded_size: i64,
    // encoded failed size and missed size, captured while the file was queued
    failed_size: i64,
    missed_size: i64,
    failed_articles: i32,
    par: bool,
    // (offset, size) pairs, as the host passes them
    holes: Vec<[i64; 2]>,
    steps_hash: u64,
    // the live pass already tried this job once (not persisted)
    live_attempted: bool,
}

/// Completed files and stream-repair jobs (see `nzbget_rs_nzb_files`).
#[derive(Default)]
struct Files {
    completed: Vec<Completed>,
    jobs: Vec<RepairJob>,
    // archives unpacked so far (cleared when post-processing ends)
    extracted: Vec<CString>,
}

/// A collection's small lists (see `nzbget_rs_nzb_coll`).
#[derive(Default)]
struct Coll {
    // post-processing parameters, names compared case-insensitively
    params: Vec<(CString, CString)>,
    // (script name, status)
    script_statuses: Vec<(CString, i32)>,
    // donors this collection must not borrow from again
    blocked_donors: BTreeSet<i32>,
    // (server id, success, failed): totals and current-session counts
    stats: [Vec<[i32; 3]>; 2],
}

impl Coll {
    fn set_param(&mut self, name: &CStr, value: &CStr) {
        let pos = self
            .params
            .iter()
            .position(|(n, _)| n.to_bytes().eq_ignore_ascii_case(name.to_bytes()));
        match (value.to_bytes().is_empty(), pos) {
            (true, Some(i)) => {
                self.params.remove(i);
            }
            (false, Some(i)) => self.params[i].1 = value.to_owned(),
            (false, None) => self.params.push((name.to_owned(), value.to_owned())),
            (true, None) => {}
        }
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

fn cstr<'a>(p: *const c_char) -> &'a CStr {
    if p.is_null() {
        c""
    } else {
        unsafe { CStr::from_ptr(p) }
    }
}

fn data<'a>(p: *const NzbScalars) -> &'a NzbData {
    unsafe { &*p.cast::<NzbData>() }
}

#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_nzb_scalars_new() -> *mut NzbScalars {
    let mut texts: [Option<CString>; TEXTS] = Default::default();
    for (i, t) in texts.iter_mut().enumerate() {
        if i != 2 {
            *t = Some(CString::default());
        }
    }
    let data = Box::new(NzbData {
        s: NzbScalars::default(),
        texts: Mutex::new(texts),
        coll: Mutex::new(Coll::default()),
        files: Mutex::new(Files::default()),
        msgs: Mutex::new(std::collections::VecDeque::new()),
        slots: std::array::from_fn(|_| std::sync::atomic::AtomicUsize::new(0)),
    });
    Box::into_raw(data).cast()
}

static NZB_ID_GEN: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static NZB_ID_MAX: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Id generation shared by all collections (was `NzbInfo::m_idGen` and
/// `m_idMax`): 0 last generated id, 1 reset to the highest id seen, 2 reset
/// to zero, 3 raise the floor to `v`, 4 generate the next id.
#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_nzb_ids(op: c_int, v: c_int) -> c_int {
    use std::sync::atomic::Ordering::SeqCst;
    match op {
        0 => NZB_ID_GEN.load(SeqCst),
        1 => {
            NZB_ID_GEN.store(NZB_ID_MAX.load(SeqCst), SeqCst);
            0
        }
        2 => {
            NZB_ID_GEN.store(0, SeqCst);
            NZB_ID_MAX.store(0, SeqCst);
            0
        }
        3 => {
            NZB_ID_MAX.fetch_max(v, SeqCst);
            0
        }
        4 => NZB_ID_GEN.fetch_add(1, SeqCst) + 1,
        _ => 0,
    }
}

unsafe extern "C" fn drop_file(p: usize) {
    unsafe { crate::qfile::nzbget_rs_file_free(p as *mut crate::qfile::File) };
}

/// The `NzbInfo` constructor: the next id and an empty file list.
///
/// # Safety
/// `p` is from `nzbget_rs_nzb_scalars_new` and not initialised yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_nzb_init(p: *mut NzbScalars) {
    unsafe { (*p).id = nzbget_rs_nzb_ids(4, 0) };
    let files = crate::qlist::nzbget_rs_ptrlist_new(drop_file);
    unsafe { nzbget_rs_nzb_slot(p, 0, 1, files as usize) };
}

/// # Safety
/// `p` is null or from `nzbget_rs_nzb_scalars_new`, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_nzb_scalars_free(p: *mut NzbScalars) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(p.cast::<NzbData>()) });
    }
}

/// Text field `id` (null when unset); valid until that field changes.
///
/// # Safety
/// `p` is from `nzbget_rs_nzb_scalars_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_nzb_text(p: *const NzbScalars, id: c_int) -> *const c_char {
    let data = unsafe { &*p.cast::<NzbData>() };
    let texts = data.texts.lock().unwrap_or_else(|e| e.into_inner());
    texts
        .get(id as usize)
        .and_then(|t| t.as_ref())
        .map_or(std::ptr::null(), |t| t.as_ptr())
}

/// Sets text field `id` to the first `len` bytes of `s` (`len` < 0: up to its
/// NUL); a null `s` makes it unset.
///
/// # Safety
/// `p` is from `nzbget_rs_nzb_scalars_new`; `s` is null or readable as stated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_nzb_set_text(
    p: *const NzbScalars,
    id: c_int,
    s: *const c_char,
    len: isize,
) {
    let value = if s.is_null() {
        None
    } else if len < 0 {
        Some(unsafe { CStr::from_ptr(s) }.to_owned())
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(s.cast::<u8>(), len as usize) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        Some(CString::new(&bytes[..end]).unwrap_or_default())
    };
    let data = unsafe { &*p.cast::<NzbData>() };
    let mut texts = data.texts.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = texts.get_mut(id as usize) {
        *slot = value;
    }
}

/// A collection's small lists. `p2` is another collection where noted.
///
/// - Parameters: 0 find `s1` (value into `strs[0]`, returns 1 when found);
///   1 set `s1` = `s2` (an empty value removes it); 2 copy all of `p2`'s in;
///   3 count; 4 parameter `a2` into `strs[0..2]`; 5 clear.
/// - Script statuses: 10 add `s1` with status `a2`; 11 count; 12 status `a2`
///   into `ints[0]` and `strs[0]`; 13 clear; 14 total status.
/// - Blocked donors: 20 contains `a2`; 21 add `a2`.
/// - Server stats (`w` = 0 totals, 1 current): 40+w count; 42+w entry `a2`
///   into `ints[0..3]`; 44+w apply op `a2` (0 set, 1 add, 2 subtract) with
///   `ints[0..3]`; 46+w clear; 48 set current from the totals; 49 apply op
///   `a2` of `p2`'s list `ints[1]` to this one's list `ints[0]`.
///
/// # Safety
/// `p` (and `p2` where used) come from `nzbget_rs_nzb_scalars_new`; output
/// arrays are large enough; strings are null or NUL-terminated.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_nzb_coll(
    p: *const NzbScalars,
    op: c_int,
    a2: c_int,
    p2: *const NzbScalars,
    s1: *const c_char,
    s2: *const c_char,
    ints: *mut c_int,
    strs: *mut *const c_char,
) -> i64 {
    // copy another collection's lists before locking this one (p2 may be p)
    let other: Option<Coll> = match op {
        2 | 49 if !p2.is_null() => {
            let o = data(p2).coll.lock().unwrap_or_else(|e| e.into_inner());
            Some(Coll {
                params: o.params.clone(),
                stats: o.stats.clone(),
                ..Coll::default()
            })
        }
        _ => None,
    };
    let mut c = data(p).coll.lock().unwrap_or_else(|e| e.into_inner());
    match op {
        0 => {
            let name = cstr(s1).to_bytes();
            match c
                .params
                .iter()
                .find(|(n, _)| n.to_bytes().eq_ignore_ascii_case(name))
            {
                Some((_, v)) => {
                    unsafe { *strs = v.as_ptr() };
                    1
                }
                None => 0,
            }
        }
        1 => {
            c.set_param(cstr(s1), cstr(s2));
            0
        }
        2 => {
            if let Some(o) = other {
                for (n, v) in &o.params {
                    c.set_param(n, v);
                }
            }
            0
        }
        3 => c.params.len() as i64,
        4 => match c.params.get(a2 as usize) {
            Some((n, v)) => {
                unsafe {
                    *strs = n.as_ptr();
                    *strs.add(1) = v.as_ptr();
                }
                0
            }
            None => 0,
        },
        5 => {
            c.params.clear();
            0
        }
        10 => {
            c.script_statuses.push((cstr(s1).to_owned(), a2));
            0
        }
        11 => c.script_statuses.len() as i64,
        12 => match c.script_statuses.get(a2 as usize) {
            Some((n, st)) => {
                unsafe {
                    *ints = *st;
                    *strs = n.as_ptr();
                }
                0
            }
            None => 0,
        },
        13 => {
            c.script_statuses.clear();
            0
        }
        14 => {
            let all: Vec<c_int> = c.script_statuses.iter().map(|(_, s)| *s).collect();
            i64::from(unsafe {
                crate::ffi::nzbget_rs_script_calc_total_status(all.as_ptr(), all.len())
            })
        }
        20 => i64::from(c.blocked_donors.contains(&a2)),
        21 => i64::from(c.blocked_donors.insert(a2) || true),
        40 | 41 => c.stats[(op - 40) as usize].len() as i64,
        42 | 43 => match c.stats[(op - 42) as usize].get(a2 as usize) {
            Some(e) => {
                unsafe {
                    *ints = e[0];
                    *ints.add(1) = e[1];
                    *ints.add(2) = e[2];
                }
                0
            }
            None => 0,
        },
        44 | 45 => {
            let (id, ok, bad) = unsafe { (*ints, *ints.add(1), *ints.add(2)) };
            stat_op(&mut c.stats[(op - 44) as usize], id, ok, bad, a2);
            0
        }
        46 | 47 => {
            c.stats[(op - 46) as usize].clear();
            0
        }
        48 => {
            let totals = c.stats[0].clone();
            for e in totals {
                stat_op(&mut c.stats[1], e[0], e[1], e[2], 0);
            }
            0
        }
        49 => {
            if let Some(o) = other {
                let (dst, src) = unsafe {
                    (
                        (*ints).clamp(0, 1) as usize,
                        (*ints.add(1)).clamp(0, 1) as usize,
                    )
                };
                for e in &o.stats[src] {
                    stat_op(&mut c.stats[dst], e[0], e[1], e[2], a2);
                }
            }
            0
        }
        _ => 0,
    }
}

fn owned(s: *const c_char) -> CString {
    cstr(s).to_owned()
}

fn ranges(raw: *const i64, count: c_int) -> Vec<[i64; 2]> {
    if raw.is_null() || count <= 0 {
        return Vec::new();
    }
    let flat = unsafe { std::slice::from_raw_parts(raw, count as usize * 2) };
    flat.chunks_exact(2).map(|r| [r[0], r[1]]).collect()
}

/// Completed files (ops 0-8) and stream-repair jobs (ops 20-27).
/// Returned strings and hole pointers stay valid until the list changes;
/// callers hold the queue lock.
///
/// Completed file: ints = id, status, par, crc; strs = filename, origname,
/// hash16k, par set id.
///  0 count; 1 read at a2; 2 add (ints = id, status, crc, par); 3 rename at a2 (s1 filename, s2 origname
///  when not null); 4 rename by name s1 to s2, returns 1 when found;
///  5 for id a2: ints[0]=1 mark success (returns 1 when it changed),
///  ints[0]=2 failure becomes partial; 6 erase at a2; 7 clear;
///  8 append and take p2's list.
/// Job: ints = file id, failed articles, par, live attempted, hole count;
/// i64s = decoded size, failed size, missed size, steps hash.
///  20 count; 21 summary at a2 (ints[0] file id; i64s decoded size, hole
///  total, failed+missed; strs[0] filename); 22 read at a2 (strs[0]
///  filename, strs[1] holes); 23 add (s1 filename, `raw` a2 holes);
///  24 mark live attempted at a2, returns the previous value; 25 erase at
///  a2; 26 at a2 set decoded size i64s[0] when >= 0 and holes `raw`
///  ints[1]; 27 clear.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_nzb_files(
    p: *const NzbScalars,
    op: c_int,
    a2: c_int,
    p2: *const NzbScalars,
    s1: *const c_char,
    s2: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
    raw: *const i64,
) -> i64 {
    let taken = if op == 8 && !p2.is_null() && p2 != p {
        let mut o = data(p2).files.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut o.completed)
    } else {
        Vec::new()
    };
    let mut f = data(p).files.lock().unwrap_or_else(|e| e.into_inner());
    let i = a2.max(0) as usize;
    let int = |k: usize| unsafe { *ints.add(k) };
    let put = |k: usize, v: c_int| unsafe { *ints.add(k) = v };
    let put64 = |k: usize, v: i64| unsafe { *i64s.add(k) = v };
    let put_s = |k: usize, v: *const c_char| unsafe { *strs.add(k) = v };
    match op {
        0 => f.completed.len() as i64,
        1 => match f.completed.get(i) {
            Some(c) if a2 >= 0 => {
                put(0, c.id);
                put(1, c.status);
                put(2, i32::from(c.par));
                put(3, c.crc as i32);
                put_s(0, c.filename.as_ptr());
                put_s(1, c.origname.as_ptr());
                put_s(2, c.hash16k.as_ptr());
                put_s(3, c.par_set_id.as_ptr());
                0
            }
            _ => -1,
        },
        2 => {
            let s = |k: usize| owned(unsafe { *strs.add(k) });
            f.completed.push(Completed {
                id: int(0),
                status: int(1),
                crc: int(2) as u32,
                par: int(3) != 0,
                filename: s(0),
                origname: s(1),
                hash16k: s(2),
                par_set_id: s(3),
            });
            0
        }
        3 => {
            if let Some(c) = f.completed.get_mut(i) {
                if !s2.is_null() {
                    c.origname = owned(s2);
                }
                c.filename = owned(s1);
            }
            0
        }
        4 => {
            let (old, new) = (cstr(s1).to_bytes(), cstr(s2));
            if old.is_empty() || new.to_bytes().is_empty() {
                return 0;
            }
            for c in f.completed.iter_mut() {
                if crate::downloadinfo::completed_same_filename(c.filename.to_bytes(), old) {
                    if c.origname.to_bytes().is_empty() {
                        c.origname = c.filename.clone();
                    }
                    c.filename = new.to_owned();
                    return 1;
                }
            }
            0
        }
        5 => {
            let mode = int(0);
            if let Some(c) = f.completed.iter_mut().find(|c| c.id == a2) {
                if mode == 1 && c.status != 1 {
                    c.status = 1;
                    return 1;
                }
                if mode == 2 && c.status == 3 {
                    c.status = 2;
                }
            }
            0
        }
        6 => {
            if a2 >= 0 && i < f.completed.len() {
                f.completed.remove(i);
            }
            0
        }
        7 => {
            f.completed.clear();
            0
        }
        8 => {
            f.completed.extend(taken);
            0
        }
        20 => f.jobs.len() as i64,
        21 | 22 => match f.jobs.get(i) {
            Some(j) if a2 >= 0 => {
                put_s(0, j.filename.as_ptr());
                if op == 21 {
                    put(0, j.file_id);
                    put64(0, j.decoded_size);
                    put64(1, j.holes.iter().map(|r| r[1]).sum());
                    put64(2, j.failed_size + j.missed_size);
                } else {
                    put(0, j.file_id);
                    put(1, j.failed_articles);
                    put(2, i32::from(j.par));
                    put(3, i32::from(j.live_attempted));
                    put(4, j.holes.len() as c_int);
                    put64(0, j.decoded_size);
                    put64(1, j.failed_size);
                    put64(2, j.missed_size);
                    put64(3, j.steps_hash as i64);
                    put_s(
                        1,
                        if j.holes.is_empty() {
                            std::ptr::null()
                        } else {
                            j.holes.as_ptr().cast()
                        },
                    );
                }
                0
            }
            _ => -1,
        },
        23 => {
            let g = |k: usize| unsafe { *i64s.add(k) };
            f.jobs.push(RepairJob {
                file_id: int(0),
                filename: owned(s1),
                decoded_size: g(0),
                failed_size: g(1),
                missed_size: g(2),
                failed_articles: int(1),
                par: int(2) != 0,
                holes: ranges(raw, a2),
                steps_hash: g(3) as u64,
                live_attempted: false,
            });
            0
        }
        24 => match f.jobs.get_mut(i) {
            Some(j) if a2 >= 0 => i64::from(std::mem::replace(&mut j.live_attempted, true)),
            _ => 0,
        },
        25 => {
            if a2 >= 0 && i < f.jobs.len() {
                f.jobs.remove(i);
            }
            0
        }
        26 => {
            let size = unsafe { *i64s };
            let holes = ranges(raw, int(1));
            if let Some(j) = f.jobs.get_mut(i) {
                if size >= 0 {
                    j.decoded_size = size;
                }
                j.holes = holes;
            }
            0
        }
        27 => {
            f.jobs.clear();
            0
        }
        30..=33 => list_op(&mut f.extracted, op - 30, a2, s1, strs),
        _ => 0,
    }
}

/// 0 count; 1 read at a2 into strs[0] (-1 when out of range); 2 add s1;
/// 3 clear.
fn list_op(
    v: &mut Vec<CString>,
    op: c_int,
    a2: c_int,
    s1: *const c_char,
    strs: *mut *const c_char,
) -> i64 {
    match op {
        0 => v.len() as i64,
        1 => match v.get(a2.max(0) as usize) {
            Some(e) if a2 >= 0 => {
                unsafe { *strs = e.as_ptr() };
                0
            }
            _ => -1,
        },
        2 => {
            v.push(owned(s1));
            0
        }
        3 => {
            v.clear();
            0
        }
        _ => 0,
    }
}

/// Post-processing state of a collection. The C++ PostInfo is this
/// allocation (a PostInfo* is a Post*): it has no members of its own.
pub struct Post {
    // scalar fields by id (see `nzbget_rs_post_op`)
    vals: [std::sync::atomic::AtomicI64; POST_VALS],
    // the collection being post-processed (an NzbScalars*), 0 for none
    nzb: std::sync::atomic::AtomicUsize,
    lists: Mutex<PostLists>,
}

const POST_VALS: usize = 20;

#[derive(Default)]
struct PostLists {
    label: CString,
    parred: Vec<CString>,
    // used only when the post job has no collection
    extracted: Vec<CString>,
}

#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_post_new() -> *mut Post {
    Box::into_raw(Box::new(Post {
        vals: std::array::from_fn(|_| std::sync::atomic::AtomicI64::new(0)),
        nzb: std::sync::atomic::AtomicUsize::new(0),
        lists: Mutex::new(PostLists::default()),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_post_free(p: *mut Post) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(p) });
    }
}

/// A copy of the progress label: it may change on another thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_post_label(p: *const Post) -> crate::ffi::RsBuf {
    let post = unsafe { &*p };
    let l = post.lists.lock().unwrap_or_else(|e| e.into_inner());
    crate::ffi::into_buf(l.label.as_bytes().to_vec())
}

/// Fields 0-19: 0 stage, 1 working, 2 deleted, 3 request par check,
/// 4 force par full, 5 par cannot cover, 6 need par check, 7 need unpack,
/// 8 force repair, 9 par repaired, 10 unpack tried, 11 pass list tried,
/// 12 last unpack status, 13 file progress, 14 stage progress (kept in
/// 0..=1000), 15 stream repair done, 16 stream repair recovered,
/// 17 start time, 18 stage time, 19 post thread pointer. Fields 9, 10 and
/// 12 read the collection's own value when the post job has one, and a set
/// writes both.
///  0 get field a2; 1 set field a2 to v;
///  2 get the collection pointer; 3 set it to v;
///  11 set the progress label to s1 (read it with `nzbget_rs_post_label`);
///  20-23 parred files, 30-33 extracted archives (see `list_op`; the
///  collection's list when there is one): strings returned stay valid until
///  that list changes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_post_op(
    p: *const Post,
    op: c_int,
    a2: c_int,
    v: i64,
    s1: *const c_char,
    strs: *mut *const c_char,
) -> i64 {
    use std::sync::atomic::Ordering::SeqCst;
    let post = unsafe { &*p };
    let field = || post.vals.get(a2.max(0) as usize).filter(|_| a2 >= 0);
    let nzb = post.nzb.load(SeqCst) as *mut NzbScalars;
    match op {
        0 => {
            if !nzb.is_null() {
                let s = unsafe { &*nzb };
                match a2 {
                    9 => return i64::from(s.parRepaired),
                    10 => return i64::from(s.unpackTried),
                    12 => return i64::from(s.lastUnpackStatus),
                    _ => {}
                }
            }
            field().map_or(0, |f| f.load(SeqCst))
        }
        1 => {
            let v = if a2 == 14 { v.clamp(0, 1000) } else { v };
            if let Some(f) = field() {
                f.store(v, SeqCst);
            }
            if !nzb.is_null() {
                let s = unsafe { &mut *nzb };
                match a2 {
                    9 => s.parRepaired = v != 0,
                    10 => s.unpackTried = v != 0,
                    12 => s.lastUnpackStatus = v as i32,
                    _ => {}
                }
            }
            0
        }
        2 => nzb as usize as i64,
        3 => {
            post.nzb.store(v as usize, SeqCst);
            0
        }
        11 => {
            post.lists.lock().unwrap_or_else(|e| e.into_inner()).label = owned(s1);
            0
        }
        30..=33 if !nzb.is_null() => unsafe {
            nzbget_rs_nzb_files(
                nzb,
                op,
                a2,
                std::ptr::null(),
                s1,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                strs,
                std::ptr::null(),
            )
        },
        20..=23 | 30..=33 => {
            let mut l = post.lists.lock().unwrap_or_else(|e| e.into_inner());
            let list = if op < 30 {
                &mut l.parred
            } else {
                &mut l.extracted
            };
            list_op(list, op % 10, a2, s1, strs)
        }
        _ => 0,
    }
}

/// Field 19 of a post job: its post-processing thread.
fn post_thread(p: *const Post) -> *mut crate::jobthread::JobThread {
    let ns = std::ptr::null_mut();
    (unsafe { nzbget_rs_post_op(p, 0, 19, 0, std::ptr::null(), ns) }) as usize as *mut _
}

fn post_set(p: *const Post, id: c_int, v: i64) {
    unsafe { nzbget_rs_post_op(p, 1, id, v, std::ptr::null(), std::ptr::null_mut()) };
}

fn post_get(p: *const Post, id: c_int) -> i64 {
    unsafe { nzbget_rs_post_op(p, 0, id, 0, std::ptr::null(), std::ptr::null_mut()) }
}

/// Starts post-processing for a collection (host op 32 sub-op 54): a fresh
/// post job replaces any earlier one.
///
/// # Safety
/// `p` is a live collection; the caller holds the queue lock.
pub(crate) unsafe fn enter_post_process(p: *mut NzbScalars) {
    let post = nzbget_rs_post_new();
    unsafe {
        nzbget_rs_post_op(
            post,
            3,
            0,
            p as usize as i64,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    let old = unsafe { nzbget_rs_nzb_slot(p, 1, 1, post as usize) };
    unsafe { nzbget_rs_post_free(old as *mut Post) };
}

/// Ends post-processing for a collection (host op 32 sub-op 55): frees the
/// post job and resets the per-run unpack state. Messages stay.
///
/// # Safety
/// `p` is a live collection; the caller holds the queue lock.
pub(crate) unsafe fn leave_post_process(p: *mut NzbScalars) {
    let old = unsafe { nzbget_rs_nzb_slot(p, 1, 1, 0) };
    unsafe { nzbget_rs_post_free(old as *mut Post) };
    let s = unsafe { &mut *p };
    s.unpackTried = false;
    s.lastUnpackStatus = 0;
    unsafe {
        nzbget_rs_nzb_files(
            p,
            33,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
}

thread_local! {
    // the progress label host op 60 hands out: valid on this thread until
    // the next snapshot
    static POST_LABEL: std::cell::RefCell<CString> = std::cell::RefCell::new(CString::default());
}

/// Host op 60 on a post job `p` (sub-op `a1`), as the C++ host did:
///  0 snapshot (ints[0..21], i64s[0..3], strs[0] the progress label);
///  1 set field a2 (0-16) to p2; 2 start time = i64s[0];
///  3 new stage at time i64s[0] (progress and label cleared);
///  4 stage time = i64s[0]; 5 progress label = s1;
///  6 parred files: add s1 when a2 < 0, else strs[0] = file a2;
///  7 extracted archives: clear when a2 < 0, else strs[0] = archive a2.
///
/// # Safety
/// `p` is null or a live post job; the arrays fit the sub-op.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn host_post_op(
    p: *const Post,
    a1: c_int,
    a2: c_int,
    p2: usize,
    s1: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    if p.is_null() {
        return 0;
    }
    let ns: *const c_char = std::ptr::null();
    let none: *mut *const c_char = std::ptr::null_mut();
    let flag = |id| c_int::from(post_get(p, id) != 0);
    match a1 {
        0 => {
            let ints = unsafe { std::slice::from_raw_parts_mut(ints, 21) };
            let i64s = unsafe { std::slice::from_raw_parts_mut(i64s, 3) };
            let pt = post_thread(p);
            ints[0] = post_get(p, 0) as c_int;
            for (slot, id) in [1, 2, 3, 4, 8, 15, 16, 5, 6, 7].into_iter().enumerate() {
                ints[slot + 1] = flag(id);
            }
            ints[11] = c_int::from(!pt.is_null());
            ints[12] = c_int::from(
                !pt.is_null() && unsafe { crate::jobthread::nzbget_rs_job_op(pt, 4, 0, 0, 0) } != 0,
            );
            ints[13] = flag(10);
            ints[14] = flag(9);
            ints[15] = flag(11);
            ints[16] = post_get(p, 12) as c_int;
            ints[17] = unsafe { nzbget_rs_post_op(p, 30, 0, 0, ns, none) } as c_int;
            ints[18] = unsafe { nzbget_rs_post_op(p, 20, 0, 0, ns, none) } as c_int;
            ints[19] = post_get(p, 14) as c_int;
            ints[20] = post_get(p, 13) as c_int;
            i64s[0] = post_get(p, 17);
            i64s[1] = unsafe { nzbget_rs_post_op(p, 2, 0, 0, ns, none) };
            i64s[2] = post_get(p, 18);
            if !strs.is_null() {
                let label = unsafe { &*p }
                    .lists
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .label
                    .clone();
                let ptr = POST_LABEL.with(|l| {
                    *l.borrow_mut() = label;
                    l.borrow().as_ptr()
                });
                unsafe { *strs = ptr };
            }
        }
        1 => {
            let v = p2 as c_int;
            match a2 {
                0 | 12 | 13 | 14 => post_set(p, a2, i64::from(v)),
                1..=11 | 15 | 16 => post_set(p, a2, i64::from(v != 0)),
                _ => {}
            }
        }
        2 => post_set(p, 17, unsafe { *i64s }),
        3 => {
            post_set(p, 18, unsafe { *i64s });
            post_set(p, 14, 0);
            post_set(p, 13, 0);
            unsafe { nzbget_rs_post_op(p, 11, 0, 0, c"".as_ptr(), none) };
        }
        4 => post_set(p, 18, unsafe { *i64s }),
        5 => {
            unsafe { nzbget_rs_post_op(p, 11, 0, 0, s1, none) };
        }
        6 | 7 => {
            let base = if a1 == 6 { 20 } else { 30 };
            if a2 < 0 {
                // add a parred file, or clear the extracted archives
                let (op, s) = if a1 == 6 { (22, s1) } else { (33, ns) };
                unsafe { nzbget_rs_post_op(p, op, 0, 0, s, none) };
            } else {
                unsafe {
                    *strs = c"".as_ptr();
                    nzbget_rs_post_op(p, base + 1, a2, 0, ns, strs);
                }
            }
        }
        _ => {}
    }
    0
}

/// Starts a direct-unpack job for collection `p` (host op 62 sub-op 8): the
/// job goes in slot 2 and the direct-unpack status becomes running (back to
/// none, with the slot cleared, when the thread cannot start).
///
/// # Safety
/// `p` is a live collection; the caller holds the queue lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_nzb_start_direct_unpack(p: *mut NzbScalars) {
    use crate::jobthread::{nzbget_rs_job_free, nzbget_rs_job_new_unpack, nzbget_rs_job_start};
    if p.is_null() {
        return;
    }
    let job = nzbget_rs_job_new_unpack(unsafe { (*p).id });
    unsafe { nzbget_rs_nzb_slot(p, 2, 1, job as usize) };
    // NzbInfo::nsRunning
    unsafe { (*p).directUnpackStatus = 1 };
    if unsafe { nzbget_rs_job_start(job) } == 0 {
        unsafe {
            nzbget_rs_nzb_slot(p, 2, 1, 0);
            (*p).directUnpackStatus = 0;
            nzbget_rs_job_free(job, 0);
        }
    }
}

/// Host op 62: post-processing and direct-unpack job handles (sub-op `a1`):
///  0 stop the post job p1's thread; 1 detach and free it; 2 kill and free it;
///  3 is collection p1's direct unpack active (a2 != 0: any running one);
///  4-7 direct-unpack ops on collection p2's job (4 stop, 5 collection
///  downloaded, 6 deleted, 7 file i64s[0] downloaded; p1 the coordinator);
///  8 start a direct unpack for collection p1;
///  9 stop collection p1's live repair; 10 detach and stop it;
///  11 add par file p2 (a2 deleted) to the post job p1's repair;
///  12 start post job p1's controller of kind a2.
///
/// # Safety
/// `p1`/`p2` are null or live objects of the kind the sub-op takes.
pub(crate) unsafe fn host_post_job_op(
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    i64s: *const i64,
) -> usize {
    use crate::jobthread::{
        nzbget_rs_job_free, nzbget_rs_job_new_post, nzbget_rs_job_op, nzbget_rs_job_start,
    };
    let post = p1 as *const Post;
    let slot = |n: usize, id: c_int| -> *mut crate::jobthread::JobThread {
        if n == 0 {
            std::ptr::null_mut()
        } else {
            (unsafe { nzbget_rs_nzb_slot(n as *const NzbScalars, id, 0, 0) }) as *mut _
        }
    };
    match a1 {
        0 if !post.is_null() => {
            let t = post_thread(post);
            if !t.is_null() {
                unsafe { nzbget_rs_job_op(t, 0, 0, 0, 0) };
            }
        }
        1 | 2 if !post.is_null() => {
            let t = post_thread(post);
            if !t.is_null() {
                post_set(post, 19, 0);
                unsafe { nzbget_rs_job_free(t, c_int::from(a1 == 2)) };
            }
        }
        3 => {
            let du = slot(p1, 2);
            if du.is_null() {
                return 0;
            }
            let active = a2 != 0
                || (unsafe { nzbget_rs_job_op(du, 15, 0, 0, 0) } != 0
                    && unsafe { nzbget_rs_job_op(du, 16, 0, 0, 0) } == 0);
            return usize::from(active);
        }
        4..=7 => {
            let du = slot(p2, 2);
            if !du.is_null() {
                const DU_OP: [c_int; 4] = [11, 13, 14, 12];
                let arg = if a1 == 7 {
                    (unsafe { *i64s }) as usize
                } else {
                    p2
                };
                unsafe { nzbget_rs_job_op(du, DU_OP[(a1 - 4) as usize], 0, p1, arg) };
            }
        }
        8 if p1 != 0 => unsafe { nzbget_rs_nzb_start_direct_unpack(p1 as *mut NzbScalars) },
        9 => {
            let t = slot(p1, 3);
            if !t.is_null() {
                unsafe { nzbget_rs_job_op(t, 0, 0, 0, 0) };
            }
        }
        10 if p1 != 0 => {
            let t = unsafe { nzbget_rs_nzb_slot(p1 as *const NzbScalars, 3, 1, 0) }
                as *mut crate::jobthread::JobThread;
            if !t.is_null() {
                unsafe { nzbget_rs_job_op(t, 0, 0, 0, 0) };
            }
        }
        11 if !post.is_null() && crate::ffi::build_parcheck_enabled() => {
            let t = post_thread(post);
            if !t.is_null() {
                let added = unsafe { nzbget_rs_job_op(t, 5, c_int::from(a2 != 0), p2, 0) };
                return usize::from(added != 0);
            }
        }
        12 if !post.is_null() => {
            // the repair controller needs par-check support
            if a2 == 3 && !crate::ffi::build_parcheck_enabled() {
                return 0;
            }
            let nzb =
                unsafe { nzbget_rs_post_op(post, 2, 0, 0, std::ptr::null(), std::ptr::null_mut()) }
                    as usize as *const NzbScalars;
            if a2 == 9 && !nzb.is_null() && unsafe { (*nzb).skipScriptProcessing } {
                return 0;
            }
            let job = unsafe { nzbget_rs_job_new_post(a2, p1, 0, 0) };
            post_set(post, 19, job as usize as i64);
            if unsafe { nzbget_rs_job_start(job) } == 0 {
                // no worker will finish this stage: let the scheduler clean up and retry it
                post_set(post, 19, 0);
                post_set(post, 1, 0);
                unsafe { nzbget_rs_job_free(job, 0) };
            }
        }
        _ => {}
    }
    0
}

/// Writes a collection's download counters (what host op 33 did), in the
/// same order: ints = file count, active downloads, full hash, filtered
/// hash, paused file count, remaining par count, total, success, failed,
/// current success, current failed articles, download seconds; i64s = size,
/// remaining, paused, success, current success, failed, current failed,
/// par size, par success, par current success, par failed, par current
/// failed, min time, max time, downloaded size, download start time.
///
/// # Safety
/// `p` is a live collection; the caller holds the queue lock.
pub(crate) unsafe fn host_set_stats(p: *mut NzbScalars, ints: &[i32; 12], i64s: &[i64; 16]) {
    let s = unsafe { &mut *p };
    s.fileCount = ints[0];
    set_active_downloads(s, ints[1], unix_now());
    s.fullContentHash = ints[2] as u32;
    s.filteredContentHash = ints[3] as u32;
    s.size = i64s[0];
    s.remainingSize = i64s[1];
    s.pausedFileCount = ints[4];
    s.pausedSize = i64s[2];
    s.successSize = i64s[3];
    s.currentSuccessSize = i64s[4];
    s.failedSize = i64s[5];
    s.currentFailedSize = i64s[6];
    s.parSize = i64s[7];
    s.parSuccessSize = i64s[8];
    s.parCurrentSuccessSize = i64s[9];
    s.parFailedSize = i64s[10];
    s.parCurrentFailedSize = i64s[11];
    s.remainingParCount = ints[5];
    s.totalArticles = ints[6];
    s.successArticles = ints[7];
    s.failedArticles = ints[8];
    s.currentSuccessArticles = ints[9];
    s.currentFailedArticles = ints[10];
    s.downloadSec = ints[11];
    s.minTime = i64s[12];
    s.maxTime = i64s[13];
    s.downloadedSize = i64s[14];
    s.downloadStartTime = i64s[15];
}

fn ptr_list_op(l: usize, op: c_int, a: i64, item: usize, order: *const usize, other: usize) -> i64 {
    unsafe {
        crate::qlist::nzbget_rs_ptrlist_op(
            l as *const crate::qlist::PtrList,
            op,
            a,
            item,
            order,
            other as *const crate::qlist::PtrList,
        )
    }
}

/// A collection's file list (slot 0).
pub(crate) fn file_list(p: *const NzbScalars) -> usize {
    unsafe { nzbget_rs_nzb_slot(p, 0, 0, 0) }
}

/// Moves `src`'s files and download counters to `dst` (NzbInfo::MoveFileList).
unsafe fn move_file_list(dst: *mut NzbScalars, src: *const NzbScalars) {
    if std::ptr::eq(dst, src) {
        return;
    }
    let dl = file_list(dst);
    ptr_list_op(dl, 9, 0, 0, std::ptr::null(), 0);
    ptr_list_op(dl, 11, 0, 0, std::ptr::null(), file_list(src));
    for i in 0..ptr_list_op(dl, 0, 0, 0, std::ptr::null(), 0) {
        let f = ptr_list_op(dl, 1, i, 0, std::ptr::null(), 0);
        unsafe {
            crate::qfile::nzbget_rs_file_set(
                f as *const crate::qfile::File,
                crate::qfile::field::NZB,
                dst as i64,
            )
        };
    }
    let (d, s) = unsafe { (&mut *dst, &*src) };
    d.fullContentHash = s.fullContentHash;
    d.filteredContentHash = s.filteredContentHash;
    d.fileCount = s.fileCount;
    d.pausedFileCount = s.pausedFileCount;
    d.remainingParCount = s.remainingParCount;
    d.size = s.size;
    d.remainingSize = s.remainingSize;
    d.pausedSize = s.pausedSize;
    d.successSize = s.successSize;
    d.currentSuccessSize = s.currentSuccessSize;
    d.failedSize = s.failedSize;
    d.currentFailedSize = s.currentFailedSize;
    d.parSize = s.parSize;
    d.parSuccessSize = s.parSuccessSize;
    d.parCurrentSuccessSize = s.parCurrentSuccessSize;
    d.parFailedSize = s.parFailedSize;
    d.parCurrentFailedSize = s.parCurrentFailedSize;
    d.totalArticles = s.totalArticles;
    d.successArticles = s.successArticles;
    d.failedArticles = s.failedArticles;
    d.currentSuccessArticles = s.successArticles;
    d.currentFailedArticles = s.failedArticles;
    d.minTime = s.minTime;
    d.maxTime = s.maxTime;
}

/// Adds a completed file, raising the file id floor (NzbInfo::AddCompletedFile).
unsafe fn add_completed(p: *const NzbScalars, ints4: [c_int; 4], strs4: [*const c_char; 4]) {
    crate::qfile::nzbget_rs_file_ids(3, ints4[0]);
    let mut ints = ints4;
    let mut strs = strs4;
    unsafe {
        nzbget_rs_nzb_files(
            p,
            2,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
            std::ptr::null(),
        )
    };
}

/// A collection's file-list and completed-file ops (host op 34), as the
/// C++ host did them.
///
/// # Safety
/// Pointers are live objects of the kinds each op names; output arrays are
/// large enough for the op; the caller holds the queue lock.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn host_filelist_op(
    p: *mut NzbScalars,
    a1: c_int,
    a2: c_int,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    use crate::qfile::{
        field, nzbget_rs_file_get, nzbget_rs_file_set, nzbget_rs_file_set_text,
        nzbget_rs_file_text, File,
    };
    let np: *const NzbScalars = std::ptr::null();
    let ns: *const c_char = std::ptr::null();
    let no: *const usize = std::ptr::null();
    let empty = c"".as_ptr();
    let or_empty = |s: *const c_char| if s.is_null() { empty } else { s };
    let int = |k: usize| unsafe { *ints.add(k) };
    let i64v = |k: usize| unsafe { *i64s.add(k) };
    let fl = file_list(p);
    let files = |op, a2, s1, s2, ints: *mut c_int, i64s: *mut i64, strs, raw: *const i64| unsafe {
        nzbget_rs_nzb_files(p, op, a2, np, s1, s2, ints, i64s, strs, raw)
    };
    let coll = |p: *const NzbScalars, op, a2, other, ints| unsafe {
        nzbget_rs_nzb_coll(p, op, a2, other, ns, ns, ints, std::ptr::null_mut())
    };
    let ni: *mut c_int = std::ptr::null_mut();
    let nl: *mut i64 = std::ptr::null_mut();
    let nstr: *mut *const c_char = std::ptr::null_mut();
    let nraw: *const i64 = std::ptr::null();
    let set = |f: usize, id, v: i64| unsafe { nzbget_rs_file_set(f as *const File, id, v) };
    match a1 {
        0 => ptr_list_op(fl, 0, 0, 0, no, 0) as usize,
        1 => ptr_list_op(fl, 1, i64::from(a2), 0, no, 0) as usize,
        2 => {
            // removes the file from the list, dropping it (as the C++ unique_ptr did)
            if ptr_list_op(fl, 6, 0, p2, no, 0) != 0 {
                unsafe { crate::qfile::nzbget_rs_file_free(p2 as *mut File) };
            }
            0
        }
        3 => {
            let dst = i64v(0) as usize as *mut NzbScalars;
            if ptr_list_op(fl, 6, 0, p2, no, 0) != 0 {
                ptr_list_op(file_list(dst), 3, 0, p2, no, 0);
            }
            set(p2, field::NZB, dst as i64);
            unsafe {
                crate::qfile::add_file_stats(p as usize, p2 as *const File, 1, 2);
                crate::qfile::add_file_stats(dst as usize, p2 as *const File, 1, 1);
            }
            0
        }
        4 => {
            let src = p2 as *const NzbScalars;
            let sl = file_list(src);
            for i in 0..ptr_list_op(sl, 0, 0, 0, no, 0) {
                set(
                    ptr_list_op(sl, 1, i, 0, no, 0) as usize,
                    field::NZB,
                    p as i64,
                );
            }
            ptr_list_op(fl, 11, 0, 0, no, sl);
            let mut lists = [0 as c_int, 0];
            coll(p, 49, 1, src, lists.as_mut_ptr());
            lists = [1, 1];
            coll(p, 49, 1, src, lists.as_mut_ptr());
            unsafe { nzbget_rs_nzb_files(p, 8, 0, src, ns, ns, ni, nl, nstr, nraw) };
            0
        }
        5 => {
            if a2 < 0 {
                return files(0, 0, ns, ns, ni, nl, nstr, nraw) as usize;
            }
            files(1, a2, ns, ns, ints, nl, strs, nraw);
            0
        }
        6 => {
            let f = p2 as *const File;
            let g = |id| unsafe { nzbget_rs_file_get(f, id) };
            let t = |id| unsafe { nzbget_rs_file_text(f, id) };
            unsafe {
                add_completed(
                    p,
                    [
                        g(field::ID) as c_int,
                        a2,
                        int(0),
                        c_int::from(g(field::PAR_FILE) != 0),
                    ],
                    [
                        s1,
                        t(crate::qfile::text::ORIGNAME),
                        t(crate::qfile::text::HASH16K),
                        t(crate::qfile::text::PAR_SET_ID),
                    ],
                )
            };
            0
        }
        7 => {
            files(3, a2, s1, s2, ni, nl, nstr, nraw);
            0
        }
        8 => {
            files(23, a2, s1, ns, ints, i64s, nstr, p2 as *const i64);
            0
        }
        9 => {
            // take the file out, tell the pre/post-processor, then drop it
            let dq = i64v(0) as usize;
            let released = ptr_list_op(fl, 6, 0, p2, no, 0) != 0;
            crate::jobthread::nzbget_rs_prepost_queue_changed(a2, dq, p as usize, p2);
            if released {
                unsafe { crate::qfile::nzbget_rs_file_free(p2 as *mut File) };
            }
            0
        }
        10 => {
            ptr_list_op(fl, 10, i64::from(a2), 0, p2 as *const usize, 0);
            0
        }
        11 => {
            if a2 < 0 {
                return files(20, 0, ns, ns, ni, nl, nstr, nraw) as usize;
            }
            files(21, a2, ns, ns, ints, i64s, strs, nraw);
            0
        }
        12 => files(5, a2, ns, ns, ints, nl, nstr, nraw) as usize,
        13 => match int(0) {
            1 => files(24, a2, ns, ns, ni, nl, nstr, nraw) as usize,
            2 => {
                files(25, a2, ns, ns, ni, nl, nstr, nraw);
                if int(1) != 0 {
                    unsafe { (*p).changed = true };
                }
                0
            }
            3 => {
                files(26, a2, ns, ns, ints, i64s, nstr, p2 as *const i64);
                0
            }
            _ => {
                files(22, a2, ns, ns, ints, i64s, strs, nraw);
                0
            }
        },
        14 => {
            let f = crate::qfile::nzbget_rs_file_new(-1);
            let fp = f as usize;
            let id = if int(0) != 0 {
                int(0)
            } else {
                crate::qfile::nzbget_rs_file_ids(4, 0)
            };
            set(fp, field::ID, i64::from(id));
            set(fp, field::NZB, p as i64);
            unsafe {
                nzbget_rs_file_set_text(f, crate::qfile::text::SUBJECT, or_empty(s1));
                nzbget_rs_file_set_text(f, crate::qfile::text::FILENAME, or_empty(s2));
            }
            set(fp, field::TIME, i64::from(int(1)));
            set(fp, field::SIZE, i64v(0));
            set(fp, field::REMAINING_SIZE, i64v(1));
            set(fp, field::MISSED_SIZE, i64v(2));
            set(fp, field::TOTAL_ARTICLES, i64::from(int(2)));
            set(fp, field::MISSED_ARTICLES, i64::from(int(3)));
            set(fp, field::PAR_FILE, i64::from(int(4) != 0));
            let groups = unsafe { *strs } as *const *const c_char;
            for g in 0..int(5).max(0) as usize {
                let name = unsafe { *groups.add(g) };
                unsafe { crate::qfile::nzbget_rs_file_op(f, 2, 0, or_empty(name), ni, nstr) };
            }
            let mut art_args = [i64v(3), i64v(4)];
            unsafe {
                crate::qarticle::nzbget_rs_article_host(
                    crate::qfile::nzbget_rs_file_articles(f),
                    std::ptr::null_mut(),
                    105,
                    int(6),
                    0,
                    ns,
                    ni,
                    art_args.as_mut_ptr(),
                    (*strs.add(1)) as *mut *const c_char,
                )
            };
            ptr_list_op(fl, 3, 0, fp, no, 0);
            fp
        }
        15 => {
            files(7, 0, ns, ns, ni, nl, nstr, nraw);
            files(27, 0, ns, ns, ni, nl, nstr, nraw);
            coll(p, 46, 0, np, ni);
            coll(p, 47, 0, np, ni);
            unsafe { move_file_list(p, p2 as *const NzbScalars) };
            0
        }
        16 => {
            files(6, a2, ns, ns, ni, nl, nstr, nraw);
            0
        }
        17 => {
            ptr_list_op(fl, 3, 0, p2, no, 0);
            0
        }
        18 => {
            unsafe {
                add_completed(
                    p,
                    [int(0), int(1), int(2), c_int::from(int(3) != 0)],
                    [*strs, *strs.add(1), *strs.add(2), *strs.add(3)],
                )
            };
            0
        }
        19 => {
            let f = crate::qfile::nzbget_rs_file_new(-1) as usize;
            // new FileInfo() takes a generated id before SetId replaces it
            set(
                f,
                field::ID,
                i64::from(crate::qfile::nzbget_rs_file_ids(4, 0)),
            );
            set(f, field::ID, i64::from(int(0)));
            set(f, field::PAUSED, i64::from(int(1) != 0));
            if a2 != 0 {
                set(f, field::TIME, i64::from(int(2)));
            }
            set(f, field::EXTRA_PRIORITY, i64::from(int(3) != 0));
            set(f, field::NZB, p as i64);
            ptr_list_op(fl, 3, 0, f, no, 0);
            0
        }
        _ => 0,
    }
}

/// A collection's setters (host op 32 sub-op `a1`), as the C++ ones; false
/// for unknown sub-ops. `p2` is the file for 52 and 53.
///
/// # Safety
/// `p` is a live collection, `p2` a live file where the op names one; the
/// caller holds the queue lock.
pub(crate) unsafe fn host_coll_set(
    p: *mut NzbScalars,
    a1: c_int,
    a2: c_int,
    p2: usize,
    s1: *const c_char,
    i64s: *const i64,
) -> bool {
    let empty = c"".as_ptr();
    let or_empty = |s: *const c_char| if s.is_null() { empty } else { s };
    let v64 = || unsafe { *i64s };
    let np: *const NzbScalars = std::ptr::null();
    let ns: *const c_char = std::ptr::null();
    let coll = move |op| unsafe {
        nzbget_rs_nzb_coll(
            p,
            op,
            0,
            np,
            ns,
            ns,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    let files = move |op| unsafe {
        nzbget_rs_nzb_files(
            p,
            op,
            0,
            np,
            ns,
            ns,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    let s = unsafe { &mut *p };
    match a1 {
        2 => s.deletePaused = a2 != 0,
        9 => s.changed = a2 != 0,
        10 => s.allFirst = a2 != 0,
        11 => s.healthPaused = a2 != 0,
        12 => s.deadPickProbed = a2 != 0,
        14 => s.dupeNoParNoted = a2 != 0,
        25 => s.waitingPar = a2 != 0,
        26 => s.loadingPar = a2 != 0,
        27 => s.deleting = a2 != 0,
        28 => s.parking = a2 != 0,
        29 => s.avoidHistory = a2 != 0,
        60 => s.cleanupDisk = a2 != 0,
        68 => s.parFull = a2 != 0,
        73 => s.unpackCleanedUpDisk = a2 != 0,
        83 => s.addUrlPaused = a2 != 0,
        84 => s.autoCategory = a2 != 0,
        86 => s.skipScriptProcessing = a2 != 0,
        87 => s.skipDiskWrite = a2 != 0,
        89 => s.manyDupeFiles = a2 != 0,
        91 => s.reprocess = a2 != 0,
        92 => s.parRepaired = a2 != 0,
        93 => s.unpackTried = a2 != 0,
        1 => s.deleteStatus = a2,
        3 => s.markStatus = a2,
        4 => s.dupeHint = a2,
        5 => s.dupeScore = a2,
        6 => s.priority = a2,
        13 => s.dupeParDeferState = a2,
        15 => s.directRenameStatus = a2,
        17 => s.currentSuccessArticles = a2,
        18 => s.currentFailedArticles = a2,
        19 => s.dupeRecoveredArticles = a2,
        20 => s.dupeAttemptedArticles = a2,
        21 => s.dupeUnsourcedArticles = a2,
        22 => s.dupeFailedRun = a2,
        23 => s.dupeFailoverChecked = a2,
        24 => s.remainingParCount = a2,
        61 => s.dupeMode = a2,
        62 => s.parStatus = a2,
        63 => s.unpackStatus = a2,
        64 => s.directUnpackStatus = a2,
        65 => s.postTotalSec = a2,
        66 => s.urlStatus = a2,
        67 => s.parSec = a2,
        69 => s.repairSec = a2,
        70 => s.extraParBlocks = a2,
        71 => s.dupeRecoveredHoles = a2,
        72 => s.parRenameStatus = a2,
        74 => s.unpackSec = a2,
        114 => s.liveUnpackSec = a2,
        115 => s.liveParSec = a2,
        75 => s.moveStatus = a2,
        76 => s.cleanupStatus = a2,
        77 => s.rarRenameStatus = a2,
        78 => s.postDownloadRenamingStatus = a2,
        79 => s.postUnpackRenamingStatus = a2,
        81 => s.kind = a2,
        82 => s.feedId = a2,
        85 => s.desiredServerId = a2,
        88 => s.extraPriority = a2,
        90 => s.parkedFileCount = a2,
        94 => s.lastUnpackStatus = a2,
        99 => s.downloadSec = a2,
        100 => s.failedArticles = a2,
        101 => s.successArticles = a2,
        106 => s.fileCount = a2,
        108 => s.totalArticles = a2,
        7 => s.fullContentHash = a2 as u32,
        8 => s.filteredContentHash = a2 as u32,
        30 => s.remainingSize = v64(),
        31 => s.pausedSize = v64(),
        32 => s.currentSuccessSize = v64(),
        33 => s.currentFailedSize = v64(),
        34 => s.parCurrentSuccessSize = v64(),
        35 => s.parCurrentFailedSize = v64(),
        36 => s.downloadedSize = v64(),
        37 => s.parSize = v64(),
        38 => s.parFailedSize = v64(),
        39 => s.dupeRecoveredBytes = v64(),
        47 => s.failedSize = v64(),
        48 => s.successSize = v64(),
        49 => s.parSuccessSize = v64(),
        102 => s.size = v64(),
        103 => s.minTime = v64(),
        104 => s.maxTime = v64(),
        105 => s.queueScriptTime = v64(),
        40 => unsafe { nzbget_rs_nzb_set_text(p, 2, s1, -1) },
        42 => unsafe { nzbget_rs_nzb_set_text(p, 5, s1, -1) },
        43 => unsafe { nzbget_rs_nzb_set_text(p, 3, s1, -1) },
        44 => unsafe { nzbget_rs_nzb_set_text(p, 7, or_empty(s1), -1) },
        45 => unsafe { nzbget_rs_nzb_set_text(p, 6, s1, -1) },
        46 => unsafe { nzbget_rs_nzb_set_text(p, 8, or_empty(s1), -1) },
        59 => unsafe { nzbget_rs_nzb_set_text(p, 4, or_empty(s1), -1) },
        56 => {
            coll(13);
        }
        57 => unsafe {
            nzbget_rs_nzb_slot(p, 2, 1, 0);
            nzbget_rs_nzb_slot(p, 3, 1, 0);
        },
        58 => unsafe {
            nzbget_rs_nzb_slot(p, 3, 1, 0);
        },
        80 => unsafe {
            nzbget_rs_nzb_slot(p, 2, 1, 0);
        },
        95 => {
            files(33);
        }
        107 => unsafe {
            nzbget_rs_nzb_slot(p, 4, 1, a2 as isize as usize);
        },
        111 => {
            coll(5);
        }
        112 => {
            files(27);
        }
        113 => {
            files(7);
        }
        0 => set_id(s, a2),
        16 => set_active_downloads(s, a2, unix_now()),
        41 => unsafe { set_filename(p, s1) },
        50 => unsafe { build_dest_dir_name(p) },
        51 => unsafe { update_min_max_time(p) },
        52 => unsafe { update_completed_stats(p, p2 as *const crate::qfile::File) },
        53 => unsafe { update_deleted_stats(p, p2 as *const crate::qfile::File) },
        96 => unsafe { set_url(p, s1) },
        97 => {
            if file_list(p) != 0 {
                ptr_list_op(file_list(p), 9, 0, 0, std::ptr::null(), 0);
            }
        }
        98 => unsafe { update_current_stats(p) },
        54 => unsafe { enter_post_process(p) },
        55 => unsafe { leave_post_process(p) },
        109 | 110 => {
            let post = unsafe { nzbget_rs_nzb_slot(p, 1, 0, 0) } as *const Post;
            if !post.is_null() {
                if a1 == 109 {
                    post_set(post, 0, i64::from(a2));
                } else {
                    post_set(post, 4, i64::from(a2 & 1 != 0));
                    post_set(post, 8, i64::from(a2 & 2 != 0));
                }
            }
        }
        _ => return false,
    }
    true
}

/// A collection's list and text ops (host op 31), as the C++ host did them.
/// Strings handed back in `strs` by 3 and 16 stay valid on this thread until
/// the next such call.
///
/// # Safety
/// `p` is a live collection, `p2` one too where the op names it; output
/// arrays are large enough for the op.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn host_coll_op(
    p: *const NzbScalars,
    a1: c_int,
    a2: c_int,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    ints: *mut c_int,
    strs: *mut *const c_char,
) -> Option<usize> {
    let np: *const NzbScalars = std::ptr::null();
    let ns: *const c_char = std::ptr::null();
    let ni: *mut c_int = std::ptr::null_mut();
    let nstr: *mut *const c_char = std::ptr::null_mut();
    let empty = c"".as_ptr();
    let or_empty = |s: *const c_char| if s.is_null() { empty } else { s };
    let other = p2 as *const NzbScalars;
    let coll = |op, a2, other, s1, s2, ints, strs| unsafe {
        nzbget_rs_nzb_coll(p, op, a2, other, s1, s2, ints, strs)
    };
    let files = |op, a2, s1, s2, strs| unsafe {
        nzbget_rs_nzb_files(
            p,
            op,
            a2,
            np,
            s1,
            s2,
            ni,
            std::ptr::null_mut(),
            strs,
            std::ptr::null(),
        )
    };
    let log_buffer = || crate::options::active_options().map_or(0, |o| o.scalars().log_buffer);
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as c_int)
    };
    Some(match a1 {
        0 => usize::from(coll(0, 0, np, s1, ns, ni, strs) != 0),
        1 => {
            coll(1, 0, np, s1, s2, ni, nstr);
            0
        }
        2 => {
            coll(2, 0, other, ns, ns, ni, nstr);
            0
        }
        4 => usize::from(coll(20, a2, np, ns, ns, ni, nstr) != 0),
        5 => {
            coll(21, a2, np, ns, ns, ni, nstr);
            1
        }
        6 | 7 => {
            unsafe { *strs = nzbget_rs_nzb_text(p, if a1 == 6 { 8 } else { 9 }) };
            0
        }
        8 => {
            if a2 < 0 {
                return Some(coll(3, 0, np, ns, ns, ni, nstr) as usize);
            }
            coll(4, a2, np, ns, ns, ni, strs);
            0
        }
        9 => {
            if a2 < 0 {
                return Some(coll(40, 0, np, ns, ns, ni, nstr) as usize);
            }
            coll(42, a2, np, ns, ns, ints, nstr);
            0
        }
        10 => {
            coll(10, a2, np, or_empty(s1), ns, ni, nstr);
            0
        }
        11 => usize::from(files(4, 0, or_empty(s1), or_empty(s2), nstr) != 0),
        12 => unsafe { nzbget_rs_nzb_slot(p, 2, 0, 0) },
        13 => {
            files(32, 0, or_empty(s1), ns, nstr);
            0
        }
        14 => {
            unsafe { nzbget_rs_nzb_set_text(p, 9, or_empty(s1), -1) };
            0
        }
        15 => {
            let lim = [now(), log_buffer()];
            unsafe { nzbget_rs_nzb_msgs(p, 3, 0, other, ns, lim.as_ptr(), None, 0) };
            0
        }
        17 => {
            if a2 < 0 {
                return Some(coll(11, 0, np, ns, ns, ni, nstr) as usize);
            }
            coll(12, a2, np, ns, ns, ints, strs);
            0
        }
        18 => {
            if a2 < 0 {
                return Some(files(30, 0, ns, ns, nstr) as usize);
            }
            unsafe { *strs = empty };
            files(31, a2, ns, ns, strs);
            0
        }
        19 => {
            if a2 == 0 {
                coll(44, 0, np, ns, ns, ints, nstr);
            } else {
                coll(48, 0, np, ns, ns, ni, nstr);
            }
            0
        }
        20 => {
            let visit: MsgVisit = unsafe { std::mem::transmute::<*const c_char, MsgVisit>(s1) };
            unsafe { nzbget_rs_nzb_msgs(p, 4, 0, np, ns, ints, visit, p2) };
            0
        }
        21 => {
            if a2 < 0 {
                return Some(coll(41, 0, np, ns, ns, ni, nstr) as usize);
            }
            coll(43, a2, np, ns, ns, ints, nstr);
            0
        }
        3 => {
            let dir = unsafe { final_dir_name(p) };
            unsafe { *strs = keep_text(&FINAL_DIR, dir) };
            0
        }
        16 => {
            let name = crate::downloadinfo::make_nice_url_name(Some(cstr(s1)), cstr(s2).to_bytes());
            unsafe { *strs = keep_text(&NICE_URL, name) };
            0
        }
        22 => usize::from(unsafe { has_active_output(p) }),
        _ => 0,
    })
}

/// NzbInfo::SetId: the id, raising the id floor.
fn set_id(s: &mut NzbScalars, id: c_int) {
    s.id = id;
    nzbget_rs_nzb_ids(3, id);
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// NzbInfo::SetActiveDownloads: accounts download time when downloads start
/// or stop.
fn set_active_downloads(s: &mut NzbScalars, active: c_int, now: i64) {
    let (sec, start_time, start_sec, changed) = crate::downloadinfo::calc_active_downloads_sec(
        s.activeDownloads,
        active,
        s.kind == 0,
        now,
        s.downloadSec,
        s.downloadStartTime,
        s.downloadStartSec,
    );
    s.downloadSec = sec;
    s.downloadStartTime = start_time;
    s.downloadStartSec = start_sec;
    if changed {
        s.changed = true;
    }
    s.activeDownloads = active;
}

/// Text field `id` as bytes (empty when unset).
fn text_bytes(p: *const NzbScalars, id: c_int) -> Vec<u8> {
    let t = unsafe { nzbget_rs_nzb_text(p, id) };
    if t.is_null() {
        Vec::new()
    } else {
        unsafe { CStr::from_ptr(t) }.to_bytes().to_vec()
    }
}

fn text_is_set(p: *const NzbScalars, id: c_int) -> bool {
    !unsafe { nzbget_rs_nzb_text(p, id) }.is_null()
}

/// Sets text field `id` to `bytes` (up to an embedded NUL, as C++ did).
fn set_text_bytes(p: *const NzbScalars, id: c_int, bytes: &[u8]) {
    unsafe { nzbget_rs_nzb_set_text(p, id, bytes.as_ptr().cast(), bytes.len() as isize) };
}

/// The destination directory of the collection's category, when it names
/// one (empty otherwise).
fn category_dest_dir(opts: Option<&crate::options::NzbgetRsOptions>, category: &[u8]) -> Vec<u8> {
    let (Some(opts), false) = (opts, category.is_empty()) else {
        return Vec::new();
    };
    let Ok(name) = CString::new(category) else {
        return Vec::new();
    };
    opts.find_category(&name, false)
        .map(|c| c.dest_dir.to_bytes().to_vec())
        .unwrap_or_default()
}

/// NzbInfo::BuildFinalDirName.
unsafe fn final_dir_name(p: *const NzbScalars) -> Vec<u8> {
    let opts = crate::options::active_options();
    let o = opts.as_deref();
    let category = text_bytes(p, 5);
    crate::downloadinfo::build_final_dir_name(
        o.map_or(&[][..], |o| o.str_bytes(3)),
        &category_dest_dir(o, &category),
        &category,
        o.is_some_and(|o| o.scalars().append_category_dir != 0),
        &text_bytes(p, 2),
        unsafe { (*p).id },
    )
}

/// NzbInfo::BuildDestDirName: sets the destination directory.
unsafe fn build_dest_dir_name(p: *mut NzbScalars) {
    let opts = crate::options::active_options();
    let o = opts.as_deref();
    let inter_dir = o.map_or(&[][..], |o| o.str_bytes(4));
    let category = text_bytes(p, 5);
    let category_dest = if inter_dir.is_empty() {
        category_dest_dir(o, &category)
    } else {
        Vec::new()
    };
    let dir = crate::downloadinfo::build_dest_dir_name(
        inter_dir,
        o.map_or(&[][..], |o| o.str_bytes(3)),
        &category_dest,
        &category,
        o.is_some_and(|o| o.scalars().append_category_dir != 0),
        &text_bytes(p, 2),
        unsafe { (*p).id },
    );
    set_text_bytes(p, 3, &dir);
}

thread_local! {
    static FINAL_DIR: std::cell::RefCell<CString> = std::cell::RefCell::new(CString::default());
    static NICE_URL: std::cell::RefCell<CString> = std::cell::RefCell::new(CString::default());
}

/// Keeps `bytes` (up to an embedded NUL) in this thread's `slot` and returns
/// a pointer valid until the slot's next use on this thread.
fn keep_text(
    slot: &'static std::thread::LocalKey<std::cell::RefCell<CString>>,
    mut bytes: Vec<u8>,
) -> *const c_char {
    if let Some(end) = bytes.iter().position(|&b| b == 0) {
        bytes.truncate(end);
    }
    slot.with(|s| {
        let mut s = s.borrow_mut();
        *s = CString::new(bytes).unwrap_or_default();
        s.as_ptr()
    })
}

/// NzbInfo::SetFilename: names the collection from the file name when it
/// has no name yet, or had no file name.
unsafe fn set_filename(p: *mut NzbScalars, filename: *const c_char) {
    let had_filename = !text_bytes(p, 1).is_empty();
    unsafe { nzbget_rs_nzb_set_text(p, 1, filename, -1) };
    if (!text_is_set(p, 2) || !had_filename) && !cstr(filename).is_empty() {
        let mut nice = crate::downloadinfo::make_nice_nzb_name(&text_bytes(p, 1), true);
        if nice.is_empty() {
            nice = format!("nzb_{}", unsafe { (*p).id }).into_bytes();
        }
        set_text_bytes(p, 2, &nice);
    }
}

/// NzbInfo::SetUrl: names the collection from the URL when it has no name.
unsafe fn set_url(p: *mut NzbScalars, url: *const c_char) {
    unsafe { nzbget_rs_nzb_set_text(p, 0, url, -1) };
    if !text_is_set(p, 2) {
        let url = (!url.is_null()).then(|| unsafe { CStr::from_ptr(url) });
        let nice = crate::downloadinfo::make_nice_url_name(url, &text_bytes(p, 1));
        set_text_bytes(p, 2, &nice);
    }
}

fn files_of(p: *const NzbScalars) -> Vec<*const crate::qfile::File> {
    let fl = file_list(p);
    if fl == 0 {
        return Vec::new();
    }
    (0..ptr_list_op(fl, 0, 0, 0, std::ptr::null(), 0))
        .map(|i| {
            ptr_list_op(fl, 1, i, 0, std::ptr::null(), 0) as usize as *const crate::qfile::File
        })
        .collect()
}

fn file_get(f: *const crate::qfile::File, id: c_int) -> i64 {
    unsafe { crate::qfile::nzbget_rs_file_get(f, id) }
}

/// NzbInfo::UpdateMinMaxTime over the file list.
unsafe fn update_min_max_time(p: *mut NzbScalars) {
    let times: Vec<i64> = files_of(p)
        .into_iter()
        .map(|f| file_get(f, crate::qfile::field::TIME))
        .collect();
    let (min_t, max_t) = crate::downloadinfo::calc_min_max_time(&times);
    let s = unsafe { &mut *p };
    s.minTime = min_t;
    s.maxTime = max_t;
}

/// NzbInfo::UpdateCompletedStats: a file finished downloading.
unsafe fn update_completed_stats(p: *mut NzbScalars, f: *const crate::qfile::File) {
    use crate::qfile::field;
    let g = |id| file_get(f, id);
    {
        let s = unsafe { &mut *p };
        crate::downloadinfo::update_completed_stats(
            g(field::SUCCESS_SIZE),
            g(field::FAILED_SIZE),
            g(field::FAILED_ARTICLES) as i32,
            g(field::SUCCESS_ARTICLES) as i32,
            g(field::EXTRA_PRIORITY) != 0,
            g(field::PAR_FILE) != 0,
            g(field::PAUSED) != 0,
            &mut s.successSize,
            &mut s.failedSize,
            &mut s.failedArticles,
            &mut s.successArticles,
            &mut s.extraPriority,
            &mut s.parSuccessSize,
            &mut s.parFailedSize,
            &mut s.remainingParCount,
            &mut s.pausedFileCount,
        );
    }
    unsafe { crate::qfile::add_file_stats(p as usize, f, 0, 1) };
}

/// NzbInfo::UpdateDeletedStats: a file left the queue undownloaded.
unsafe fn update_deleted_stats(p: *mut NzbScalars, f: *const crate::qfile::File) {
    use crate::qfile::field;
    let g = |id| file_get(f, id);
    {
        let s = unsafe { &mut *p };
        crate::downloadinfo::update_deleted_stats(
            g(field::SIZE),
            g(field::SUCCESS_SIZE),
            g(field::FAILED_SIZE),
            g(field::MISSED_SIZE),
            g(field::TOTAL_ARTICLES) as i32,
            g(field::SUCCESS_ARTICLES) as i32,
            g(field::FAILED_ARTICLES) as i32,
            g(field::MISSED_ARTICLES) as i32,
            g(field::REMAINING_SIZE),
            g(field::EXTRA_PRIORITY) != 0,
            g(field::PAR_FILE) != 0,
            g(field::PAUSED) != 0,
            &mut s.fileCount,
            &mut s.size,
            &mut s.currentSuccessSize,
            &mut s.failedSize,
            &mut s.failedArticles,
            &mut s.currentFailedSize,
            &mut s.totalArticles,
            &mut s.currentSuccessArticles,
            &mut s.currentFailedArticles,
            &mut s.remainingSize,
            &mut s.extraPriority,
            &mut s.remainingParCount,
            &mut s.parSize,
            &mut s.parCurrentSuccessSize,
            &mut s.parFailedSize,
            &mut s.parCurrentFailedSize,
            &mut s.pausedFileCount,
            &mut s.pausedSize,
        );
    }
    unsafe { crate::qfile::add_file_stats(p as usize, f, 1, 2) };
}

/// NzbInfo::UpdateCurrentStats: recomputes the current-session counters
/// from the totals and the files still queued.
unsafe fn update_current_stats(p: *mut NzbScalars) {
    use crate::qfile::field;
    let mut acc = {
        let s = unsafe { &*p };
        crate::downloadinfo::RsDownloadCurrentStatsAcc {
            current_success_articles: s.successArticles,
            current_failed_articles: s.failedArticles,
            current_success_size: s.successSize,
            current_failed_size: s.failedSize,
            par_current_success_size: s.parSuccessSize,
            par_current_failed_size: s.parFailedSize,
            ..Default::default()
        }
    };
    unsafe {
        nzbget_rs_nzb_coll(
            p,
            48,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    for f in files_of(p) {
        let g = |id| file_get(f, id);
        crate::downloadinfo::update_current_stats_step(
            &mut acc,
            g(field::REMAINING_SIZE),
            g(field::SUCCESS_ARTICLES) as i32,
            g(field::FAILED_ARTICLES) as i32,
            g(field::SUCCESS_SIZE),
            g(field::FAILED_SIZE),
            g(field::EXTRA_PRIORITY) != 0,
            g(field::PAUSED) != 0,
            g(field::PAR_FILE) != 0,
        );
        unsafe { crate::qfile::add_file_stats(p as usize, f, 1, 1) };
    }
    let s = unsafe { &mut *p };
    s.pausedFileCount = acc.paused_file_count;
    s.remainingParCount = acc.remaining_par_count;
    s.remainingSize = acc.remaining_size;
    s.pausedSize = acc.paused_size;
    s.currentSuccessArticles = acc.current_success_articles;
    s.currentFailedArticles = acc.current_failed_articles;
    s.currentSuccessSize = acc.current_success_size;
    s.currentFailedSize = acc.current_failed_size;
    s.parCurrentSuccessSize = acc.par_current_success_size;
    s.parCurrentFailedSize = acc.par_current_failed_size;
    s.extraPriority = acc.extra_priority;
}

/// Whether a file still downloading has its output file open (host op 31
/// sub-op 22): checked again under the file's output lock.
unsafe fn has_active_output(p: *const NzbScalars) -> bool {
    use crate::qfile::{field, nzbget_rs_file_op, nzbget_rs_file_text, text};
    for f in files_of(p) {
        let out = unsafe { nzbget_rs_file_text(f, text::OUTPUT_FILENAME) };
        let has_out = !out.is_null() && unsafe { *out } != 0;
        if file_get(f, field::OUTPUT_INITIALIZED) != 0
            && has_out
            && file_get(f, field::ACTIVE_DOWNLOADS) > 0
        {
            let ns = std::ptr::null();
            let taken = unsafe {
                nzbget_rs_file_op(f, 20, 0, ns, std::ptr::null_mut(), std::ptr::null_mut())
            } != 0;
            let initialized = file_get(f, field::OUTPUT_INITIALIZED) != 0;
            if taken {
                unsafe {
                    nzbget_rs_file_op(f, 21, 0, ns, std::ptr::null_mut(), std::ptr::null_mut())
                };
            }
            if initialized {
                return true;
            }
        }
    }
    false
}

fn calc_health(s: &NzbScalars) -> i32 {
    crate::downloadinfo::calc_health(
        s.size,
        s.parSize,
        s.currentFailedSize,
        s.parCurrentFailedSize,
    )
}

fn calc_critical_health(s: &NzbScalars, allow_estimation: bool) -> i32 {
    crate::downloadinfo::calc_critical_health(
        s.size,
        s.parSize,
        s.parCurrentFailedSize,
        allow_estimation,
    )
}

/// NzbInfo::IsDownloadCompleted over the file list.
fn is_download_completed(p: *const NzbScalars, ignore_paused_pars: bool) -> bool {
    use crate::qfile::field;
    let active = unsafe { (*p).activeDownloads };
    if active != 0 {
        return false;
    }
    let files = files_of(p);
    crate::downloadinfo::is_download_completed_with(active, ignore_paused_pars, files.len(), |i| {
        let f = files[i];
        (
            file_get(f, field::PAUSED) != 0,
            file_get(f, field::PAR_FILE) != 0,
            file_get(f, field::DELETED) != 0,
        )
    })
}

/// Health check option value `hcDupe`.
const HEALTH_CHECK_DUPE: i32 = 4;
/// Priority at and above which a collection is forced.
const FORCE_PRIORITY: i32 = 900;
/// `ScriptStatus::srSuccess`.
const SCRIPT_STATUS_SUCCESS: i32 = 2;

/// A collection's snapshot (host op 30): 90 ints, 21 i64s and, when `strs`
/// is not null, 9 strings, in the order the C++ host wrote them. `a1` 1 or 2
/// adds the text status (ignoring, or not, the script status).
///
/// # Safety
/// `p` is a live collection; `ints`, `i64s` and `strs` (if not null) hold
/// 90, 21 and 9 entries; the caller holds the queue lock.
pub(crate) unsafe fn host_coll_snapshot(
    p: *const NzbScalars,
    a1: c_int,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) {
    let s = unsafe { &*p };
    let ints = unsafe { std::slice::from_raw_parts_mut(ints, 90) };
    let i64s = unsafe { std::slice::from_raw_parts_mut(i64s, 21) };
    let ns: *const c_char = std::ptr::null();
    let coll = |op| unsafe {
        nzbget_rs_nzb_coll(
            p,
            op,
            0,
            std::ptr::null(),
            ns,
            ns,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    let files = |op| unsafe {
        nzbget_rs_nzb_files(
            p,
            op,
            0,
            std::ptr::null(),
            ns,
            ns,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    let post = unsafe { nzbget_rs_nzb_slot(p, 1, 0, 0) } as *const Post;
    let post_flag = |id| !post.is_null() && post_get(post, id) != 0;
    let unpack_thread = unsafe { nzbget_rs_nzb_slot(p, 2, 0, 0) };
    let live_repair_thread = unsafe { nzbget_rs_nzb_slot(p, 3, 0, 0) };
    let message_count = unsafe { nzbget_rs_nzb_slot(p, 4, 0, 0) } as isize as c_int;
    let b = c_int::from;
    let health = calc_health(s);
    let critical_est = calc_critical_health(s, true);
    let critical_exact = calc_critical_health(s, false);
    let health_check_dupe = crate::options::active_options()
        .is_some_and(|o| o.scalars().health_check == HEALTH_CHECK_DUPE);
    let dupe_success = crate::downloadinfo::is_dupe_success(
        s.markStatus,
        s.deleteStatus,
        s.parStatus,
        s.unpackStatus,
        s.urlStatus,
        health,
        critical_est,
        critical_exact,
        health_check_dupe,
    );
    ints[0] = s.id;
    ints[1] = s.kind;
    ints[2] = s.dupeMode;
    ints[3] = s.dupeScore;
    ints[4] = s.dupeHint;
    ints[5] = s.deleteStatus;
    ints[6] = s.markStatus;
    ints[7] = s.parStatus;
    ints[8] = s.directRenameStatus;
    ints[9] = s.directUnpackStatus;
    ints[10] = s.dupeParDeferState;
    ints[11] = b(s.deleting);
    ints[12] = b(s.parking);
    ints[13] = b(s.healthPaused);
    ints[14] = b(s.priority >= FORCE_PRIORITY);
    ints[15] = b(s.extraPriority > 0);
    ints[16] = b(s.manyDupeFiles);
    ints[17] = b(s.desiredServerId > 0);
    ints[18] = s.desiredServerId;
    ints[19] = s.priority;
    ints[20] = s.feedId;
    ints[21] = b(s.changed);
    ints[22] = b(s.allFirst);
    ints[23] = b(s.deadPickProbed);
    ints[24] = b(s.dupeNoParNoted);
    ints[25] = b(!post.is_null());
    ints[26] = b(post_flag(1));
    ints[27] = if post.is_null() {
        0
    } else {
        post_get(post, 0) as c_int
    };
    ints[28] = b(live_repair_thread != 0);
    ints[29] = b(dupe_success);
    ints[30] = health;
    ints[31] = critical_est;
    ints[32] = critical_exact;
    ints[33] = b(is_download_completed(p, true));
    ints[34] = s.fileCount;
    ints[35] = s.activeDownloads;
    ints[36] = s.pausedFileCount;
    ints[37] = s.remainingParCount;
    ints[38] = s.totalArticles;
    ints[39] = s.successArticles;
    ints[40] = s.failedArticles;
    ints[41] = s.currentSuccessArticles;
    ints[42] = s.currentFailedArticles;
    ints[43] = s.dupeRecoveredArticles;
    ints[44] = s.dupeAttemptedArticles;
    ints[45] = s.dupeUnsourcedArticles;
    ints[46] = s.dupeFailedRun;
    ints[47] = s.dupeFailoverChecked;
    ints[48] = s.downloadSec;
    ints[49] = s.fullContentHash as c_int;
    ints[50] = b(s.skipDiskWrite);
    ints[51] = b(s.waitingPar);
    ints[52] = b(s.loadingPar);
    ints[53] = b(s.unpackCleanedUpDisk);
    ints[54] = b(is_download_completed(p, false));
    ints[55] = b(s.skipScriptProcessing);
    ints[56] = b(s.avoidHistory);
    ints[57] = b(s.cleanupDisk);
    ints[58] = b(s.unpackTried);
    ints[59] = b(s.parFull);
    ints[60] = s.postTotalSec;
    ints[61] = s.parRenameStatus;
    ints[62] = s.rarRenameStatus;
    ints[63] = s.unpackStatus;
    ints[64] = s.cleanupStatus;
    ints[65] = s.moveStatus;
    ints[66] = s.postDownloadRenamingStatus;
    ints[67] = s.postUnpackRenamingStatus;
    ints[68] = b(unpack_thread != 0);
    ints[69] = b(files(20) > 0);
    ints[70] = s.urlStatus;
    ints[71] = s.parSec;
    ints[72] = s.repairSec;
    ints[73] = s.unpackSec;
    ints[74] = b(s.reprocess);
    ints[75] = s.extraParBlocks;
    ints[76] = s.dupeRecoveredHoles;
    let script_total = coll(14) as c_int;
    ints[77] = script_total;
    ints[78] = s.parkedFileCount;
    ints[79] = b(s.deletePaused);
    ints[80] = b(s.autoCategory);
    ints[81] = b(s.addUrlPaused);
    ints[82] = message_count;
    ints[83] = b(post_flag(4));
    ints[84] = b(post_flag(8));
    ints[85] = b(s.parRepaired);
    ints[86] = s.lastUnpackStatus;
    ints[87] = s.cachedMessageCount;
    ints[88] = s.liveUnpackSec;
    ints[89] = s.liveParSec;
    i64s[0] = s.size;
    i64s[1] = s.remainingSize;
    i64s[2] = s.pausedSize;
    i64s[3] = s.successSize;
    i64s[4] = s.currentSuccessSize;
    i64s[5] = s.failedSize;
    i64s[6] = s.currentFailedSize;
    i64s[7] = s.parSize;
    i64s[8] = s.parSuccessSize;
    i64s[9] = s.parCurrentSuccessSize;
    i64s[10] = s.parFailedSize;
    i64s[11] = s.parCurrentFailedSize;
    i64s[12] = s.downloadedSize;
    i64s[13] = s.minTime;
    i64s[14] = s.maxTime;
    i64s[15] = i64::from(s.filteredContentHash);
    i64s[16] = s.downloadStartTime;
    i64s[17] = s.dupeRecoveredBytes;
    i64s[18] = post as i64;
    i64s[19] = live_repair_thread as i64;
    i64s[20] = s.queueScriptTime;
    if strs.is_null() {
        return;
    }
    let strs = unsafe { std::slice::from_raw_parts_mut(strs, 9) };
    let t = |id| unsafe { nzbget_rs_nzb_text(p, id) };
    strs[0] = t(2);
    strs[1] = t(1);
    strs[2] = t(3);
    strs[3] = t(4);
    strs[4] = t(5);
    strs[5] = t(7);
    strs[6] = t(6);
    // a static literal, so it outlives the call
    strs[7] = match a1 {
        1 | 2 => crate::downloadinfo::make_text_status_cstr(
            s.kind,
            s.markStatus,
            s.deleteStatus,
            s.parStatus,
            s.unpackStatus,
            s.moveStatus,
            s.urlStatus,
            if a1 == 1 {
                SCRIPT_STATUS_SUCCESS
            } else {
                script_total
            },
            health,
            critical_exact,
        )
        .as_ptr()
        .cast(),
        _ => std::ptr::null(),
    };
    strs[8] = t(0);
}

/// Host handles kept with a collection: 0 file list, 1 post-processing info,
/// 2 direct-unpack job, 3 live-repair job, 4 message count. The host owns
/// what they point to. set != 0 stores v; returns the (previous) value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_nzb_slot(
    p: *const NzbScalars,
    id: c_int,
    set: c_int,
    v: usize,
) -> usize {
    use std::sync::atomic::Ordering::SeqCst;
    let Some(slot) = usize::try_from(id).ok().and_then(|i| data(p).slots.get(i)) else {
        return 0;
    };
    if set != 0 {
        slot.swap(v, SeqCst)
    } else {
        slot.load(SeqCst)
    }
}

/// Called once per message, oldest first: (ctx, id, kind, time, text).
pub type MsgVisit = Option<unsafe extern "C" fn(usize, c_int, c_int, c_int, *const c_char)>;

/// A collection's message log. The caller prints; this keeps the messages.
///  0 add kind a2, text s1, time ints[0]; keep at most ints[1] when > 0;
///    the id comes from the collection's idMessageGen; returns the count
///  1 count; 2 clear;
///  3 add p2's messages (fresh ids, time now in ints[0], limit ints[1]);
///  4 visit messages: from id ints[0] when > 0, else the last ints[1] when
///    > 0, else none.
/// cachedMessageCount follows every change.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_nzb_msgs(
    p: *const NzbScalars,
    op: c_int,
    a2: c_int,
    p2: *const NzbScalars,
    s1: *const c_char,
    ints: *const c_int,
    visit: MsgVisit,
    ctx: usize,
) -> i64 {
    let copied: Vec<(i32, CString)> = if op == 3 && !p2.is_null() && p2 != p {
        let o = data(p2).msgs.lock().unwrap_or_else(|e| e.into_inner());
        o.iter().map(|m| (m.1, m.3.clone())).collect()
    } else {
        Vec::new()
    };
    let mut m = data(p).msgs.lock().unwrap_or_else(|e| e.into_inner());
    // the scalars are C++-owned plain memory; these two fields change only
    // under this lock
    let sp = p.cast_mut();
    let int = |k: usize| unsafe { *ints.add(k) };
    let push = |m: &mut std::collections::VecDeque<_>, kind: i32, text: CString| unsafe {
        let id = (*sp).idMessageGen.wrapping_add(1);
        (*sp).idMessageGen = id;
        m.push_back((id as u32, kind, i64::from(int(0)), text));
        let limit = int(1);
        while limit > 0 && m.len() > limit as usize {
            m.pop_front();
        }
    };
    let ret = match op {
        0 => {
            push(&mut m, a2, owned(s1));
            m.len() as i64
        }
        1 => return m.len() as i64,
        2 => {
            m.clear();
            0
        }
        3 => {
            for (kind, text) in copied {
                push(&mut m, kind, text);
            }
            0
        }
        4 => {
            let Some(f) = visit else { return 0 };
            let sz = m.len();
            let mut start = sz;
            let (from_id, last) = (int(0), int(1));
            if last > 0 {
                start = sz - (last as usize).min(sz);
            }
            if from_id > 0 && sz > 0 {
                start = (i64::from(from_id) - i64::from(m[0].0)).max(0) as usize;
            }
            for e in m.iter().skip(start) {
                unsafe { f(ctx, e.0 as c_int, e.1, e.2 as c_int, e.3.as_ptr()) };
            }
            return 0;
        }
        _ => return 0,
    };
    unsafe { (*sp).cachedMessageCount = m.len() as i32 };
    ret
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_start_empty_except_name() {
        let p = nzbget_rs_nzb_scalars_new();
        unsafe {
            assert!(nzbget_rs_nzb_text(p, 2).is_null());
            assert_eq!(CStr::from_ptr(nzbget_rs_nzb_text(p, 0)).to_bytes(), b"");
            let v = CString::new("Show.S01E01").unwrap();
            nzbget_rs_nzb_set_text(p, 2, v.as_ptr(), -1);
            assert_eq!(
                CStr::from_ptr(nzbget_rs_nzb_text(p, 2)).to_bytes(),
                b"Show.S01E01"
            );
            nzbget_rs_nzb_set_text(p, 3, v.as_ptr(), 4);
            assert_eq!(CStr::from_ptr(nzbget_rs_nzb_text(p, 3)).to_bytes(), b"Show");
            nzbget_rs_nzb_set_text(p, 2, std::ptr::null(), -1);
            assert!(nzbget_rs_nzb_text(p, 2).is_null());
            (*p).id = 7;
            assert_eq!((*p).id, 7);
            nzbget_rs_nzb_scalars_free(p);
        }
    }

    #[test]
    fn small_collections() {
        let p = nzbget_rs_nzb_scalars_new();
        let q = nzbget_rs_nzb_scalars_new();
        let np: *const NzbScalars = std::ptr::null();
        let ns: *const c_char = std::ptr::null();
        let mut ints = [0 as c_int; 4];
        let mut strs = [ns; 2];
        let mut coll = |n: *const NzbScalars,
                        op: c_int,
                        a2: c_int,
                        p2: *const NzbScalars,
                        s1: *const c_char,
                        s2: *const c_char| unsafe {
            nzbget_rs_nzb_coll(n, op, a2, p2, s1, s2, ints.as_mut_ptr(), strs.as_mut_ptr())
        };
        coll(p, 1, 0, np, c"Category".as_ptr(), c"tv".as_ptr());
        assert_eq!(coll(p, 0, 0, np, c"category".as_ptr(), ns), 1);
        coll(p, 1, 0, np, c"CATEGORY".as_ptr(), c"".as_ptr());
        assert_eq!(coll(p, 3, 0, np, ns, ns), 0);
        coll(q, 1, 0, np, c"a".as_ptr(), c"1".as_ptr());
        coll(p, 2, 0, q, ns, ns);
        assert_eq!(coll(p, 3, 0, np, ns, ns), 1);
        coll(p, 10, 2, np, c"script.py".as_ptr(), ns);
        assert_eq!(coll(p, 11, 0, np, ns, ns), 1);
        assert_eq!(coll(p, 20, 7, np, ns, ns), 0);
        coll(p, 21, 7, np, ns, ns);
        assert_eq!(coll(p, 20, 7, np, ns, ns), 1);
        drop(coll);
        unsafe {
            ints[..3].copy_from_slice(&[1, 5, 2]);
            nzbget_rs_nzb_coll(p, 44, 1, np, ns, ns, ints.as_mut_ptr(), strs.as_mut_ptr());
            nzbget_rs_nzb_coll(p, 48, 0, np, ns, ns, ints.as_mut_ptr(), strs.as_mut_ptr());
            nzbget_rs_nzb_coll(p, 43, 0, np, ns, ns, ints.as_mut_ptr(), strs.as_mut_ptr());
            assert_eq!(&ints[..3], &[1, 5, 2]);
            ints[0] = 0;
            ints[1] = 1;
            nzbget_rs_nzb_coll(q, 49, 1, p, ns, ns, ints.as_mut_ptr(), strs.as_mut_ptr());
            nzbget_rs_nzb_coll(q, 42, 0, np, ns, ns, ints.as_mut_ptr(), strs.as_mut_ptr());
            assert_eq!(&ints[..3], &[1, 5, 2]);
            nzbget_rs_nzb_scalars_free(p);
            nzbget_rs_nzb_scalars_free(q);
        }
    }

    #[test]
    fn completed_files_and_jobs() {
        let p = nzbget_rs_nzb_scalars_new();
        let q = nzbget_rs_nzb_scalars_new();
        let np: *const NzbScalars = std::ptr::null();
        let ns: *const c_char = std::ptr::null();
        let mut ints = [0 as c_int; 5];
        let mut i64s = [0i64; 4];
        let mut strs: [*const c_char; 4];
        unsafe {
            let call = |n,
                        op,
                        a2,
                        p2,
                        s1,
                        s2,
                        raw: *const i64,
                        ints: &mut [c_int; 5],
                        i64s: &mut [i64; 4],
                        strs: &mut [*const c_char; 4]| {
                nzbget_rs_nzb_files(
                    n,
                    op,
                    a2,
                    p2,
                    s1,
                    s2,
                    ints.as_mut_ptr(),
                    i64s.as_mut_ptr(),
                    strs.as_mut_ptr(),
                    raw,
                )
            };
            let nr: *const i64 = std::ptr::null();
            ints[..4].copy_from_slice(&[5, 3, 77, 0]);
            strs = [
                c"a.r00".as_ptr(),
                c"".as_ptr(),
                c"h".as_ptr(),
                c"set".as_ptr(),
            ];
            call(q, 2, 0, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs);
            assert_eq!(
                call(p, 8, 0, q, ns, ns, nr, &mut ints, &mut i64s, &mut strs),
                0
            );
            assert_eq!(
                call(q, 0, 0, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs),
                0
            );
            assert_eq!(
                call(p, 0, 0, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs),
                1
            );
            assert_eq!(
                call(
                    p,
                    4,
                    0,
                    np,
                    c"a.r00".as_ptr(),
                    c"b.r00".as_ptr(),
                    nr,
                    &mut ints,
                    &mut i64s,
                    &mut strs
                ),
                1
            );
            ints[0] = 2;
            call(p, 5, 5, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs);
            call(p, 1, 0, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs);
            assert_eq!(&ints[..4], &[5, 2, 0, 77]);
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"b.r00");
            assert_eq!(CStr::from_ptr(strs[1]).to_bytes(), b"a.r00");
            ints[0] = 1;
            assert_eq!(
                call(p, 5, 5, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs),
                1
            );
            assert_eq!(
                call(p, 5, 5, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs),
                0
            );

            let holes = [10i64, 5, 40, 7];
            ints[..3].copy_from_slice(&[9, 4, 1]);
            i64s = [100, 30, 2, 0xabc];
            call(
                p,
                23,
                2,
                np,
                c"m.mkv".as_ptr(),
                ns,
                holes.as_ptr(),
                &mut ints,
                &mut i64s,
                &mut strs,
            );
            assert_eq!(
                call(p, 20, 0, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs),
                1
            );
            call(p, 21, 0, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs);
            assert_eq!((ints[0], i64s[0], i64s[1], i64s[2]), (9, 100, 12, 32));
            assert_eq!(
                call(p, 24, 0, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs),
                0
            );
            assert_eq!(
                call(p, 24, 0, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs),
                1
            );
            i64s[0] = 200;
            ints[1] = 1;
            call(
                p,
                26,
                0,
                np,
                ns,
                ns,
                holes.as_ptr(),
                &mut ints,
                &mut i64s,
                &mut strs,
            );
            call(p, 22, 0, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs);
            assert_eq!(&ints[..5], &[9, 4, 1, 1, 1]);
            assert_eq!(i64s, [200, 30, 2, 0xabc]);
            assert_eq!(*strs[1].cast::<[i64; 2]>(), [10, 5]);
            call(p, 25, 0, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs);
            assert_eq!(
                call(p, 20, 0, np, ns, ns, nr, &mut ints, &mut i64s, &mut strs),
                0
            );
            nzbget_rs_nzb_scalars_free(p);
            nzbget_rs_nzb_scalars_free(q);
        }
    }

    #[test]
    fn message_log() {
        unsafe extern "C" fn collect(
            ctx: usize,
            id: c_int,
            kind: c_int,
            _t: c_int,
            text: *const c_char,
        ) {
            let v = unsafe { &mut *(ctx as *mut Vec<(i32, i32, Vec<u8>)>) };
            v.push((
                id,
                kind,
                unsafe { CStr::from_ptr(text) }.to_bytes().to_vec(),
            ));
        }
        let p = nzbget_rs_nzb_scalars_new();
        let q = nzbget_rs_nzb_scalars_new();
        let np: *const NzbScalars = std::ptr::null();
        let ns: *const c_char = std::ptr::null();
        unsafe {
            let lim = [100, 2];
            nzbget_rs_nzb_msgs(p, 0, 0, np, c"one".as_ptr(), lim.as_ptr(), None, 0);
            nzbget_rs_nzb_msgs(p, 0, 1, np, c"two".as_ptr(), lim.as_ptr(), None, 0);
            assert_eq!(
                nzbget_rs_nzb_msgs(p, 0, 2, np, c"three".as_ptr(), lim.as_ptr(), None, 0),
                2
            );
            assert_eq!((*p).cachedMessageCount, 2);
            assert_eq!((*p).idMessageGen, 3);
            let mut got: Vec<(i32, i32, Vec<u8>)> = Vec::new();
            let all = [0, 10];
            nzbget_rs_nzb_msgs(
                p,
                4,
                0,
                np,
                ns,
                all.as_ptr(),
                Some(collect),
                &mut got as *mut _ as usize,
            );
            assert_eq!(
                got,
                vec![(2, 1, b"two".to_vec()), (3, 2, b"three".to_vec())]
            );
            got.clear();
            let from = [3, 0];
            nzbget_rs_nzb_msgs(
                p,
                4,
                0,
                np,
                ns,
                from.as_ptr(),
                Some(collect),
                &mut got as *mut _ as usize,
            );
            assert_eq!(got.len(), 1);
            let nolimit = [100, 0];
            nzbget_rs_nzb_msgs(q, 3, 0, p, ns, nolimit.as_ptr(), None, 0);
            assert_eq!(
                nzbget_rs_nzb_msgs(q, 1, 0, np, ns, nolimit.as_ptr(), None, 0),
                2
            );
            assert_eq!((*q).idMessageGen, 2);
            nzbget_rs_nzb_msgs(p, 2, 0, np, ns, nolimit.as_ptr(), None, 0);
            assert_eq!((*p).cachedMessageCount, 0);
            nzbget_rs_nzb_scalars_free(p);
            nzbget_rs_nzb_scalars_free(q);
        }
    }

    #[test]
    fn post_state_and_lists() {
        let ns: *const c_char = std::ptr::null();
        let mut out = ns;
        unsafe {
            let p = nzbget_rs_post_new();
            nzbget_rs_post_op(p, 1, 14, 750, ns, &mut out);
            assert_eq!(nzbget_rs_post_op(p, 0, 14, 0, ns, &mut out), 750);
            assert_eq!(nzbget_rs_post_op(p, 0, 99, 0, ns, &mut out), 0);
            nzbget_rs_post_op(p, 11, 0, 0, c"Unpacking".as_ptr(), &mut out);
            let b = nzbget_rs_post_label(p);
            assert_eq!(
                std::slice::from_raw_parts(b.data.cast::<u8>(), b.len),
                b"Unpacking"
            );
            crate::ffi::nzbget_rs_free(b);
            nzbget_rs_post_op(p, 22, 0, 0, c"a.par2".as_ptr(), &mut out);
            assert_eq!(nzbget_rs_post_op(p, 20, 0, 0, ns, &mut out), 1);
            assert_eq!(nzbget_rs_post_op(p, 21, 0, 0, ns, &mut out), 0);
            assert_eq!(CStr::from_ptr(out).to_bytes(), b"a.par2");
            assert_eq!(nzbget_rs_post_op(p, 31, 0, 0, ns, &mut out), -1);
            nzbget_rs_post_op(p, 32, 0, 0, c"x.rar".as_ptr(), &mut out);
            nzbget_rs_post_op(p, 33, 0, 0, ns, &mut out);
            assert_eq!(nzbget_rs_post_op(p, 30, 0, 0, ns, &mut out), 0);
            nzbget_rs_post_free(p);
        }
    }

    #[test]
    fn post_fields_follow_the_collection() {
        let ns: *const c_char = std::ptr::null();
        let mut out = ns;
        unsafe {
            let n = nzbget_rs_nzb_scalars_new();
            let p = nzbget_rs_post_new();
            // without a collection the post job keeps its own values
            nzbget_rs_post_op(p, 1, 9, 1, ns, &mut out);
            assert_eq!(nzbget_rs_post_op(p, 0, 9, 0, ns, &mut out), 1);
            assert!(!(*n).parRepaired);
            nzbget_rs_post_op(p, 3, 0, n as usize as i64, ns, &mut out);
            assert_eq!(
                nzbget_rs_post_op(p, 2, 0, 0, ns, &mut out),
                n as usize as i64
            );
            // reads come from the collection, writes go to both
            assert_eq!(nzbget_rs_post_op(p, 0, 9, 0, ns, &mut out), 0);
            nzbget_rs_post_op(p, 1, 10, 1, ns, &mut out);
            nzbget_rs_post_op(p, 1, 12, 2, ns, &mut out);
            assert!((*n).unpackTried);
            assert_eq!((*n).lastUnpackStatus, 2);
            (*n).lastUnpackStatus = 3;
            assert_eq!(nzbget_rs_post_op(p, 0, 12, 0, ns, &mut out), 3);
            // stage progress stays in 0..=1000
            nzbget_rs_post_op(p, 1, 14, 5000, ns, &mut out);
            assert_eq!(nzbget_rs_post_op(p, 0, 14, 0, ns, &mut out), 1000);
            nzbget_rs_post_op(p, 1, 14, -4, ns, &mut out);
            assert_eq!(nzbget_rs_post_op(p, 0, 14, 0, ns, &mut out), 0);
            // extracted archives are the collection's list
            nzbget_rs_post_op(p, 32, 0, 0, c"a.rar".as_ptr(), &mut out);
            let np: *const NzbScalars = std::ptr::null();
            let nf = |op, a2, strs: *mut *const c_char| {
                nzbget_rs_nzb_files(
                    n,
                    op,
                    a2,
                    np,
                    ns,
                    ns,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    strs,
                    std::ptr::null(),
                )
            };
            assert_eq!(nf(30, 0, std::ptr::null_mut()), 1);
            assert_eq!(nzbget_rs_post_op(p, 31, 0, 0, ns, &mut out), 0);
            assert_eq!(CStr::from_ptr(out).to_bytes(), b"a.rar");
            nzbget_rs_post_op(p, 33, 0, 0, ns, &mut out);
            assert_eq!(nf(30, 0, std::ptr::null_mut()), 0);
            nzbget_rs_post_free(p);
            nzbget_rs_nzb_scalars_free(n);
        }
    }

    #[test]
    fn coll_set_enters_and_leaves_post_process() {
        let ns: *const c_char = std::ptr::null();
        unsafe {
            let n = nzbget_rs_nzb_scalars_new();
            // stage and force flags need a post job
            assert!(host_coll_set(n, 109, 4, 0, ns, std::ptr::null()));
            assert_eq!(nzbget_rs_nzb_slot(n, 1, 0, 0), 0);
            assert!(host_coll_set(n, 54, 0, 0, ns, std::ptr::null()));
            let p = nzbget_rs_nzb_slot(n, 1, 0, 0) as *const Post;
            assert!(!p.is_null());
            assert_eq!(post_get(p, 2) as usize, 0);
            assert_eq!(
                nzbget_rs_post_op(p, 2, 0, 0, ns, std::ptr::null_mut()) as usize,
                n as usize
            );
            assert!(host_coll_set(n, 109, 7, 0, ns, std::ptr::null()));
            assert!(host_coll_set(n, 110, 2, 0, ns, std::ptr::null()));
            assert_eq!(post_get(p, 0), 7);
            assert_eq!(post_get(p, 4), 0);
            assert_eq!(post_get(p, 8), 1);
            // a second enter replaces the post job
            assert!(host_coll_set(n, 54, 0, 0, ns, std::ptr::null()));
            let p2 = nzbget_rs_nzb_slot(n, 1, 0, 0) as *const Post;
            assert_eq!(post_get(p2, 0), 0);
            post_set(p2, 10, 1);
            post_set(p2, 12, 2);
            nzbget_rs_post_op(p2, 32, 0, 0, c"x.rar".as_ptr(), std::ptr::null_mut());
            assert!(host_coll_set(n, 55, 0, 0, ns, std::ptr::null()));
            assert_eq!(nzbget_rs_nzb_slot(n, 1, 0, 0), 0);
            assert!(!(*n).unpackTried);
            assert_eq!((*n).lastUnpackStatus, 0);
            let np: *const NzbScalars = std::ptr::null();
            let count = nzbget_rs_nzb_files(
                n,
                30,
                0,
                np,
                ns,
                ns,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert_eq!(count, 0);
            nzbget_rs_nzb_scalars_free(n);
        }
    }

    #[test]
    fn host_post_op_snapshot_and_setters() {
        let ns: *const c_char = std::ptr::null();
        let mut ints = [0 as c_int; 21];
        let mut i64s = [0i64; 3];
        let mut strs = [ns; 1];
        unsafe {
            // a null post job is ignored
            assert_eq!(
                host_post_op(
                    std::ptr::null(),
                    0,
                    0,
                    0,
                    ns,
                    ints.as_mut_ptr(),
                    i64s.as_mut_ptr(),
                    strs.as_mut_ptr()
                ),
                0
            );
            let p = nzbget_rs_post_new();
            let op =
                |a1, a2, p2: usize, s1: *const c_char, i64s: *mut i64, strs: *mut *const c_char| {
                    host_post_op(p, a1, a2, p2, s1, std::ptr::null_mut(), i64s, strs)
                };
            let ni: *mut i64 = std::ptr::null_mut();
            let nsn: *mut *const c_char = std::ptr::null_mut();
            // flags are stored as 0/1, unknown fields ignored
            op(1, 1, 5, ns, ni, nsn);
            op(1, 8, 1, ns, ni, nsn);
            op(1, 0, 7, ns, ni, nsn);
            op(1, 12, 3, ns, ni, nsn);
            op(1, 19, 99, ns, ni, nsn);
            assert_eq!(post_get(p, 1), 1);
            assert_eq!(post_get(p, 19), 0);
            let mut t = [1234i64];
            op(2, 0, 0, ns, t.as_mut_ptr(), nsn);
            op(1, 14, 500, ns, ni, nsn);
            op(5, 0, 0, c"Repairing".as_ptr(), ni, nsn);
            op(6, -1, 0, c"a.par2".as_ptr(), ni, nsn);
            op(6, -1, 0, ns, ni, nsn);
            op(7, 0, 0, ns, ni, strs.as_mut_ptr());
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"");
            op(6, 0, 0, ns, ni, strs.as_mut_ptr());
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"a.par2");
            op(6, 1, 0, ns, ni, strs.as_mut_ptr());
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"");
            op(6, 5, 0, ns, ni, strs.as_mut_ptr());
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"");
            nzbget_rs_post_op(p, 32, 0, 0, c"b.rar".as_ptr(), nsn);
            host_post_op(
                p,
                0,
                0,
                0,
                ns,
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                strs.as_mut_ptr(),
            );
            assert_eq!(ints[0], 7);
            assert_eq!(ints[1], 1);
            assert_eq!(ints[5], 1);
            assert_eq!(ints[11], 0);
            assert_eq!(ints[12], 0);
            assert_eq!(ints[16], 3);
            assert_eq!(ints[17], 1);
            assert_eq!(ints[18], 2);
            assert_eq!(ints[19], 500);
            assert_eq!(i64s, [1234, 0, 0]);
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"Repairing");
            // the label copy outlives a later change
            let label = strs[0];
            op(5, 0, 0, c"Unpacking".as_ptr(), ni, nsn);
            assert_eq!(CStr::from_ptr(label).to_bytes(), b"Repairing");
            // a new stage clears progress and label
            t[0] = 99;
            op(3, 0, 0, ns, t.as_mut_ptr(), nsn);
            host_post_op(
                p,
                0,
                0,
                0,
                ns,
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            assert_eq!((ints[19], ints[20], i64s[2]), (0, 0, 99));
            assert_eq!(nzbget_rs_post_label(p).len, 0);
            op(4, 0, 0, ns, [77i64].as_mut_ptr(), nsn);
            assert_eq!(post_get(p, 18), 77);
            op(7, -1, 0, ns, ni, nsn);
            assert_eq!(nzbget_rs_post_op(p, 30, 0, 0, ns, nsn), 0);
            nzbget_rs_post_free(p);
        }
    }

    #[test]
    fn host_post_job_op_without_jobs() {
        unsafe {
            let n = nzbget_rs_nzb_scalars_new();
            let p = nzbget_rs_post_new();
            let i64s = [0i64];
            for a1 in [0, 1, 2, 11] {
                assert_eq!(host_post_job_op(a1, 1, p as usize, 0, i64s.as_ptr()), 0);
            }
            for a1 in [3, 9, 10] {
                assert_eq!(host_post_job_op(a1, 0, n as usize, 0, i64s.as_ptr()), 0);
            }
            for a1 in 4..=7 {
                assert_eq!(host_post_job_op(a1, 0, 0, n as usize, i64s.as_ptr()), 0);
            }
            for a1 in [0, 1, 2, 3, 8, 9, 10, 11, 12] {
                assert_eq!(host_post_job_op(a1, 0, 0, 0, i64s.as_ptr()), 0);
            }
            // direct unpack queries need a job
            assert_eq!(host_post_job_op(3, 1, n as usize, 0, i64s.as_ptr()), 0);
            // the script controller is skipped when the collection skips scripts
            nzbget_rs_post_op(
                p,
                3,
                0,
                n as usize as i64,
                std::ptr::null(),
                std::ptr::null_mut(),
            );
            (*n).skipScriptProcessing = true;
            assert_eq!(host_post_job_op(12, 9, p as usize, 0, i64s.as_ptr()), 0);
            assert_eq!(post_get(p, 19), 0);
            nzbget_rs_post_free(p);
            nzbget_rs_nzb_scalars_free(n);
        }
    }

    unsafe extern "C" fn drop_test_file(p: usize) {
        unsafe { crate::qfile::nzbget_rs_file_free(p as *mut crate::qfile::File) };
    }

    /// A collection with a file list, as the host's constructor makes it.
    fn coll_with_list() -> *mut NzbScalars {
        let n = nzbget_rs_nzb_scalars_new();
        let l = crate::qlist::nzbget_rs_ptrlist_new(drop_test_file);
        unsafe { nzbget_rs_nzb_slot(n, 0, 1, l as usize) };
        n
    }

    fn free_coll_with_list(n: *mut NzbScalars) {
        unsafe {
            let l = nzbget_rs_nzb_slot(n, 0, 1, 0);
            crate::qlist::nzbget_rs_ptrlist_free(l as *mut crate::qlist::PtrList);
            nzbget_rs_nzb_scalars_free(n);
        }
    }

    fn add_test_file(n: *mut NzbScalars, fields: &[(c_int, i64)]) -> *const crate::qfile::File {
        let f = crate::qfile::nzbget_rs_file_new(-1);
        unsafe {
            for &(id, v) in fields {
                crate::qfile::nzbget_rs_file_set(f, id, v);
            }
            crate::qfile::nzbget_rs_file_set(f, crate::qfile::field::NZB, n as i64);
        }
        ptr_list_op(file_list(n), 3, 0, f as usize, std::ptr::null(), 0);
        f
    }

    fn text_of(n: *const NzbScalars, id: c_int) -> Option<Vec<u8>> {
        let t = unsafe { nzbget_rs_nzb_text(n, id) };
        (!t.is_null()).then(|| unsafe { CStr::from_ptr(t) }.to_bytes().to_vec())
    }

    #[test]
    fn coll_set_filename_and_url_name_the_collection() {
        let ns: *const c_char = std::ptr::null();
        unsafe {
            let n = nzbget_rs_nzb_scalars_new();
            (*n).id = 5;
            assert!(host_coll_set(
                n,
                41,
                0,
                0,
                c"/tmp/My.Show.nzb".as_ptr(),
                std::ptr::null()
            ));
            assert_eq!(text_of(n, 1).unwrap(), b"/tmp/My.Show.nzb");
            assert_eq!(text_of(n, 2).unwrap(), b"My.Show");
            // a second file name keeps the name taken from the first
            assert!(host_coll_set(
                n,
                41,
                0,
                0,
                c"Other.nzb".as_ptr(),
                std::ptr::null()
            ));
            assert_eq!(text_of(n, 2).unwrap(), b"My.Show");
            // a URL leaves an existing name alone
            assert!(host_coll_set(
                n,
                96,
                0,
                0,
                c"http://host/x.nzb".as_ptr(),
                std::ptr::null()
            ));
            assert_eq!(text_of(n, 0).unwrap(), b"http://host/x.nzb");
            assert_eq!(text_of(n, 2).unwrap(), b"My.Show");
            nzbget_rs_nzb_scalars_free(n);

            let u = nzbget_rs_nzb_scalars_new();
            assert!(host_coll_set(
                u,
                96,
                0,
                0,
                c"http://host.example/get/123".as_ptr(),
                std::ptr::null()
            ));
            assert_eq!(text_of(u, 2).unwrap(), b"host.example/get/123");
            // a collection named after no file yet takes the file's name
            assert!(host_coll_set(
                u,
                41,
                0,
                0,
                c"Real.Name.nzb".as_ptr(),
                std::ptr::null()
            ));
            assert_eq!(text_of(u, 2).unwrap(), b"Real.Name");
            // an empty file name never renames
            assert!(host_coll_set(u, 41, 0, 0, c"".as_ptr(), std::ptr::null()));
            assert_eq!(text_of(u, 2).unwrap(), b"Real.Name");
            nzbget_rs_nzb_scalars_free(u);

            let v = nzbget_rs_nzb_scalars_new();
            let mut strs = [ns; 1];
            host_coll_op(
                v,
                16,
                0,
                0,
                c"https://idx.example/api".as_ptr(),
                c"Some.Name.nzb".as_ptr(),
                std::ptr::null_mut(),
                strs.as_mut_ptr(),
            );
            assert_eq!(
                CStr::from_ptr(strs[0]).to_bytes(),
                b"Some.Name @ idx.example"
            );
            nzbget_rs_nzb_scalars_free(v);
        }
    }

    #[test]
    fn coll_dir_names_end_with_the_collection_name() {
        unsafe {
            let n = nzbget_rs_nzb_scalars_new();
            (*n).id = 9;
            nzbget_rs_nzb_set_text(n, 2, c"Some.Name".as_ptr(), -1);
            let mut strs = [std::ptr::null(); 1];
            host_coll_op(
                n,
                3,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                strs.as_mut_ptr(),
            );
            let final_dir = CStr::from_ptr(strs[0]).to_bytes().to_vec();
            assert!(final_dir.ends_with(b"Some.Name"), "{final_dir:?}");
            assert!(host_coll_set(
                n,
                50,
                0,
                0,
                std::ptr::null(),
                std::ptr::null()
            ));
            let dest = text_of(n, 3).unwrap();
            assert!(dest.windows(9).any(|w| w == b"Some.Name"), "{dest:?}");
            nzbget_rs_nzb_scalars_free(n);
        }
    }

    #[test]
    fn coll_set_active_downloads_accounts_download_time() {
        let mut s = NzbScalars::default();
        set_active_downloads(&mut s, 2, 1000);
        assert_eq!(s.activeDownloads, 2);
        assert_eq!(s.downloadStartTime, 1000);
        set_active_downloads(&mut s, 0, 1030);
        assert_eq!(s.activeDownloads, 0);
        assert_eq!(s.downloadSec, 30);
        assert!(s.changed);
    }

    #[test]
    fn coll_file_stats_and_times_follow_the_file_list() {
        use crate::qfile::field;
        unsafe {
            let n = coll_with_list();
            let f1 = add_test_file(
                n,
                &[
                    (field::TIME, 300),
                    (field::SIZE, 100),
                    (field::REMAINING_SIZE, 60),
                    (field::SUCCESS_SIZE, 30),
                    (field::FAILED_SIZE, 10),
                    (field::SUCCESS_ARTICLES, 3),
                    (field::FAILED_ARTICLES, 1),
                    (field::TOTAL_ARTICLES, 10),
                ],
            );
            add_test_file(
                n,
                &[
                    (field::TIME, 100),
                    (field::SIZE, 50),
                    (field::REMAINING_SIZE, 50),
                    (field::PAR_FILE, 1),
                    (field::PAUSED, 1),
                ],
            );
            assert!(host_coll_set(
                n,
                51,
                0,
                0,
                std::ptr::null(),
                std::ptr::null()
            ));
            assert_eq!(((*n).minTime, (*n).maxTime), (100, 300));

            (*n).successArticles = 5;
            (*n).successSize = 500;
            assert!(host_coll_set(
                n,
                98,
                0,
                0,
                std::ptr::null(),
                std::ptr::null()
            ));
            assert_eq!((*n).remainingSize, 110);
            assert_eq!((*n).pausedFileCount, 1);
            assert_eq!((*n).pausedSize, 50);
            assert_eq!((*n).remainingParCount, 1);
            assert_eq!((*n).currentSuccessArticles, 8);
            assert_eq!((*n).currentFailedArticles, 1);
            assert_eq!((*n).currentSuccessSize, 530);
            assert_eq!((*n).currentFailedSize, 10);

            let before = (*n).successSize;
            assert!(host_coll_set(
                n,
                52,
                0,
                f1 as usize,
                std::ptr::null(),
                std::ptr::null()
            ));
            assert_eq!((*n).successSize, before + 30);
            assert_eq!((*n).failedArticles, 1);

            (*n).fileCount = 2;
            (*n).size = 150;
            assert!(host_coll_set(
                n,
                53,
                0,
                f1 as usize,
                std::ptr::null(),
                std::ptr::null()
            ));
            assert_eq!((*n).fileCount, 1);
            assert_eq!((*n).size, 50);
            assert_eq!((*n).totalArticles, -10);

            assert!(host_coll_set(
                n,
                97,
                0,
                0,
                std::ptr::null(),
                std::ptr::null()
            ));
            assert_eq!(ptr_list_op(file_list(n), 0, 0, 0, std::ptr::null(), 0), 0);
            free_coll_with_list(n);
        }
    }

    #[test]
    fn coll_active_output_needs_an_open_downloading_file() {
        use crate::qfile::field;
        unsafe {
            let n = coll_with_list();
            let ns = std::ptr::null();
            let op = |n| {
                host_coll_op(
                    n,
                    22,
                    0,
                    0,
                    ns,
                    ns,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            assert_eq!(op(n), Some(0));
            let f = add_test_file(
                n,
                &[(field::OUTPUT_INITIALIZED, 1), (field::ACTIVE_DOWNLOADS, 1)],
            );
            // no output file name yet
            assert_eq!(op(n), Some(0));
            crate::qfile::nzbget_rs_file_set_text(
                f,
                crate::qfile::text::OUTPUT_FILENAME,
                c"out.tmp".as_ptr(),
            );
            assert_eq!(op(n), Some(1));
            crate::qfile::nzbget_rs_file_set(f, field::ACTIVE_DOWNLOADS, 0);
            assert_eq!(op(n), Some(0));
            free_coll_with_list(n);
        }
    }

    #[test]
    fn coll_snapshot_reports_fields_and_computed_values() {
        use crate::qfile::field;
        unsafe {
            let n = coll_with_list();
            (*n).id = 77;
            (*n).kind = 0;
            (*n).priority = 900;
            (*n).extraPriority = 1;
            (*n).desiredServerId = 3;
            (*n).size = 1000;
            (*n).currentFailedSize = 100;
            (*n).queueScriptTime = 1234;
            nzbget_rs_nzb_set_text(n, 2, c"Name".as_ptr(), -1);
            nzbget_rs_nzb_set_text(n, 0, c"http://u".as_ptr(), -1);
            add_test_file(n, &[(field::PAR_FILE, 1), (field::PAUSED, 1)]);
            let mut ints = [0; 90];
            let mut i64s = [0i64; 21];
            let mut strs = [std::ptr::null(); 9];
            host_coll_snapshot(
                n,
                1,
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                strs.as_mut_ptr(),
            );
            assert_eq!(ints[0], 77);
            assert_eq!((ints[14], ints[15], ints[17], ints[18]), (1, 1, 1, 3));
            assert_eq!(ints[25], 0);
            assert_eq!(ints[30], 900);
            // a paused par file blocks completion only when paused pars count
            assert_eq!((ints[33], ints[54]), (1, 0));
            assert_eq!(ints[47], -1);
            assert_eq!(i64s[0], 1000);
            assert_eq!(i64s[18], 0);
            assert_eq!(i64s[20], 1234);
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"Name");
            assert_eq!(CStr::from_ptr(strs[8]).to_bytes(), b"http://u");
            assert!(!strs[7].is_null());
            // with a post job
            assert!(host_coll_set(
                n,
                54,
                0,
                0,
                std::ptr::null(),
                std::ptr::null()
            ));
            assert!(host_coll_set(
                n,
                109,
                7,
                0,
                std::ptr::null(),
                std::ptr::null()
            ));
            assert!(host_coll_set(
                n,
                110,
                2,
                0,
                std::ptr::null(),
                std::ptr::null()
            ));
            host_coll_snapshot(
                n,
                0,
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            assert_eq!((ints[25], ints[27], ints[83], ints[84]), (1, 7, 0, 1));
            assert_ne!(i64s[18], 0);
            // active downloads: never completed
            (*n).activeDownloads = 1;
            host_coll_snapshot(
                n,
                0,
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            assert_eq!((ints[33], ints[54]), (0, 0));
            assert!(host_coll_set(
                n,
                55,
                0,
                0,
                std::ptr::null(),
                std::ptr::null()
            ));
            free_coll_with_list(n);
        }
    }
}
