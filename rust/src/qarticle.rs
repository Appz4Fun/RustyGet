//! Articles of a queued file: the data that was the C++ `ArticleInfo` and its
//! `ArticleList`, now owned by Rust.
//!
//! The C++ `FileInfo` keeps one [`ArticleList`] handle. Host operation 43 (and
//! the article parts of 34, 40, 42, 44 and 45) forward to
//! [`nzbget_rs_article_host`], so the `article_ptr` values Rust code passes
//! around are addresses of [`Article`]s. As before, plain fields are guarded by
//! the download-queue lock and the cache geometry fields are atomics.

use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, Ordering};

pub const AI_UNDEFINED: i32 = 0;
pub const AI_RUNNING: i32 = 1;
pub const AI_FINISHED: i32 = 2;
pub const AI_FAILED: i32 = 3;

/// One article of a file.
pub struct Article {
    part_number: i32,
    size: i32,
    status: i32,
    message_id: CString,
    result_filename: Option<CString>,
    segment_data: usize,
    segment_alloc_size: i32,
    segment_offset: AtomicI64,
    segment_size: AtomicI32,
    crc: AtomicU32,
    live_par_claim_begin: i64,
    live_par_claim_end: i64,
    // requeues with a message-id borrowed from a duplicate (not persisted)
    dupe_fallback_round: i32,
    // the article's own message-id, kept before the first substitution
    dupe_original_message_id: CString,
    // decoded range a donor article is expected to fill; -1 unknown
    dupe_expected_offset: i64,
    dupe_expected_end: i64,
    // sources pinned at the first fallback and their donors (not persisted)
    dupe_sources: Vec<CString>,
    dupe_source_donors: Vec<i32>,
    dupe_donor_id: i32,
    dupe_lead_snapshot: i32,
    dupe_next_lead: i32,
    dupe_donor_count: i32,
    dupe_proactive: bool,
}

impl Default for Article {
    fn default() -> Self {
        Self {
            part_number: 0,
            size: 0,
            status: AI_UNDEFINED,
            message_id: CString::default(),
            result_filename: None,
            segment_data: 0,
            segment_alloc_size: 0,
            segment_offset: AtomicI64::new(0),
            segment_size: AtomicI32::new(0),
            crc: AtomicU32::new(0),
            live_par_claim_begin: -1,
            live_par_claim_end: -1,
            dupe_fallback_round: 0,
            dupe_original_message_id: CString::default(),
            dupe_expected_offset: -1,
            dupe_expected_end: -1,
            dupe_sources: Vec::new(),
            dupe_source_donors: Vec::new(),
            dupe_donor_id: 0,
            dupe_lead_snapshot: 0,
            dupe_next_lead: 0,
            dupe_donor_count: 0,
            dupe_proactive: false,
        }
    }
}

impl Drop for Article {
    fn drop(&mut self) {
        self.discard_segment();
    }
}

fn cstring(p: *const c_char) -> CString {
    if p.is_null() {
        CString::default()
    } else {
        unsafe { CStr::from_ptr(p) }.to_owned()
    }
}

impl Article {
    fn new(part_number: i32, size: i32, message_id: *const c_char) -> Self {
        let mut a = Self::default();
        a.part_number = part_number;
        a.size = size;
        a.message_id = cstring(message_id);
        a
    }

    fn segment_offset(&self) -> i64 {
        self.segment_offset.load(Ordering::SeqCst)
    }

    fn segment_size(&self) -> i32 {
        self.segment_size.load(Ordering::SeqCst)
    }

    fn crc(&self) -> u32 {
        self.crc.load(Ordering::SeqCst)
    }

    fn result_filename_ptr(&self) -> *const c_char {
        self.result_filename
            .as_ref()
            .map_or(ptr::null(), |s| s.as_ptr())
    }

    fn set_result_filename(&mut self, s: *const c_char) {
        self.result_filename = if s.is_null() { None } else { Some(cstring(s)) };
    }

    fn attach_segment(&mut self, data: usize, alloc_size: i32, offset: i64, size: i32) {
        self.discard_segment();
        self.segment_data = data;
        self.segment_alloc_size = alloc_size;
        self.segment_offset.store(offset, Ordering::SeqCst);
        self.segment_size.store(size, Ordering::SeqCst);
    }

