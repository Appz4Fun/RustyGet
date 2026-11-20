//! A history-only duplicate record: what is left of a collection after
//! "mark good" or a dupe-history cleanup. A history entry of kind dup holds
//! the record's address (from [`Dup::alloc`]).

use std::ffi::{c_char, CString};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::queuesubsys_host::DupSnap;
use crate::status::{DupStatus, DupeMode};

#[derive(Default)]
struct Fields {
    id: i32,
    mode: DupeMode,
    score: i32,
    status: DupStatus,
    full_hash: u32,
    filtered_hash: u32,
    size: i64,
    name: CString,
    dupe_key: CString,
}

pub struct Dup(Mutex<Fields>);

/// One field change on a duplicate record. Text stops at the first NUL.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DupEdit<'a> {
    Id(i32),
    Mode(DupeMode),
    Score(i32),
    Status(DupStatus),
    FullHash(u32),
    FilteredHash(u32),
    Size(i64),
    Name(&'a [u8]),
    DupeKey(&'a [u8]),
}

fn c_string(bytes: &[u8]) -> CString {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    CString::new(&bytes[..end]).unwrap_or_default()
}

impl Dup {
    /// A new, empty record on the heap; free it with [`Dup::free`].
    pub(crate) fn alloc() -> *mut Dup {
        Box::into_raw(Box::new(Dup(Mutex::new(Fields::default()))))
    }

    /// Frees a record from [`Dup::alloc`].
    ///
    /// # Safety
    /// `d` is null (ignored) or from [`Dup::alloc`] and not used afterwards.
    pub(crate) unsafe fn free(d: *mut Dup) {
        if !d.is_null() {
            drop(unsafe { Box::from_raw(d) });
        }
    }

    /// The record at `addr`, or `None` for 0.
    ///
    /// # Safety
    /// A non-zero `addr` is from [`Dup::alloc`] and still alive for `'a`.
    pub(crate) unsafe fn from_addr<'a>(addr: usize) -> Option<&'a Dup> {
        (addr != 0).then(|| unsafe { &*(addr as *const Dup) })
    }

    fn fields(&self) -> MutexGuard<'_, Fields> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn edit(&self, edit: DupEdit<'_>) {
        let mut f = self.fields();
        match edit {
            DupEdit::Id(v) => f.id = v,
            DupEdit::Mode(v) => f.mode = v,
            DupEdit::Score(v) => f.score = v,
            DupEdit::Status(v) => f.status = v,
            DupEdit::FullHash(v) => f.full_hash = v,
            DupEdit::FilteredHash(v) => f.filtered_hash = v,
            DupEdit::Size(v) => f.size = v,
            DupEdit::Name(v) => f.name = c_string(v),
            DupEdit::DupeKey(v) => f.dupe_key = c_string(v),
        }
    }

    pub(crate) fn id(&self) -> i32 {
        self.fields().id
    }

    /// The name as a C string, valid until the name changes or the record
    /// is freed.
    pub(crate) fn name_ptr(&self) -> *const c_char {
        self.fields().name.as_ptr()
    }

    /// A copy of every field but the id.
    pub(crate) fn snapshot(&self) -> DupSnap {
        let f = self.fields();
        DupSnap {
            dupe_mode: f.mode,
            dupe_score: f.score,
            status: f.status,
            size: f.size,
            full_content_hash: f.full_hash,
            filtered_content_hash: f.filtered_hash,
            name: f.name.to_bytes().to_vec(),
            dupe_key: f.dupe_key.to_bytes().to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_round_trip() {
        let d = Dup::alloc();
        let r = unsafe { Dup::from_addr(d as usize) }.unwrap();
        r.edit(DupEdit::Status(DupStatus::Good));
        r.edit(DupEdit::Name(b"Movie.2020"));
        r.edit(DupEdit::DupeKey(b"imdb=1\0tail"));
        r.edit(DupEdit::Score(250));
        r.edit(DupEdit::Mode(DupeMode::Force));
        r.edit(DupEdit::FullHash(0xdead_beef));
        r.edit(DupEdit::FilteredHash(9));
        r.edit(DupEdit::Size(1 << 40));
        r.edit(DupEdit::Id(42));
        let s = r.snapshot();
        assert_eq!(i32::from(s.status), 6);
        assert_eq!(i32::from(s.dupe_mode), 2);
        assert_eq!(s.dupe_score, 250);
        assert_eq!(s.full_content_hash, 0xdead_beef);
        assert_eq!(s.filtered_content_hash, 9);
        assert_eq!(s.size, 1 << 40);
        assert_eq!(s.name, b"Movie.2020");
        assert_eq!(s.dupe_key, b"imdb=1");
        assert_eq!(r.id(), 42);
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(r.name_ptr()) }.to_bytes(),
            b"Movie.2020"
        );
        // unknown stored numbers are kept as they are
        r.edit(DupEdit::Status(DupStatus::from(77)));
        r.edit(DupEdit::Mode(DupeMode::from(-3)));
        r.edit(DupEdit::Name(b""));
        let s = r.snapshot();
        assert_eq!((i32::from(s.status), i32::from(s.dupe_mode)), (77, -3));
        assert!(s.name.is_empty());
        assert!(unsafe { Dup::from_addr(0) }.is_none());
        unsafe {
            Dup::free(d);
            Dup::free(std::ptr::null_mut());
        }
    }
}
