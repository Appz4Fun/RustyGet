//! Fixed-capacity and heap-allocated NUL-terminated string buffers (`BString`,
//! `CString`, `StringBuilder`, `CharBuffer`) ported from `daemon/util/NString.h`
//! and `daemon/util/NString.cpp`.

use std::ffi::{c_char, CStr};

mod sys {
    use std::ffi::c_void;
    extern "C" {
        pub fn malloc(size: usize) -> *mut c_void;
        pub fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
        pub fn free(ptr: *mut c_void);
    }
}

#[inline]
unsafe fn c_strlen(ptr: *const c_char) -> usize {
    if ptr.is_null() {
        0
    } else {
        CStr::from_ptr(ptr).to_bytes().len()
    }
}

/// Copies up to `count` bytes from `src` to `dst`, stopping at the first NUL byte in `src`
/// and zero-padding the remainder of `0..count` (matching C `strncpy`).
#[inline]
unsafe fn strncpy_bytes(dst: *mut c_char, src: *const c_char, count: usize) {
    let mut i = 0usize;
    if !src.is_null() {
        while i < count {
            let b = *src.add(i);
            *dst.add(i) = b;
            i += 1;
            if b == 0 {
                break;
            }
        }
    }
    while i < count {
        *dst.add(i) = 0;
        i += 1;
    }
}

/// Copies a NUL-terminated string from `src` to `dst` including the trailing NUL (matching C `strcpy`).
#[inline]
unsafe fn strcpy_bytes(dst: *mut c_char, src: *const c_char) {
    if src.is_null() {
        *dst = 0;
        return;
    }
    let mut i = 0usize;
    loop {
        let b = *src.add(i);
        *dst.add(i) = b;
        if b == 0 {
            break;
        }
        i += 1;
    }
}

/// Simulates `vsnprintf(dst, size, ...)` given already-formatted bytes `formatted`.
unsafe fn write_vsnprintf_slice(dst: *mut c_char, size: usize, formatted: &[u8]) -> i32 {
    if !dst.is_null() && size > 0 {
        let copy_len = formatted.len().min(size - 1);
        std::ptr::copy_nonoverlapping(formatted.as_ptr().cast::<c_char>(), dst, copy_len);
        *dst.add(copy_len) = 0;
    }
    formatted.len() as i32
}

// ============================================================================
// Raw operations shared by C++ FFI (`daemon/util/NString.cpp`) and Rust types
// ============================================================================

/// `BString<size>::Set(const char* str, int len)` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `buf` must point to a writable buffer of at least `size` bytes; `str_ptr` must be
/// valid for up to `min(size - 1, len)` bytes (or NUL-terminated when `len <= 0`).
pub unsafe fn bstring_set_raw(buf: *mut c_char, size: i32, str_ptr: *const c_char, len: i32) {
    if buf.is_null() || size <= 0 {
        return;
    }
    *buf = 0;
    if str_ptr.is_null() {
        return;
    }
    let add_len = crate::util::nstring_bstring_set_len(size, len).max(0) as usize;
    strncpy_bytes(buf, str_ptr, add_len);
    *buf.add(add_len) = 0;
}

/// `BString<size>::Append(const char* str, int len)` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `buf` must point to a NUL-terminated buffer of capacity `size`; `str_ptr` must be valid.
pub unsafe fn bstring_append_raw(buf: *mut c_char, size: i32, str_ptr: *const c_char, len: i32) {
    if buf.is_null() || size <= 0 || str_ptr.is_null() {
        return;
    }
    let eff_len = if len == 0 {
        c_strlen(str_ptr) as i32
    } else {
        len
    };
    let cur_len = c_strlen(buf) as i32;
    let add_len = crate::util::nstring_bstring_append_len(size, cur_len, eff_len);
    if add_len > 0 {
        let cur_u = cur_len as usize;
        let add_u = add_len as usize;
        strncpy_bytes(buf.add(cur_u), str_ptr, add_u);
        *buf.add(cur_u + add_u) = 0;
    }
}

/// `BString<size>::AppendFmtV` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `buf` must point to a NUL-terminated buffer of capacity `size`.
pub unsafe fn bstring_append_fmt_raw(
    buf: *mut c_char,
    size: i32,
    fmt_cb: impl FnOnce(*mut c_char, usize) -> i32,
) {
    if buf.is_null() || size <= 0 {
        return;
    }
    let cur_len = c_strlen(buf) as i32;
    let avail = size - cur_len;
    if avail > 0 {
        let _ = fmt_cb(buf.add(cur_len as usize), avail as usize);
    }
}

/// `BString<size>::FormatV` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `buf` must point to a writable buffer of capacity `size`.
pub unsafe fn bstring_format_raw(
    buf: *mut c_char,
    size: i32,
    fmt_cb: impl FnOnce(*mut c_char, usize) -> i32,
) -> i32 {
    if buf.is_null() || size <= 0 {
        return 0;
    }
    fmt_cb(buf, size as usize).max(0)
}