    fn discard_segment(&mut self) {
        if self.segment_alloc_size != 0 {
            crate::articledl::cache_free_raw(self.segment_data as *mut u8, self.segment_alloc_size);
        }
        self.segment_data = 0;
        self.segment_alloc_size = 0;
    }

    /// Fills the summary the Rust side reads for an article (op 43, a1 0/1/7/8).
    unsafe fn fill(
        &self,
        done: bool,
        ints: *mut c_int,
        i64s: *mut i64,
        strs: *mut *const c_char,
    ) -> usize {
        let run = !done && self.status == AI_RUNNING;
        unsafe {
            *ints = self.part_number;
            *ints.add(1) = self.size;
            *ints.add(2) = self.status;
            *ints.add(3) = if run { 0 } else { self.segment_size() };
            *ints.add(4) = if run { 0 } else { self.crc() as i32 };
            *ints.add(5) = self.dupe_fallback_round;
            *ints.add(6) = self.dupe_lead_snapshot;
            *ints.add(7) = self.dupe_next_lead;
            *ints.add(8) = self.dupe_donor_count;
            *ints.add(9) = self.dupe_donor_id;
            *ints.add(10) = c_int::from(self.dupe_proactive);
            *i64s = if run { -1 } else { self.segment_offset() };
            *strs = self.message_id.as_ptr();
            *strs.add(1) = self.dupe_original_message_id.as_ptr();
        }
        self as *const Article as usize
    }
}

/// The articles of one file, in order.
#[derive(Default)]
pub struct ArticleList {
    items: Vec<Box<Article>>,
}

impl ArticleList {
    fn at(&mut self, index: usize) -> Option<&mut Article> {
        self.items.get_mut(index).map(|b| &mut **b)
    }

    fn index_of(&self, target: usize) -> Option<usize> {
        self.items
            .iter()
            .position(|a| (&**a as *const Article as usize) == target)
    }

    /// Admits a live-par claim on `target`'s header range when it keeps the
    /// file's known article ranges in order (was C++
    /// `live_par_claim_source_header`). Numeric only: no cache geometry or
    /// strings change. The queue lock serializes admission; a rejected claim
    /// changes nothing.
    fn claim_source_header(&mut self, target: usize, begin: i64, end: i64) -> bool {
        if begin < 0 || end <= begin {
            return false;
        }
        let Some(index) = self.index_of(target) else {
            return false;
        };
        {
            let t = &self.items[index];
            // An independently fetched replacement cannot prove same-payload identity.
            if t.live_par_claim_begin != -1
                || t.live_par_claim_end != -1
                || t.segment_size() != 0
                || t.status == AI_FINISHED
            {
                return false;
            }
        }
        let range = |a: &Article| -> (i32, i64, i64) {
            let (lo, hi) = (a.live_par_claim_begin, a.live_par_claim_end);
            if lo != -1 || hi != -1 {
                return (if lo >= 0 && hi > lo { 1 } else { -1 }, lo, hi);
            }
            // Finished geometry is stable under the queue lock; running
            // geometry is not a claim, cache attachment can still change it.
            if a.status != AI_FINISHED {
                return (0, 0, 0);
            }
            let lo = a.segment_offset();
            let size = a.segment_size();
            if lo < 0 || size <= 0 || lo > i64::MAX - i64::from(size) {
                return (-1, 0, 0);
            }
            (1, lo, lo + i64::from(size))
        };
        // Restored ranges were not admitted here: validate the whole chain,
        // including the proposed insertion, before trusting it.
        let mut previous_end = -1i64;
        for (i, a) in self.items.iter().enumerate() {
            let (known, lo, hi) = if i == index {
                (1, begin, end)
            } else {
                range(a)
            };
            if known < 0 {
                return false;
            }
            if known == 0 {
                continue;
            }
            if previous_end > lo {
                return false;
            }
            previous_end = hi;
        }
        let t = &mut self.items[index];
        t.live_par_claim_begin = begin;
        t.live_par_claim_end = end;
        true
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_artlist_new() -> *mut ArticleList {
    Box::into_raw(Box::default())
}

/// # Safety
/// `list` is null or from `nzbget_rs_artlist_new`, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_artlist_free(list: *mut ArticleList) {
    if !list.is_null() {
        drop(unsafe { Box::from_raw(list) });
    }
}

unsafe fn str_or_empty<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        unsafe { CStr::from_ptr(p) }.to_bytes()
    }
}

