//! A history-only duplicate record: what is left of a collection after
//! "mark good" or a dupe-history cleanup. The C++ `DupInfo` object is this
//! allocation (its operator new calls `nzbget_rs_dup_new`), so a `DupInfo*`
//! is a `*mut Dup`.

use std::ffi::{c_char, c_int, CStr, CString};
use std::sync::Mutex;

#[derive(Default)]
struct Fields {
    id: i32,
    // EDupeMode
    mode: i32,
    score: i32,
    // DupInfo::EStatus
    status: i32,
    full_hash: u32,
    filtered_hash: u32,
    size: i64,
    name: CString,
    dupe_key: CString,
}

pub struct Dup(Mutex<Fields>);

pub extern "C" fn nzbget_rs_dup_new() -> *mut Dup {
    Box::into_raw(Box::new(Dup(Mutex::new(Fields::default()))))
}

pub unsafe extern "C" fn nzbget_rs_dup_free(d: *mut Dup) {
    if !d.is_null() {
        drop(unsafe { Box::from_raw(d) });
    }
}

/// Reads every field: ints = mode, score, status, full hash, filtered hash,
/// id; i64s[0] = size (when not null); strs = name, dupe key (valid until
/// the record changes).
pub unsafe extern "C" fn nzbget_rs_dup_get(
    d: *const Dup,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) {
    let f = unsafe { &*d }.0.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        for (k, v) in [
            f.mode,
            f.score,
            f.status,
            f.full_hash as i32,
            f.filtered_hash as i32,
            f.id,
        ]
        .into_iter()
        .enumerate()
        {
            *ints.add(k) = v;
        }
        if !i64s.is_null() {
            *i64s = f.size;
        }
        *strs = f.name.as_ptr();
        *strs.add(1) = f.dupe_key.as_ptr();
    }
}

/// Sets one field: 0 mode, 1 score, 2 status, 3 full hash, 4 filtered hash,
/// 5 id, 6 size (from v); 7 name, 8 dupe key (from s, null = empty).
pub unsafe extern "C" fn nzbget_rs_dup_set(d: *const Dup, field: c_int, v: i64, s: *const c_char) {
    let mut f = unsafe { &*d }.0.lock().unwrap_or_else(|e| e.into_inner());
    let text = || {
        if s.is_null() {
            CString::default()
        } else {
            unsafe { CStr::from_ptr(s) }.to_owned()
        }
    };
    match field {
        0 => f.mode = v as i32,
        1 => f.score = v as i32,
        2 => f.status = v as i32,
        3 => f.full_hash = v as u32,
        4 => f.filtered_hash = v as u32,
        5 => f.id = v as i32,
        6 => f.size = v,
        7 => f.name = text(),
        8 => f.dupe_key = text(),
        _ => {}
    }
}

/// Host op 35 on a duplicate record (sub-op 2, the history entry summary,
/// is `qhist::host_hist_summary`): 0 reads mode, score, status, full hash and
/// filtered hash into ints[0..5], the size into i64s[0] (when not null) and
/// name, dupe key into strs[0..2]; 1 sets the status (a2); 3 the name and
/// 4 the dupe key (s1, null = empty); 5 the score and 6 the mode (a2).
///
/// # Safety
/// `d` is null (ignored) or from `nzbget_rs_dup_new`; for sub-op 0 `ints` holds 5 ints and
/// `strs` 2 pointers (or is null), `i64s` is null or holds 1.
pub unsafe fn host_dup_op(
    d: *const Dup,
    a1: c_int,
    a2: c_int,
    s1: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    if d.is_null() {
        return 0;
    }
    match a1 {
        0 => {
            let mut all = [0 as c_int; 6];
            let mut texts = [std::ptr::null::<c_char>(); 2];
            unsafe {
                nzbget_rs_dup_get(d, all.as_mut_ptr(), i64s, texts.as_mut_ptr());
                if !ints.is_null() {
                    for (k, v) in all.iter().take(5).enumerate() {
                        *ints.add(k) = *v;
                    }
                }
                if !strs.is_null() {
                    *strs = texts[0];
                    *strs.add(1) = texts[1];
                }
            }
        }
        1 => unsafe { nzbget_rs_dup_set(d, 2, i64::from(a2), std::ptr::null()) },
        3 => unsafe { nzbget_rs_dup_set(d, 7, 0, s1) },
        4 => unsafe { nzbget_rs_dup_set(d, 8, 0, s1) },
        5 => unsafe { nzbget_rs_dup_set(d, 1, i64::from(a2), std::ptr::null()) },
        6 => unsafe { nzbget_rs_dup_set(d, 0, i64::from(a2), std::ptr::null()) },
        _ => {}
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_round_trip() {
        let ns: *const c_char = std::ptr::null();
        let mut ints = [0 as c_int; 6];
        let mut size = 0i64;
        let mut strs = [ns; 2];
        unsafe {
            let d = nzbget_rs_dup_new();
            nzbget_rs_dup_set(d, 3, 0xdead_beef, ns);
            nzbget_rs_dup_set(d, 5, 42, ns);
            nzbget_rs_dup_set(d, 6, 1 << 40, ns);
            nzbget_rs_dup_set(d, 7, 0, c"Show.S01E01".as_ptr());
            nzbget_rs_dup_get(d, ints.as_mut_ptr(), &mut size, strs.as_mut_ptr());
            assert_eq!(ints[3] as u32, 0xdead_beef);
            assert_eq!(ints[5], 42);
            assert_eq!(size, 1 << 40);
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"Show.S01E01");
            assert_eq!(CStr::from_ptr(strs[1]).to_bytes(), b"");
            nzbget_rs_dup_free(d);
        }
    }

    #[test]
    fn host_op_reads_and_writes() {
        let ns: *const c_char = std::ptr::null();
        let mut ints = [-1 as c_int; 5];
        let mut size = 0i64;
        let mut strs = [ns; 2];
        unsafe {
            let d = nzbget_rs_dup_new();
            host_dup_op(
                d,
                1,
                6,
                ns,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            host_dup_op(
                d,
                3,
                0,
                c"Movie.2020".as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            host_dup_op(
                d,
                4,
                0,
                c"imdb=1".as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            host_dup_op(
                d,
                5,
                250,
                ns,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            host_dup_op(
                d,
                6,
                2,
                ns,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            nzbget_rs_dup_set(d, 3, 7, ns);
            nzbget_rs_dup_set(d, 4, 9, ns);
            nzbget_rs_dup_set(d, 6, 1234, ns);
            assert_eq!(
                host_dup_op(d, 0, 0, ns, ints.as_mut_ptr(), &mut size, strs.as_mut_ptr()),
                0
            );
            assert_eq!(ints, [2, 250, 6, 7, 9]);
            assert_eq!(size, 1234);
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"Movie.2020");
            assert_eq!(CStr::from_ptr(strs[1]).to_bytes(), b"imdb=1");
            // a null name clears it; a null size slot is skipped
            host_dup_op(
                d,
                3,
                0,
                ns,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            host_dup_op(
                d,
                0,
                0,
                ns,
                ints.as_mut_ptr(),
                std::ptr::null_mut(),
                strs.as_mut_ptr(),
            );
            assert_eq!(CStr::from_ptr(strs[0]).to_bytes(), b"");
            nzbget_rs_dup_free(d);
        }
    }
}