/// `CString::operator==` (`daemon/util/NString.cpp`).
///
/// # Safety
/// Non-null pointers must point to valid NUL-terminated C strings.
pub unsafe fn cstring_eq_raw(a: *const c_char, b: *const c_char) -> bool {
    if a.is_null() && b.is_null() {
        true
    } else if !a.is_null() && !b.is_null() {
        CStr::from_ptr(a).to_bytes() == CStr::from_ptr(b).to_bytes()
    } else {
        false
    }
}

/// `CString::Clear` (`daemon/util/NString.h`).
///
/// # Safety
/// `m_data` must point to a valid `*mut c_char` slot allocated with `malloc`/`realloc` (or null).
pub unsafe fn cstring_clear_raw(m_data: *mut *mut c_char) {
    if m_data.is_null() {
        return;
    }
    sys::free((*m_data).cast());
    *m_data = std::ptr::null_mut();
}

/// `CString::Bind` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `m_data` must point to a valid `*mut c_char` slot.
pub unsafe fn cstring_bind_raw(m_data: *mut *mut c_char, str_ptr: *mut c_char) {
    if m_data.is_null() {
        return;
    }
    sys::free((*m_data).cast());
    *m_data = str_ptr;
}

/// `CString::Unbind` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `m_data` must point to a valid `*mut c_char` slot.
pub unsafe fn cstring_unbind_raw(m_data: *mut *mut c_char) -> *mut c_char {
    if m_data.is_null() {
        return std::ptr::null_mut();
    }
    let old = *m_data;
    *m_data = std::ptr::null_mut();
    old
}

/// `CString::Set(const char* str, int len)` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `m_data` must point to a valid `*mut c_char` slot; `str_ptr` must be null or valid.
pub unsafe fn cstring_set_raw(m_data: *mut *mut c_char, str_ptr: *const c_char, len: i32) {
    if m_data.is_null() {
        return;
    }
    if !str_ptr.is_null() {
        let eff_len = if len <= 0 {
            c_strlen(str_ptr)
        } else {
            len as usize
        };
        let old_ptr = *m_data;
        let alias_offset = if !old_ptr.is_null() {
            let old_len = c_strlen(old_ptr);
            let old_start = old_ptr as usize;
            let old_end = old_start.saturating_add(old_len + 1);
            let src_addr = str_ptr as usize;
            if src_addr >= old_start && src_addr < old_end {
                Some(src_addr - old_start)
            } else {
                None
            }
        } else {
            None
        };
        let data = sys::realloc(old_ptr.cast(), eff_len + 1).cast::<c_char>();
        if !data.is_null() {
            *m_data = data;
            let src = match alias_offset {
                Some(off) => data.add(off) as *const c_char,
                None => str_ptr,
            };
            if src != data as *const c_char {
                strncpy_bytes(data, src, eff_len);
            }
            *data.add(eff_len) = 0;
        }
    } else {
        sys::free((*m_data).cast());
        *m_data = std::ptr::null_mut();
    }
}

/// `CString::Append(const char* str, int len)` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `m_data` must point to a valid `*mut c_char` slot; `str_ptr` must be valid.
pub unsafe fn cstring_append_raw(m_data: *mut *mut c_char, str_ptr: *const c_char, len: i32) {
    if m_data.is_null() || str_ptr.is_null() {
        return;
    }
    let eff_len = if len == 0 {
        c_strlen(str_ptr)
    } else if len > 0 {
        len as usize
    } else {
        return;
    };
    let old_ptr = *m_data;
    let cur_len = c_strlen(old_ptr);
    let new_len = cur_len + eff_len;
    let alias_offset = if !old_ptr.is_null() {
        let old_start = old_ptr as usize;
        let old_end = old_start.saturating_add(cur_len + 1);
        let src_addr = str_ptr as usize;
        if src_addr >= old_start && src_addr < old_end {
            Some(src_addr - old_start)
        } else {
            None
        }
    } else {
        None
    };
    let data = sys::realloc(old_ptr.cast(), new_len + 1).cast::<c_char>();
    if !data.is_null() {
        *m_data = data;
        let src = match alias_offset {
            Some(off) => data.add(off) as *const c_char,
            None => str_ptr,
        };
        strncpy_bytes(data.add(cur_len), src, eff_len);
        *data.add(new_len) = 0;
    }
}

/// `CString::AppendFmtV` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `m_data` must point to a valid `*mut c_char` slot.
pub unsafe fn cstring_append_fmt_raw(
    m_data: *mut *mut c_char,
    add_len: i32,
    fmt_cb: impl FnOnce(*mut c_char, usize) -> i32,
) {
    if m_data.is_null() || add_len <= 0 {
        return;
    }
    let cur_len = c_strlen(*m_data);
    let add_u = add_len as usize;
    let new_len = cur_len + add_u;
    let data = sys::realloc((*m_data).cast(), new_len + 1).cast::<c_char>();
    if data.is_null() {
        return;
    }
    *m_data = data;
    if fmt_cb(data.add(cur_len), add_u + 1) < 0 {
        *data.add(cur_len) = 0;
    }
}

