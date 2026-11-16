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
}

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
    });
    Box::into_raw(data).cast()
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

/// Size of the struct, so C++ can check it matches its declaration.
#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_nzb_scalars_size() -> usize {
    std::mem::size_of::<NzbScalars>()
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

/// Post-processing state of a collection (the C++ PostInfo is a thin owner).
pub struct Post {
    // scalar fields by id (see `nzbget_rs_post_op`)
    vals: [std::sync::atomic::AtomicI64; POST_VALS],
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
/// 12 last unpack status, 13 file progress, 14 stage progress,
/// 15 stream repair done, 16 stream repair recovered, 17 start time,
/// 18 stage time, 19 post thread pointer.
///  0 get field a2; 1 set field a2 to v;
///  11 set the progress label to s1 (read it with `nzbget_rs_post_label`);
///  20-23 parred files, 30-33 extracted archives (see `list_op`): strings
///  returned stay valid until that list changes.
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
    match op {
        0 => field().map_or(0, |f| f.load(SeqCst)),
        1 => {
            if let Some(f) = field() {
                f.store(v, SeqCst);
            }
            0
        }
        11 => {
            post.lists.lock().unwrap_or_else(|e| e.into_inner()).label = owned(s1);
            0
        }
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
}
