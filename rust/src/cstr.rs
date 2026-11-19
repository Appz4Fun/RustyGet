//! Helpers for NUL-terminated byte strings at the C boundary and in the
//! fixed-size buffers ported from C++.

use std::ffi::{c_char, CStr, CString};

/// The part of `bytes` before the first NUL, or all of it.
pub(crate) fn until_nul(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    &bytes[..end]
}

/// `bytes` up to the first NUL as a `CString`.
pub(crate) fn to_cstring(bytes: &[u8]) -> CString {
    // Cannot fail: `until_nul` leaves no interior NUL.
    CString::new(until_nul(bytes)).unwrap_or_default()
}

/// `bytes` up to the first NUL, followed by exactly one NUL.
pub(crate) fn nul_terminated(bytes: &[u8]) -> Vec<u8> {
    to_cstring(bytes).into_bytes_with_nul()
}

/// Borrows the bytes of a C string; a null pointer reads as empty.
///
/// # Safety
/// `p` must be null or point to a NUL-terminated string that outlives `'a`.
pub(crate) unsafe fn bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        unsafe { CStr::from_ptr(p) }.to_bytes()
    }
}

/// Copies the bytes of a C string; a null pointer reads as empty.
///
/// # Safety
/// Same as [`bytes`].
pub(crate) unsafe fn owned(p: *const c_char) -> Vec<u8> {
    unsafe { bytes(p) }.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_at_first_nul() {
        assert_eq!(until_nul(b"ab\0cd"), b"ab");
        assert_eq!(until_nul(b"abc"), b"abc");
        assert_eq!(to_cstring(b"ab\0cd").as_bytes(), b"ab");
        assert_eq!(nul_terminated(b"ab\0cd"), b"ab\0");
        assert_eq!(nul_terminated(b""), b"\0");
    }

    #[test]
    fn null_pointer_reads_empty() {
        unsafe {
            assert_eq!(bytes(std::ptr::null()), b"");
            assert_eq!(owned(c"hi".as_ptr()), b"hi");
        }
    }
}
