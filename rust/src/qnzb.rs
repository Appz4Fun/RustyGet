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
}