/// `CString::FormatV` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `m_data` must point to a valid `*mut c_char` slot.
pub unsafe fn cstring_format_raw(
    m_data: *mut *mut c_char,
    new_len_no_nul: i32,
    fmt_cb: impl FnOnce(*mut c_char, usize) -> i32,
) -> i32 {
    if m_data.is_null() || new_len_no_nul < 0 {
        return 0;
    }
    let new_len = (new_len_no_nul + 1) as usize;
    let data = sys::realloc((*m_data).cast(), new_len).cast::<c_char>();
    if data.is_null() {
        return 0;
    }
    *m_data = data;
    if fmt_cb(data, new_len) < 0 {
        *data = 0;
        return 0;
    }
    new_len as i32
}

/// `CString::Find(const char* str, int pos)` (`daemon/util/NString.cpp`).
///
/// # Safety
/// Non-null pointers must be valid NUL-terminated C strings.
pub unsafe fn cstring_find_raw(m_data: *const c_char, str_ptr: *const c_char, pos: i32) -> i32 {
    if m_data.is_null() || str_ptr.is_null() {
        return -1;
    }
    crate::util::nstring_find_substr_from(
        CStr::from_ptr(m_data).to_bytes(),
        CStr::from_ptr(str_ptr).to_bytes(),
        pos,
    )
}

/// `CString::Replace(int pos, int len, const char* str, int strLen)` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `m_data` must point to a valid `*mut c_char` slot; `str_ptr` must be valid.
pub unsafe fn cstring_replace_range_raw(
    m_data: *mut *mut c_char,
    pos: i32,
    len: i32,
    str_ptr: *const c_char,
    str_len: i32,
) {
    if m_data.is_null() || str_ptr.is_null() || pos < 0 || len < 0 {
        return;
    }
    let cur_len = c_strlen(*m_data) as i32;
    let raw_str_len = c_strlen(str_ptr) as i32;
    let (ok, add_len, del_len, new_len) =
        crate::util::nstring_cstring_replace_bounds(pos, len, cur_len, raw_str_len, str_len);
    if !ok || new_len < 0 {
        return;
    }
    let new_u = new_len as usize;
    let newvalue = sys::malloc(new_u + 1).cast::<c_char>();
    if newvalue.is_null() {
        return;
    }
    let pos_u = pos as usize;
    let add_u = add_len.max(0) as usize;
    let del_u = del_len.max(0) as usize;
    if !(*m_data).is_null() {
        strncpy_bytes(newvalue, *m_data, pos_u);
    } else if pos_u > 0 {
        strncpy_bytes(newvalue, std::ptr::null(), pos_u);
    }
    strncpy_bytes(newvalue.add(pos_u), str_ptr, add_u);
    if !(*m_data).is_null() {
        strcpy_bytes(newvalue.add(pos_u + add_u), (*m_data).add(pos_u + del_u));
    } else {
        *newvalue.add(pos_u + add_u) = 0;
    }
    sys::free((*m_data).cast());
    *m_data = newvalue;
    *newvalue.add(new_u) = 0;
}

/// `CString::Replace(const char* from, const char* to)` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `m_data` must point to a valid `*mut c_char` slot; `from_ptr` and `to_ptr` must be valid.
pub unsafe fn cstring_replace_all_raw(
    m_data: *mut *mut c_char,
    from_ptr: *const c_char,
    to_ptr: *const c_char,
) {
    if m_data.is_null() || from_ptr.is_null() || to_ptr.is_null() {
        return;
    }
    let from_len = c_strlen(from_ptr) as i32;
    if from_len == 0 {
        return;
    }
    let to_len = c_strlen(to_ptr) as i32;
    let mut pos = 0i32;
    loop {
        let found = cstring_find_raw(*m_data, from_ptr, pos);
        if found == -1 {
            break;
        }
        cstring_replace_range_raw(m_data, found, from_len, to_ptr, 0);
        pos = found + to_len;
    }
}

/// `CString::Reserve(int capacity)` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `m_data` must point to a valid `*mut c_char` slot.
pub unsafe fn cstring_reserve_raw(m_data: *mut *mut c_char, capacity: i32) {
    if m_data.is_null() {
        return;
    }
    let cur_len = c_strlen(*m_data) as i32;
    if (capacity > 0 && capacity > cur_len) || cur_len == 0 {
        let cap_u = capacity.max(0) as usize;
        let data = sys::realloc((*m_data).cast(), cap_u + 1).cast::<c_char>();
        if !data.is_null() {
            *m_data = data;
            *data.add(cur_len as usize) = 0;
        }
    }
}

/// `CString::TrimRight()` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `m_data` must be null or point to a valid NUL-terminated mutable C string.
pub unsafe fn cstring_trim_right_raw(m_data: *mut c_char) {
    if m_data.is_null() {
        return;
    }
    let len = c_strlen(m_data);
    if len == 0 {
        return;
    }
    let slice = std::slice::from_raw_parts(m_data.cast::<u8>(), len);
    let new_len = crate::util::nstring_cstring_trim_right_len(slice);
    for i in new_len..len {
        *m_data.add(i) = 0;
    }
}

