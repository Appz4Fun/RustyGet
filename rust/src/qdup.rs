//! A history-only duplicate record (the C++ DupInfo is a thin owner): what
//! is left of a collection after "mark good" or a dupe-history cleanup.

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

#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_dup_new() -> *mut Dup {
    Box::into_raw(Box::new(Dup(Mutex::new(Fields::default()))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_dup_free(d: *mut Dup) {
    if !d.is_null() {
        drop(unsafe { Box::from_raw(d) });
    }
}

/// Reads every field: ints = mode, score, status, full hash, filtered hash,
/// id; i64s[0] = size (when not null); strs = name, dupe key (valid until
/// the record changes).
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
}
