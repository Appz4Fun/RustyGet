//! A history entry: its kind, the time it entered history and the inner
//! record (a C++ `NzbInfo*` for nzb and url entries, a `DupInfo*` for dup
//! entries). The C++ `HistoryInfo` object is this allocation (its operator new
//! calls `nzbget_rs_hist_new`), so a `HistoryInfo*` is a `*mut Hist`. The C++
//! destructor still deletes the inner record before the storage is freed.

use std::ffi::{c_char, c_int};
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicUsize, Ordering};

use crate::qdup::Dup;
use crate::qnzb::{nzbget_rs_nzb_text, NzbScalars};

/// `HistoryInfo::EKind`.
pub const KIND_UNKNOWN: i32 = 0;
pub const KIND_NZB: i32 = 1;
pub const KIND_URL: i32 = 2;
pub const KIND_DUP: i32 = 3;

/// Field numbers for `nzbget_rs_hist_get` / `nzbget_rs_hist_set`.
pub mod field {
    pub const KIND: i32 = 0;
    pub const TIME: i32 = 1;
    pub const INFO: i32 = 2;
}

#[derive(Default)]
pub struct Hist {
    kind: AtomicI32,
    time: AtomicI64,
    info: AtomicUsize,
}

pub extern "C" fn nzbget_rs_hist_new() -> *mut Hist {
    Box::into_raw(Box::default())
}

/// Frees the entry only; the inner record is the caller's.
///
/// # Safety
/// `h` is null or from `nzbget_rs_hist_new`, freed once.
pub unsafe extern "C" fn nzbget_rs_hist_free(h: *mut Hist) {
    if !h.is_null() {
        drop(unsafe { Box::from_raw(h) });
    }
}

/// Reads one field: 0 kind, 1 time, 2 inner record pointer (0 for others).
///
/// # Safety
/// `h` is from `nzbget_rs_hist_new`.
pub unsafe extern "C" fn nzbget_rs_hist_get(h: *const Hist, field: c_int) -> i64 {
    let h = unsafe { &*h };
    match field {
        field::KIND => i64::from(h.kind.load(Ordering::Relaxed)),
        field::TIME => h.time.load(Ordering::Relaxed),
        field::INFO => h.info.load(Ordering::Relaxed) as i64,
        _ => 0,
    }
}

/// Sets one field (numbered as in `nzbget_rs_hist_get`); others are ignored.
///
/// # Safety
/// `h` is from `nzbget_rs_hist_new`.
pub unsafe extern "C" fn nzbget_rs_hist_set(h: *const Hist, field: c_int, v: i64) {
    let h = unsafe { &*h };
    match field {
        field::KIND => h.kind.store(v as i32, Ordering::Relaxed),
        field::TIME => h.time.store(v, Ordering::Relaxed),
        field::INFO => h.info.store(v as usize, Ordering::Relaxed),
        _ => {}
    }
}