/// `StringBuilder::Clear()` (`daemon/util/NString.cpp`).
///
/// # Safety
/// All pointers must be valid.
pub unsafe fn string_builder_clear_raw(
    m_data: *mut *mut c_char,
    m_length: *mut i32,
    m_capacity: *mut i32,
) {
    if m_data.is_null() {
        return;
    }
    sys::free((*m_data).cast());
    *m_data = std::ptr::null_mut();
    if !m_length.is_null() {
        *m_length = 0;
    }
    if !m_capacity.is_null() {
        *m_capacity = 0;
    }
}

/// `StringBuilder::Unbind()` (`daemon/util/NString.cpp`).
///
/// # Safety
/// All pointers must be valid.
pub unsafe fn string_builder_unbind_raw(
    m_data: *mut *mut c_char,
    m_length: *mut i32,
    m_capacity: *mut i32,
) -> *mut c_char {
    if m_data.is_null() {
        return std::ptr::null_mut();
    }
    let old = *m_data;
    *m_data = std::ptr::null_mut();
    if !m_length.is_null() {
        *m_length = 0;
    }
    if !m_capacity.is_null() {
        *m_capacity = 0;
    }
    old
}

/// `StringBuilder::Reserve(int capacity, bool exact)` (`daemon/util/NString.cpp`).
///
/// # Safety
/// `m_data` and `m_capacity` must be valid pointers.
pub unsafe fn string_builder_reserve_raw(
    m_data: *mut *mut c_char,
    m_capacity: *mut i32,
    capacity: i32,
    exact: bool,
) {
    if m_data.is_null() || m_capacity.is_null() {
        return;
    }
    let old_capacity = *m_capacity;
    let new_capacity =
        crate::util::string_builder_calc_reserve_capacity(old_capacity, capacity, exact);
    if new_capacity >= 0 {
        let old_data = *m_data;
        let data = sys::realloc(old_data.cast(), (new_capacity as usize) + 1).cast::<c_char>();
        if data.is_null() {
            return;
        }
        *m_capacity = new_capacity;
        *m_data = data;
        if old_data.is_null() {
            *data = 0;
        }
    }
}

/// `StringBuilder::Append(const char* str, int len)` (`daemon/util/NString.cpp`).
///
/// # Safety
/// All pointers must be valid.
pub unsafe fn string_builder_append_raw(
    m_data: *mut *mut c_char,
    m_length: *mut i32,
    m_capacity: *mut i32,
    str_ptr: *const c_char,
    len: i32,
) {
    if m_data.is_null() || m_length.is_null() || m_capacity.is_null() || str_ptr.is_null() {
        return;
    }
    let eff_len = if len == 0 {
        c_strlen(str_ptr) as i32
    } else if len > 0 {
        len
    } else {
        return;
    };
    let cur_len = *m_length;
    let new_len = cur_len + eff_len;
    string_builder_reserve_raw(m_data, m_capacity, new_len, false);
    if (*m_data).is_null() {
        return;
    }
    strncpy_bytes((*m_data).add(cur_len as usize), str_ptr, eff_len as usize);
    *(*m_data).add(new_len as usize) = 0;
    *m_length = new_len;
}

/// `StringBuilder::AppendFmtV` (`daemon/util/NString.cpp`).
///
/// # Safety
/// All pointers must be valid.
pub unsafe fn string_builder_append_fmt_raw(
    m_data: *mut *mut c_char,
    m_length: *mut i32,
    m_capacity: *mut i32,
    add_len: i32,
    fmt_cb: impl FnOnce(*mut c_char, usize) -> i32,
) {
    if m_data.is_null() || m_length.is_null() || m_capacity.is_null() || add_len <= 0 {
        return;
    }
    let cur_len = *m_length;
    let new_len = cur_len + add_len;
    string_builder_reserve_raw(m_data, m_capacity, new_len, false);
    if (*m_data).is_null() {
        return;
    }
    if fmt_cb((*m_data).add(cur_len as usize), (new_len + 1) as usize) > 0 {
        *m_length = new_len;
        *(*m_data).add(new_len as usize) = 0;
    }
}

/// `CharBuffer::Reserve(int size)` (`daemon/util/NString.h`).
///
/// # Safety
/// `m_data` and `m_size` must be valid pointers.
pub unsafe fn char_buffer_reserve_raw(m_data: *mut *mut c_char, m_size: *mut i32, size: i32) {
    if m_data.is_null() || m_size.is_null() || size <= 0 {
        return;
    }
    let data = sys::realloc((*m_data).cast(), size as usize).cast::<c_char>();
    if !data.is_null() {
        *m_data = data;
        *m_size = size;
    }
}

/// `CharBuffer::Clear()` (`daemon/util/NString.h`).
///
/// # Safety
/// `m_data` and `m_size` must be valid pointers.
pub unsafe fn char_buffer_clear_raw(m_data: *mut *mut c_char, m_size: *mut i32) {
    if m_data.is_null() {
        return;
    }
    sys::free((*m_data).cast());
    *m_data = std::ptr::null_mut();
    if !m_size.is_null() {
        *m_size = 0;
    }
}