/// Article operations. `list` is the file's [`ArticleList`] (may be null for
/// the operations on one `article`). Codes 0-14 are host operation 43's
/// (`a1` there); 100 and up serve the C++ queue model:
///
/// - 0 summary of the article at index `a2`; 1 summary of `article` (done);
///   7 first undefined article; 8 summary of `article` with its index in
///   `ints[11]`. Summaries return the article address.
/// - 2 set `a2`-th field of `article` to `p2` (0 status, 1 fallback round,
///   2 donor id, 3 proactive); 3 pinned source `a2` (`a2` < 0: count);
///   4 pin sources (`a2` sources in `strs`, `ints` = lead, next lead, donor
///   count, donors count, donors...); 5 expected range from `i64s`;
///   6 message-id (`a2` 0) or original message-id (`a2` 1) from `s1`;
///   9 reset for a new attempt.
/// - 10 persisted fields of article `a2`; 11 append article (part `a2`, size
///   `p2`, message-id `s1`); 12 restore article `a2` (with `p2` 0 returns
///   `part << 1 | has result file`); 13 admit a live-par header claim on
///   `article` for `i64s[0]..i64s[1]`; 14 whether `article` has a claim or a
///   cached segment.
/// - 100 count; 101 clear; 102 reset every article; 103 mark finished ones
///   failed; 104 delete result files of non-running articles (all with `p2`,
///   else only borrowed ones); 105 append `a2` articles (parts `i64s[0]`,
///   sizes `i64s[1]`, message-ids `strs`); 106 article at index `a2`.
/// - 110 cached segment data; 111 discard the segment; 112 attach a segment
///   (`i64s[1]` data, `ints[0]` allocation, `i64s[0]` offset, `ints[1]`
///   size); 113 set offset `i64s[0]` and size `ints[0]`; 114 result filename;
///   115 set result filename `s1` (null keeps it); 116 set crc `a2`;
///   117 fallback round; 118 part number; 119 expected range into `i64s`;
///   120 status, segment size, crc, part, fallback round into `ints`, offset
///   into `i64s[0]`, data and result filename into `strs`; returns 1 when
///   finished; 121 live-par claim range into `i64s`.
///
/// # Safety
/// `list` and `article` are null or live; output arrays are large enough for
/// the operation. The caller holds the queue lock where the C++ code did.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_article_host(
    list: *mut ArticleList,
    article: *mut Article,
    op: c_int,
    a2: c_int,
    p2: usize,
    s1: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    let list = unsafe { list.as_mut() };
    let art = unsafe { article.as_mut() };
    match (op, list, art) {
        (0, Some(l), _) => match l.at(a2 as usize) {
            Some(a) => unsafe { a.fill(false, ints, i64s, strs) },
            None => 0,
        },
        (1, _, Some(a)) => unsafe { a.fill(true, ints, i64s, strs) },
        (7, Some(l), _) | (8, Some(l), _) => {
            let found = if op == 7 {
                l.items.iter().position(|a| a.status == AI_UNDEFINED)
            } else {
                l.index_of(article as usize)
            };
            match found {
                Some(i) => {
                    unsafe { *ints.add(11) = i as c_int };
                    unsafe { l.items[i].fill(op == 8, ints, i64s, strs) }
                }
                None => 0,
            }
        }
        (2, _, Some(a)) => {
            let v = p2 as i32;
            match a2 {
                0 => a.status = v,
                1 => a.dupe_fallback_round = v,
                2 => a.dupe_donor_id = v,
                3 => a.dupe_proactive = v != 0,
                _ => {}
            }
            0
        }
        (3, _, Some(a)) => {
            if a2 < 0 {
                return a.dupe_sources.len();
            }
            let i = a2 as usize;
            let has = i < a.dupe_source_donors.len();
            unsafe {
                *ints.add(1) = c_int::from(has);
                *ints = if has { a.dupe_source_donors[i] } else { 0 };
                *strs = a.dupe_sources.get(i).map_or(ptr::null(), |s| s.as_ptr());
            }
            0
        }
        (4, _, Some(a)) => {
            unsafe {
                a.dupe_lead_snapshot = *ints;
                a.dupe_next_lead = *ints.add(1);
                a.dupe_donor_count = *ints.add(2);
                a.dupe_sources = (0..a2.max(0) as usize)
                    .map(|i| cstring(*strs.add(i)))
                    .collect();
                let n = (*ints.add(3)).max(0) as usize;
                a.dupe_source_donors = (0..n).map(|i| *ints.add(4 + i)).collect();
            }
            0
        }
        (5, _, Some(a)) => {
            unsafe {
                a.dupe_expected_offset = *i64s;
                a.dupe_expected_end = *i64s.add(1);
            }
            0
        }
        (6, _, Some(a)) => {
            if a2 == 0 {
                a.message_id = cstring(s1);
            } else if a2 == 1 {
                a.dupe_original_message_id = cstring(s1);
            }
            0
        }
        (9, _, Some(a)) => {
            a.status = AI_UNDEFINED;
            a.crc.store(0, Ordering::SeqCst);
            a.segment_offset.store(0, Ordering::SeqCst);
            a.segment_size.store(0, Ordering::SeqCst);
            a.dupe_expected_offset = -1;
            a.dupe_expected_end = -1;
            0
        }
        (10, Some(l), _) => {
            let Some(a) = l.at(a2 as usize) else {
                return 0;
            };
            unsafe {
                *ints = a.status;
                *ints.add(1) = a.segment_size();
                *ints.add(2) = a.crc() as i32;
                *ints.add(3) = a.dupe_fallback_round;
                *ints.add(4) = c_int::from(a.result_filename.is_some());
                *ints.add(5) = a.part_number;
                *ints.add(6) = a.size;
                *i64s = a.segment_offset();
                *strs = a.message_id.as_ptr();
                *strs.add(1) = a.dupe_original_message_id.as_ptr();
            }
            0
        }
        (11, Some(l), _) => {
            l.items.push(Box::new(Article::new(a2, p2 as i32, s1)));
            0
        }
        (12, Some(l), _) => {
            let i = a2.max(0) as usize;
            while l.items.len() <= i {
                l.items.push(Box::default());
            }
            let a = &mut l.items[i];
            if p2 == 0 {
                return ((a.part_number << 1) | i32::from(a.result_filename.is_some())) as usize;
            }
            unsafe {
                if *ints >= 2 {
                    a.segment_offset.store(*i64s, Ordering::SeqCst);
                    a.segment_size.store(*ints.add(1), Ordering::SeqCst);
                    a.crc.store(*ints.add(2) as u32, Ordering::SeqCst);
                }
                a.dupe_fallback_round = *ints.add(3);
                if !s1.is_null() {
                    a.set_result_filename(s1);
                }
                a.status = *ints.add(4);
            }
            0
        }
        (13, Some(l), _) => {
            let (begin, end) = unsafe { (*i64s, *i64s.add(1)) };
            usize::from(l.claim_source_header(article as usize, begin, end))
        }
        (14, _, Some(a)) => usize::from(a.live_par_claim_begin >= 0 || a.segment_size() > 0),
        (100, Some(l), _) => l.items.len(),
        (101, Some(l), _) => {
            l.items.clear();
            0
        }
        (102, Some(l), _) => {
            for a in &mut l.items {
                a.status = AI_UNDEFINED;
                a.result_filename = None;
                a.discard_segment();
            }
            0
        }
        (103, Some(l), _) => {
            for a in &mut l.items {
                if a.status == AI_FINISHED {
                    a.status = AI_FAILED;
                }
            }
            0
        }
        (104, Some(l), _) => {
            for a in &l.items {
                if a.status != AI_RUNNING && (p2 != 0 || a.dupe_fallback_round > 0) {
                    if let Some(f) = &a.result_filename {
                        use std::os::unix::ffi::OsStrExt;
                        let _ = std::fs::remove_file(std::ffi::OsStr::from_bytes(f.to_bytes()));
                    }
                }
            }
            0
        }
        (105, Some(l), _) => {
            let n = a2.max(0) as usize;
            let (parts, sizes) = unsafe { (*i64s as *const c_int, *i64s.add(1) as *const i64) };
            l.items.reserve(n);
            for k in 0..n {
                unsafe {
                    l.items.push(Box::new(Article::new(
                        *parts.add(k),
                        *sizes.add(k) as i32,
                        *strs.add(k),
                    )));
                }
            }
            0
        }
        (106, Some(l), _) => l
            .items
            .get(a2.max(0) as usize)
            .map_or(0, |a| &**a as *const Article as usize),
        (110, _, Some(a)) => a.segment_data,
        (111, _, Some(a)) => {
            a.discard_segment();
            0
        }
        (112, _, Some(a)) => {
            unsafe { a.attach_segment(*i64s.add(1) as usize, *ints, *i64s, *ints.add(1)) };
            0
        }
        (113, _, Some(a)) => {
            unsafe {
                a.segment_offset.store(*i64s, Ordering::SeqCst);
                a.segment_size.store(*ints, Ordering::SeqCst);
            }
            0
        }
        (114, _, Some(a)) => a.result_filename_ptr() as usize,
        (115, _, Some(a)) => {
            if !s1.is_null() {
                a.set_result_filename(s1);
            }
            a.result_filename_ptr() as usize
        }
        (116, _, Some(a)) => {
            a.crc.store(a2 as u32, Ordering::SeqCst);
            0
        }
        (117, _, Some(a)) => a.dupe_fallback_round as usize,
        (118, _, Some(a)) => a.part_number as usize,
        (119, _, Some(a)) => {
            unsafe {
                *i64s = a.dupe_expected_offset;
                *i64s.add(1) = a.dupe_expected_end;
            }
            0
        }
        (121, _, Some(a)) => {
            unsafe {
                *i64s = a.live_par_claim_begin;
                *i64s.add(1) = a.live_par_claim_end;
            }
            0
        }
        (120, _, Some(a)) => {
            unsafe {
                *ints = a.status;
                *ints.add(1) = a.segment_size();
                *ints.add(2) = a.crc() as i32;
                *ints.add(3) = a.part_number;
                *ints.add(4) = a.dupe_fallback_round;
                *i64s = a.segment_offset();
                *strs = a.segment_data as *const c_char;
                *strs.add(1) = a.result_filename_ptr();
            }
            usize::from(a.status == AI_FINISHED)
        }
        _ => {
            let _ = unsafe { str_or_empty(s1) };
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(
        list: *mut ArticleList,
        art: usize,
        op: c_int,
        a2: c_int,
        p2: usize,
        s1: *const c_char,
    ) -> (usize, [c_int; 16], [i64; 4], [*const c_char; 4]) {
        let mut ints = [0; 16];
        let mut i64s = [0; 4];
        let mut strs = [ptr::null(); 4];
        let r = unsafe {
            nzbget_rs_article_host(
                list,
                art as *mut Article,
                op,
                a2,
                p2,
                s1,
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                strs.as_mut_ptr(),
            )
        };
        (r, ints, i64s, strs)
    }

    #[test]
    fn append_summarize_and_reset() {
        let list = nzbget_rs_artlist_new();
        let mid = CString::new("a@b").unwrap();
        host(list, 0, 11, 1, 700, mid.as_ptr());
        host(list, 0, 11, 2, 300, mid.as_ptr());
        assert_eq!(host(list, 0, 100, 0, 0, ptr::null()).0, 2);
        let (addr, ints, i64s, strs) = host(list, 0, 0, 1, 0, ptr::null());
        assert_ne!(addr, 0);
        assert_eq!((ints[0], ints[1], ints[2]), (2, 300, AI_UNDEFINED));
        assert_eq!(i64s[0], 0);
        assert_eq!(unsafe { CStr::from_ptr(strs[0]) }.to_bytes(), b"a@b");
        host(list, addr, 2, 0, AI_FINISHED as usize, ptr::null());
        assert_eq!(host(list, 0, 7, 0, 0, ptr::null()).1[11], 0);
        let (_, ints, _, _) = host(list, addr, 8, 0, 0, ptr::null());
        assert_eq!((ints[11], ints[2]), (1, AI_FINISHED));
        host(list, 0, 103, 0, 0, ptr::null());
        assert_eq!(host(list, addr, 1, 0, 0, ptr::null()).1[2], AI_FAILED);
        host(list, 0, 102, 0, 0, ptr::null());
        assert_eq!(host(list, addr, 1, 0, 0, ptr::null()).1[2], AI_UNDEFINED);
        unsafe { nzbget_rs_artlist_free(list) };
    }

    #[test]
    fn restore_and_pinned_sources() {
        let list = nzbget_rs_artlist_new();
        let name = CString::new("/tmp/x.part").unwrap();
        let mut ints = [0 as c_int; 16];
        let mut i64s = [0i64; 4];
        ints[0] = 2;
        ints[1] = 4096;
        ints[2] = 7;
        ints[3] = 1;
        ints[4] = AI_FINISHED;
        i64s[0] = 8192;
        unsafe {
            nzbget_rs_article_host(
                list,
                ptr::null_mut(),
                12,
                3,
                1,
                name.as_ptr(),
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                ptr::null_mut(),
            );
        }
        assert_eq!(host(list, 0, 100, 0, 0, ptr::null()).0, 4);
        let (_, ints, i64s, _) = host(list, 0, 10, 3, 0, ptr::null());
        assert_eq!(
            (ints[0], ints[1], ints[2], ints[3], ints[4]),
            (AI_FINISHED, 4096, 7, 1, 1)
        );
        assert_eq!(i64s[0], 8192);
        let addr = host(list, 0, 106, 3, 0, ptr::null()).0;
        let s1 = CString::new("d1@x").unwrap();
        let s2 = CString::new("d2@x").unwrap();
        let mut strs = [s1.as_ptr(), s2.as_ptr()];
        let mut ints = [5, 6, 2, 1, 42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        unsafe {
            nzbget_rs_article_host(
                ptr::null_mut(),
                addr as *mut Article,
                4,
                2,
                0,
                ptr::null(),
                ints.as_mut_ptr(),
                ptr::null_mut(),
                strs.as_mut_ptr(),
            );
        }
        assert_eq!(host(ptr::null_mut(), addr, 3, -1, 0, ptr::null()).0, 2);
        let (_, ints, _, strs) = host(ptr::null_mut(), addr, 3, 0, 0, ptr::null());
        assert_eq!((ints[0], ints[1]), (42, 1));
        assert_eq!(unsafe { CStr::from_ptr(strs[0]) }.to_bytes(), b"d1@x");
        assert_eq!(host(ptr::null_mut(), addr, 3, 1, 0, ptr::null()).1[1], 0);
        unsafe { nzbget_rs_artlist_free(list) };
    }

    #[test]
    fn live_par_claims_keep_ranges_ordered() {
        let list = nzbget_rs_artlist_new();
        for part in 1..=3 {
            host(list, 0, 11, part, 100, ptr::null());
        }
        let first = host(list, 0, 106, 0, 0, ptr::null()).0;
        let second = host(list, 0, 106, 1, 0, ptr::null()).0;
        let claim = |art: usize, b: i64, e: i64| {
            let mut i64s = [b, e, 0, 0];
            unsafe {
                nzbget_rs_article_host(
                    list,
                    art as *mut Article,
                    13,
                    0,
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                    i64s.as_mut_ptr(),
                    ptr::null_mut(),
                )
            }
        };
        assert_eq!(claim(second, 100, 200), 1);
        assert_eq!(host(ptr::null_mut(), second, 14, 0, 0, ptr::null()).0, 1);
        // a second claim on the same article is refused
        assert_eq!(claim(second, 100, 200), 0);
        // overlapping the later claim is out of order
        assert_eq!(claim(first, 150, 250), 0);
        assert_eq!(claim(first, 0, 100), 1);
        assert_eq!(claim(first, -1, 5), 0);
        unsafe { nzbget_rs_artlist_free(list) };
    }

    /// Ported from tests/queue/LiveParSourceClaimTest.h.
    struct Claims {
        list: *mut ArticleList,
    }
    impl Claims {
        fn new() -> Self {
            Self {
                list: nzbget_rs_artlist_new(),
            }
        }
        fn running(&self) -> usize {
            let n = host(self.list, 0, 100, 0, 0, ptr::null()).0;
            host(self.list, 0, 11, n as c_int + 1, 0, ptr::null());
            let a = host(self.list, 0, 106, n as c_int, 0, ptr::null()).0;
            host(ptr::null_mut(), a, 2, 0, AI_RUNNING as usize, ptr::null());
            a
        }
        fn claim(&self, art: usize, b: i64, e: i64) -> bool {
            let mut i64s = [b, e, 0, 0];
            unsafe {
                nzbget_rs_article_host(
                    self.list,
                    art as *mut Article,
                    13,
                    0,
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                    i64s.as_mut_ptr(),
                    ptr::null_mut(),
                ) == 1
            }
        }
        fn finished(&self, art: usize, offset: i64, size: c_int) {
            host(
                ptr::null_mut(),
                art,
                2,
                0,
                AI_FINISHED as usize,
                ptr::null(),
            );
            let mut ints = [size, 0];
            let mut i64s = [offset, 0];
            unsafe {
                nzbget_rs_article_host(
                    ptr::null_mut(),
                    art as *mut Article,
                    113,
                    0,
                    0,
                    ptr::null(),
                    ints.as_mut_ptr(),
                    i64s.as_mut_ptr(),
                    ptr::null_mut(),
                );
            }
        }
    }
    impl Drop for Claims {
        fn drop(&mut self) {
            unsafe { nzbget_rs_artlist_free(self.list) };
        }
    }

    #[test]
    fn running_headers_keep_cache_geometry() {
        let c = Claims::new();
        let (first, second) = (c.running(), c.running());
        assert!(c.claim(first, 0, 1024));
        // a non-owned stand-in (allocation 0) keeps the free accounting out of it
        let content = [0u8; 8];
        let mut ints = [0, 7];
        let mut i64s = [123, content.as_ptr() as i64];
        unsafe {
            nzbget_rs_article_host(
                ptr::null_mut(),
                first as *mut Article,
                112,
                0,
                0,
                ptr::null(),
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                ptr::null_mut(),
            );
        }
        assert!(!c.claim(second, 512, 1536));
        assert!(!c.claim(first, 2048, 3072));
        let (_, ints, i64s, strs) = host(ptr::null_mut(), first, 120, 0, 0, ptr::null());
        assert_eq!((i64s[0], ints[1]), (123, 7));
        assert_eq!(strs[0] as usize, content.as_ptr() as usize);
    }

    #[test]
    fn out_of_order_headers_and_gaps() {
        let c = Claims::new();
        let (first, second, third) = (c.running(), c.running(), c.running());
        assert!(c.claim(third, 20, 30));
        assert!(c.claim(first, 0, 10));
        assert!(c.claim(second, 10, 20));
        assert!(!c.claim(first, 0, 10));
    }

    #[test]
    fn header_history_survives_segment_reset() {
        let c = Claims::new();
        let first = c.running();
        assert!(c.claim(first, 0, 10));
        host(
            ptr::null_mut(),
            first,
            2,
            0,
            AI_UNDEFINED as usize,
            ptr::null(),
        );
        let mut ints = [0, 0];
        let mut i64s = [0, 0];
        unsafe {
            nzbget_rs_article_host(
                ptr::null_mut(),
                first as *mut Article,
                113,
                0,
                0,
                ptr::null(),
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                ptr::null_mut(),
            );
        }
        host(ptr::null_mut(), first, 111, 0, 0, ptr::null());
        assert!(!c.claim(first, 0, 10));
        let (_, _, range, _) = host(ptr::null_mut(), first, 121, 0, 0, ptr::null());
        assert_eq!((range[0], range[1]), (0, 10));
    }

    #[test]
    fn invalid_and_restored_header_ranges() {
        let c = Claims::new();
        let (first, second) = (c.running(), c.running());
        assert!(!c.claim(first, -1, 10));
        assert!(!c.claim(first, 5, 5));
        // a persisted completed segment is already owned without a new claim
        c.finished(first, 0, 10);
        assert!(!c.claim(first, 0, 10));
        assert!(!c.claim(second, 0, 5));
        assert!(c.claim(second, 10, 20));
    }

    #[test]
    fn restored_history_cannot_hide_earlier_overlap() {
        let c = Claims::new();
        let (first, second, third) = (c.running(), c.running(), c.running());
        c.finished(first, 0, 300);
        c.finished(second, 100, 100);
        assert!(!c.claim(third, 200, 300));
    }

    #[test]
    fn ordered_restored_history_remains_usable() {
        let c = Claims::new();
        let (first, second, third) = (c.running(), c.running(), c.running());
        c.finished(first, 0, 100);
        c.finished(second, 100, 100);
        assert!(c.claim(third, 200, 300));
    }

    #[test]
    fn malformed_restored_history_is_ambiguous() {
        for overflow in [false, true] {
            let c = Claims::new();
            let (first, second) = (c.running(), c.running());
            if overflow {
                c.finished(first, i64::MAX - 10, 30);
            } else {
                c.finished(first, 0, -1);
            }
            assert!(!c.claim(second, 100, 200));
        }
    }
}