/// Host op 35 sub-op 2: ints[0] = kind, ints[1] = id, strs[0] = name; returns
/// the inner record (0 for an unknown kind, whose name is "<unknown>").
///
/// # Safety
/// `h` is null (ignored) or from `nzbget_rs_hist_new` with a live inner record; `ints`
/// holds 2 ints and `strs` 1 pointer (either may be null).
pub unsafe fn host_hist_summary(
    h: *const Hist,
    ints: *mut c_int,
    strs: *mut *const c_char,
) -> usize {
    if h.is_null() {
        return 0;
    }
    let entry = unsafe { &*h };
    let kind = entry.kind.load(Ordering::Relaxed);
    let info = entry.info.load(Ordering::Relaxed);
    let (id, name, inner) = match kind {
        KIND_NZB | KIND_URL if info != 0 => {
            let p = info as *const NzbScalars;
            (
                unsafe { (*p).id },
                unsafe { nzbget_rs_nzb_text(p, 2) },
                info,
            )
        }
        KIND_DUP if info != 0 => {
            let d = unsafe { &*(info as *const Dup) };
            (d.id(), d.name_ptr(), info)
        }
        KIND_NZB | KIND_URL | KIND_DUP => (0, std::ptr::null(), 0),
        _ => (0, c"<unknown>".as_ptr(), 0),
    };
    unsafe {
        if !ints.is_null() {
            *ints = kind;
            *ints.add(1) = id;
        }
        if !strs.is_null() {
            *strs = name;
        }
    }
    inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qdup::DupEdit;
    use crate::qnzb::{
        nzbget_rs_nzb_scalars_free, nzbget_rs_nzb_scalars_new, nzbget_rs_nzb_set_text,
    };
    use std::ffi::CStr;

    #[test]
    fn fields_round_trip() {
        unsafe {
            let h = nzbget_rs_hist_new();
            assert_eq!(nzbget_rs_hist_get(h, field::KIND), i64::from(KIND_UNKNOWN));
            assert_eq!(nzbget_rs_hist_get(h, field::TIME), 0);
            assert_eq!(nzbget_rs_hist_get(h, field::INFO), 0);
            nzbget_rs_hist_set(h, field::KIND, i64::from(KIND_DUP));
            nzbget_rs_hist_set(h, field::TIME, 1_700_000_000);
            nzbget_rs_hist_set(h, field::INFO, 0x1000);
            nzbget_rs_hist_set(h, 9, 5);
            assert_eq!(nzbget_rs_hist_get(h, field::KIND), i64::from(KIND_DUP));
            assert_eq!(nzbget_rs_hist_get(h, field::TIME), 1_700_000_000);
            assert_eq!(nzbget_rs_hist_get(h, field::INFO), 0x1000);
            assert_eq!(nzbget_rs_hist_get(h, 9), 0);
            nzbget_rs_hist_free(h);
            nzbget_rs_hist_free(std::ptr::null_mut());
        }
    }

    #[test]
    fn summary_of_nzb_entry() {
        let mut ints = [-1 as c_int; 2];
        let mut strs = [std::ptr::null::<c_char>(); 1];
        unsafe {
            let n = nzbget_rs_nzb_scalars_new();
            (*n).id = 17;
            nzbget_rs_nzb_set_text(n, 2, c"Show.S02E03".as_ptr(), -1);
            let h = nzbget_rs_hist_new();
            nzbget_rs_hist_set(h, field::KIND, i64::from(KIND_URL));
            nzbget_rs_hist_set(h, field::INFO, n as i64);
            let inner = host_hist_summary(h, ints.as_mut_ptr(), strs.as_mut_ptr());
            assert_eq!(inner, n as usize);
            assert_eq!(ints, [KIND_URL, 17]);
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"Show.S02E03");
            nzbget_rs_hist_free(h);
            nzbget_rs_nzb_scalars_free(n);
        }
    }

    #[test]
    fn summary_of_dup_and_unknown_entries() {
        let ns: *const c_char = std::ptr::null();
        let mut ints = [-1 as c_int; 2];
        let mut strs = [ns; 1];
        unsafe {
            let d = Dup::alloc();
            (*d).edit(DupEdit::Id(42));
            (*d).edit(DupEdit::Name(b"Movie.2021"));
            let h = nzbget_rs_hist_new();
            nzbget_rs_hist_set(h, field::KIND, i64::from(KIND_DUP));
            nzbget_rs_hist_set(h, field::INFO, d as i64);
            assert_eq!(
                host_hist_summary(h, ints.as_mut_ptr(), strs.as_mut_ptr()),
                d as usize
            );
            assert_eq!(ints, [KIND_DUP, 42]);
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"Movie.2021");

            nzbget_rs_hist_set(h, field::KIND, i64::from(KIND_UNKNOWN));
            assert_eq!(
                host_hist_summary(h, ints.as_mut_ptr(), strs.as_mut_ptr()),
                0
            );
            assert_eq!(ints, [KIND_UNKNOWN, 0]);
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"<unknown>");

            // a discarded inner record reads as empty
            nzbget_rs_hist_set(h, field::KIND, i64::from(KIND_NZB));
            nzbget_rs_hist_set(h, field::INFO, 0);
            assert_eq!(
                host_hist_summary(h, ints.as_mut_ptr(), strs.as_mut_ptr()),
                0
            );
            assert_eq!(ints, [KIND_NZB, 0]);
            assert!(strs[0].is_null());
            nzbget_rs_hist_free(h);
            Dup::free(d);
        }
    }
}