// ============================================================================
// Safe, idiomatic Rust wrappers backed by the raw functions above
// ============================================================================

/// Stack-allocated fixed-capacity string buffer (`BString<size>` in `daemon/util/NString.h`).
#[repr(C)]
pub struct BString<const N: usize> {
    data: [c_char; N],
}

impl<const N: usize> Default for BString<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> BString<N> {
    pub fn new() -> Self {
        assert!(N > 0, "BString<N> requires N > 0");
        let mut s = Self { data: [0; N] };
        s.data[0] = 0;
        s
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut s = Self::new();
        s.set(bytes);
        s
    }

    pub fn from_fmt(args: std::fmt::Arguments<'_>) -> Self {
        let mut s = Self::new();
        s.format_args(args);
        s
    }

    pub fn capacity(&self) -> usize {
        N.saturating_sub(1)
    }

    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        if N > 0 {
            self.data[0] = 0;
        }
    }

    pub fn as_ptr(&self) -> *const c_char {
        self.data.as_ptr()
    }

    pub fn as_mut_ptr(&mut self) -> *mut c_char {
        self.data.as_mut_ptr()
    }

    pub fn as_bytes(&self) -> &[u8] {
        if N == 0 {
            return b"";
        }
        let raw = unsafe { std::slice::from_raw_parts(self.data.as_ptr().cast::<u8>(), N) };
        let end = raw.iter().position(|&b| b == 0).unwrap_or(N);
        &raw[..end]
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(self.as_bytes()).unwrap_or("")
    }

    pub fn set(&mut self, bytes: &[u8]) {
        let mut nul_terminated = Vec::with_capacity(bytes.len() + 1);
        nul_terminated.extend_from_slice(bytes);
        nul_terminated.push(0);
        unsafe {
            bstring_set_raw(
                self.data.as_mut_ptr(),
                N as i32,
                nul_terminated.as_ptr().cast(),
                0,
            );
        }
    }

    pub fn set_len(&mut self, bytes: &[u8], len: i32) {
        let mut nul_terminated = Vec::with_capacity(bytes.len() + 1);
        nul_terminated.extend_from_slice(bytes);
        nul_terminated.push(0);
        unsafe {
            bstring_set_raw(
                self.data.as_mut_ptr(),
                N as i32,
                nul_terminated.as_ptr().cast(),
                len,
            );
        }
    }

    pub fn append(&mut self, bytes: &[u8]) {
        self.append_len(bytes, 0);
    }

    pub fn append_len(&mut self, bytes: &[u8], len: i32) {
        let mut nul_terminated = Vec::with_capacity(bytes.len() + 1);
        nul_terminated.extend_from_slice(bytes);
        nul_terminated.push(0);
        unsafe {
            bstring_append_raw(
                self.data.as_mut_ptr(),
                N as i32,
                nul_terminated.as_ptr().cast(),
                len,
            );
        }
    }

    pub fn format_args(&mut self, args: std::fmt::Arguments<'_>) -> i32 {
        let formatted = std::fmt::format(args);
        unsafe {
            bstring_format_raw(self.data.as_mut_ptr(), N as i32, |dst, sz| {
                write_vsnprintf_slice(dst, sz, formatted.as_bytes())
            })
        }
    }

    pub fn append_fmt_args(&mut self, args: std::fmt::Arguments<'_>) {
        let formatted = std::fmt::format(args);
        unsafe {
            bstring_append_fmt_raw(self.data.as_mut_ptr(), N as i32, |dst, sz| {
                write_vsnprintf_slice(dst, sz, formatted.as_bytes())
            });
        }
    }
}

/// Heap-allocated NUL-terminated C string (`CString` in `daemon/util/NString.h`).
#[repr(transparent)]
#[derive(Debug)]
pub struct CStringBuf {
    data: *mut c_char,
}

impl Default for CStringBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CStringBuf {
    fn drop(&mut self) {
        self.clear();
    }
}

impl PartialEq for CStringBuf {
    fn eq(&self, other: &Self) -> bool {
        unsafe { cstring_eq_raw(self.data, other.data) }
    }
}

impl PartialEq<&[u8]> for CStringBuf {
    fn eq(&self, other: &&[u8]) -> bool {
        let mut buf = Vec::with_capacity(other.len() + 1);
        buf.extend_from_slice(other);
        buf.push(0);
        unsafe { cstring_eq_raw(self.data, buf.as_ptr().cast()) }
    }
}

impl PartialEq<&str> for CStringBuf {
    fn eq(&self, other: &&str) -> bool {
        self == &other.as_bytes()
    }
}

