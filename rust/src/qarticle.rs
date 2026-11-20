//! Articles of a queued file: the data that was the C++ `ArticleInfo` and its
//! `ArticleList`, now owned by Rust.
//!
//! The C++ `FileInfo` keeps one [`ArticleList`] handle. Host operation 43 (and
//! the article parts of 34, 40, 42, 44 and 45) forward to
//! [`nzbget_rs_article_host`], so the `article_ptr` values Rust code passes
//! around are addresses of [`Article`]s.
//!
//! Article operations only ever take a shared reference: the cache segment
//! (pointer, allocation, offset, size, crc) is atomic because the article
//! writer attaches it outside the queue lock, and every other field sits behind
//! the article's own mutex. Changing the list itself (append, clear, restore)
//! still relies on the download-queue lock, as the C++ code did.

use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

pub const AI_UNDEFINED: i32 = 0;
pub const AI_RUNNING: i32 = 1;
pub const AI_FINISHED: i32 = 2;
pub const AI_FAILED: i32 = 3;

/// The fields of an article other than its cache segment.
struct Data {
    part_number: i32,
    size: i32,
    status: i32,
    message_id: CString,
    result_filename: Option<CString>,
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

impl Default for Data {
    fn default() -> Self {
        Self {
            part_number: 0,
            size: 0,
            status: AI_UNDEFINED,
            message_id: CString::default(),
            result_filename: None,
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

impl Data {
    fn result_filename_ptr(&self) -> *const c_char {
        self.result_filename
            .as_ref()
            .map_or(ptr::null(), |s| s.as_ptr())
    }
}

/// One article of a file.
#[derive(Default)]
pub struct Article {
    segment_data: AtomicUsize,
    segment_alloc_size: AtomicI32,
    segment_offset: AtomicI64,
    segment_size: AtomicI32,
    crc: AtomicU32,
    d: Mutex<Data>,
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

fn opt_cstring(p: *const c_char) -> Option<CString> {
    (!p.is_null()).then(|| cstring(p))
}

/// One change to an article's fields. Message-ids stop at the first NUL.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ArticleEdit<'a> {
    /// One of the `AI_*` states.
    Status(i32),
    FallbackRound(i32),
    DonorId(i32),
    Proactive(bool),
    /// The decoded range a donor article is expected to fill (-1 unknown).
    ExpectedRange {
        offset: i64,
        end: i64,
    },
    MessageId(&'a [u8]),
    OriginalMessageId(&'a [u8]),
    /// Back to undefined with no segment, crc or expected range, for a new
    /// attempt.
    ResetForRetry,
}

fn cstring_from(bytes: &[u8]) -> CString {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    CString::new(&bytes[..end]).unwrap_or_default()
}

impl Article {
    fn new(part_number: i32, size: i32, message_id: *const c_char) -> Self {
        let a = Self::default();
        {
            let mut d = a.lock();
            d.part_number = part_number;
            d.size = size;
            d.message_id = cstring(message_id);
        }
        a
    }

    /// The article at `addr`, or `None` for 0.
    ///
    /// # Safety
    /// A non-zero `addr` names a live article for `'a`.
    pub(crate) unsafe fn from_addr<'a>(addr: usize) -> Option<&'a Article> {
        (addr != 0).then(|| unsafe { &*(addr as *const Article) })
    }

    pub(crate) fn edit(&self, edit: ArticleEdit<'_>) {
        let mut d = self.lock();
        match edit {
            ArticleEdit::Status(v) => d.status = v,
            ArticleEdit::FallbackRound(v) => d.dupe_fallback_round = v,
            ArticleEdit::DonorId(v) => d.dupe_donor_id = v,
            ArticleEdit::Proactive(v) => d.dupe_proactive = v,
            ArticleEdit::ExpectedRange { offset, end } => {
                d.dupe_expected_offset = offset;
                d.dupe_expected_end = end;
            }
            ArticleEdit::MessageId(v) => d.message_id = cstring_from(v),
            ArticleEdit::OriginalMessageId(v) => d.dupe_original_message_id = cstring_from(v),
            ArticleEdit::ResetForRetry => {
                d.status = AI_UNDEFINED;
                self.crc.store(0, Ordering::SeqCst);
                self.set_geometry(0, 0);
                d.dupe_expected_offset = -1;
                d.dupe_expected_end = -1;
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, Data> {
        self.d.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn addr(&self) -> usize {
        self as *const Article as usize
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

    fn set_geometry(&self, offset: i64, size: i32) {
        self.segment_offset.store(offset, Ordering::SeqCst);
        self.segment_size.store(size, Ordering::SeqCst);
    }

    fn attach_segment(&self, data: usize, alloc_size: i32, offset: i64, size: i32) {
        self.discard_segment();
        self.segment_data.store(data, Ordering::SeqCst);
        self.segment_alloc_size.store(alloc_size, Ordering::SeqCst);
        self.set_geometry(offset, size);
    }

    fn discard_segment(&self) {
        let alloc = self.segment_alloc_size.swap(0, Ordering::SeqCst);
        let data = self.segment_data.swap(0, Ordering::SeqCst);
        if alloc != 0 {
            crate::articledl::cache_free_raw(data as *mut u8, alloc);
        }
    }

    /// Fills the summary the Rust side reads for an article (op 43, a1 0/1/7/8).
    unsafe fn fill(
        &self,
        done: bool,
        ints: *mut c_int,
        i64s: *mut i64,
        strs: *mut *const c_char,
    ) -> usize {
        let d = self.lock();
        let run = !done && d.status == AI_RUNNING;
        unsafe {
            *ints = d.part_number;
            *ints.add(1) = d.size;
            *ints.add(2) = d.status;
            *ints.add(3) = if run { 0 } else { self.segment_size() };
            *ints.add(4) = if run { 0 } else { self.crc() as i32 };
            *ints.add(5) = d.dupe_fallback_round;
            *ints.add(6) = d.dupe_lead_snapshot;
            *ints.add(7) = d.dupe_next_lead;
            *ints.add(8) = d.dupe_donor_count;
            *ints.add(9) = d.dupe_donor_id;
            *ints.add(10) = c_int::from(d.dupe_proactive);
            *i64s = if run { -1 } else { self.segment_offset() };
            *strs = d.message_id.as_ptr();
            *strs.add(1) = d.dupe_original_message_id.as_ptr();
        }
        self.addr()
    }

    /// The range this article is known to cover, for claim validation:
    /// (1, lo, hi) known, (0, ..) unknown, (-1, ..) malformed.
    fn known_range(&self) -> (i32, i64, i64) {
        let d = self.lock();
        let (lo, hi) = (d.live_par_claim_begin, d.live_par_claim_end);
        if lo != -1 || hi != -1 {
            return (if lo >= 0 && hi > lo { 1 } else { -1 }, lo, hi);
        }
        // Finished geometry is stable under the queue lock; running geometry
        // is not a claim, cache attachment can still change it.
        if d.status != AI_FINISHED {
            return (0, 0, 0);
        }
        let lo = self.segment_offset();
        let size = self.segment_size();
        if lo < 0 || size <= 0 || lo > i64::MAX - i64::from(size) {
            return (-1, 0, 0);
        }
        (1, lo, lo + i64::from(size))
    }
}

/// The articles of one file, in order.
#[derive(Default)]
pub struct ArticleList {
    items: Vec<Box<Article>>,
}

impl ArticleList {
    fn at(&self, index: usize) -> Option<&Article> {
        self.items.get(index).map(|b| &**b)
    }

    fn index_of(&self, target: usize) -> Option<usize> {
        self.items.iter().position(|a| a.addr() == target)
    }

    /// Admits a live-par claim on `target`'s header range when it keeps the
    /// file's known article ranges in order (was C++
    /// `live_par_claim_source_header`). Numeric only: no cache geometry or
    /// strings change. The queue lock serializes admission; a rejected claim
    /// changes nothing.
    fn claim_source_header(&self, target: usize, begin: i64, end: i64) -> bool {
        if begin < 0 || end <= begin {
            return false;
        }
        let Some(index) = self.index_of(target) else {
            return false;
        };
        {
            let t = &self.items[index];
            let d = t.lock();
            // An independently fetched replacement cannot prove same-payload identity.
            if d.live_par_claim_begin != -1
                || d.live_par_claim_end != -1
                || t.segment_size() != 0
                || d.status == AI_FINISHED
            {
                return false;
            }
        }
        // Restored ranges were not admitted here: validate the whole chain,
        // including the proposed insertion, before trusting it.
        let mut previous_end = -1i64;
        for (i, a) in self.items.iter().enumerate() {
            let (known, lo, hi) = if i == index {
                (1, begin, end)
            } else {
                a.known_range()
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
        let mut d = self.items[index].lock();
        d.live_par_claim_begin = begin;
        d.live_par_claim_end = end;
        true
    }
}

pub extern "C" fn nzbget_rs_artlist_new() -> *mut ArticleList {
    Box::into_raw(Box::default())
}

/// # Safety
/// `list` is null or from `nzbget_rs_artlist_new`, freed once.
pub unsafe extern "C" fn nzbget_rs_artlist_free(list: *mut ArticleList) {
    if !list.is_null() {
        drop(unsafe { Box::from_raw(list) });
    }
}

/// Article operations. `list` is the file's [`ArticleList`] (may be null for
/// the operations on one `article`). Codes 0-14 are host operation 43's
/// (`a1` there); 100 and up serve the C++ queue model:
///
/// - 0 summary of the article at index `a2`; 1 summary of `article` (done);
///   7 first undefined article; 8 summary of `article` with its index in
///   `ints[11]`. Summaries return the article address.
/// - 3 pinned source `a2` (`a2` < 0: count); 4 pin sources (`a2` sources in
///   `strs`, `ints` = lead, next lead, donor count, donors count, donors...).
///   Field changes go through [`Article::edit`].
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
/// the operation. Operations that change the list (11, 12, 101, 105) need the
/// queue lock, as in the C++ code; article operations lock the article.
#[allow(clippy::too_many_arguments)]
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
    // List changes need exclusive access; everything else only reads the list.
    let list_ref: Option<&ArticleList> = unsafe { list.as_ref() };
    let art: Option<&Article> = unsafe { article.cast_const().as_ref() };
    match op {
        11 | 12 | 101 | 105 => {
            let Some(l) = (unsafe { list.as_mut() }) else {
                return 0;
            };
            return unsafe { list_change(l, op, a2, p2, s1, ints, i64s, strs) };
        }
        _ => {}
    }
    match (op, list_ref, art) {
        (0, Some(l), _) => match l.at(a2 as usize) {
            Some(a) => unsafe { a.fill(false, ints, i64s, strs) },
            None => 0,
        },
        (1, _, Some(a)) => unsafe { a.fill(true, ints, i64s, strs) },
        (7, Some(l), _) | (8, Some(l), _) => {
            let found = if op == 7 {
                l.items.iter().position(|a| a.lock().status == AI_UNDEFINED)
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
        (3, _, Some(a)) => {
            let d = a.lock();
            if a2 < 0 {
                return d.dupe_sources.len();
            }
            let i = a2 as usize;
            let has = i < d.dupe_source_donors.len();
            unsafe {
                *ints.add(1) = c_int::from(has);
                *ints = if has { d.dupe_source_donors[i] } else { 0 };
                *strs = d.dupe_sources.get(i).map_or(ptr::null(), |s| s.as_ptr());
            }
            0
        }
        (4, _, Some(a)) => {
            let mut d = a.lock();
            unsafe {
                d.dupe_lead_snapshot = *ints;
                d.dupe_next_lead = *ints.add(1);
                d.dupe_donor_count = *ints.add(2);
                d.dupe_sources = (0..a2.max(0) as usize)
                    .map(|i| cstring(*strs.add(i)))
                    .collect();
                let n = (*ints.add(3)).max(0) as usize;
                d.dupe_source_donors = (0..n).map(|i| *ints.add(4 + i)).collect();
            }
            0
        }
        (10, Some(l), _) => {
            let Some(a) = l.at(a2 as usize) else {
                return 0;
            };
            let d = a.lock();
            unsafe {
                *ints = d.status;
                *ints.add(1) = a.segment_size();
                *ints.add(2) = a.crc() as i32;
                *ints.add(3) = d.dupe_fallback_round;
                *ints.add(4) = c_int::from(d.result_filename.is_some());
                *ints.add(5) = d.part_number;
                *ints.add(6) = d.size;
                *i64s = a.segment_offset();
                *strs = d.message_id.as_ptr();
                *strs.add(1) = d.dupe_original_message_id.as_ptr();
            }
            0
        }
        (13, Some(l), _) => {
            let (begin, end) = unsafe { (*i64s, *i64s.add(1)) };
            usize::from(l.claim_source_header(article as usize, begin, end))
        }
        (14, _, Some(a)) => usize::from(a.lock().live_par_claim_begin >= 0 || a.segment_size() > 0),
        (100, Some(l), _) => l.items.len(),
        (102, Some(l), _) => {
            for a in &l.items {
                let mut d = a.lock();
                d.status = AI_UNDEFINED;
                d.result_filename = None;
                drop(d);
                a.discard_segment();
            }
            0
        }
        (103, Some(l), _) => {
            for a in &l.items {
                let mut d = a.lock();
                if d.status == AI_FINISHED {
                    d.status = AI_FAILED;
                }
            }
            0
        }
        (104, Some(l), _) => {
            for a in &l.items {
                let d = a.lock();
                if d.status != AI_RUNNING && (p2 != 0 || d.dupe_fallback_round > 0) {
                    if let Some(f) = &d.result_filename {
                        use std::os::unix::ffi::OsStrExt;
                        let _ = std::fs::remove_file(std::ffi::OsStr::from_bytes(f.to_bytes()));
                    }
                }
            }
            0
        }
        (106, Some(l), _) => l.at(a2.max(0) as usize).map_or(0, Article::addr),
        (110, _, Some(a)) => a.segment_data.load(Ordering::SeqCst),
        (111, _, Some(a)) => {
            a.discard_segment();
            0
        }
        (112, _, Some(a)) => {
            unsafe { a.attach_segment(*i64s.add(1) as usize, *ints, *i64s, *ints.add(1)) };
            0
        }
        (113, _, Some(a)) => {
            unsafe { a.set_geometry(*i64s, *ints) };
            0
        }
        (114, _, Some(a)) => a.lock().result_filename_ptr() as usize,
        (115, _, Some(a)) => {
            let mut d = a.lock();
            if !s1.is_null() {
                d.result_filename = opt_cstring(s1);
            }
            d.result_filename_ptr() as usize
        }
        (116, _, Some(a)) => {
            a.crc.store(a2 as u32, Ordering::SeqCst);
            0
        }
        (117, _, Some(a)) => a.lock().dupe_fallback_round as usize,
        (118, _, Some(a)) => a.lock().part_number as usize,
        (119, _, Some(a)) => {
            let d = a.lock();
            unsafe {
                *i64s = d.dupe_expected_offset;
                *i64s.add(1) = d.dupe_expected_end;
            }
            0
        }
        (120, _, Some(a)) => {
            let d = a.lock();
            unsafe {
                *ints = d.status;
                *ints.add(1) = a.segment_size();
                *ints.add(2) = a.crc() as i32;
                *ints.add(3) = d.part_number;
                *ints.add(4) = d.dupe_fallback_round;
                *i64s = a.segment_offset();
                *strs = a.segment_data.load(Ordering::SeqCst) as *const c_char;
                *strs.add(1) = d.result_filename_ptr();
            }
            usize::from(d.status == AI_FINISHED)
        }
        (121, _, Some(a)) => {
            let d = a.lock();
            unsafe {
                *i64s = d.live_par_claim_begin;
                *i64s.add(1) = d.live_par_claim_end;
            }
            0
        }
        _ => 0,
    }
}

/// The list-changing operations (11, 12, 101, 105); see [`nzbget_rs_article_host`].
#[allow(clippy::too_many_arguments)]
unsafe fn list_change(
    l: &mut ArticleList,
    op: c_int,
    a2: c_int,
    p2: usize,
    s1: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    match op {
        11 => {
            l.items.push(Box::new(Article::new(a2, p2 as i32, s1)));
            0
        }
        12 => {
            let i = a2.max(0) as usize;
            while l.items.len() <= i {
                l.items.push(Box::default());
            }
            let a = &l.items[i];
            let mut d = a.lock();
            if p2 == 0 {
                return ((d.part_number << 1) | i32::from(d.result_filename.is_some())) as usize;
            }
            unsafe {
                if *ints >= 2 {
                    a.set_geometry(*i64s, *ints.add(1));
                    a.crc.store(*ints.add(2) as u32, Ordering::SeqCst);
                }
                d.dupe_fallback_round = *ints.add(3);
                if !s1.is_null() {
                    d.result_filename = opt_cstring(s1);
                }
                d.status = *ints.add(4);
            }
            0
        }
        101 => {
            l.items.clear();
            0
        }
        105 => {
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
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_status(addr: usize, status: i32) {
        unsafe { Article::from_addr(addr) }
            .unwrap()
            .edit(ArticleEdit::Status(status));
    }

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
        set_status(addr, AI_FINISHED);
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
            set_status(a, AI_RUNNING);
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
            set_status(art, AI_FINISHED);
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
        set_status(first, AI_UNDEFINED);
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