impl CStringBuf {
    pub fn new() -> Self {
        Self {
            data: std::ptr::null_mut(),
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut s = Self::new();
        s.set(bytes);
        s
    }

    pub fn format_str(args: std::fmt::Arguments<'_>) -> Self {
        let mut s = Self::new();
        s.format_args(args);
        s
    }

    pub fn ptr(&self) -> *mut c_char {
        self.data
    }

    pub fn str_non_null(&self) -> &[u8] {
        if self.data.is_null() {
            b""
        } else {
            unsafe { CStr::from_ptr(self.data).to_bytes() }
        }
    }

    pub fn len(&self) -> usize {
        unsafe { c_strlen(self.data) }
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_null() || unsafe { *self.data == 0 }
    }

    pub fn clear(&mut self) {
        unsafe { cstring_clear_raw(&mut self.data) };
    }

    pub fn reserve(&mut self, capacity: i32) {
        unsafe { cstring_reserve_raw(&mut self.data, capacity) };
    }

    /// Binds a `malloc`/`strdup`-allocated C string pointer directly.
    ///
    /// # Safety
    /// `ptr` must have been allocated by `malloc`/`realloc`/`strdup` (or be unbound before drop).
    pub unsafe fn bind(&mut self, ptr: *mut c_char) {
        cstring_bind_raw(&mut self.data, ptr);
    }

    pub fn unbind(&mut self) -> *mut c_char {
        unsafe { cstring_unbind_raw(&mut self.data) }
    }

    pub fn set(&mut self, bytes: &[u8]) {
        self.set_len(bytes, 0);
    }

    pub fn set_len(&mut self, bytes: &[u8], len: i32) {
        let mut buf = Vec::with_capacity(bytes.len() + 1);
        buf.extend_from_slice(bytes);
        buf.push(0);
        unsafe { cstring_set_raw(&mut self.data, buf.as_ptr().cast(), len) };
    }

    pub fn append(&mut self, bytes: &[u8]) {
        self.append_len(bytes, 0);
    }

    pub fn append_len(&mut self, bytes: &[u8], len: i32) {
        let mut buf = Vec::with_capacity(bytes.len() + 1);
        buf.extend_from_slice(bytes);
        buf.push(0);
        unsafe { cstring_append_raw(&mut self.data, buf.as_ptr().cast(), len) };
    }

    pub fn format_args(&mut self, args: std::fmt::Arguments<'_>) -> i32 {
        let formatted = std::fmt::format(args);
        let bytes = formatted.as_bytes();
        unsafe {
            cstring_format_raw(&mut self.data, bytes.len() as i32, |dst, sz| {
                write_vsnprintf_slice(dst, sz, bytes)
            })
        }
    }

    pub fn append_fmt_args(&mut self, args: std::fmt::Arguments<'_>) {
        let formatted = std::fmt::format(args);
        let bytes = formatted.as_bytes();
        unsafe {
            cstring_append_fmt_raw(&mut self.data, bytes.len() as i32, |dst, sz| {
                write_vsnprintf_slice(dst, sz, bytes)
            });
        }
    }

    pub fn find(&self, needle: &[u8], pos: i32) -> i32 {
        let mut buf = Vec::with_capacity(needle.len() + 1);
        buf.extend_from_slice(needle);
        buf.push(0);
        unsafe { cstring_find_raw(self.data, buf.as_ptr().cast(), pos) }
    }

    pub fn replace_range(&mut self, pos: i32, len: i32, replacement: &[u8], str_len: i32) {
        let mut buf = Vec::with_capacity(replacement.len() + 1);
        buf.extend_from_slice(replacement);
        buf.push(0);
        unsafe {
            cstring_replace_range_raw(&mut self.data, pos, len, buf.as_ptr().cast(), str_len);
        }
    }

    pub fn replace_all(&mut self, from: &[u8], to: &[u8]) {
        let mut from_buf = Vec::with_capacity(from.len() + 1);
        from_buf.extend_from_slice(from);
        from_buf.push(0);
        let mut to_buf = Vec::with_capacity(to.len() + 1);
        to_buf.extend_from_slice(to);
        to_buf.push(0);
        unsafe {
            cstring_replace_all_raw(&mut self.data, from_buf.as_ptr().cast(), to_buf.as_ptr().cast());
        }
    }

    pub fn trim_right(&mut self) {
        unsafe { cstring_trim_right_raw(self.data) };
    }
}

/// Preallocating string builder (`StringBuilder` in `daemon/util/NString.h`).
pub struct StringBuilder {
    data: *mut c_char,
    length: i32,
    capacity: i32,
}

impl Default for StringBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for StringBuilder {
    fn drop(&mut self) {
        self.clear();
    }
}

impl StringBuilder {
    pub fn new() -> Self {
        Self {
            data: std::ptr::null_mut(),
            length: 0,
            capacity: 0,
        }
    }

    pub fn len(&self) -> i32 {
        self.length
    }

    pub fn capacity(&self) -> i32 {
        self.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn as_bytes(&self) -> &[u8] {
        if self.data.is_null() {
            b""
        } else {
            unsafe { CStr::from_ptr(self.data).to_bytes() }
        }
    }

    pub fn clear(&mut self) {
        unsafe {
            string_builder_clear_raw(&mut self.data, &mut self.length, &mut self.capacity);
        }
    }

    pub fn unbind(&mut self) -> *mut c_char {
        unsafe { string_builder_unbind_raw(&mut self.data, &mut self.length, &mut self.capacity) }
    }

    pub fn reserve(&mut self, capacity: i32, exact: bool) {
        unsafe { string_builder_reserve_raw(&mut self.data, &mut self.capacity, capacity, exact) };
    }

    pub fn append(&mut self, bytes: &[u8]) {
        self.append_len(bytes, 0);
    }

    pub fn append_len(&mut self, bytes: &[u8], len: i32) {
        let mut buf = Vec::with_capacity(bytes.len() + 1);
        buf.extend_from_slice(bytes);
        buf.push(0);
        unsafe {
            string_builder_append_raw(
                &mut self.data,
                &mut self.length,
                &mut self.capacity,
                buf.as_ptr().cast(),
                len,
            );
        }
    }

    pub fn append_fmt_args(&mut self, args: std::fmt::Arguments<'_>) {
        let formatted = std::fmt::format(args);
        let bytes = formatted.as_bytes();
        unsafe {
            string_builder_append_fmt_raw(
                &mut self.data,
                &mut self.length,
                &mut self.capacity,
                bytes.len() as i32,
                |dst, sz| write_vsnprintf_slice(dst, sz, bytes),
            );
        }
    }
}

/// Plain char buffer for I/O operations (`CharBuffer` in `daemon/util/NString.h`).
pub struct CharBuffer {
    data: *mut c_char,
    size: i32,
}

impl Default for CharBuffer {
    fn default() -> Self {
        Self {
            data: std::ptr::null_mut(),
            size: 0,
        }
    }
}

impl Drop for CharBuffer {
    fn drop(&mut self) {
        self.clear();
    }
}

impl CharBuffer {
    pub fn new(size: i32) -> Self {
        let mut buf = Self::default();
        buf.reserve(size);
        buf
    }

    pub fn size(&self) -> i32 {
        self.size
    }

    pub fn reserve(&mut self, size: i32) {
        unsafe { char_buffer_reserve_raw(&mut self.data, &mut self.size, size) };
    }

    pub fn clear(&mut self) {
        unsafe { char_buffer_clear_raw(&mut self.data, &mut self.size) };
    }

    pub fn as_mut_ptr(&mut self) -> *mut c_char {
        self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nstring_full_suite_port_from_cpp() {
        // 1. BStringTest
        {
            let mut str1: BString<100> = BString::new();
            assert_eq!(std::mem::size_of_val(&str1), std::mem::size_of::<[c_char; 100]>());
            assert!(str1.is_empty());
            assert!(!str1.as_ptr().is_null());
            str1.set(b"Hello, world");
            assert!(!str1.is_empty());
            assert_eq!(str1.as_bytes(), b"Hello, world");

            str1.format_args(format_args!("Hi, {}{}: {}", "World", '!', 21));
            assert_eq!(str1.as_bytes(), b"Hi, World!: 21");

            let mut str2: BString<20> = BString::new();
            str2.set(b"Hello, world 01234567890123456789");
            assert_eq!(str2.as_bytes(), b"Hello, world 012345");

            str2.format_args(format_args!("0123456789 Hi, {}{}: {}", "World", '!', 21));
            assert_eq!(str2.as_bytes(), b"0123456789 Hi, Worl");

            let mut str3: BString<20> = BString::new();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    b"Hello, 0123456789 world".as_ptr().cast::<c_char>(),
                    str3.as_mut_ptr(),
                    str3.capacity(),
                );
                *str3.as_mut_ptr().add(str3.capacity()) = 0;
            }
            assert_eq!(str3.as_bytes(), b"Hello, 0123456789 w");

            str3.set(b"String 3 Test World, Hello!");
            assert_eq!(str3.as_bytes(), b"String 3 Test World");

            let mut str4: BString<100> = BString::new();
            str4.set(b"String 4 initialized");
            assert_eq!(str4.as_bytes(), b"String 4 initialized");

            let str5: BString<20> =
                BString::from_fmt(format_args!("Hi, {}{}: {}", "World", '!', 21));
            assert_eq!(str5.as_bytes(), b"Hi, World!: 21");

            let mut str6: BString<20> = BString::new();
            str6.append(b"Hello, World");
            str6.append(b"String5String5");
            str6.append(b"67");
            str6.append_len(b"0123456789", 5);
            assert_eq!(str6.as_bytes(), b"Hello, WorldString5");

            let mut str7: BString<20> = BString::new();
            str7.append_len(b"0123456789", 5);
            assert_eq!(str7.as_bytes(), b"01234");

            let mut str8: BString<20> = BString::new();
            str8.append_len(b"0123456789", 5);
            str8.append_fmt_args(format_args!("{}:{}", 87, 65));
            assert_eq!(str8.as_bytes(), b"0123487:65");

            let txt = b"String 9 initialized";
            let str9: BString<100> = BString::from_bytes(txt);
            assert_eq!(str9.as_bytes(), b"String 9 initialized");
        }

        // 2. CStringTest
        {
            let mut str1 = CStringBuf::new();
            assert_eq!(
                std::mem::size_of_val(&str1),
                std::mem::size_of::<*mut c_char>()
            );
            assert!(str1.is_empty());
            assert!(str1.ptr().is_null());
            str1.set(b"Hello, world");
            assert!(!str1.is_empty());
            assert!(!str1.ptr().is_null());
            assert_eq!(str1.str_non_null(), b"Hello, world");

            str1.format_args(format_args!("Hi, {}{}: {}", "World", '!', 21));
            assert_eq!(str1.str_non_null(), b"Hi, World!: 21");

            unsafe {
                let raw_src = b"Hello there\0";
                let tmp = sys::malloc(raw_src.len()).cast::<c_char>();
                std::ptr::copy_nonoverlapping(raw_src.as_ptr().cast::<c_char>(), tmp, raw_src.len());
                let mut str2 = CStringBuf::new();
                str2.bind(tmp);
                let tmp3 = str2.ptr() as *const c_char;
                assert_eq!(tmp as *const c_char, tmp3);
                assert_eq!(str2.str_non_null(), b"Hello there");
                assert_eq!(tmp, str2.ptr());
                sys::free(tmp.cast());

                let tmp2 = str2.unbind();
                assert_eq!(tmp2, tmp);
                assert!(str2.is_empty());
                assert!(str2.ptr().is_null());
            }

            let str3 = CStringBuf::from_bytes(b"World 12345678901234567890");
            let buf = format!(
                "Hi, {}{}: {}",
                std::str::from_utf8(str3.str_non_null()).unwrap(),
                '!',
                21
            );
            assert_eq!(buf, "Hi, World 12345678901234567890!: 21");

            let str4 = CStringBuf::new();
            assert!(str4.ptr().is_null());
            assert_eq!(str4.str_non_null(), b"");

            let mut str6 = CStringBuf::new();
            str6.append(b"");
            str6.append(b"Hello, World");
            str6.append(b"String5String5");
            str6.append(b"67");
            assert_eq!(str6.str_non_null(), b"Hello, WorldString5String567");

            str6.clear();
            str6.append_len(b"0123456789", 5);
            str6.append_fmt_args(format_args!("{}:{}", 87, 65));
            assert_eq!(str6.str_non_null(), b"0123487:65");

            let mut vec1 = Vec::new();
            vec1.push(CStringBuf::from_bytes(b"Hello, there"));
            let str7 = vec1.last().unwrap();
            assert_eq!(str7.str_non_null(), b"Hello, there");

            assert_eq!(
                CStringBuf::format_str(format_args!("Many {} ago", "days")).str_non_null(),
                b"Many days ago"
            );

            let mut str8 = CStringBuf::from_bytes(b"Hello, World");
            str8.replace_range(1, 4, b"i", 0);
            assert_eq!(str8.str_non_null(), b"Hi, World");
            str8.replace_range(4, 5, b"everybody", 0);
            assert_eq!(str8.str_non_null(), b"Hi, everybody");
            str8.replace_range(4, 5, b"nome", 2);
            assert_eq!(str8.str_non_null(), b"Hi, nobody");

            let mut str9 = CStringBuf::from_bytes(b" Long string with spaces \t\r\n ");
            str9.trim_right();
            assert_eq!(str9.str_non_null(), b" Long string with spaces");
            let pos = str9.find(b"with", 0);
            assert_eq!(pos, 13);
            str9.replace_all(b"string", b"with");
            assert_eq!(str9.str_non_null(), b" Long with with spaces");
            str9.replace_all(b"with", b"without");
            assert_eq!(str9.str_non_null(), b" Long without without spaces");
            str9.replace_all(b"without", b"");
            assert_eq!(str9.str_non_null(), b" Long   spaces");

            str8.set(b"test string");
            str9.set(b"test string");
            assert_eq!(str8, str9);
            assert!(str8 == "test string");
        }

        // 3. StringBuilderTest
        {
            let mut str6 = StringBuilder::new();
            str6.append(b"");
            str6.append(b"Hello, World");
            str6.append(b"String5String5");
            str6.append(b"67");
            assert_eq!(str6.as_bytes(), b"Hello, WorldString5String567");
        }

        // 4. CharBufferTest
        {
            let mut buf = CharBuffer::new(1024);
            assert_eq!(buf.size(), 1024);
            buf.reserve(2048);
            assert_eq!(buf.size(), 2048);
            buf.clear();
            assert_eq!(buf.size(), 0);
        }

        // 5. CStringEdgeTest
        {
            let mut str1 = CStringBuf::from_bytes(b"some text that's long enough to move");
            assert_eq!(str1.format_args(format_args!("{}", "")), 1);
            assert_eq!(str1.len(), 0);
            assert!(!str1.ptr().is_null());
            assert_eq!(str1.str_non_null(), b"");
            str1.append(b"next");
            assert_eq!(str1.str_non_null(), b"next");
            str1.append_fmt_args(format_args!("{}", ""));
            assert_eq!(str1.str_non_null(), b"next");

            let mut text = CStringBuf::from_bytes(b"abcdef");
            text.replace_range(4, 10, b"XY", 0);
            assert_eq!(text.str_non_null(), b"abcdXY");

            let mut none = CStringBuf::new();
            assert_eq!(none.find(b"a", 0), -1);
            none.replace_all(b"a", b"b");
            assert!(none.ptr().is_null());
        }
    }
}
