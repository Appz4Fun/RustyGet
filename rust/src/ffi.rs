//! C ABI. Strings come in as NUL-terminated C strings; results go out as a
//! buffer the caller copies and then frees with nzbget_rs_free.

use std::ffi::{c_char, c_int, c_void, CStr, CString};

#[path = "tlscipher/native.rs"]
pub(crate) mod tls_cipher_native;

#[no_mangle]
pub extern "C" fn nzbget_rs_tls_cipher_pin(protocol: c_int, bits: u32, flags: u32) -> c_int {
    use crate::tlscipher::{eligible, Protocol, Suite};
    let version = match protocol {
        12 => Protocol::Tls12,
        13 => Protocol::Tls13,
        _ => return 0,
    };
    if bits > u16::MAX.into() {
        return 0;
    }
    let suite = Suite {
        name: String::new(),
        algorithm: String::new(),
        protocol: version,
        security_bits: bits as u16,
        aead: flags & 1 != 0,
        certificate: flags & 2 != 0,
        ephemeral: flags & 4 != 0,
    };
    if eligible(&suite) {
        protocol
    } else {
        0
    }
}

fn cipher_response(result: Result<String, crate::tlscipher::Error>) -> RsBuf {
    match result {
        Ok(json) => into_buf(json.into_bytes()),
        Err(error) => {
            let mut message = Vec::new();
            crate::escape::json_encode(error.0.as_bytes(), &mut message);
            let mut out = b"{\"Error\":\"".to_vec();
            out.extend(message);
            out.extend_from_slice(b"\"}");
            into_buf(out)
        }
    }
}

/// # Safety
/// backend points to a complete non-null callback table. Its context transfers
/// to Rust exactly once, including rejected/duplicate starts. Callbacks are
/// thread-safe for the documented single-worker use and must never unwind.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_tls_cipher_start(
    backend: *const tls_cipher_native::Io,
) -> RsBuf {
    if backend.is_null() {
        return cipher_response(Err(crate::tlscipher::Error("Missing TLS backend.")));
    }
    cipher_response(tls_cipher_native::start(unsafe { *backend }))
}
/// # Safety
/// id is null or a readable NUL-terminated string; null/empty queries current job.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_tls_cipher_status(id: *const c_char) -> RsBuf {
    let id = if id.is_null() {
        None
    } else {
        let Ok(value) = unsafe { CStr::from_ptr(id) }.to_str() else {
            return cipher_response(Err(crate::tlscipher::Error("Invalid cipher job ID.")));
        };
        (!value.is_empty()).then_some(value)
    };
    cipher_response(tls_cipher_native::service().status(id))
}
/// # Safety
/// id points to a readable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_tls_cipher_cancel(id: *const c_char) -> RsBuf {
    if id.is_null() {
        return cipher_response(Err(crate::tlscipher::Error("Missing cipher job ID.")));
    }
    let Ok(id) = unsafe { CStr::from_ptr(id) }.to_str() else {
        return cipher_response(Err(crate::tlscipher::Error("Invalid cipher job ID.")));
    };
    cipher_response(
        tls_cipher_native::service()
            .cancel(id)
            .and_then(|()| tls_cipher_native::service().status(Some(id))),
    )
}
#[no_mangle]
pub extern "C" fn nzbget_rs_tls_cipher_shutdown() {
    tls_cipher_native::service().shutdown();
}

#[cfg(test)]
mod tls_cipher_abi_tests {
    use super::*;
    #[test]
    fn cipher_pin_validates_protocol_security_and_authentication_flags() {
        assert_eq!(nzbget_rs_tls_cipher_pin(13, 128, 3), 13);
        assert_eq!(nzbget_rs_tls_cipher_pin(12, 128, 7), 12);
        for (protocol, bits, flags) in [
            (11, 256, 7),
            (12, 128, 3),
            (13, 64, 7),
            (13, 128, 1),
            (13, 128, 2),
            (13, u32::MAX, 7),
        ] {
            assert_eq!(nzbget_rs_tls_cipher_pin(protocol, bits, flags), 0);
        }
    }
    #[test]
    fn invalid_utf8_status_id_is_an_error_instead_of_current_job() {
        let bytes = [255u8, 0];
        let response = unsafe { nzbget_rs_tls_cipher_status(bytes.as_ptr().cast()) };
        let text = unsafe { std::slice::from_raw_parts(response.data.cast::<u8>(), response.len) };
        assert!(text.starts_with(b"{\"Error\":"));
        unsafe {
            nzbget_rs_free(response);
        }
    }
}

#[repr(C)]
pub struct RsBuf {
    pub data: *mut c_char,
    pub len: usize,
    pub(crate) cap: usize,
}

pub(crate) fn into_buf(mut v: Vec<u8>) -> RsBuf {
    v.push(0);
    let len = v.len() - 1;
    let mut v = std::mem::ManuallyDrop::new(v);
    RsBuf {
        data: v.as_mut_ptr() as *mut c_char,
        len,
        cap: v.capacity(),
    }
}

pub(crate) unsafe fn input<'a>(raw: *const c_char) -> &'a [u8] {
    if raw.is_null() {
        &[]
    } else {
        CStr::from_ptr(raw).to_bytes()
    }
}

// ---- twins and alts (twin.rs); lists cross the ABI as text lines ----

/// A file system path from C bytes, kept byte for byte (paths needn't be UTF-8).
unsafe fn path_arg(raw: *const c_char) -> std::path::PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(input(raw)))
    }
    #[cfg(not(unix))]
    {
        std::path::PathBuf::from(String::from_utf8_lossy(input(raw)).into_owned())
    }
}

#[cfg(test)]
unsafe fn bytes_arg<'a>(data: *const u8, len: usize) -> &'a [u8] {
    if data.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(data, len)
    }
}

/// The fingerprint of `count` files given as md5 texts and lengths (the
/// names are not part of it).
///
/// # Safety
/// `md5s` points to `count` valid NUL-terminated strings (null entries are
/// empty) and `lengths` to `count` values; both may be null for 0.
#[cfg(test)]
unsafe fn nzbget_rs_twin_fingerprint(
    md5s: *const *const c_char,
    lengths: *const u64,
    count: usize,
) -> RsBuf {
    if count > 0 && (md5s.is_null() || lengths.is_null()) {
        return into_buf(Vec::new());
    }
    let files = (0..count).map(|i| {
        (
            String::from_utf8_lossy(input(*md5s.add(i))),
            *lengths.add(i),
        )
    });
    into_buf(crate::twin::fingerprint_cow(files).into_bytes())
}

/// # Safety
/// `buf` came from an nzbget_rs_* function and wasn't freed yet.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_free(buf: RsBuf) {
    if !buf.data.is_null() {
        drop(Vec::from_raw_parts(
            buf.data as *mut u8,
            buf.len + 1,
            buf.cap,
        ));
    }
}

/// Crc32::Append: hardware-accelerated IEEE 802.3 CRC-32 update via `crc32fast`.
///
/// # Safety
/// `block` is null with `len == 0`, or points to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_crc32_append(block: *const u8, len: u32, init: u32) -> u32 {
    if block.is_null() || len == 0 {
        return init;
    }
    let slice = std::slice::from_raw_parts(block, len as usize);
    crate::crc::crc32_append(slice, init)
}

/// Runs an in-place text helper on the NUL-terminated `raw` and puts the NUL
/// at the new end.
unsafe fn in_place(raw: *mut c_char, f: impl FnOnce(&mut [u8]) -> usize) {
    if raw.is_null() {
        return;
    }
    let len = CStr::from_ptr(raw).to_bytes().len();
    let n = f(std::slice::from_raw_parts_mut(raw.cast::<u8>(), len));
    *raw.add(n) = 0;
}

/// Util::FormatSize; free the result with nzbget_rs_free.
#[no_mangle]
pub extern "C" fn nzbget_rs_format_size(size: i64) -> RsBuf {
    into_buf(crate::util::format_size(size))
}

/// Util::MatchFileExt.
///
/// # Safety
/// The strings are null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_match_file_ext(
    filename: *const c_char,
    list: *const c_char,
    separators: *const c_char,
) -> c_int {
    c_int::from(crate::util::match_file_ext_locale(
        input(filename),
        input(list),
        input(separators),
    ))
}

unsafe fn bytes<'a>(p: *const c_char, len: usize) -> &'a [u8] {
    if p.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(p.cast(), len)
    }
}

/// Deobfuscation::IsExcessivelyObfuscated (rust/src/deobfuscation.rs).
///
/// # Safety
/// `s` is null or readable for `len` bytes.
#[cfg(test)]
pub(super) unsafe extern "C" fn nzbget_rs_is_excessively_obfuscated(
    s: *const c_char,
    len: usize,
) -> c_int {
    crate::deobfuscation::is_excessively_obfuscated(bytes(s, len)) as c_int
}

/// FileTypes' name checks (rust/src/filetypes.rs), by number (see nzbget_rs.h).
///
/// # Safety
/// `s` is null or readable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_file_type(which: c_int, s: *const c_char, len: usize) -> c_int {
    use crate::filetypes::*;
    const CHECKS: [fn(&[u8]) -> bool; 26] = [
        is_seven_zip_ext,
        is_rar_ext,
        is_rar_volume_ext,
        is_numeric_volume_ext,
        is_all_digits_ext,
        is_archive_ext,
        is_disc_structure_ext,
        is_disc_structure_dir,
        is_disc_descriptor_ext,
        is_disc_image_ext,
        is_generic_disc_image_ext,
        is_clutter_dir,
        is_clutter_file,
        is_parity_ext,
        is_video_ext,
        is_audio_ext,
        is_subtitle_ext,
        is_nfo_ext,
        is_book_ext,
        is_image_ext,
        is_sample_stem,
        is_seven_zip_file,
        is_rar_file,
        is_archive_file,
        is_sample_file,
        is_known_collection_ext,
    ];
    match usize::try_from(which).ok().and_then(|w| CHECKS.get(w)) {
        Some(check) => check(bytes(s, len)) as c_int,
        None => 0,
    }
}

/// FileSystem's path texts (rust/src/paths.rs): 0 MakeValidFilename (`flag`:
/// allow slashes), 1 SanitizePathSegment, 2 SanitizeRelativePath,
/// 3 EscapePathForShell; free the result with nzbget_rs_free.
///
/// # Safety
/// `s` is null or readable for `len` bytes within one allocation, with
/// `len <= isize::MAX`. Null means empty regardless of `len`. Panics abort
/// rather than unwinding across the ABI.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_path_text(
    op: c_int,
    s: *const c_char,
    len: usize,
    flag: c_int,
) -> RsBuf {
    let b = bytes(s, len);
    into_buf(match op {
        0 => crate::paths::make_valid_filename(b, flag != 0),
        1 => crate::paths::sanitize_path_segment(b),
        2 => crate::paths::sanitize_relative_path(b),
        3 => crate::paths::escape_path_for_shell(b),
        _ => Vec::new(),
    })
}

/// FileSystem's path positions (rust/src/paths.rs): 0 BaseFileName (where
/// the name starts), 1 SplitPathAndFilename (the last '/' or '\\', or
/// SIZE_MAX), 2 ExtractFilePathFromCmd (the length of the path).
///
/// # Safety
/// `s` is null or readable for `len` bytes within one allocation, with
/// `len <= isize::MAX`. Null means empty regardless of `len`.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_path_position(op: c_int, s: *const c_char, len: usize) -> usize {
    let b = bytes(s, len);
    match op {
        0 => crate::paths::base_file_name(b),
        1 => {
            let (path, name) = crate::paths::split_path_and_filename(b);
            if name.is_empty() && path.len() == b.len() {
                usize::MAX
            } else {
                path.len()
            }
        }
        2 => crate::paths::extract_file_path_from_cmd(b).len(),
        _ => 0,
    }
}

/// FileSystem::NormalizePathSeparators, in place.
///
/// # Safety
/// `path` is null or a writable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_normalize_path_separators(path: *mut c_char) {
    in_place(path, |b| {
        crate::paths::normalize_path_separators(b);
        b.len()
    })
}

/// Decoder (rust/src/decoder.rs) with built-in Rust `nntmux-yenc` + `crc32fast` (or custom callbacks).
#[no_mangle]
pub extern "C" fn nzbget_rs_decoder_new(
    decode: Option<crate::decoder::DecodeFn>,
    crc: Option<crate::decoder::CrcFn>,
) -> *mut crate::decoder::Decoder {
    let (decode, crc) = match (decode, crc) {
        (Some(d), Some(c)) => (d, c),
        (None, None) => (
            crate::decoder::rust_yenc_decode_incremental as crate::decoder::DecodeFn,
            crate::decoder::rust_yenc_crc as crate::decoder::CrcFn,
        ),
        _ => return std::ptr::null_mut(),
    };
    Box::into_raw(Box::new(crate::decoder::Decoder::new(decode, crc)))
}

/// # Safety
/// `d` is null or from nzbget_rs_decoder_new, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_decoder_free(d: *mut crate::decoder::Decoder) {
    if !d.is_null() {
        drop(Box::from_raw(d));
    }
}

/// Decoder::Clear.
///
/// # Safety
/// `d` is null or a live, exclusively borrowed handle from nzbget_rs_decoder_new.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_decoder_clear(d: *mut crate::decoder::Decoder) {
    if let Some(d) = d.as_mut() {
        d.clear();
    }
}

/// Decoder::DecodeBuffer.
///
/// # Safety
/// `d` is null or a live, exclusively borrowed handle from nzbget_rs_decoder_new; `buffer` is writable as the C++
/// Decoder needed (see Decoder::decode_buffer), disjoint from `d`. NULL
/// input or a negative length is a no-op. Panics abort at this C ABI boundary.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_decoder_decode(
    d: *mut crate::decoder::Decoder,
    buffer: *mut c_char,
    len: c_int,
) -> c_int {
    if d.is_null() || buffer.is_null() || len < 0 {
        return 0;
    }
    (*d).decode_buffer(buffer.cast(), len as usize) as c_int
}

/// Decoder::Check: the EStatus.
///
/// # Safety
/// `d` is null or a live, exclusively borrowed handle from nzbget_rs_decoder_new.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_decoder_check(d: *mut crate::decoder::Decoder) -> c_int {
    d.as_mut()
        .map_or(crate::decoder::Status::UnknownError, |d| d.check()) as c_int
}

/// The decoder's settings and fields: `which` 0 crc check, 1 raw mode
/// (set to `value`).
///
/// # Safety
/// `d` is null or a live, exclusively borrowed handle from nzbget_rs_decoder_new.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_decoder_set(
    d: *mut crate::decoder::Decoder,
    which: c_int,
    value: c_int,
) {
    let Some(d) = d.as_mut() else { return };
    match which {
        0 => d.crc_check = value != 0,
        1 => d.raw_mode = value != 0,
        _ => {}
    }
}

/// `which`: 0 format, 1 begin, 2 end, 3 size, 4 expected CRC, 5 calculated
/// CRC, 6 EOF.
///
/// # Safety
/// `d` is null or a live, exclusively borrowed handle from nzbget_rs_decoder_new.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_decoder_get(
    d: *mut crate::decoder::Decoder,
    which: c_int,
) -> i64 {
    let Some(d) = d.as_ref() else { return 0 };
    match which {
        0 => d.format as i64,
        1 => d.begin_pos,
        2 => d.end_pos,
        3 => d.size,
        4 => d.expected_crc as i64,
        5 => d.calculated_crc as i64,
        6 => d.eof as i64,
        _ => 0,
    }
}

/// GetArticleFilename: valid until the decoder changes.
///
/// # Safety
/// `d` is null or a live, exclusively borrowed handle from nzbget_rs_decoder_new.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_decoder_filename(
    d: *mut crate::decoder::Decoder,
) -> *const c_char {
    d.as_mut().map_or(c"".as_ptr(), |d| d.filename_c().cast())
}

/// Scheduler::CheckTasks' timing: updates the tasks' last runs and
/// `*last_check`, writes the indexes of the tasks to run (in order) to `due`
/// and returns their count; `*reset` is set to whether the clock jumped.
/// Missing required pointers or unrepresentable buffer sizes return zero
/// without changing any output. With zero tasks, `tasks` may be null.
/// If the return value exceeds `due_capacity`, no outputs are changed: retry
/// with a buffer of that size. NULL `due` is allowed with zero capacity.
///
/// # Safety
/// Non-null `tasks` points to `count` tasks, `due` to `due_capacity` writable
/// entries, and `last_check` and `reset` to writable values. These buffers
/// must be aligned and disjoint, and remain caller-owned. Panics abort
/// rather than unwinding across the ABI.
/// `gmtime`, when non-null, must initialize the supplied calendar fields as
/// libc's gmtime_r does for the given time, and must not unwind. NULL is rejected.
#[cfg(test)]
pub(super) unsafe extern "C" fn nzbget_rs_scheduler_check(
    tasks: *mut crate::scheduler::Task,
    count: usize,
    last_check: *mut i64,
    current: i64,
    current_offset: i64,
    last_check_offset: i64,
    due: *mut usize,
    due_capacity: usize,
    reset: *mut c_int,
    gmtime: Option<unsafe extern "C" fn(i64, *mut crate::scheduler::Tm)>,
) -> usize {
    let Some(gmtime) = gmtime else { return 0 };
    if last_check.is_null()
        || reset.is_null()
        || (count != 0 && tasks.is_null())
        || (due_capacity != 0 && due.is_null())
        || count > isize::MAX as usize / std::mem::size_of::<crate::scheduler::Task>()
        || due_capacity > isize::MAX as usize / std::mem::size_of::<usize>()
    {
        return 0;
    }
    let tasks: &mut [crate::scheduler::Task] = if count == 0 {
        &mut []
    } else {
        std::slice::from_raw_parts_mut(tasks, count)
    };
    // Keep the retry transactional, including each task's last execution time.
    let mut updated_tasks = tasks.to_vec();
    let mut updated_last_check = *last_check;
    let r = crate::scheduler::check_tasks(
        &mut updated_tasks,
        &mut updated_last_check,
        current,
        current_offset,
        last_check_offset,
        |time| {
            let mut fields = std::mem::MaybeUninit::uninit();
            gmtime(time, fields.as_mut_ptr());
            fields.assume_init()
        },
    );
    let n = r.due.len();
    if n > due_capacity {
        return n;
    }
    tasks.copy_from_slice(&updated_tasks);
    *last_check = updated_last_check;
    *reset = r.reset as c_int;
    if n > 0 {
        std::ptr::copy_nonoverlapping(r.due.as_ptr(), due, n);
    }
    n
}

/// `len` bytes at `data` (empty when null or zero).
unsafe fn span<'a>(data: *const c_char, len: usize) -> &'a [u8] {
    if data.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(data.cast::<u8>(), len)
    }
}

/// Util::SplitCommandLine: the words, each NUL-terminated, one after the
/// other (`len` covers them all); free with nzbget_rs_free.
///
/// # Safety
/// `s` is null or NUL-terminated.
#[cfg(test)]
pub(super) unsafe extern "C" fn nzbget_rs_split_command_line(s: *const c_char) -> RsBuf {
    let mut v = Vec::new();
    for word in crate::util::split_command_line(input(s)) {
        v.extend_from_slice(&word);
        v.push(0);
    }
    into_buf(v)
}

/// ServerVolume::CalcSlots: the slots of a local time; updates `*first_day`.
///
/// # Safety
/// `first_day` and `slots` are null (a no-op) or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_volume_calc_slots(
    loc_cur_time: i64,
    first_day: *mut c_int,
    slots: *mut crate::statmeter::Slots,
) {
    if first_day.is_null() || slots.is_null() {
        return;
    }
    *slots = crate::statmeter::calc_slots(loc_cur_time, &mut *first_day);
}

/// ServerVolume::AddStats for the second, minute and hour counters: clears
/// the slots passed since `loc_data_time` and adds `bytes` at `slots`. Slots
/// outside an array are skipped.
///
/// # Safety
/// Each array is null (ignored regardless of length) or aligned and writable
/// for its length, with a byte size at most `isize::MAX`; the arrays are
/// disjoint. `slots` is null (a no-op) or aligned and readable. It is copied
/// before any array writes, so it may overlap an array. All storage remains
/// caller-owned; no buffers are allocated or freed here.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_volume_add(
    seconds: *mut i64,
    seconds_len: usize,
    minutes: *mut i64,
    minutes_len: usize,
    hours: *mut i64,
    hours_len: usize,
    slots: *const crate::statmeter::Slots,
    last_min_slot: c_int,
    last_hour_slot: c_int,
    loc_cur_time: i64,
    loc_data_time: i64,
    bytes: i64,
) {
    unsafe fn array<'a>(p: *mut i64, len: usize) -> &'a mut [i64] {
        if p.is_null() || len == 0 {
            &mut []
        } else {
            std::slice::from_raw_parts_mut(p, len)
        }
    }
    let Some(slots) = slots.as_ref().copied() else {
        return;
    };
    crate::statmeter::add_stats(
        array(seconds, seconds_len),
        array(minutes, minutes_len),
        array(hours, hours_len),
        &slots,
        last_min_slot,
        last_hour_slot,
        loc_cur_time,
        loc_data_time,
        bytes,
    );
}

/// Options::ValidateOptionName: 0 invalid, 1 valid, 2 obsolete (warn), 3 an
/// obsolete script option (an error with a value), 4 an obsolete log option.
/// `predefined(ctx, name)` is GetOption's answer, asked after the read-only check.
///
/// # Safety
/// `name` is null (invalid) or NUL-terminated; `predefined` must not unwind or
/// mutate/invalidate `name`. `ctx` must satisfy the callback's requirements.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_validate_option_name(
    name: *const c_char,
    predefined: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    ctx: *mut std::ffi::c_void,
) -> c_int {
    if name.is_null() {
        return 0;
    }
    let mut ask = || predefined.is_some_and(|f| f(ctx, name) != 0);
    crate::options::validate_option_name(CStr::from_ptr(name), &mut ask) as c_int
}

/// Options::OptEntry::Restricted: 1 when the option's value is protected.
///
/// # Safety
/// `name` is null (not protected), or points to a readable NUL-terminated
/// string in one allocation, unchanged for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_option_restricted(
    name: *const c_char,
    signed_char: c_int,
) -> c_int {
    (!name.is_null() && crate::options::option_restricted(CStr::from_ptr(name), signed_char != 0))
        as c_int
}

/// NzbgetRsNntpIo: the C++ connection for the NNTP exchange. Each callback
/// returns 0, or -1 when it threw (the exchange stops; C++ rethrows).
#[repr(C)]
pub struct NntpIo {
    pub ctx: *mut std::ffi::c_void,
    pub write_line: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub read_line: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut *mut c_char) -> c_int>,
    pub report_error:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *const c_char) -> c_int>,
    pub debug: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub cancelled: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut c_int) -> c_int>,
    pub user: *const c_char,
    pub password: *const c_char,
    pub name: *const c_char,
    pub host: *const c_char,
    pub conn_host: *const c_char,
}

/// NntpIo as the exchange's Io (checked: callbacks set, strings read as empty when null).
struct FfiNntp<'a> {
    io: &'a NntpIo,
}

impl FfiNntp<'_> {
    unsafe fn text<'a>(p: *const c_char) -> &'a CStr {
        // the C++ strings outlive the exchange
        if p.is_null() {
            c""
        } else {
            unsafe { CStr::from_ptr(p) }
        }
    }
}

fn nntp_status(r: c_int) -> Result<(), crate::nntp::Abort> {
    if r == 0 {
        Ok(())
    } else {
        Err(crate::nntp::Abort)
    }
}

// The caller of the NNTP exports guarantees the Io buffer/callback contract.
unsafe impl crate::nntp::Io for FfiNntp<'_> {
    fn write_line(&mut self, line: &CStr) -> Result<(), crate::nntp::Abort> {
        let f = self.io.write_line.ok_or(crate::nntp::Abort)?;
        nntp_status(unsafe { f(self.io.ctx, line.as_ptr()) })
    }
    fn read_line(&mut self) -> Result<*mut c_char, crate::nntp::Abort> {
        let f = self.io.read_line.ok_or(crate::nntp::Abort)?;
        let mut line = std::ptr::null_mut();
        nntp_status(unsafe { f(self.io.ctx, &mut line) })?;
        Ok(line)
    }
    fn report_error(&mut self, prefix: &CStr, arg: &CStr) -> Result<(), crate::nntp::Abort> {
        let f = self.io.report_error.ok_or(crate::nntp::Abort)?;
        nntp_status(unsafe { f(self.io.ctx, prefix.as_ptr(), arg.as_ptr()) })
    }
    fn debug(&mut self, msg: &CStr) -> Result<(), crate::nntp::Abort> {
        match self.io.debug {
            Some(f) => nntp_status(unsafe { f(self.io.ctx, msg.as_ptr()) }),
            None => Ok(()),
        }
    }
    fn debug_enabled(&self) -> bool {
        self.io.debug.is_some()
    }
    fn cancelled(&mut self) -> Result<bool, crate::nntp::Abort> {
        let f = self.io.cancelled.ok_or(crate::nntp::Abort)?;
        let mut c = 0;
        nntp_status(unsafe { f(self.io.ctx, &mut c) })?;
        Ok(c != 0)
    }
    fn user(&self) -> &CStr {
        unsafe { Self::text(self.io.user) }
    }
    fn password(&self) -> &CStr {
        unsafe { Self::text(self.io.password) }
    }
    fn name(&self) -> &CStr {
        unsafe { Self::text(self.io.name) }
    }
    fn host(&self) -> &CStr {
        unsafe { Self::text(self.io.host) }
    }
    fn conn_host(&self) -> &CStr {
        unsafe { Self::text(self.io.conn_host) }
    }
}

/// Runs an exchange with the login state in `auth_error`/`auth_rejected`
/// (read and written back, also when a callback threw): 0, or -1 aborted.
unsafe fn nntp_run<T>(
    io: *const NntpIo,
    auth_error: *mut c_int,
    auth_rejected: *mut c_int,
    f: impl FnOnce(&mut dyn crate::nntp::Io, &mut crate::nntp::Auth) -> Result<T, crate::nntp::Abort>,
    out: impl FnOnce(T),
) -> c_int {
    let Some(io) = io.as_ref() else { return -1 };
    if auth_error.is_null() || auth_rejected.is_null() {
        return -1;
    }
    let mut auth = crate::nntp::Auth {
        error: *auth_error != 0,
        rejected: *auth_rejected != 0,
    };
    let r = f(&mut FfiNntp { io }, &mut auth);
    *auth_error = auth.error as c_int;
    *auth_rejected = auth.rejected as c_int;
    match r {
        Ok(v) => {
            out(v);
            0
        }
        Err(_) => -1,
    }
}

/// NntpConnection::Request: `*answer` is the C++ line buffer or null.
///
/// # Safety
/// `io` is null or valid for the exchange, with callbacks that don't unwind
/// and satisfy `nntp::Io`'s safety contract. Its configuration strings are null
/// or NUL-terminated and immutable for the exchange. `req` is null or remains
/// NUL-terminated (it may alias the line buffer). Non-null outputs are writable,
/// mutually disjoint and do not overlap `io` or strings. No pointers are retained.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nntp_request(
    io: *const NntpIo,
    req: *const c_char,
    auth_error: *mut c_int,
    auth_rejected: *mut c_int,
    answer: *mut *mut c_char,
) -> c_int {
    if req.is_null() || answer.is_null() {
        return -1;
    }
    nntp_run(
        io,
        auth_error,
        auth_rejected,
        |io, auth| crate::nntp::request(io, auth, req),
        |a| *answer = a,
    )
}

/// NntpConnection::Connect after the socket connected: `*result` 0 connected,
/// 1 the greeting failed (disconnect), 2 the login failed.
///
/// # Safety
/// As nzbget_rs_nntp_request.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nntp_handshake(
    io: *const NntpIo,
    auth_error: *mut c_int,
    auth_rejected: *mut c_int,
    result: *mut c_int,
) -> c_int {
    if result.is_null() {
        return -1;
    }
    nntp_run(io, auth_error, auth_rejected, crate::nntp::handshake, |r| {
        *result = r
    })
}

/// NntpConnection::JoinGroup when not in `group`: the answer (or null) and
/// whether the group changed.
///
/// # Safety
/// As nzbget_rs_nntp_request.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nntp_join_group(
    io: *const NntpIo,
    group: *const c_char,
    auth_error: *mut c_int,
    auth_rejected: *mut c_int,
    answer: *mut *mut c_char,
    joined: *mut c_int,
) -> c_int {
    if group.is_null() || answer.is_null() || joined.is_null() {
        return -1;
    }
    nntp_run(
        io,
        auth_error,
        auth_rejected,
        |io, auth| crate::nntp::join_group(io, auth, group),
        |(a, j)| {
            *answer = a;
            *joined = j as c_int;
        },
    )
}

#[repr(C)]
pub struct NzbgetRsCmdline(crate::cmdline::CmdlineHandle);

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cmdline_new(
    argc: c_int,
    argv: *const *const c_char,
    write_stdout_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char, usize)>,
    ctx: *mut c_void,
) -> *mut NzbgetRsCmdline {
    match crate::cmdline::CmdlineHandle::parse_argv(argc, argv, |bytes| {
        if let Some(cb) = write_stdout_cb {
            if !bytes.is_empty() {
                cb(ctx, bytes.as_ptr().cast(), bytes.len());
            }
        }
    }) {
        Some(h) => Box::into_raw(Box::new(NzbgetRsCmdline(h))),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cmdline_free(cmd: *mut NzbgetRsCmdline) {
    if !cmd.is_null() {
        drop(Box::from_raw(cmd));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cmdline_get_flag(
    cmd: *const NzbgetRsCmdline,
    field: c_int,
) -> c_int {
    match cmd.as_ref() {
        Some(c) => c.0.get_flag(field),
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cmdline_get_str(
    cmd: *const NzbgetRsCmdline,
    field: c_int,
) -> *const c_char {
    match cmd.as_ref() {
        Some(c) => c.0.get_cstr_ptr(field),
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cmdline_run_client(
    cmd: *const NzbgetRsCmdline,
    control_ip: *const c_char,
    control_port: c_int,
    control_username: *const c_char,
    control_password: *const c_char,
    write_stdout_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char, usize)>,
    write_perror_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    ctx: *mut c_void,
) -> c_int {
    #[cfg(unix)]
    {
        let _cancel_guard = PosixCancelGuard::disable();
        let Some(c) = cmd.as_ref() else {
            return 0;
        };
        let ok = c.0.execute_client(
            input(control_ip),
            control_port,
            input(control_username),
            input(control_password),
            |bytes| {
                if let Some(cb) = write_stdout_cb {
                    if !bytes.is_empty() {
                        cb(ctx, bytes.as_ptr().cast(), bytes.len());
                    }
                }
            },
            |cmsg| {
                if let Some(cb) = write_perror_cb {
                    cb(ctx, cmsg.as_ptr());
                }
            },
        );
        return c_int::from(ok);
    }
    #[cfg(not(unix))]
    {
        let _ = (
            cmd,
            control_ip,
            control_port,
            control_username,
            control_password,
            write_stdout_cb,
            write_perror_cb,
            ctx,
        );
        0
    }
}

/// FileSystem::GetLastErrorMessage's text for `errnum`, into `buf` (1024
/// bytes, NUL-terminated).
///
/// # Safety
/// `buf` is null (a no-op) or writable for 1024 bytes.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_error_message(errnum: c_int, buf: *mut c_char) {
    if buf.is_null() {
        return;
    }
    let msg = crate::fsops::error_message(errnum);
    std::ptr::copy_nonoverlapping(msg.as_ptr().cast(), buf, msg.len());
    *buf.add(msg.len()) = 0;
}

/// FileSystem::FileExists (0 for null).
///
/// # Safety
/// `path` is null or NUL-terminated.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_file_exists(path: *const c_char) -> c_int {
    (!path.is_null() && crate::fsops::file_exists(input(path), &crate::fsops::posix_stat)) as c_int
}

/// FileSystem::DirectoryExists (0 for null).
///
/// # Safety
/// `path` is null or NUL-terminated.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_directory_exists(path: *const c_char) -> c_int {
    (!path.is_null() && crate::fsops::directory_exists(input(path), &crate::fsops::posix_stat))
        as c_int
}

/// FileSystem::ForceDirectories: 1 created or there, else 0 with the message
/// in `*errmsg` (free with nzbget_rs_free; empty on success).
///
/// # Safety
/// `path` is null or NUL-terminated; `errmsg` is null or writable and disjoint
/// from the input. Every non-null output is initialized, including on failure.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_force_directories(
    path: *const c_char,
    errmsg: *mut RsBuf,
) -> c_int {
    if errmsg.is_null() {
        return 0;
    }
    if path.is_null() {
        errmsg.write(into_buf(Vec::new()));
        return 0;
    }
    let mut msg = Vec::new();
    let ok = crate::fsops::force_directories(input(path), &mut msg, &crate::fsops::posix_stat);
    errmsg.write(into_buf(msg));
    ok as c_int
}

// Copy exactly the bounded input BString<1024>::Set read. CStr::from_ptr on
// the original pointer would scan beyond that bound, possibly off its allocation.
unsafe fn par_filename_input(name: *const c_char) -> [u8; crate::parparser::BSTRING_MAX + 1] {
    let mut copy = [0; crate::parparser::BSTRING_MAX + 1];
    for (i, byte) in copy[..crate::parparser::BSTRING_MAX].iter_mut().enumerate() {
        *byte = name.add(i).read() as u8;
        if *byte == 0 {
            break;
        }
    }
    copy
}

/// ParParser::ParseParFilename: 1 for a par2 name, else 0. Optional outputs
/// are written only on success, base_len then blocks, even if they alias.
///
/// # Safety
/// `name` is null or readable through the first NUL or 1023 bytes, whichever
/// comes first. Outputs are null or aligned writable C ints. All storage stays
/// caller-owned.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_parse_par_filename(
    name: *const c_char,
    confirmed: c_int,
    base_len: *mut c_int,
    blocks: *mut c_int,
) -> c_int {
    if name.is_null() {
        return 0;
    }
    let name = par_filename_input(name);
    match crate::parparser::parse_par_filename(&name, confirmed != 0) {
        Some((len, b)) => {
            if !base_len.is_null() {
                base_len.write(len as c_int);
            }
            if !blocks.is_null() {
                blocks.write(b);
            }
            1
        }
        None => 0,
    }
}

/// ParParser::SameParCollection (0 for null names).
///
/// # Safety
/// Each name is null or readable through its first NUL or 1023 bytes. Inputs stay borrowed.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_same_par_collection(
    name1: *const c_char,
    name2: *const c_char,
    confirmed: c_int,
) -> c_int {
    if name1.is_null() || name2.is_null() {
        return 0;
    }
    let name1 = par_filename_input(name1);
    let name2 = par_filename_input(name2);
    crate::parparser::same_par_collection(
        CStr::from_ptr(name1.as_ptr().cast()).to_bytes(),
        CStr::from_ptr(name2.as_ptr().cast()).to_bytes(),
        confirmed != 0,
    ) as c_int
}

/// ParParser::FindMainPars: scans `path` for confirmed `.par2` files,
/// invoking `add_file(ctx, name, len)` for each unique collection when
/// `collect != 0`.
///
/// # Safety
/// `path` is null or a valid NUL-terminated string; `add_file`, if supplied, must not unwind.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_find_main_pars(
    path: *const c_char,
    collect: c_int,
    add_file: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, usize)>,
    ctx: *mut std::ffi::c_void,
) -> c_int {
    if path.is_null() {
        return 0;
    }
    let dir_path = path_arg(path);
    i32::from(crate::parparser::find_main_pars(
        &dir_path,
        collect != 0,
        |name| {
            if let Some(cb) = add_file {
                cb(ctx, name.as_ptr().cast(), name.len());
            }
        },
    ))
}

fn c_string(b: &[u8]) -> std::ffi::CString {
    // the names come from C strings (no NUL inside)
    std::ffi::CString::new(b).unwrap_or_default()
}

/// NzbgetRsRarRenamerRunEnv
#[repr(C)]
pub struct RarRenamerRunEnv {
    pub ctx: *mut std::ffi::c_void,
    pub stopped: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    pub progress:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, c_int) -> c_int>,
    pub message: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char) -> c_int>,
    pub register_renamed:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *const c_char) -> c_int>,
}

struct FfiRarRenamerRun<'a>(&'a RarRenamerRunEnv);

impl crate::rarrenamer::RunCallbacks for FfiRarRenamerRun<'_> {
    fn stopped(&mut self) -> Result<bool, ()> {
        let Some(f) = self.0.stopped else {
            return Ok(false);
        };
        match unsafe { f(self.0.ctx) } {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(()),
        }
    }
    fn progress(&mut self, label: &[u8], stage_progress: i32) -> Result<(), ()> {
        let Some(f) = self.0.progress else {
            return Ok(());
        };
        let l = c_string(label);
        if unsafe { f(self.0.ctx, l.as_ptr(), stage_progress) } == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
    fn message(&mut self, kind: i32, text: &[u8]) -> Result<(), ()> {
        let Some(f) = self.0.message else {
            return Ok(());
        };
        let t = c_string(text);
        if unsafe { f(self.0.ctx, kind, t.as_ptr()) } == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
    fn register_renamed(&mut self, old_base: &[u8], new_base: &[u8]) -> Result<(), ()> {
        let Some(f) = self.0.register_renamed else {
            return Ok(());
        };
        let o = c_string(old_base);
        let n = c_string(new_base);
        if unsafe { f(self.0.ctx, o.as_ptr(), n.as_ptr()) } == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}

/// RarRenamer::Execute (`rust/src/rarrenamer.rs`): recursively scans `dest_dir`,
/// parses RAR3/RAR5 volumes, and renames misnamed archive sets. Returns 0 on
/// completion or -1 if a callback returned nonzero (exception).
///
/// # Safety
/// `env` and `renamed_count` must be valid pointers; string arguments are null or
/// NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rar_renamer_execute(
    env: *const RarRenamerRunEnv,
    dest_dir: *const c_char,
    info_name: *const c_char,
    password: *const c_char,
    ignore_ext: *const c_char,
    tls_enabled: c_int,
    renamed_count: *mut c_int,
) -> c_int {
    let (Some(env), Some(renamed_count)) = (env.as_ref(), renamed_count.as_mut()) else {
        return -1;
    };
    let info = if info_name.is_null() {
        &b"(null)"[..]
    } else {
        input(info_name)
    };
    let res = crate::rarrenamer::execute_all(
        input(dest_dir),
        info,
        input(password),
        input(ignore_ext),
        tls_enabled != 0,
        renamed_count,
        &mut FfiRarRenamerRun(env),
    );
    if res.is_ok() {
        0
    } else {
        -1
    }
}

/// DirectRenamer's RenameContentAnalyzer: a new analyzer (free with
/// nzbget_rs_rename_analyzer_free).
#[no_mangle]
pub extern "C" fn nzbget_rs_rename_analyzer_new() -> *mut crate::renameanalyzer::Analyzer {
    Box::into_raw(Box::default())
}

/// Frees an analyzer.
///
/// # Safety
/// `a` is null or an analyzer not yet freed.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rename_analyzer_free(a: *mut crate::renameanalyzer::Analyzer) {
    if !a.is_null() {
        drop(Box::from_raw(a));
    }
}

/// RenameContentAnalyzer::Reset
///
/// # Safety
/// `a` is null or a live analyzer.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rename_analyzer_reset(a: *mut crate::renameanalyzer::Analyzer) {
    if let Some(a) = a.as_mut() {
        a.reset();
    }
}

/// RenameContentAnalyzer::Append: 1 when a PAR2 packet header was recognized,
/// its set id then in `set_id` (32 hex digits and a NUL).
///
/// # Safety
/// `a` is null or a live, exclusively accessed analyzer. For positive `len`,
/// a non-null `buf` is readable for `len` bytes and does not overlap `a`.
/// `set_id` is null or writable for 33 bytes and does not overlap `a` or `buf`.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rename_analyzer_append(
    a: *mut crate::renameanalyzer::Analyzer,
    buf: *const c_char,
    len: c_int,
    set_id: *mut c_char,
) -> c_int {
    let Some(a) = a.as_mut() else { return 0 };
    if len < 0 || (buf.is_null() && len > 0) {
        return 0;
    }
    let matched = a.append(span(buf, len as usize));
    if matched && !set_id.is_null() {
        if let Some(id) = &a.par_set_id {
            std::ptr::copy_nonoverlapping(id.as_ptr().cast(), set_id, id.len());
        }
    }
    matched as c_int
}

/// RenameContentAnalyzer::Finish: the MD5 of the first 16 KiB into `hash`
/// (32 hex digits and a NUL).
///
/// # Safety
/// `a` is null or a live, exclusively accessed analyzer; `hash` is null or
/// writable for 33 bytes and does not overlap `a`.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rename_analyzer_finish(
    a: *mut crate::renameanalyzer::Analyzer,
    hash: *mut c_char,
) {
    let Some(a) = a.as_mut() else { return };
    if hash.is_null() {
        return;
    }
    let h = a.finish();
    std::ptr::copy_nonoverlapping(h.as_ptr().cast(), hash, h.len());
}

/// Reads the par2 hash at an index of DirectRenamer's FileHashList.
pub type ParHashAt =
    unsafe extern "C" fn(*mut std::ffi::c_void, usize, *mut *const c_char, *mut *const c_char);

/// DirectRenamer::BuildNewRegularName: the index of the par2 hash whose file
/// name the file gets, or -1 to keep its name.
///
/// # Safety
/// `old_name` and `hash16k` are null (empty) or NUL-terminated. `at` writes,
/// for every index below `count`, a filename and hash that are null (empty)
/// or NUL-terminated and stay valid until this returns; it must not unwind.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_direct_regular_name(
    old_name: *const c_char,
    hash16k: *const c_char,
    count: usize,
    at: Option<ParHashAt>,
    ctx: *mut std::ffi::c_void,
) -> isize {
    let as_c = |p: *const c_char| if p.is_null() { c"" } else { CStr::from_ptr(p) };
    let Some(at) = at else { return -1 };
    let hashes = (0..count).map(|i| {
        let (mut filename, mut hash) = (std::ptr::null(), std::ptr::null());
        at(ctx, i, &mut filename, &mut hash);
        (as_c(filename), as_c(hash))
    });
    crate::directrenamer::regular_name(as_c(old_name), as_c(hash16k), hashes)
        .map_or(-1, |i| i as isize)
}

/// DirectRenamer::BuildNewParName; free the name with nzbget_rs_free.
///
/// # Safety
/// The names are null (empty) or NUL-terminated; `vol` is null (counting
/// from 1, not written back) or a valid int.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_direct_par_name(
    old_name: *const c_char,
    dest_dir: *const c_char,
    set_id: *const c_char,
    separator: c_char,
    vol: *mut c_int,
) -> RsBuf {
    let mut local = 1;
    let vol = if vol.is_null() { &mut local } else { &mut *vol };
    let name = crate::directrenamer::par_name_on_disk(
        input(old_name),
        input(dest_dir),
        input(set_id),
        separator as u8,
        vol,
    );
    into_buf(name)
}

/// NzbgetRsParName: a par2 file of a download.
#[repr(C)]
pub struct ParName {
    pub filename: *const c_char,
    pub set_id: *const c_char,
}

/// DirectRenamer::NeedRenamePars: 1 when the par2 files need new names.
///
/// # Safety
/// `pars` is null or `count` readable entries; their filenames are null or
/// NUL-terminated (null: needs renaming), set ids null (empty) or
/// NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_direct_need_rename_pars(
    pars: *const ParName,
    count: usize,
) -> c_int {
    if pars.is_null() {
        return 0;
    }
    let pars: Vec<_> = std::slice::from_raw_parts(pars, count)
        .iter()
        .map(|p| {
            (
                (!p.filename.is_null()).then(|| CStr::from_ptr(p.filename)),
                if p.set_id.is_null() {
                    c""
                } else {
                    CStr::from_ptr(p.set_id)
                },
            )
        })
        .collect();
    crate::directrenamer::need_rename_pars(&pars) as c_int
}

fn nul(s: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = s.iter().take_while(|&&b| b != 0).copied().collect();
    v.push(0);
    v
}

/// Runs `ParRenamer::Execute` end-to-end on the filesystem in Rust.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_renamer_execute(
    info_name: *const c_char,
    dest_dir: *const c_char,
    ignore_ext: *const c_char,
    detect_missing: c_int,
    separator: c_char,
    ctx: *mut std::ffi::c_void,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    update_progress_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, c_int)>,
    print_message_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
    register_parred_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
    register_renamed_cb: Option<
        unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *const c_char),
    >,
    out_renamed_count: *mut c_int,
    out_has_missed_files: *mut c_int,
    out_has_damaged_par_files: *mut c_int,
) {
    let info_bytes = input(info_name).to_vec();
    let dest_bytes = input(dest_dir).to_vec();
    let ignore_bytes = input(ignore_ext).to_vec();

    let mut is_stopped = || is_stopped_cb.is_some_and(|cb| cb(ctx) != 0);
    let mut on_progress = |label: &[u8], stage: i32| {
        if let Some(cb) = update_progress_cb {
            let c_label = nul(label);
            cb(ctx, c_label.as_ptr().cast(), stage);
        }
    };
    let mut on_message = |kind: crate::parrenamer::Kind, text: &[u8]| {
        if let Some(cb) = print_message_cb {
            let c_text = nul(text);
            cb(ctx, kind as c_int, c_text.as_ptr().cast());
        }
    };
    let mut on_register_parred = |name: &[u8]| {
        if let Some(cb) = register_parred_cb {
            let c_name = nul(name);
            cb(ctx, c_name.as_ptr().cast());
        }
    };
    let mut on_register_renamed = |old_rel: &[u8], new_rel: &[u8]| {
        if let Some(cb) = register_renamed_cb {
            let c_old = nul(old_rel);
            let c_new = nul(new_rel);
            cb(ctx, c_old.as_ptr().cast(), c_new.as_ptr().cast());
        }
    };

    let state = crate::parrenamer::execute_all(
        &info_bytes,
        &dest_bytes,
        &ignore_bytes,
        detect_missing != 0,
        separator as u8,
        &mut is_stopped,
        &mut on_progress,
        &mut on_message,
        &mut on_register_parred,
        &mut on_register_renamed,
    );

    if !out_renamed_count.is_null() {
        *out_renamed_count = state.renamed_count;
    }
    if !out_has_missed_files.is_null() {
        *out_has_missed_files = state.has_missed_files as c_int;
    }
    if !out_has_damaged_par_files.is_null() {
        *out_has_damaged_par_files = state.has_damaged_par_files as c_int;
    }
}

/// `ParRenamer::ListParredFiles`: loads description packets from `par_filename` (if at most
/// `max_index_par_size` bytes) and emits each base filename via `add_cb`.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_renamer_list_parred_files(
    par_filename: *const c_char,
    max_index_par_size: i64,
    ctx: *mut std::ffi::c_void,
    add_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) -> c_int {
    if par_filename.is_null() {
        return 0;
    }
    let path = cstr_to_path_buf(par_filename);
    let Some(files) = crate::parrenamer::list_parred_files(&path, max_index_par_size) else {
        return 0;
    };
    if let Some(cb) = add_cb {
        for f in files {
            let c_name = nul(&f);
            cb(ctx, c_name.as_ptr().cast());
        }
    }
    1
}

/// `DirectParLoader::LoadParFile`: parses `par_filename`'s PAR2 packets in Rust and emits
/// each entry (`damaged = 1` or `damaged = 0` with translated local `filename` and 16k `hash`)
/// to `entry_cb`. Returns `1` if the file was loaded, `0` if missing or empty.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_renamer_load_par_entries(
    par_filename: *const c_char,
    ctx: *mut std::ffi::c_void,
    on_loaded_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    entry_cb: Option<
        unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char, *const c_char) -> c_int,
    >,
) -> c_int {
    if par_filename.is_null() {
        return 0;
    }
    let path = cstr_to_path_buf(par_filename);
    let Some(entries) = crate::parrenamer::load_par_entries_from_file(&path) else {
        return 0;
    };
    if let Some(loaded) = on_loaded_cb {
        if loaded(ctx) == 0 {
            return 1;
        }
    }
    if let Some(cb) = entry_cb {
        for entry in entries {
            match entry {
                crate::parrenamer::ParEntry::Damaged => {
                    if cb(ctx, 1, std::ptr::null(), std::ptr::null()) == 0 {
                        break;
                    }
                }
                crate::parrenamer::ParEntry::File { name, hash } => {
                    let c_name = nul(&name);
                    let c_hash = nul(&hash);
                    if cb(ctx, 0, c_name.as_ptr().cast(), c_hash.as_ptr().cast()) == 0 {
                        break;
                    }
                }
            }
        }
    }
    1
}

/// FileSystem::DeleteDirectoryWithContent: 1 all deleted, 0 not.
/// `errmsg` (null: not written) gets the error, empty when cleared; free it
/// with nzbget_rs_free.
///
/// # Safety
/// `dir` is null or NUL-terminated; `errmsg` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_delete_directory_with_content(
    dir: *const c_char,
    separator: c_char,
    errmsg: *mut RsBuf,
) -> c_int {
    if dir.is_null() {
        crate::fsops::set_last_errno(libc::EFAULT);
        if !errmsg.is_null() {
            let last_err = crate::fsops::error_message(libc::EFAULT);
            errmsg.write(into_buf(crate::util::fs_format_null_delete_errmsg(
                &last_err,
            )));
        }
        return 0;
    }
    let mut text = Vec::new();
    let done = crate::fsdelete::delete_directory_with_content(
        &mut crate::fsops::PosixFsOps,
        input(dir),
        separator as u8,
        &mut text,
    )
    .unwrap_or(false);
    if !errmsg.is_null() {
        errmsg.write(into_buf(text));
    }
    done as c_int
}

/// FileSystem::RenameBak: 1 moved, 0 not.
/// `new_name` (null: not written) gets the name tried last; free it with
/// nzbget_rs_free.
///
/// The C library's ctype under the daemon's locale (NzbFile).
#[repr(C)]
pub struct RsCtype {
    /// isspace/isdigit classes of bytes 0..=255 (glibc's table at index 0).
    classes: *const u16,
    space: u16,
    digit: u16,
    /// tolower of -128..=255, at index 0 (glibc's table).
    lower: *const c_int,
    char_signed: c_int,
}

/// # Safety
/// `ct` is null or valid, with its tables readable over their ranges.
unsafe fn rs_ctype<'a>(ct: *const RsCtype) -> Option<crate::nzbfile::Ctype<'a>> {
    let ct = ct.as_ref()?;
    if ct.classes.is_null() || ct.lower.is_null() {
        return None;
    }
    Some(crate::nzbfile::Ctype {
        classes: std::slice::from_raw_parts(ct.classes, 256),
        space: ct.space,
        digit: ct.digit,
        lower: std::slice::from_raw_parts(ct.lower.sub(128), 384),
        char_signed: ct.char_signed != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twin_fingerprint_takes_arrays() {
        unsafe {
            let md5s = [
                b"abc\0".as_ptr().cast(),
                std::ptr::null(),
                b"abc\0".as_ptr().cast(),
            ];
            let lengths = [1u64, 7, 1];
            let result = nzbget_rs_twin_fingerprint(md5s.as_ptr(), lengths.as_ptr(), 3);
            let expected = crate::twin::fingerprint_of([("abc", 1), ("", 7)].into_iter());
            assert_eq!(input(result.data), expected.as_bytes()); // the repeated one counts once
            nzbget_rs_free(result);
            for (m, l, n) in [
                (std::ptr::null(), std::ptr::null(), 0),
                (std::ptr::null(), lengths.as_ptr(), 1),
            ] {
                let empty = nzbget_rs_twin_fingerprint(m, l, n);
                assert_eq!(input(empty.data), b"");
                nzbget_rs_free(empty);
            }
        }
    }

    #[test]
    fn fsdelete_null_inputs_initialize_outputs() {
        use std::ptr::{null, null_mut};
        // Poison rather than zero the output: a caller need not initialize
        // an out parameter, even when the operation fails.
        let poison = || RsBuf {
            data: null_mut(),
            len: usize::MAX,
            cap: usize::MAX,
        };
        unsafe {
            let mut error = poison();
            assert_eq!(
                nzbget_rs_delete_directory_with_content(null(), b'/' as c_char, &mut error),
                0
            );
            assert!(input(error.data).starts_with(b"could not delete (null): "));
            nzbget_rs_free(error);
            assert_eq!(
                nzbget_rs_delete_directory_with_content(null(), 0, null_mut()),
                0
            );
            assert!(crate::fsdelete::file_extension(b"").is_none());
            assert_eq!(crate::fsdelete::file_extension(b"a.\0b.c"), Some(&b".c"[..]));
        }
    }

    #[cfg(unix)]
    #[test]
    fn dir_browser_and_regex_ffi_roundtrip() {
        use std::ptr::null;
        unsafe {
            let null_browser = nzbget_rs_dir_browser_new(null(), 1);
            assert!(nzbget_rs_dir_browser_next(null_browser).is_null());
            nzbget_rs_dir_browser_free(null_browser);

            let dot_browser = nzbget_rs_dir_browser_new(c".".as_ptr(), 1);
            let first = nzbget_rs_dir_browser_next(dot_browser);
            assert!(!first.is_null());
            let first_bytes = CStr::from_ptr(first).to_bytes();
            assert!(!first_bytes.is_empty() && first_bytes != b"." && first_bytes != b"..");
            nzbget_rs_dir_browser_free(dot_browser);

            let rx = nzbget_rs_regex_new(c"^foo([0-9]+)$".as_ptr(), 10);
            assert_eq!(nzbget_rs_regex_is_valid(rx), 1);
            assert_eq!(nzbget_rs_regex_match(rx, c"FOO42".as_ptr()), 1);
            assert_eq!(nzbget_rs_regex_match(rx, c"bar".as_ptr()), 0);
            nzbget_rs_regex_free(rx);
        }
    }

    #[test]
    fn parrenamer_ffi_nulls() {
        use std::ptr::{null, null_mut};
        unsafe {
            let mut renamed = -1;
            let mut missed = -1;
            let mut damaged = -1;
            nzbget_rs_par_renamer_execute(
                null(),
                null(),
                null(),
                0,
                b'/' as c_char,
                null_mut(),
                None,
                None,
                None,
                None,
                None,
                &mut renamed,
                &mut missed,
                &mut damaged,
            );
            assert_eq!((renamed, missed, damaged), (0, 0, 0));
            assert_eq!(
                nzbget_rs_par_renamer_list_parred_files(null(), 1024, null_mut(), None),
                0
            );
            assert_eq!(
                nzbget_rs_par_renamer_load_par_entries(null(), null_mut(), None, None),
                0
            );
        }
    }

    #[test]
    fn rarrenamer_nulls_and_borrowed_callback_strings() {
        use std::ptr::{null, null_mut};
        unsafe extern "C" fn progress(
            ctx: *mut std::ffi::c_void,
            label: *const c_char,
            stage: c_int,
        ) -> c_int {
            let out = &mut *ctx.cast::<Vec<(Vec<u8>, c_int)>>();
            out.push((CStr::from_ptr(label).to_bytes().to_vec(), stage));
            0
        }
        let mut out: Vec<(Vec<u8>, c_int)> = Vec::new();
        let env = RarRenamerRunEnv {
            ctx: (&mut out as *mut Vec<(Vec<u8>, c_int)>).cast(),
            stopped: None,
            progress: Some(progress),
            message: None,
            register_renamed: None,
        };
        let mut renamed = 0;
        unsafe {
            assert_eq!(
                nzbget_rs_rar_renamer_execute(
                    null(),
                    null(),
                    null(),
                    null(),
                    null(),
                    1,
                    null_mut()
                ),
                -1
            );
            assert_eq!(
                nzbget_rs_rar_renamer_execute(&env, null(), null(), null(), null(), 1, null_mut()),
                -1
            );
            assert_eq!(
                nzbget_rs_rar_renamer_execute(
                    &env,
                    c"/nonexistent_dir_12345".as_ptr(),
                    c"job".as_ptr(),
                    null(),
                    null(),
                    1,
                    &mut renamed
                ),
                0
            );
            assert_eq!(renamed, 0);
            assert_eq!(out, vec![(b"Checkingrar-files for job".to_vec(), 0)]);
        }
    }

    #[test]
    fn parparser_bounded_input_and_outputs() {
        use std::ptr::{null, null_mut};
        unsafe {
            let mut output = 77;
            let p = &mut output as *mut c_int;
            for name in [null(), c"bad".as_ptr()] {
                assert_eq!(nzbget_rs_parse_par_filename(name, 1, p, p), 0);
                assert_eq!(output, 77);
            }
            assert_eq!(
                nzbget_rs_parse_par_filename(c"a.vol+2.par2".as_ptr(), -1, p, p),
                1
            );
            assert_eq!(output, -1); // blocks wins when outputs alias
            assert_eq!(
                nzbget_rs_parse_par_filename(c"a.par2".as_ptr(), 1, null_mut(), null_mut()),
                1
            );
            assert_eq!(
                nzbget_rs_same_par_collection(null(), c"a.par2".as_ptr(), 1),
                0
            );
            assert_eq!(
                nzbget_rs_same_par_collection(c"a.par2".as_ptr(), null(), 1),
                0
            );

            let mut name = [b'a'; crate::parparser::BSTRING_MAX];
            name[1018..].copy_from_slice(b".par2");
            assert_eq!(
                nzbget_rs_parse_par_filename(name.as_ptr().cast(), 1, p, null_mut()),
                1
            );
            assert_eq!(output, 1018);
            assert_eq!(
                nzbget_rs_same_par_collection(name.as_ptr().cast(), name.as_ptr().cast(), 1),
                1
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn fsops_null_outputs_and_ownership() {
        unsafe {
            let mut msg = std::mem::MaybeUninit::uninit();
            assert_eq!(
                nzbget_rs_force_directories(std::ptr::null(), msg.as_mut_ptr()),
                0
            );
            let msg = msg.assume_init();
            assert_eq!(msg.len, 0);
            assert_eq!(CStr::from_ptr(msg.data), c"");
            nzbget_rs_free(msg);
            assert_eq!(
                nzbget_rs_force_directories(c".".as_ptr(), std::ptr::null_mut()),
                0
            );
            assert_eq!(nzbget_rs_file_exists(std::ptr::null()), 0);
            assert_eq!(nzbget_rs_directory_exists(std::ptr::null()), 0);
            assert_eq!(nzbget_rs_directory_exists(c".".as_ptr()), 1);
            let mut source = b"~/x\0".to_vec();
            let expanded = crate::fsops::expand_home_path(
                Some(CStr::from_ptr(source.as_ptr().cast())),
                Some(b""),
            );
            source[2] = b'y';
            assert_eq!(expanded, b"/x");
        }
    }

    #[test]
    fn nntp_nulls_and_missing_callbacks() {
        use std::ptr::{null, null_mut};

        unsafe extern "C" fn greeting(ctx: *mut std::ffi::c_void, line: *mut *mut c_char) -> c_int {
            *line = ctx.cast();
            0
        }
        let mut buffer = *b"200 ready\r\n\0";
        let mut io = NntpIo {
            ctx: buffer.as_mut_ptr().cast(),
            write_line: None,
            read_line: Some(greeting),
            report_error: None,
            debug: None,
            cancelled: None,
            user: null(),
            password: null(),
            name: null(),
            host: null(),
            conn_host: null(),
        };
        unsafe {
            let (mut error, mut rejected, mut result, mut joined) = (1, 1, -9, -9);
            let mut answer = null_mut();
            assert_eq!(
                nzbget_rs_nntp_request(null(), null(), null_mut(), null_mut(), null_mut()),
                -1
            );
            assert_eq!(
                nzbget_rs_nntp_handshake(null(), null_mut(), null_mut(), null_mut()),
                -1
            );
            assert_eq!(
                nzbget_rs_nntp_join_group(
                    null(),
                    null(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut()
                ),
                -1
            );
            assert_eq!(
                nzbget_rs_nntp_request(&io, null(), &mut error, &mut rejected, &mut answer),
                -1
            );
            assert_eq!(
                nzbget_rs_nntp_join_group(
                    &io,
                    null(),
                    &mut error,
                    &mut rejected,
                    &mut answer,
                    &mut joined
                ),
                -1
            );
            assert_eq!(
                nzbget_rs_nntp_handshake(null(), &mut error, &mut rejected, &mut result),
                -1
            );
            assert_eq!((error, rejected, result, joined), (1, 1, -9, -9));

            // NULL configuration strings are empty; optional debug is absent.
            assert_eq!(
                nzbget_rs_nntp_handshake(&io, &mut error, &mut rejected, &mut result),
                0
            );
            assert_eq!((error, rejected, result), (1, 1, 0));

            // Missing required write/read callbacks stop the exchange and leave
            // result outputs untouched, while writing back reached auth state.
            assert_eq!(
                nzbget_rs_nntp_request(
                    &io,
                    c"BODY x\r\n".as_ptr(),
                    &mut error,
                    &mut rejected,
                    &mut answer
                ),
                -1
            );
            assert_eq!((error, rejected), (0, 0));
            assert!(answer.is_null());
            io.read_line = None;
            result = -9;
            assert_eq!(
                nzbget_rs_nntp_handshake(&io, &mut error, &mut rejected, &mut result),
                -1
            );
            assert_eq!(result, -9);
        }
    }

    #[test]
    fn options_ffi_outputs() {
        use std::ptr::null_mut;
        unsafe {
            assert_eq!(
                nzbget_rs_validate_option_name(std::ptr::null(), None, null_mut()),
                0
            );
            assert_eq!(
                nzbget_rs_validate_option_name(c"Server1.Host".as_ptr(), None, null_mut()),
                1
            );
            assert_eq!(nzbget_rs_option_restricted(std::ptr::null(), 1), 0);
            assert_eq!(
                nzbget_rs_option_restricted(c"Server1.Password".as_ptr(), 1),
                1
            );
            assert_eq!(nzbget_rs_option_restricted(c"Server1.Host".as_ptr(), 1), 0);
        }
    }

    #[test]
    fn volume_buffers_and_null_inputs() {
        use crate::statmeter::Slots;
        use std::ptr::{null, null_mut};
        unsafe {
            let mut first = 17;
            let mut slots = Slots::default();
            nzbget_rs_volume_calc_slots(0, &mut first, null_mut());
            nzbget_rs_volume_calc_slots(0, null_mut(), &mut slots);
            assert_eq!(first, 17);
            assert_eq!(slots, Slots::default());
            // Canaries around deliberately short output arrays, including
            // counters above 32 bits, check both the ABI width and lengths.
            let mut sec = [91, 1i64 << 40, 92];
            let mut min = [93, 7, 94];
            let mut hour = [95, 8, 96];
            slots = Slots {
                sec: 0,
                min: -1,
                hour: 1,
                ..Slots::default()
            };
            nzbget_rs_volume_add(
                sec.as_mut_ptr().add(1),
                1,
                min.as_mut_ptr().add(1),
                1,
                hour.as_mut_ptr().add(1),
                1,
                &slots,
                0,
                0,
                0,
                0,
                5,
            );
            assert_eq!(sec, [91, (1i64 << 40) + 5, 92]);
            assert_eq!(min, [93, 7, 94]);
            assert_eq!(hour, [95, 8, 96]);
            nzbget_rs_volume_add(
                sec.as_mut_ptr(),
                3,
                null_mut(),
                0,
                null_mut(),
                0,
                null(),
                0,
                0,
                60,
                0,
                5,
            );
            assert_eq!(sec, [91, (1i64 << 40) + 5, 92]);
            for t in [i64::MIN, i64::MAX, i64::from(i32::MIN), -2_147_483_647, -61] {
                nzbget_rs_volume_calc_slots(t, &mut first, &mut slots);
                nzbget_rs_volume_add(
                    null_mut(),
                    usize::MAX,
                    sec.as_mut_ptr().add(1),
                    1,
                    hour.as_mut_ptr().add(1),
                    0,
                    &slots,
                    0,
                    0,
                    t,
                    0,
                    i64::MAX,
                );
            }
            assert_eq!((sec[0], sec[2]), (91, 92));
            assert_eq!(hour, [95, 8, 96]);

            // The readable descriptor can occupy output storage. Read it
            // before constructing exclusive array references.
            let mut storage = [0i64; 3];
            let buffer = storage.as_mut_ptr();
            let descriptor = buffer.cast::<Slots>();
            descriptor.write(Slots::default());
            nzbget_rs_volume_add(
                buffer,
                3,
                null_mut(),
                0,
                null_mut(),
                0,
                descriptor,
                0,
                0,
                0,
                0,
                9,
            );
            assert_eq!(storage, [9, 0, 0]);
        }
    }

    #[test]
    fn textutil_in_place_ranges_nulls_and_ownership() {
        unsafe {
            let null = std::ptr::null_mut();
            let buf = nzbget_rs_split_command_line(null);
            assert_eq!(buf.len, 0);
            assert_eq!(*buf.data, 0);
            nzbget_rs_free(buf);
            let mut source = b"a 'b c'\0".to_vec();
            let words = nzbget_rs_split_command_line(source.as_ptr().cast());
            source.fill(b'x');
            assert_eq!(bytes_arg(words.data.cast(), words.len), b"a\0b c\0");
            nzbget_rs_free(words);
        }
    }

    #[test]
    fn rpc_route_null_inputs_borrowing_and_method_capacity() {
        assert_eq!(crate::rpcroute::protocol(b""), 0);
        for get in [false, true] {
            for protocol in [0, 1, 2, 3] {
                let r = crate::rpcroute::route(b"", b"", get, protocol);
                assert!(r.method.is_empty());
                assert_eq!(r.params, if get { Some(0) } else { None });
                assert!(r.id.is_none());
            }
        }
        let mut url = b"/jsonrpc/".to_vec();
        url.extend_from_slice(&[b'x'; 200]);
        url.extend_from_slice(b"?a=1");
        let r = crate::rpcroute::route(&url, b"", true, 2);
        assert_eq!(r.method, vec![b'x'; 99]);
        assert_eq!(r.params, Some(210));
        assert!(r.id.is_none());
        let request = b"\"id\":},\"method\":\"\"";
        let r = crate::rpcroute::route(b"", request, false, 2);
        assert!(r.params.is_none());
        assert_eq!(r.id, Some((5, 0)));
        assert_eq!(r.method, b"\"");
    }

    #[test]
    fn rpc_decide_outputs() {
        assert_eq!(crate::rpcroute::access(3, 0), 1);
        assert_eq!(crate::rpcroute::access(2, 0), 0);
        let d = crate::rpcroute::decide(3, 1, false, false, true);
        assert_eq!(d.dispatch, 1);
        assert_eq!(
            (d.code, d.text),
            (
                4,
                Some(
                    c"Not safe procedure for JSON-P. Use JSON-RPC or XML-RPC with HTTP-Method POST instead"
                )
            )
        );
        let d2 = crate::rpcroute::decide(2, 0, false, false, false);
        assert_eq!((d2.dispatch, d2.code, d2.text), (0, 0, None));
        assert_eq!(
            crate::rpcroute::decide(3, 0, true, false, false).dispatch,
            1
        );
    }

    #[test]
    fn rpc_envelope_owns_both_outputs() {
        let (head, tail) = crate::rpcroute::envelope(3, false, Some(b"callback"), Some(b"7"));
        assert_eq!(
            head,
            b"callback({\n\"version\" : \"1.1\",\n\"id\" : 7,\n\"result\" : "
        );
        assert_eq!(tail, b"\n})");
    }

    #[test]
    fn rpc_null_arguments_and_unknown_kind() {
        use crate::rpcparams::{Kind, Raw};
        let mut req = b"{\"params\":[12,true,\"x y\"]}\0".to_vec();
        let off = crate::rpcparams::skip_to_params(&mut req);
        unsafe {
            let raw = Raw::new(req.as_mut_ptr().add(off));
            let (p1, v1) = raw.next_int(Kind::Json);
            assert_eq!(v1, Some(12));
            let raw2 = Raw::new(req.as_mut_ptr().add(off + p1));
            let (p2, v2) = raw2.next_bool(Kind::Json, true);
            assert_eq!(v2, Some(true));
            let raw3 = Raw::new(req.as_mut_ptr().add(off + p1 + p2));
            let (_p3, v3) = raw3.next_str(Kind::Json);
            assert!(v3.is_some());
        }
    }

    #[test]
    fn scheduler_capacity_retry_preserves_all_outputs() {
        use crate::scheduler::{gmtime, Task, Tm};
        // The large-correction TZif fixture in scheduler_differential.py,
        // expressed here independently of the host's libc timezone support.
        unsafe extern "C" fn calendar(time: i64, fields: *mut Tm) {
            let correction = if time < 78_796_800 {
                0
            } else {
                1 + ((time - 78_796_800) / 86400).min(14) * 86400
            };
            fields.write(gmtime(time - correction));
        }
        let original = Task {
            hours: 336,
            minutes: 0,
            week_days: 0,
            last_executed: 0,
        };
        let mut task = original;
        let mut last = 0;
        let mut reset = -1;
        let mut due = [usize::MAX; 17];
        for capacity in [0, 1, 9, 14] {
            let n = unsafe {
                nzbget_rs_scheduler_check(
                    &mut task,
                    1,
                    &mut last,
                    80_049_600,
                    0,
                    0,
                    if capacity == 0 {
                        std::ptr::null_mut()
                    } else {
                        due.as_mut_ptr().add(1)
                    },
                    capacity,
                    &mut reset,
                    Some(calendar),
                )
            };
            assert_eq!(n, 15);
            assert_eq!(task, original);
            assert_eq!(last, 0);
            assert_eq!(reset, -1);
            assert_eq!(due, [usize::MAX; 17]);
        }
        let n = unsafe {
            nzbget_rs_scheduler_check(
                &mut task,
                1,
                &mut last,
                80_049_600,
                0,
                0,
                due.as_mut_ptr().add(1),
                15,
                &mut reset,
                Some(calendar),
            )
        };
        assert_eq!(n, 15);
        assert_eq!(&due[1..16], &[0; 15]);
        assert_eq!((due[0], due[16]), (usize::MAX, usize::MAX));
        assert_eq!(task.last_executed, 80_049_599);
        assert_eq!(last, 80_049_600);
        assert_eq!(reset, 1);
    }

    #[test]
    fn scheduler_null_and_oversized_buffers() {
        use crate::scheduler::Task;
        unsafe extern "C" fn calendar(time: i64, fields: *mut crate::scheduler::Tm) {
            fields.write(crate::scheduler::gmtime(time));
        }
        let original = Task {
            hours: -1,
            minutes: 0,
            week_days: 0,
            last_executed: 0,
        };
        // Each required pointer may be absent. Rejection must be atomic.
        for missing in 0..5 {
            let mut task = original;
            let mut last = 123;
            let mut reset = -1;
            let mut due = [usize::MAX; 9];
            let n = unsafe {
                nzbget_rs_scheduler_check(
                    if missing == 0 {
                        std::ptr::null_mut()
                    } else {
                        &mut task
                    },
                    1,
                    if missing == 1 {
                        std::ptr::null_mut()
                    } else {
                        &mut last
                    },
                    456,
                    0,
                    0,
                    if missing == 2 {
                        std::ptr::null_mut()
                    } else {
                        due.as_mut_ptr()
                    },
                    due.len(),
                    if missing == 3 {
                        std::ptr::null_mut()
                    } else {
                        &mut reset
                    },
                    if missing == 4 { None } else { Some(calendar) },
                )
            };
            assert_eq!(n, 0);
            assert_eq!(task, original);
            assert_eq!(last, 123);
            assert_eq!(reset, -1);
            assert_eq!(due, [usize::MAX; 9]);
        }
        let mut task = original;
        let mut last = 123;
        let mut reset = -1;
        let mut due = [usize::MAX; 9];
        unsafe {
            assert_eq!(
                nzbget_rs_scheduler_check(
                    &mut task,
                    usize::MAX,
                    &mut last,
                    456,
                    0,
                    0,
                    due.as_mut_ptr(),
                    due.len(),
                    &mut reset,
                    Some(calendar)
                ),
                0
            );
            assert_eq!(
                nzbget_rs_scheduler_check(
                    &mut task,
                    1,
                    &mut last,
                    456,
                    0,
                    0,
                    due.as_mut_ptr(),
                    usize::MAX,
                    &mut reset,
                    Some(calendar)
                ),
                0
            );
            assert_eq!(task, original);
            assert_eq!(last, 123);
            assert_eq!(reset, -1);
            assert_eq!(due, [usize::MAX; 9]);
            assert_eq!(
                nzbget_rs_scheduler_check(
                    std::ptr::null_mut(),
                    0,
                    &mut last,
                    456,
                    0,
                    0,
                    std::ptr::null_mut(),
                    0,
                    &mut reset,
                    Some(calendar)
                ),
                0
            );
        }
        assert_eq!(last, 456);
        assert_eq!(reset, 0);
    }

    #[test]
    fn path_nulls_lengths_and_owned_results() {
        unsafe {
            for len in [0, 42, usize::MAX] {
                for op in -1..=4 {
                    let result = nzbget_rs_path_text(op, std::ptr::null(), len, 0);
                    assert_eq!(result.len, 0);
                    assert_eq!(*result.data, 0);
                    nzbget_rs_free(result);
                }
                for (op, expected) in [(0, 0), (1, usize::MAX), (2, 0)] {
                    assert_eq!(nzbget_rs_path_position(op, std::ptr::null(), len), expected);
                }
            }
            nzbget_rs_normalize_path_separators(std::ptr::null_mut());
            let mut raw = [
                b'a',
                crate::paths::ALT_PATH_SEPARATOR,
                0,
                crate::paths::ALT_PATH_SEPARATOR,
            ];
            nzbget_rs_normalize_path_separators(raw.as_mut_ptr().cast());
            assert_eq!(
                raw,
                [
                    b'a',
                    crate::paths::PATH_SEPARATOR,
                    0,
                    crate::paths::ALT_PATH_SEPARATOR
                ]
            );

            // Length-bounded input need not have a terminator. The result
            // owns its bytes, including embedded NULs, and an extra terminator.
            let mut input = b"a\0b".to_vec();
            let first = nzbget_rs_path_text(3, input.as_ptr().cast(), input.len(), 0);
            let second = nzbget_rs_path_text(1, input.as_ptr().cast(), input.len(), 0);
            input.fill(b'x');
            drop(input);
            assert_eq!(
                std::slice::from_raw_parts(first.data.cast::<u8>(), first.len + 1),
                b"\"a\0b\"\0"
            );
            assert_eq!(CStr::from_ptr(second.data), c"a");
            nzbget_rs_free(second);
            nzbget_rs_free(first);
            let path = b"a/b";
            assert_eq!(
                nzbget_rs_path_position(0, path.as_ptr().cast(), path.len()),
                2
            );
            assert_eq!(
                nzbget_rs_path_position(1, path.as_ptr().cast(), path.len()),
                1
            );
            assert_eq!(
                nzbget_rs_path_position(1, path.as_ptr().cast(), 1),
                usize::MAX
            );
        }
    }

    #[test]
    fn filetypes_nulls_lengths_and_static_results() {
        unsafe {
            for len in [0, 42, usize::MAX] {
                for which in -1..=26 {
                    assert_eq!(nzbget_rs_file_type(which, std::ptr::null(), len), 0);
                }
            }
            let name = b".rar\0.mkv";
            assert_eq!(nzbget_rs_file_type(1, name.as_ptr().cast(), 4), 1);
            assert_eq!(nzbget_rs_file_type(1, name.as_ptr().cast(), name.len()), 0);
            let header = b"%PDF-".to_vec();
            let ext = crate::filetypes::sniff_extension(&header);
            drop(header);
            assert_eq!(ext, c".pdf");
            assert_eq!(crate::filetypes::sniff_extension(b"ID3"), c".mp3");
            assert_eq!(crate::filetypes::sniff_extension(b""), c"");
        }
    }

    #[test]
    fn deobfuscation_nulls_lengths_and_owned_buffers() {
        unsafe {
            for len in [0, 10, usize::MAX] {
                assert_eq!(
                    nzbget_rs_is_excessively_obfuscated(std::ptr::null(), len),
                    0
                );
            }
            let mut input = b"prefix \"a\0b\" suffix".to_vec();
            let first = crate::deobfuscation::deobfuscate(&input);
            let second = crate::deobfuscation::deobfuscate(&input[..6]);
            input.fill(b'x');
            drop(input);
            assert_eq!(first, b"a\0b");
            assert_eq!(second, b"prefix");
            assert_eq!(
                nzbget_rs_is_excessively_obfuscated(b"abcx".as_ptr().cast(), 3),
                1
            );
            assert_eq!(
                nzbget_rs_is_excessively_obfuscated(b"abcx".as_ptr().cast(), 4),
                0
            );
        }
    }

    #[test]
    fn util_ffi_nulls_ownership_and_table_without_callbacks() {
        unsafe {
            crate::util::reduce_str(std::ptr::null_mut(), std::ptr::null(), std::ptr::null());
            let mut raw = *b"abc\0";
            crate::util::reduce_str(raw.as_mut_ptr().cast(), std::ptr::null(), c"".as_ptr());
            assert_eq!(&raw, b"abc\0");
            let first = nzbget_rs_format_size(512);
            let second = nzbget_rs_format_size(12345);
            nzbget_rs_free(second);
            assert_eq!(
                std::slice::from_raw_parts(first.data.cast::<u8>(), first.len + 1),
                &b"512 B\0"[..]
            );
            nzbget_rs_free(first);
            for list in [c".NZB", c"*.NZ?"] {
                assert_eq!(
                    nzbget_rs_match_file_ext(c"a.nzb".as_ptr(), list.as_ptr(), c",".as_ptr()),
                    1
                );
            }
            assert_eq!(
                nzbget_rs_match_file_ext(std::ptr::null(), std::ptr::null(), std::ptr::null()),
                0
            );
        }
    }

    #[test]
    fn webutil_nulls_borrowing_and_ownership() {
        assert!(crate::webutil::xml_find_tag(b"", b"").is_none());
        assert!(crate::webutil::xml_find_tag(b"", b"x").is_none());
        assert_eq!(
            crate::webutil::xml_find_tag(b"<x>abc</x>", b"x"),
            Some((3, 3))
        );
    }

    #[test]
    fn text_null_inputs_and_callback() {
        unsafe {
            in_place(std::ptr::null_mut(), |b| {
                crate::text::xml_decode(b, &crate::util::byte_tolower_locale)
            });
            let mut xml = *b"&#xA;\0tail";
            in_place(xml.as_mut_ptr().cast(), |b| {
                crate::text::xml_decode(b, &crate::util::byte_tolower_locale)
            });
            assert_eq!(&xml[..2], b"\n\0");
        }
    }

    #[test]
    fn decoder_in_place_buffers_and_borrowed_value_pointer() {
        unsafe {
            in_place(std::ptr::null_mut(), crate::decode::json_decode);
            let mut json = *b"\\ud83d\\ude00\\u0000\0!";
            in_place(json.as_mut_ptr().cast(), crate::decode::json_decode);
            assert_eq!(&json[..8], b"\xf0\x9f\x98\x80\xef\xbf\xbd\0");
            assert_eq!(json[19], b'!');
        }
    }

    #[test]
    fn null_buffer_can_be_freed() {
        unsafe {
            nzbget_rs_free(RsBuf {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            })
        };
    }
}

// Memory Pressure Monitor C ABI (Requirement R8)

#[no_mangle]
pub extern "C" fn nzbget_rs_memory_pressure_sample() -> i32 {
    let metrics = crate::memory_pressure::sample_system_metrics(None);
    crate::memory_pressure::evaluate_state(&metrics) as i32
}

#[no_mangle]
pub extern "C" fn nzbget_rs_memory_pressure_effective_cache(configured_mb: u32) -> u32 {
    let tier = crate::memory_pressure::get_last_tier();
    let headroom = crate::memory_pressure::get_last_headroom_mb();
    crate::memory_pressure::calculate_effective_cache_limit(configured_mb, tier, headroom)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_memory_pressure_effective_par_buffer(configured_mb: u32) -> u32 {
    let tier = crate::memory_pressure::get_last_tier();
    let headroom = crate::memory_pressure::get_last_headroom_mb();
    crate::memory_pressure::calculate_effective_par_buffer(configured_mb, tier, headroom)
}

/// # Safety
/// mock_root must be null or point to a readable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_memory_pressure_set_mock_root(mock_root: *const c_char) {
    if mock_root.is_null() {
        crate::memory_pressure::set_mock_root(None);
    } else if let Ok(s) = CStr::from_ptr(mock_root).to_str() {
        crate::memory_pressure::set_mock_root(Some(s));
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_memory_pressure_trim_heap() {
    crate::memory_pressure::trim_heap();
}

// Version 1 purge preflight ABI. Trusted guarded host snapshots only.
use crate::dupepurge as purge;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeBytesC {
    pub data: *const u8,
    pub len: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeBindingC {
    pub collection_id: i32,
    pub file_id: i32,
    pub has_file: u32,
    pub incarnation: u64,
    pub posting_source: PurgeBytesC,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeMemberC {
    pub binding: PurgeBindingC,
    pub name: PurgeBytesC,
    pub key: PurgeBytesC,
    pub kind: u32,
    pub complete_inventory: u32,
    pub source_validation_id: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeSourceC {
    pub id: u64,
    pub version: u32,
    pub validator_version: u32,
    pub source_record_id: [u8; 32],
    pub source_bytes_digest: [u8; 32],
    pub binding: PurgeBindingC,
    pub has_parent: u32,
    pub parent: PurgeBindingC,
    pub custody_receipt_id: [u8; 32],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeCustodyC {
    pub id: [u8; 32],
    pub validator_version: u32,
    pub source_record_id: [u8; 32],
    pub source_bytes_digest: [u8; 32],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeParentC {
    pub file: PurgeBindingC,
    pub collection: PurgeBindingC,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeComparisonC {
    pub seed: PurgeBindingC,
    pub candidate: PurgeBindingC,
    pub name_equal: u32,
    pub key_equal: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeDirectC {
    pub seed: PurgeBindingC,
    pub candidate: PurgeBindingC,
    pub seed_source_validation_id: u64,
    pub candidate_source_validation_id: u64,
    pub pair_membership_receipt_id: [u8; 32],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgePairC {
    pub id: [u8; 32],
    pub version: u32,
    pub validator_version: u32,
    pub owner_kind: u32,
    pub seed: PurgeBindingC,
    pub candidate: PurgeBindingC,
    pub seed_source_record_id: [u8; 32],
    pub seed_source_bytes_digest: [u8; 32],
    pub candidate_source_record_id: [u8; 32],
    pub candidate_source_bytes_digest: [u8; 32],
    pub relation_id: PurgeBytesC,
    pub evidence_version: u32,
    pub evidence_id: [u8; 32],
    pub evidence_bytes: PurgeBytesC,
    pub custody_receipt_id: [u8; 32],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgePairCustodyC {
    pub id: [u8; 32],
    pub version: u32,
    pub validator_version: u32,
    pub owner_kind: u32,
    pub relation_id: PurgeBytesC,
    pub evidence_version: u32,
    pub evidence_id: [u8; 32],
    pub evidence_bytes: PurgeBytesC,
    pub seed: PurgeBindingC,
    pub candidate: PurgeBindingC,
    pub seed_source_record_id: [u8; 32],
    pub seed_source_bytes_digest: [u8; 32],
    pub candidate_source_record_id: [u8; 32],
    pub candidate_source_bytes_digest: [u8; 32],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeRootC {
    pub id: u64,
    pub descriptor_identity: PurgeBytesC,
    pub configured_container: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeNamespaceC {
    pub id: [u8; 32],
    pub root_id: u64,
    pub relative_path: PurgeBytesC,
    pub root_identity: PurgeBytesC,
    pub ancestor_identity: PurgeBytesC,
    pub mount_identity: PurgeBytesC,
    pub exclusive: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeEntryC {
    pub id: u64,
    pub root_id: u64,
    pub relative_path: PurgeBytesC,
    pub kind: u32,
    pub object_identity: PurgeBytesC,
    pub member: PurgeBindingC,
    pub namespace_proof_id: [u8; 32],
    pub ownership_receipt_id: [u8; 32],
    pub inventory_revision: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeOwnershipC {
    pub id: [u8; 32],
    pub member: PurgeBindingC,
    pub root_id: u64,
    pub relative_path: PurgeBytesC,
    pub kind: u32,
    pub object_identity: PurgeBytesC,
    pub namespace_proof_id: [u8; 32],
    pub inventory_revision: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeOutsideC {
    pub id: u64,
    pub root_id: u64,
    pub relative_path: PurgeBytesC,
    pub claimant: PurgeBindingC,
    pub object_identity: PurgeBytesC,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeSnapshotC {
    pub version: u32,
    pub struct_size: u32,
    pub revision: u64,
    pub generation: u64,
    pub captured_context: PurgeBytesC,
    pub current_context: PurgeBytesC,
    pub seeds: *const PurgeBindingC,
    pub seeds_count: u32,
    pub members: *const PurgeMemberC,
    pub members_count: u32,
    pub sources: *const PurgeSourceC,
    pub sources_count: u32,
    pub custody: *const PurgeCustodyC,
    pub custody_count: u32,
    pub parents: *const PurgeParentC,
    pub parents_count: u32,
    pub comparisons: *const PurgeComparisonC,
    pub comparisons_count: u32,
    pub direct: *const PurgeDirectC,
    pub direct_count: u32,
    pub pairs: *const PurgePairC,
    pub pairs_count: u32,
    pub pair_custody: *const PurgePairCustodyC,
    pub pair_custody_count: u32,
    pub roots: *const PurgeRootC,
    pub roots_count: u32,
    pub namespaces: *const PurgeNamespaceC,
    pub namespaces_count: u32,
    pub entries: *const PurgeEntryC,
    pub entries_count: u32,
    pub ownership: *const PurgeOwnershipC,
    pub ownership_count: u32,
    pub outside: *const PurgeOutsideC,
    pub outside_count: u32,
}

fn purge_error_code(e: purge::PurgeError) -> i32 {
    use purge::PurgeError::*;
    match e {
        InvalidInput => 1,
        MissingSeed => 2,
        MissingInventory => 3,
        Conflict => 4,
        LimitExceeded => 5,
        Unsupported => 6,
        AllocationFailure => 7,
        Persistence => 8,
        HostFailure => 9,
        CorruptJournal => 10,
        RecoveryRequired => 11,
    }
}
fn purge_bool(v: u32) -> Result<bool, purge::PurgeError> {
    match v {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(purge::PurgeError::InvalidInput),
    }
}
// Safety: caller owns initialized immutable accessible memory for the declared
// count. Structural bounds/alignment are checked; pointer accessibility is the
// host's C ABI obligation, never an authentication or sandbox guarantee.
unsafe fn purge_slice<'a, T>(
    p: *const T,
    n: u32,
    cap: usize,
    b: &mut purge::Budget,
) -> Result<&'a [T], purge::PurgeError> {
    let n = n as usize;
    if n > cap {
        return Err(purge::PurgeError::LimitExceeded);
    }
    b.table::<T>(n)?;
    if n == 0 {
        return Ok(&[]);
    }
    if p.is_null() || !(p as usize).is_multiple_of(std::mem::align_of::<T>()) {
        return Err(purge::PurgeError::InvalidInput);
    }
    Ok(std::slice::from_raw_parts(p, n))
}
unsafe fn purge_raw_bytes<'a>(
    x: PurgeBytesC,
    cap: usize,
    b: &mut purge::Budget,
) -> Result<&'a [u8], purge::PurgeError> {
    purge_slice(x.data, x.len, cap, b)
}
unsafe fn purge_owned_bytes(x: PurgeBytesC) -> Result<Vec<u8>, purge::PurgeError> {
    purge::copy_bytes(if x.len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(x.data, x.len as usize)
    })
}
unsafe fn purge_check_binding(
    x: &PurgeBindingC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    if !purge_bool(x.has_file)? && x.file_id != 0 {
        return Err(purge::PurgeError::InvalidInput);
    }
    purge_raw_bytes(x.posting_source, purge::MAX_IDENTITY, b)?;
    Ok(())
}
unsafe fn purge_owned_binding(
    x: &PurgeBindingC,
) -> Result<purge::MemberBinding, purge::PurgeError> {
    Ok(purge::MemberBinding {
        collection_id: x.collection_id,
        file_id: if purge_bool(x.has_file)? {
            Some(x.file_id)
        } else {
            None
        },
        incarnation: x.incarnation,
        posting_source: purge_owned_bytes(x.posting_source)?,
    })
}
fn purge_absent_parent(x: &PurgeBindingC) -> bool {
    x.collection_id == 0
        && x.file_id == 0
        && x.has_file == 0
        && x.incarnation == 0
        && x.posting_source.len == 0
        && x.posting_source.data.is_null()
}
fn purge_member_kind(v: u32) -> Result<purge::MemberKind, purge::PurgeError> {
    use purge::MemberKind::*;
    match v {
        0 => Ok(Queue),
        1 => Ok(Post),
        2 => Ok(History),
        3 => Ok(CompactHistory),
        _ => Err(purge::PurgeError::Unsupported),
    }
}
fn purge_entry_kind(v: u32) -> Result<purge::EntryKind, purge::PurgeError> {
    use purge::EntryKind::*;
    match v {
        0 => Ok(Regular),
        1 => Ok(Directory),
        2 => Ok(Symlink),
        _ => Err(purge::PurgeError::Unsupported),
    }
}
unsafe fn purge_check_member(
    x: &PurgeMemberC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    purge_check_binding(&x.binding, b)?;
    purge_raw_bytes(x.name, purge::MAX_IDENTITY, b)?;
    purge_raw_bytes(x.key, purge::MAX_IDENTITY, b)?;
    purge_member_kind(x.kind)?;
    purge_bool(x.complete_inventory)?;
    Ok(())
}
unsafe fn purge_check_source(
    x: &PurgeSourceC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    purge_check_binding(&x.binding, b)?;
    if purge_bool(x.has_parent)? {
        purge_check_binding(&x.parent, b)?;
    } else if !purge_absent_parent(&x.parent) {
        return Err(purge::PurgeError::InvalidInput);
    }
    Ok(())
}
unsafe fn purge_check_custody(
    x: &PurgeCustodyC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    let _ = (x, b);
    Ok(())
}
unsafe fn purge_check_parent(
    x: &PurgeParentC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    purge_check_binding(&x.file, b)?;
    purge_check_binding(&x.collection, b)?;
    Ok(())
}
unsafe fn purge_check_comparison(
    x: &PurgeComparisonC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    purge_check_binding(&x.seed, b)?;
    purge_check_binding(&x.candidate, b)?;
    purge_bool(x.name_equal)?;
    purge_bool(x.key_equal)?;
    Ok(())
}
unsafe fn purge_check_direct(
    x: &PurgeDirectC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    purge_check_binding(&x.seed, b)?;
    purge_check_binding(&x.candidate, b)?;
    Ok(())
}
unsafe fn purge_check_pair(x: &PurgePairC, b: &mut purge::Budget) -> Result<(), purge::PurgeError> {
    purge_check_binding(&x.seed, b)?;
    purge_check_binding(&x.candidate, b)?;
    purge_raw_bytes(x.relation_id, purge::MAX_IDENTITY, b)?;
    purge_raw_bytes(x.evidence_bytes, purge::MAX_IDENTITY, b)?;
    Ok(())
}
unsafe fn purge_check_paircustody(
    x: &PurgePairCustodyC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    purge_raw_bytes(x.relation_id, purge::MAX_IDENTITY, b)?;
    purge_raw_bytes(x.evidence_bytes, purge::MAX_IDENTITY, b)?;
    purge_check_binding(&x.seed, b)?;
    purge_check_binding(&x.candidate, b)?;
    Ok(())
}
unsafe fn purge_check_root(x: &PurgeRootC, b: &mut purge::Budget) -> Result<(), purge::PurgeError> {
    purge_raw_bytes(x.descriptor_identity, purge::MAX_IDENTITY, b)?;
    purge_bool(x.configured_container)?;
    Ok(())
}
unsafe fn purge_check_namespace(
    x: &PurgeNamespaceC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    purge_raw_bytes(x.relative_path, purge::MAX_PATH, b)?;
    purge_raw_bytes(x.root_identity, purge::MAX_IDENTITY, b)?;
    purge_raw_bytes(x.ancestor_identity, purge::MAX_IDENTITY, b)?;
    purge_raw_bytes(x.mount_identity, purge::MAX_IDENTITY, b)?;
    purge_bool(x.exclusive)?;
    Ok(())
}
unsafe fn purge_check_entry(
    x: &PurgeEntryC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    purge_raw_bytes(x.relative_path, purge::MAX_PATH, b)?;
    purge_entry_kind(x.kind)?;
    purge_raw_bytes(x.object_identity, purge::MAX_IDENTITY, b)?;
    purge_check_binding(&x.member, b)?;
    Ok(())
}
unsafe fn purge_check_ownership(
    x: &PurgeOwnershipC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    purge_check_binding(&x.member, b)?;
    purge_raw_bytes(x.relative_path, purge::MAX_PATH, b)?;
    purge_entry_kind(x.kind)?;
    purge_raw_bytes(x.object_identity, purge::MAX_IDENTITY, b)?;
    Ok(())
}
unsafe fn purge_check_outside(
    x: &PurgeOutsideC,
    b: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    purge_raw_bytes(x.relative_path, purge::MAX_PATH, b)?;
    purge_check_binding(&x.claimant, b)?;
    purge_raw_bytes(x.object_identity, purge::MAX_IDENTITY, b)?;
    Ok(())
}
unsafe fn purge_owned_member(x: &PurgeMemberC) -> Result<purge::MemberRecord, purge::PurgeError> {
    Ok(purge::MemberRecord {
        binding: purge_owned_binding(&x.binding)?,
        name: purge_owned_bytes(x.name)?,
        key: purge_owned_bytes(x.key)?,
        kind: purge_member_kind(x.kind)?,
        complete_inventory: purge_bool(x.complete_inventory)?,
        source_validation_id: x.source_validation_id,
    })
}
unsafe fn purge_owned_source(
    x: &PurgeSourceC,
) -> Result<purge::SourceValidation, purge::PurgeError> {
    Ok(purge::SourceValidation {
        id: x.id,
        version: x.version,
        validator_version: x.validator_version,
        source_record_id: x.source_record_id,
        source_bytes_digest: x.source_bytes_digest,
        binding: purge_owned_binding(&x.binding)?,
        parent_collection: if purge_bool(x.has_parent)? {
            Some(purge_owned_binding(&x.parent)?)
        } else {
            None
        },
        custody_receipt_id: x.custody_receipt_id,
    })
}
unsafe fn purge_owned_custody(
    x: &PurgeCustodyC,
) -> Result<purge::CustodyReceipt, purge::PurgeError> {
    Ok(purge::CustodyReceipt {
        id: x.id,
        validator_version: x.validator_version,
        source_record_id: x.source_record_id,
        source_bytes_digest: x.source_bytes_digest,
    })
}
unsafe fn purge_owned_parent(
    x: &PurgeParentC,
) -> Result<purge::ParentMembership, purge::PurgeError> {
    Ok(purge::ParentMembership {
        file: purge_owned_binding(&x.file)?,
        collection: purge_owned_binding(&x.collection)?,
    })
}
unsafe fn purge_owned_comparison(
    x: &PurgeComparisonC,
) -> Result<purge::AnchorComparison, purge::PurgeError> {
    Ok(purge::AnchorComparison {
        seed: purge_owned_binding(&x.seed)?,
        candidate: purge_owned_binding(&x.candidate)?,
        name_equal: purge_bool(x.name_equal)?,
        key_equal: purge_bool(x.key_equal)?,
    })
}
unsafe fn purge_owned_direct(x: &PurgeDirectC) -> Result<purge::DirectBinding, purge::PurgeError> {
    Ok(purge::DirectBinding {
        seed: purge_owned_binding(&x.seed)?,
        candidate: purge_owned_binding(&x.candidate)?,
        seed_source_validation_id: x.seed_source_validation_id,
        candidate_source_validation_id: x.candidate_source_validation_id,
        pair_membership_receipt_id: x.pair_membership_receipt_id,
    })
}
unsafe fn purge_owned_pair(
    x: &PurgePairC,
) -> Result<purge::PairMembershipReceipt, purge::PurgeError> {
    Ok(purge::PairMembershipReceipt {
        id: x.id,
        version: x.version,
        validator_version: x.validator_version,
        owner_kind: x.owner_kind,
        seed: purge_owned_binding(&x.seed)?,
        candidate: purge_owned_binding(&x.candidate)?,
        seed_source_record_id: x.seed_source_record_id,
        seed_source_bytes_digest: x.seed_source_bytes_digest,
        candidate_source_record_id: x.candidate_source_record_id,
        candidate_source_bytes_digest: x.candidate_source_bytes_digest,
        relation_id: purge_owned_bytes(x.relation_id)?,
        evidence_version: x.evidence_version,
        evidence_id: x.evidence_id,
        evidence_bytes: purge_owned_bytes(x.evidence_bytes)?,
        custody_receipt_id: x.custody_receipt_id,
    })
}
unsafe fn purge_owned_paircustody(
    x: &PurgePairCustodyC,
) -> Result<purge::PairCustodyReceipt, purge::PurgeError> {
    Ok(purge::PairCustodyReceipt {
        id: x.id,
        version: x.version,
        validator_version: x.validator_version,
        owner_kind: x.owner_kind,
        relation_id: purge_owned_bytes(x.relation_id)?,
        evidence_version: x.evidence_version,
        evidence_id: x.evidence_id,
        evidence_bytes: purge_owned_bytes(x.evidence_bytes)?,
        seed: purge_owned_binding(&x.seed)?,
        candidate: purge_owned_binding(&x.candidate)?,
        seed_source_record_id: x.seed_source_record_id,
        seed_source_bytes_digest: x.seed_source_bytes_digest,
        candidate_source_record_id: x.candidate_source_record_id,
        candidate_source_bytes_digest: x.candidate_source_bytes_digest,
    })
}
unsafe fn purge_owned_root(x: &PurgeRootC) -> Result<purge::RootRecord, purge::PurgeError> {
    Ok(purge::RootRecord {
        id: x.id,
        descriptor_identity: purge_owned_bytes(x.descriptor_identity)?,
        configured_container: purge_bool(x.configured_container)?,
    })
}
unsafe fn purge_owned_namespace(
    x: &PurgeNamespaceC,
) -> Result<purge::NamespaceProof, purge::PurgeError> {
    Ok(purge::NamespaceProof {
        id: x.id,
        root_id: x.root_id,
        relative_path: purge_owned_bytes(x.relative_path)?,
        root_identity: purge_owned_bytes(x.root_identity)?,
        ancestor_identity: purge_owned_bytes(x.ancestor_identity)?,
        mount_identity: purge_owned_bytes(x.mount_identity)?,
        exclusive: purge_bool(x.exclusive)?,
    })
}
unsafe fn purge_owned_entry(x: &PurgeEntryC) -> Result<purge::InventoryEntry, purge::PurgeError> {
    Ok(purge::InventoryEntry {
        id: x.id,
        root_id: x.root_id,
        relative_path: purge_owned_bytes(x.relative_path)?,
        kind: purge_entry_kind(x.kind)?,
        object_identity: purge_owned_bytes(x.object_identity)?,
        member: purge_owned_binding(&x.member)?,
        namespace_proof_id: x.namespace_proof_id,
        ownership_receipt_id: x.ownership_receipt_id,
        inventory_revision: x.inventory_revision,
    })
}
unsafe fn purge_owned_ownership(
    x: &PurgeOwnershipC,
) -> Result<purge::OwnershipReceipt, purge::PurgeError> {
    Ok(purge::OwnershipReceipt {
        id: x.id,
        member: purge_owned_binding(&x.member)?,
        root_id: x.root_id,
        relative_path: purge_owned_bytes(x.relative_path)?,
        kind: purge_entry_kind(x.kind)?,
        object_identity: purge_owned_bytes(x.object_identity)?,
        namespace_proof_id: x.namespace_proof_id,
        inventory_revision: x.inventory_revision,
    })
}
unsafe fn purge_owned_outside(x: &PurgeOutsideC) -> Result<purge::OutsideClaim, purge::PurgeError> {
    Ok(purge::OutsideClaim {
        id: x.id,
        root_id: x.root_id,
        relative_path: purge_owned_bytes(x.relative_path)?,
        claimant: purge_owned_binding(&x.claimant)?,
        object_identity: purge_owned_bytes(x.object_identity)?,
    })
}

// purge_slice already charges each borrowed C record. Preserve that bound and
// charge any additional fixed storage required by its owned Rust counterpart.
// All tables and nested byte spans are budgeted before conversion allocates.
fn purge_owned_table_overhead<C, R>(
    count: usize,
    budget: &mut purge::Budget,
) -> Result<(), purge::PurgeError> {
    let extra = std::mem::size_of::<R>().saturating_sub(std::mem::size_of::<C>());
    budget.add(
        count
            .checked_mul(extra)
            .ok_or(purge::PurgeError::LimitExceeded)?,
    )
}

unsafe fn purge_snapshot(x: &PurgeSnapshotC) -> Result<purge::Snapshot, purge::PurgeError> {
    let mut b = purge::Budget::new();
    let seeds = purge_slice(x.seeds, x.seeds_count, purge::MAX_SEEDS, &mut b)?;
    let members = purge_slice(x.members, x.members_count, purge::MAX_MEMBERS, &mut b)?;
    let sources = purge_slice(x.sources, x.sources_count, purge::MAX_PAYLOAD, &mut b)?;
    let custody = purge_slice(x.custody, x.custody_count, purge::MAX_PAYLOAD, &mut b)?;
    let parents = purge_slice(x.parents, x.parents_count, purge::MAX_PAYLOAD, &mut b)?;
    let comparisons = purge_slice(
        x.comparisons,
        x.comparisons_count,
        purge::MAX_PAYLOAD,
        &mut b,
    )?;
    let direct = purge_slice(x.direct, x.direct_count, purge::MAX_PAYLOAD, &mut b)?;
    let pairs = purge_slice(x.pairs, x.pairs_count, purge::MAX_PAYLOAD, &mut b)?;
    let pair_custody = purge_slice(
        x.pair_custody,
        x.pair_custody_count,
        purge::MAX_PAYLOAD,
        &mut b,
    )?;
    let roots = purge_slice(x.roots, x.roots_count, purge::MAX_PAYLOAD, &mut b)?;
    let namespaces = purge_slice(x.namespaces, x.namespaces_count, purge::MAX_PAYLOAD, &mut b)?;
    let entries = purge_slice(x.entries, x.entries_count, purge::MAX_ENTRIES, &mut b)?;
    let ownership = purge_slice(x.ownership, x.ownership_count, purge::MAX_PAYLOAD, &mut b)?;
    let outside = purge_slice(x.outside, x.outside_count, purge::MAX_PAYLOAD, &mut b)?;
    purge_owned_table_overhead::<PurgeBindingC, purge::MemberBinding>(seeds.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeMemberC, purge::MemberRecord>(members.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeSourceC, purge::SourceValidation>(sources.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeCustodyC, purge::CustodyReceipt>(custody.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeParentC, purge::ParentMembership>(parents.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeComparisonC, purge::AnchorComparison>(
        comparisons.len(),
        &mut b,
    )?;
    purge_owned_table_overhead::<PurgeDirectC, purge::DirectBinding>(direct.len(), &mut b)?;
    purge_owned_table_overhead::<PurgePairC, purge::PairMembershipReceipt>(pairs.len(), &mut b)?;
    purge_owned_table_overhead::<PurgePairCustodyC, purge::PairCustodyReceipt>(
        pair_custody.len(),
        &mut b,
    )?;
    purge_owned_table_overhead::<PurgeRootC, purge::RootRecord>(roots.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeNamespaceC, purge::NamespaceProof>(namespaces.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeEntryC, purge::InventoryEntry>(entries.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeOwnershipC, purge::OwnershipReceipt>(
        ownership.len(),
        &mut b,
    )?;
    purge_owned_table_overhead::<PurgeOutsideC, purge::OutsideClaim>(outside.len(), &mut b)?;
    purge_raw_bytes(x.captured_context, purge::MAX_IDENTITY, &mut b)?;
    purge_raw_bytes(x.current_context, purge::MAX_IDENTITY, &mut b)?;
    for x in seeds {
        purge_check_binding(x, &mut b)?;
    }
    for x in members {
        purge_check_member(x, &mut b)?;
    }
    for x in sources {
        purge_check_source(x, &mut b)?;
    }
    for x in custody {
        purge_check_custody(x, &mut b)?;
    }
    for x in parents {
        purge_check_parent(x, &mut b)?;
    }
    for x in comparisons {
        purge_check_comparison(x, &mut b)?;
    }
    for x in direct {
        purge_check_direct(x, &mut b)?;
    }
    for x in pairs {
        purge_check_pair(x, &mut b)?;
    }
    for x in pair_custody {
        purge_check_paircustody(x, &mut b)?;
    }
    for x in roots {
        purge_check_root(x, &mut b)?;
    }
    for x in namespaces {
        purge_check_namespace(x, &mut b)?;
    }
    for x in entries {
        purge_check_entry(x, &mut b)?;
    }
    for x in ownership {
        purge_check_ownership(x, &mut b)?;
    }
    for x in outside {
        purge_check_outside(x, &mut b)?;
    }
    unsafe fn convert<A, B>(
        input: &[A],
        f: unsafe fn(&A) -> Result<B, purge::PurgeError>,
    ) -> Result<Vec<B>, purge::PurgeError> {
        let mut out = purge::reserved(input.len())?;
        for x in input {
            out.push(f(x)?);
        }
        Ok(out)
    }
    Ok(purge::Snapshot {
        version: x.version,
        revision: x.revision,
        generation: x.generation,
        comparison_context: purge::ComparisonContext {
            captured: purge_owned_bytes(x.captured_context)?,
            current: purge_owned_bytes(x.current_context)?,
        },
        seeds: convert(seeds, purge_owned_binding)?,
        members: convert(members, purge_owned_member)?,
        source_validations: convert(sources, purge_owned_source)?,
        custody_receipts: convert(custody, purge_owned_custody)?,
        parents: convert(parents, purge_owned_parent)?,
        comparisons: convert(comparisons, purge_owned_comparison)?,
        direct_bindings: convert(direct, purge_owned_direct)?,
        pair_memberships: convert(pairs, purge_owned_pair)?,
        pair_custody_receipts: convert(pair_custody, purge_owned_paircustody)?,
        roots: convert(roots, purge_owned_root)?,
        namespace_proofs: convert(namespaces, purge_owned_namespace)?,
        entries: convert(entries, purge_owned_entry)?,
        ownership_receipts: convert(ownership, purge_owned_ownership)?,
        outside_claims: convert(outside, purge_owned_outside)?,
    })
}
/// Prepare a plan from a version 1 trusted guarded host snapshot.
///
/// # Safety
/// `out` must be writable; request must cover 16 initialized bytes. All counted
/// descriptors must be aligned, initialized and accessible for their count and
/// immutable until return. On success the owned opaque handle must be destroyed
/// exactly once, after all users have stopped. No borrowed storage is retained.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_purge_prepare(
    snapshot: *const PurgeSnapshotC,
    request: *const u8,
    out: *mut *mut std::ffi::c_void,
) -> i32 {
    if out.is_null() {
        return purge_error_code(purge::PurgeError::InvalidInput);
    }
    out.write(std::ptr::null_mut());
    if snapshot.is_null()
        || request.is_null()
        || !(snapshot as usize).is_multiple_of(std::mem::align_of::<PurgeSnapshotC>())
    {
        return purge_error_code(purge::PurgeError::InvalidInput);
    }
    // Read only the eight-byte header before trusting the complete record size.
    let header = snapshot.cast::<u32>();
    if header.read() != 1 || header.add(1).read() != std::mem::size_of::<PurgeSnapshotC>() as u32 {
        return purge_error_code(purge::PurgeError::InvalidInput);
    }
    let mut token = [0u8; 16];
    std::ptr::copy_nonoverlapping(request, token.as_mut_ptr(), 16);
    let result = purge_snapshot(&*snapshot).and_then(|s| purge::prepare(s, token));
    match result {
        Err(e) => purge_error_code(e),
        Ok(plan) => {
            let layout = std::alloc::Layout::new::<purge::Plan>();
            let p = std::alloc::alloc(layout).cast::<purge::Plan>();
            if p.is_null() {
                return purge_error_code(purge::PurgeError::AllocationFailure);
            }
            p.write(plan);
            out.write(p.cast());
            0
        }
    }
}
/// # Safety
/// A nonnull pointer must be the live handle from prepare, with no concurrent
/// users; destroying a handle twice or passing another pointer is invalid.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_purge_destroy(handle: *mut std::ffi::c_void) {
    if !handle.is_null() {
        let p = handle.cast::<purge::Plan>();
        std::ptr::drop_in_place(p);
        std::alloc::dealloc(handle.cast(), std::alloc::Layout::new::<purge::Plan>());
    }
}

// Version-one durable journal ABI. The host serializes each registry and must
// not reenter it from a callback. All callback views expire with that callback;
// returned views must remain accessible until the enclosing API returns.
use crate::purgejournal as journal;
#[repr(C)]
pub struct PurgeRegistryC {
    inner: journal::Registry,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeAckIdentityC {
    pub version: u32,
    pub struct_size: u32,
    pub domain: [u8; 16],
    pub manifest_sequence: u64,
    pub request: [u8; 16],
    pub generation: u64,
    pub members: *const PurgeBindingC,
    pub members_count: u32,
    pub state_revision: u64,
    pub receipt_sequence: u64,
    pub payload_digest: [u8; 32],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeOwnerBarrierC {
    pub owner_kind: u32,
    pub barrier_revision: u64,
    pub queued: u64,
    pub active: u64,
    pub pinned: u64,
    pub publication: u64,
    pub unacknowledged_deltas: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeActionC {
    pub present: u32,
    pub recursive: u32,
    pub entry_id: u64,
    pub root_id: u64,
    pub relative_path: PurgeBytesC,
    pub kind: u32,
    pub object_identity: PurgeBytesC,
}
#[repr(C)]
pub struct PurgeCallC {
    pub version: u32,
    pub struct_size: u32,
    pub identity: PurgeAckIdentityC,
    pub operation: u32,
    pub action: PurgeActionC,
    pub root_descriptor_identity: PurgeBytesC,
    pub completed_actions: u64,
    pub action_receipt_high_water: u64,
    pub cleanup_receipt_sequence: u64,
    pub receipt_budget: u32,
    pub barrier_revision: u64,
    pub required_owners: *const u32,
    pub required_owners_count: u32,
    pub inventory_revision: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeStopDispositionC {
    pub owner_kind: u32,
    pub barrier_revision: u64,
    pub disposition: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeFenceReceiptC {
    pub already_fenced: u32,
    pub dispositions: *const PurgeStopDispositionC,
    pub dispositions_count: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeOwnerCoverageC {
    pub owner_kind: u32,
    pub barrier_revision: u64,
    pub complete: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeArtifactCoverageC {
    pub kind: u32,
    pub count: u32,
    pub complete: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeStoreReceiptC {
    pub identity: PurgeBytesC,
    pub revision: u64,
    pub file_synced: u32,
    pub published: u32,
    pub directory_synced: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeArtifactReceiptC {
    pub kind: u32,
    pub member: PurgeBindingC,
    pub identity: PurgeBytesC,
    pub has_entry: u32,
    pub entry_id: u64,
    pub outcome: u32,
    pub error: i32,
    pub retained_store: PurgeBytesC,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeCleanupReceiptC {
    pub owners: *const PurgeOwnerCoverageC,
    pub owners_count: u32,
    pub coverage: *const PurgeArtifactCoverageC,
    pub coverage_count: u32,
    pub artifacts: *const PurgeArtifactReceiptC,
    pub artifacts_count: u32,
    pub stores: *const PurgeStoreReceiptC,
    pub stores_count: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgePersistenceReceiptC {
    pub completed_actions: u64,
    pub action_receipt_high_water: u64,
    pub cleanup_receipt_sequence: u64,
    pub owners: *const PurgeOwnerCoverageC,
    pub owners_count: u32,
    pub stores: *const PurgeStoreReceiptC,
    pub stores_count: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeAckC {
    pub version: u32,
    pub struct_size: u32,
    pub identity: PurgeAckIdentityC,
    pub outcome: u32,
    pub inventory_revision: u64,
    pub owners: *const PurgeOwnerBarrierC,
    pub owners_count: u32,
    pub fence: *const PurgeFenceReceiptC,
    pub cleanup: *const PurgeCleanupReceiptC,
    pub persistence: *const PurgePersistenceReceiptC,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgePublicationAckC {
    pub version: u32,
    pub struct_size: u32,
    pub sequence: u64,
    pub frame: PurgeBytesC,
    pub file_synced: u32,
    pub published: u32,
    pub directory_synced: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeHostOpsC {
    pub version: u32,
    pub struct_size: u32,
    pub registry: *mut PurgeRegistryC,
    pub context: *mut std::ffi::c_void,
    /// Exactly 1 means all checked journal/lifecycle capabilities are present.
    pub capabilities: u32,
    pub journal_identity: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut u8) -> i32>,
    pub publish: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            u64,
            PurgeBytesC,
            *mut PurgePublicationAckC,
        ) -> i32,
    >,
    pub execute: Option<
        unsafe extern "C" fn(*mut std::ffi::c_void, *const PurgeCallC, *mut PurgeAckC) -> i32,
    >,
    pub required_owners: *const u32,
    pub required_owners_count: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeStatusC {
    pub version: u32,
    pub struct_size: u32,
    pub phase: u32,
    pub released: u32,
    pub generation: u64,
    pub state_revision: u64,
    pub receipt_sequence: u64,
    pub recovery_required: u32,
    pub error: i32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PurgeInventoryChangeC {
    pub entry_id: u64,
    pub before: *const PurgeEntryC,
    pub after: *const PurgeEntryC,
    pub ownership: *const PurgeOwnershipC,
    pub prior_absent: u32,
}
#[repr(C)]
pub struct PurgeOutputDeltaC {
    pub version: u32,
    pub struct_size: u32,
    pub producer_ticket: u64,
    pub sequence: u64,
    pub prior_inventory_revision: u64,
    pub changes: *const PurgeInventoryChangeC,
    pub changes_count: u32,
    pub final_for_producer: u32,
}
fn purge_view(bytes: &[u8]) -> PurgeBytesC {
    PurgeBytesC {
        data: bytes.as_ptr(),
        len: bytes.len() as u32,
    }
}
fn purge_binding_view(b: &purge::MemberBinding) -> PurgeBindingC {
    PurgeBindingC {
        collection_id: b.collection_id,
        file_id: b.file_id.unwrap_or(0),
        has_file: u32::from(b.file_id.is_some()),
        incarnation: b.incarnation,
        posting_source: purge_view(&b.posting_source),
    }
}
unsafe fn purge_record<'a, T>(p: *const T) -> Result<&'a T, purge::PurgeError> {
    if p.is_null() || !(p as usize).is_multiple_of(std::mem::align_of::<T>()) {
        return Err(purge::PurgeError::InvalidInput);
    }
    Ok(&*p)
}
unsafe fn purge_versioned<'a, T>(p: *const T) -> Result<&'a T, purge::PurgeError> {
    if p.is_null() || !(p as usize).is_multiple_of(std::mem::align_of::<T>()) {
        return Err(purge::PurgeError::InvalidInput);
    }
    let h = p.cast::<u32>();
    if h.read() != 1 || h.add(1).read() != std::mem::size_of::<T>() as u32 {
        return Err(purge::PurgeError::InvalidInput);
    }
    Ok(&*p)
}
// The readiness allowance reserves these bounded repeated-token/envelope terms.
// Unsupported ABI widths fail compilation instead of overstating safe capacity.
const _: () = assert!(std::mem::size_of::<PurgeBindingC>() <= 128);
const _: () = assert!(std::mem::size_of::<purge::MemberBinding>() <= 128);
const _: () = assert!(std::mem::size_of::<PurgeAckC>() <= 1024);
const _: () = assert!(std::mem::size_of::<journal::Ack>() <= 1024);
unsafe fn purge_host_ops(p: *const PurgeHostOpsC) -> Result<PurgeHostOpsC, purge::PurgeError> {
    // Only fixed scalars and pointers: no allocation and no borrowed reference
    // to the caller's record survives an output write to ops.registry.
    Ok(*purge_versioned(p)?)
}
struct PurgeHostAdapter<'a>(&'a PurgeHostOpsC);
impl journal::Host for PurgeHostAdapter<'_> {
    fn supported(&self) -> bool {
        self.0.capabilities == 1
            && self.0.journal_identity.is_some()
            && self.0.publish.is_some()
            && self.0.execute.is_some()
    }
    fn journal_identity(&self) -> Result<[u8; 32], purge::PurgeError> {
        let callback = self
            .0
            .journal_identity
            .ok_or(purge::PurgeError::Unsupported)?;
        let mut bytes = [0; 32];
        let code = unsafe { callback(self.0.context, bytes.as_mut_ptr()) };
        if code != 0 {
            return Err(purge_receipt_error(code)?.unwrap_or(purge::PurgeError::HostFailure));
        }
        if &bytes[..8] != b"NZBPJID1" {
            return Err(purge::PurgeError::InvalidInput);
        }
        Ok(bytes)
    }
    fn publish(
        &mut self,
        sequence: u64,
        frame: &[u8],
    ) -> Result<journal::PublicationAck, purge::PurgeError> {
        let callback = self.0.publish.ok_or(purge::PurgeError::Unsupported)?;
        // A zeroed output is invalid until the callback explicitly fills v1.
        let mut out: PurgePublicationAckC = unsafe { std::mem::zeroed() };
        if unsafe { callback(self.0.context, sequence, purge_view(frame), &mut out) } != 0 {
            return Err(purge::PurgeError::Persistence);
        }
        unsafe {
            purge_versioned(&out)?;
        }
        let mut budget = purge::Budget::new();
        let bytes = unsafe { purge_raw_bytes(out.frame, purge::MAX_PAYLOAD, &mut budget)? };
        Ok(journal::PublicationAck {
            sequence: out.sequence,
            frame: purge::copy_bytes(bytes)?,
            file_synced: purge_bool(out.file_synced)?,
            published: purge_bool(out.published)?,
            directory_synced: purge_bool(out.directory_synced)?,
        })
    }
    fn execute(&mut self, c: &journal::Call<'_>) -> Result<journal::Ack, purge::PurgeError> {
        let callback = self.0.execute.ok_or(purge::PurgeError::Unsupported)?;
        let mut members = purge::reserved(c.identity.token.members.len())?;
        for b in &c.identity.token.members {
            members.push(purge_binding_view(b));
        }
        let identity = PurgeAckIdentityC {
            version: 1,
            struct_size: std::mem::size_of::<PurgeAckIdentityC>() as u32,
            domain: c.identity.domain,
            manifest_sequence: c.identity.manifest_sequence,
            request: c.identity.token.request,
            generation: c.identity.token.generation,
            members: members.as_ptr(),
            members_count: members.len() as u32,
            state_revision: c.identity.state_revision,
            receipt_sequence: c.identity.receipt_sequence,
            payload_digest: c.identity.payload_digest,
        };
        let action = match c.action {
            None => unsafe { std::mem::zeroed() },
            Some(purge::Action::RemoveEntry {
                entry_id,
                root_id,
                relative_path,
                kind,
                object_identity,
            }) => PurgeActionC {
                present: 1,
                recursive: 0,
                entry_id: *entry_id,
                root_id: *root_id,
                relative_path: purge_view(relative_path),
                kind: match kind {
                    purge::EntryKind::Regular => 0,
                    purge::EntryKind::Directory => 1,
                    purge::EntryKind::Symlink => 2,
                    purge::EntryKind::Unsupported => 3,
                },
                object_identity: purge_view(object_identity),
            },
            Some(purge::Action::RemoveTree {
                entry_id,
                root_id,
                relative_path,
                object_identity,
            }) => PurgeActionC {
                present: 1,
                recursive: 1,
                entry_id: *entry_id,
                root_id: *root_id,
                relative_path: purge_view(relative_path),
                kind: 1,
                object_identity: purge_view(object_identity),
            },
        };
        let j = c
            .plan
            .journal
            .as_ref()
            .ok_or(purge::PurgeError::CorruptJournal)?;
        let root_descriptor_identity = if action.present == 1 {
            purge_view(
                &c.plan
                    .snapshot
                    .roots
                    .iter()
                    .find(|r| r.id == action.root_id)
                    .ok_or(purge::PurgeError::CorruptJournal)?
                    .descriptor_identity,
            )
        } else {
            PurgeBytesC {
                data: std::ptr::null(),
                len: 0,
            }
        };
        let call = PurgeCallC {
            version: 1,
            struct_size: std::mem::size_of::<PurgeCallC>() as u32,
            identity,
            operation: match c.operation {
                journal::Operation::Fence => 1,
                journal::Operation::Drain => 2,
                journal::Operation::Revalidate => 3,
                journal::Operation::Remove => 4,
                journal::Operation::Reconcile => 5,
                journal::Operation::Cleanup => 6,
                journal::Operation::PersistRemoval => 7,
                journal::Operation::Release => 8,
            },
            action,
            root_descriptor_identity,
            completed_actions: j.actions.iter().filter(|a| a.outcome.is_some()).count() as u64,
            action_receipt_high_water: j
                .actions
                .iter()
                .map(|a| a.receipt_sequence)
                .max()
                .unwrap_or(0),
            cleanup_receipt_sequence: j
                .receipts
                .iter()
                .rev()
                .find(|r| {
                    r.operation == journal::Operation::Cleanup
                        && r.outcome == journal::Outcome::Acknowledged
                })
                .map(|r| r.receipt_sequence)
                .unwrap_or(0),
            receipt_budget: c.receipt_budget as u32,
            barrier_revision: c.barrier_revision,
            required_owners: c.required_owners.as_ptr(),
            required_owners_count: c.required_owners.len() as u32,
            inventory_revision: c.inventory_revision,
        };
        let mut out: PurgeAckC = unsafe { std::mem::zeroed() };
        if unsafe { callback(self.0.context, &call, &mut out) } != 0 {
            return Err(purge::PurgeError::HostFailure);
        }
        unsafe {
            purge_versioned(&out)?;
            purge_versioned(&out.identity)?;
        }
        let mut budget = purge::Budget::new();
        let bindings = unsafe {
            purge_slice(
                out.identity.members,
                out.identity.members_count,
                purge::MAX_MEMBERS,
                &mut budget,
            )?
        };
        let owners = unsafe {
            purge_slice(
                out.owners,
                out.owners_count,
                purge::MAX_MEMBERS,
                &mut budget,
            )?
        };
        for b in bindings {
            unsafe {
                purge_check_binding(b, &mut budget)?;
            }
        }
        let mut retained = purge::reserved(bindings.len())?;
        for b in bindings {
            retained.push(unsafe { purge_owned_binding(b)? });
        }
        let mut retained_owners = purge::reserved(owners.len())?;
        for o in owners {
            retained_owners.push(journal::OwnerBarrier {
                owner_kind: o.owner_kind,
                barrier_revision: o.barrier_revision,
                queued: o.queued,
                active: o.active,
                pinned: o.pinned,
                publication: o.publication,
                unacknowledged_deltas: o.unacknowledged_deltas,
            });
        }
        let outcome = match out.outcome {
            0 => journal::Outcome::Acknowledged,
            1 => journal::Outcome::Removed,
            2 => journal::Outcome::Absent,
            3 => journal::Outcome::IdentityMismatch,
            4 => journal::Outcome::Pending,
            5 => journal::Outcome::Failed,
            _ => return Err(purge::PurgeError::InvalidInput),
        };
        Ok(journal::Ack {
            identity: journal::AckIdentity {
                version: out.identity.version,
                domain: out.identity.domain,
                manifest_sequence: out.identity.manifest_sequence,
                token: purge::PurgeToken {
                    request: out.identity.request,
                    generation: out.identity.generation,
                    members: retained,
                },
                state_revision: out.identity.state_revision,
                receipt_sequence: out.identity.receipt_sequence,
                payload_digest: out.identity.payload_digest,
            },
            outcome,
            inventory_revision: out.inventory_revision,
            owners: retained_owners,
            fence: unsafe { purge_fence_receipt(out.fence, &mut budget)? },
            cleanup: unsafe { purge_cleanup_receipt(out.cleanup, &mut budget)? },
            persistence: unsafe { purge_persistence_receipt(out.persistence, &mut budget)? },
        })
    }
}
unsafe fn purge_registry_allocate(
    out: *mut *mut PurgeRegistryC,
    create: impl FnOnce() -> Result<journal::Registry, purge::PurgeError>,
) -> i32 {
    if out.is_null() {
        return purge_error_code(purge::PurgeError::InvalidInput);
    }
    out.write(std::ptr::null_mut());
    let layout = std::alloc::Layout::new::<PurgeRegistryC>();
    let p = std::alloc::alloc(layout).cast::<PurgeRegistryC>();
    if p.is_null() {
        return purge_error_code(purge::PurgeError::AllocationFailure);
    }
    match create() {
        Ok(inner) => {
            p.write(PurgeRegistryC { inner });
            out.write(p);
            0
        }
        Err(e) => {
            std::alloc::dealloc(p.cast(), layout);
            purge_error_code(e)
        }
    }
}
/// # Safety
/// Domain covers 16 bytes; ops and output are valid versioned records. Output
/// may be ops.registry; its fixed record is copied before output mutation. The host
/// owns an empty private descriptor domain and all callbacks catch exceptions.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_purge_registry_create(
    domain: *const u8,
    ops: *const PurgeHostOpsC,
    out: *mut *mut PurgeRegistryC,
) -> i32 {
    let copied_ops = purge_host_ops(ops);
    let mut id = [0; 16];
    if !domain.is_null() {
        std::ptr::copy_nonoverlapping(domain, id.as_mut_ptr(), 16);
    }
    if !out.is_null() {
        out.write(std::ptr::null_mut());
    }
    if domain.is_null() {
        return purge_error_code(purge::PurgeError::InvalidInput);
    }
    let ops = match copied_ops {
        Ok(v) => v,
        Err(e) => return purge_error_code(e),
    };
    purge_registry_allocate(out, || {
        journal::Registry::initialize(id, &mut PurgeHostAdapter(&ops))
    })
}
/// # Safety
/// Frames are the exhaustive checked descriptor scan in sequence order. Caller
/// must retain orphan/temporary files and fail the scan for ambiguous enumeration.
/// scan_complete=1 asserts that contract, not checksum authentication. Output
/// may be ops.registry, but must not overlap frame/byte storage or callback
/// pointer targets, which remain valid and immutable through this call.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_purge_registry_recover(
    domain: *const u8,
    frames: *const PurgeBytesC,
    count: u32,
    scan_complete: u32,
    ops: *const PurgeHostOpsC,
    out: *mut *mut PurgeRegistryC,
) -> i32 {
    let copied_ops = purge_host_ops(ops);
    let result = (|| {
        if domain.is_null() || scan_complete != 1 {
            return Err(purge::PurgeError::InvalidInput);
        }
        let mut budget = purge::Budget::new();
        let input = purge_slice(frames, count, journal::MAX_FRAMES, &mut budget)?;
        let mut views = purge::reserved(input.len())?;
        // Each frame retains its independent 64 MiB bound. Recovery additionally
        // bounds composed logical images and TOTAL retained owned state.
        for f in input {
            views.push(purge_raw_bytes(
                *f,
                purge::MAX_PAYLOAD,
                &mut purge::Budget::new(),
            )?);
        }
        let mut id = [0; 16];
        std::ptr::copy_nonoverlapping(domain, id.as_mut_ptr(), 16);
        let ops = copied_ops?;
        let host = PurgeHostAdapter(&ops);
        if !journal::Host::supported(&host) {
            return Err(purge::PurgeError::Unsupported);
        }
        Ok((id, journal::Host::journal_identity(&host)?, views))
    })();
    if !out.is_null() {
        out.write(std::ptr::null_mut());
    }
    match result {
        Ok((id, identity, views)) => {
            purge_registry_allocate(out, || journal::Registry::recover(id, identity, &views))
        }
        Err(e) => purge_error_code(e),
    }
}
/// # Safety
/// Nonnull handle is a live registry from create/recover with all users stopped.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_purge_registry_destroy(p: *mut PurgeRegistryC) {
    if !p.is_null() {
        std::ptr::drop_in_place(p);
        std::alloc::dealloc(p.cast(), std::alloc::Layout::new::<PurgeRegistryC>());
    }
}
unsafe fn purge_result(result: Result<(), purge::PurgeError>, out_error: *mut i32) -> i32 {
    let code = result.err().map(purge_error_code).unwrap_or(0);
    if !out_error.is_null() {
        out_error.write(code);
    }
    code
}
/// # Safety
/// handle is live prepare output. Registry is exclusively borrowed for this
/// entire call; callbacks cannot reenter or retain its views. No global lock is
/// acquired by Rust. Host must not hold queue/provider/global locks while waiting.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_purge_tick(
    handle: *const std::ffi::c_void,
    ops: *const PurgeHostOpsC,
    retry: u32,
    status: *mut PurgeStatusC,
    out_error: *mut i32,
) -> i32 {
    let copied_ops = purge_host_ops(ops);
    if !status.is_null() {
        status.write(PurgeStatusC {
            version: 1,
            struct_size: std::mem::size_of::<PurgeStatusC>() as u32,
            phase: 6,
            released: 0,
            generation: 0,
            state_revision: 0,
            receipt_sequence: 0,
            recovery_required: 0,
            error: 0,
        });
    }
    let result = (|| {
        if status.is_null() || out_error.is_null() {
            return Err(purge::PurgeError::InvalidInput);
        }
        let retry = purge_bool(retry)?;
        let ops = copied_ops?;
        let registry_ptr = ops.registry;
        if registry_ptr.is_null() {
            return Err(purge::PurgeError::RecoveryRequired);
        }
        let p = purge_record(handle.cast::<purge::Plan>())?;
        let mut host = PurgeHostAdapter(&ops);
        let owners = purge_slice(
            ops.required_owners,
            ops.required_owners_count,
            purge::MAX_MEMBERS,
            &mut purge::Budget::new(),
        )?;
        let registry = &mut (*registry_ptr).inner;
        let reserved = registry.reserve_prepared(p, owners, &mut host);
        let i = match reserved {
            Ok(journal::Reservation::Existing(i) | journal::Reservation::New(i)) => i,
            Ok(journal::Reservation::Conflict) => return Err(purge::PurgeError::Conflict),
            Err(e) => {
                (*status).recovery_required = u32::from(registry.requires_recovery());
                return Err(e);
            }
        };
        let advanced = if retry {
            registry.retry(i, &mut host).map(|_| ())
        } else {
            registry.advance(i, &mut host).map(|_| ())
        };
        let plan = registry.plan(i).ok_or(purge::PurgeError::InvalidInput)?;
        let state = plan
            .journal
            .as_ref()
            .ok_or(purge::PurgeError::InvalidInput)?;
        status.write(PurgeStatusC {
            version: 1,
            struct_size: std::mem::size_of::<PurgeStatusC>() as u32,
            phase: match plan.phase {
                purge::Phase::Prepared => 0,
                purge::Phase::Stopping => 1,
                purge::Phase::Quiescent => 2,
                purge::Phase::Cleaning => 3,
                purge::Phase::Persisting => 4,
                purge::Phase::Complete => 5,
                purge::Phase::Failed => 6,
            },
            released: u32::from(state.released && !registry.requires_recovery()),
            generation: plan.token.generation,
            state_revision: state.state_revision,
            receipt_sequence: state.receipt_sequence,
            recovery_required: u32::from(registry.requires_recovery()),
            error: state.error.map(purge_error_code).unwrap_or(0),
        });
        advanced
    })();
    let code = purge_result(result, out_error);
    if !status.is_null() {
        (*status).error = code;
    }
    code
}
/// # Safety
/// Binding/delta are accessible immutable records for the call; registry and
/// callback lifetimes/serialization follow tick. Only a durable ACK returns 0.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_purge_inventory_store(
    binding: *const PurgeBindingC,
    delta: *const PurgeOutputDeltaC,
    ops: *const PurgeHostOpsC,
    out_error: *mut i32,
) -> i32 {
    let copied_ops = purge_host_ops(ops);
    let result = (|| {
        if out_error.is_null() {
            return Err(purge::PurgeError::InvalidInput);
        }
        let ops = copied_ops?;
        let registry_ptr = ops.registry;
        if registry_ptr.is_null() {
            return Err(purge::PurgeError::RecoveryRequired);
        }
        let binding = purge_record(binding)?;
        let delta = purge_versioned(delta)?;
        let mut budget = purge::Budget::new();
        purge_check_binding(binding, &mut budget)?;
        let changes = purge_slice(
            delta.changes,
            delta.changes_count,
            purge::MAX_ENTRIES,
            &mut budget,
        )?;
        budget.add(
            changes
                .len()
                .checked_mul(
                    std::mem::size_of::<journal::InventoryChange>()
                        .saturating_sub(std::mem::size_of::<PurgeInventoryChangeC>()),
                )
                .ok_or(purge::PurgeError::LimitExceeded)?,
        )?;
        for c in changes {
            purge_bool(c.prior_absent)?;
            if !c.before.is_null() {
                purge_check_entry(purge_record(c.before)?, &mut budget)?;
            }
            if !c.after.is_null() {
                purge_check_entry(purge_record(c.after)?, &mut budget)?;
            }
            if !c.ownership.is_null() {
                purge_check_ownership(purge_record(c.ownership)?, &mut budget)?;
            }
        }
        let final_for_producer = purge_bool(delta.final_for_producer)?;
        let mut owned = purge::reserved(changes.len())?;
        for c in changes {
            owned.push(journal::InventoryChange {
                entry_id: c.entry_id,
                before: if c.before.is_null() {
                    None
                } else {
                    Some(purge_owned_entry(&*c.before)?)
                },
                after: if c.after.is_null() {
                    None
                } else {
                    Some(purge_owned_entry(&*c.after)?)
                },
                ownership: if c.ownership.is_null() {
                    None
                } else {
                    Some(purge_owned_ownership(&*c.ownership)?)
                },
                prior_absent: c.prior_absent == 1,
            });
        }
        (*registry_ptr).inner.store_inventory(
            purge_owned_binding(binding)?,
            journal::OutputDelta {
                producer_ticket: delta.producer_ticket,
                sequence: delta.sequence,
                prior_inventory_revision: delta.prior_inventory_revision,
                changes: owned,
                final_for_producer,
            },
            &mut PurgeHostAdapter(&ops),
        )
    })();
    purge_result(result, out_error)
}

unsafe fn purge_fence_receipt(
    p: *const PurgeFenceReceiptC,
    b: &mut purge::Budget,
) -> Result<Option<journal::FenceReceipt>, purge::PurgeError> {
    if p.is_null() {
        return Ok(None);
    }
    let f = purge_record(p)?;
    b.table::<journal::FenceReceipt>(1)?;
    let values = purge_slice(f.dispositions, f.dispositions_count, purge::MAX_MEMBERS, b)?;
    let already_fenced = purge_bool(f.already_fenced)?;
    let mut dispositions = purge::reserved(values.len())?;
    for v in values {
        dispositions.push(journal::StopDisposition {
            owner_kind: v.owner_kind,
            barrier_revision: v.barrier_revision,
            disposition: v.disposition,
        });
    }
    Ok(Some(journal::FenceReceipt {
        already_fenced,
        dispositions,
    }))
}
unsafe fn purge_owner_coverage(
    p: *const PurgeOwnerCoverageC,
    n: u32,
    b: &mut purge::Budget,
) -> Result<Vec<journal::OwnerCoverage>, purge::PurgeError> {
    let values = purge_slice(p, n, purge::MAX_MEMBERS, b)?;
    let mut out = purge::reserved(values.len())?;
    for v in values {
        out.push(journal::OwnerCoverage {
            owner_kind: v.owner_kind,
            barrier_revision: v.barrier_revision,
            complete: purge_bool(v.complete)?,
        });
    }
    Ok(out)
}
unsafe fn purge_store_receipts(
    p: *const PurgeStoreReceiptC,
    n: u32,
    b: &mut purge::Budget,
) -> Result<Vec<journal::StoreReceipt>, purge::PurgeError> {
    let values = purge_slice(p, n, purge::MAX_ENTRIES, b)?;
    b.add(
        values
            .len()
            .checked_mul(
                std::mem::size_of::<journal::StoreReceipt>()
                    .saturating_sub(std::mem::size_of::<PurgeStoreReceiptC>()),
            )
            .ok_or(purge::PurgeError::LimitExceeded)?,
    )?;
    for v in values {
        purge_raw_bytes(v.identity, purge::MAX_IDENTITY, b)?;
        purge_bool(v.file_synced)?;
        purge_bool(v.published)?;
        purge_bool(v.directory_synced)?;
    }
    let mut out = purge::reserved(values.len())?;
    for v in values {
        out.push(journal::StoreReceipt {
            identity: purge_owned_bytes(v.identity)?,
            revision: v.revision,
            file_synced: v.file_synced == 1,
            published: v.published == 1,
            directory_synced: v.directory_synced == 1,
        });
    }
    Ok(out)
}
fn purge_receipt_error(code: i32) -> Result<Option<purge::PurgeError>, purge::PurgeError> {
    use purge::PurgeError::*;
    Ok(match code {
        0 => None,
        1 => Some(InvalidInput),
        2 => Some(MissingSeed),
        3 => Some(MissingInventory),
        4 => Some(Conflict),
        5 => Some(LimitExceeded),
        6 => Some(Unsupported),
        7 => Some(AllocationFailure),
        8 => Some(Persistence),
        9 => Some(HostFailure),
        10 => Some(CorruptJournal),
        11 => Some(RecoveryRequired),
        _ => return Err(InvalidInput),
    })
}
unsafe fn purge_cleanup_receipt(
    p: *const PurgeCleanupReceiptC,
    b: &mut purge::Budget,
) -> Result<Option<journal::CleanupReceipt>, purge::PurgeError> {
    if p.is_null() {
        return Ok(None);
    }
    let c = purge_record(p)?;
    b.table::<journal::CleanupReceipt>(1)?;
    let owners = purge_owner_coverage(c.owners, c.owners_count, b)?;
    let coverage = purge_slice(c.coverage, c.coverage_count, 7, b)?;
    let artifacts = purge_slice(c.artifacts, c.artifacts_count, purge::MAX_ENTRIES, b)?;
    b.add(
        artifacts
            .len()
            .checked_mul(
                std::mem::size_of::<journal::ArtifactReceipt>()
                    .saturating_sub(std::mem::size_of::<PurgeArtifactReceiptC>()),
            )
            .ok_or(purge::PurgeError::LimitExceeded)?,
    )?;
    // Preflight every nested byte before allocating any artifact record.
    for a in artifacts {
        purge_check_binding(&a.member, b)?;
        purge_raw_bytes(a.identity, purge::MAX_IDENTITY, b)?;
        purge_raw_bytes(a.retained_store, purge::MAX_IDENTITY, b)?;
        if !purge_bool(a.has_entry)? && a.entry_id != 0 {
            return Err(purge::PurgeError::InvalidInput);
        }
        purge_receipt_error(a.error)?;
        if !matches!(a.outcome, 1 | 2 | 5) {
            return Err(purge::PurgeError::InvalidInput);
        }
    }
    let stores = purge_store_receipts(c.stores, c.stores_count, b)?;
    let mut covered = purge::reserved(coverage.len())?;
    for v in coverage {
        covered.push(journal::ArtifactCoverage {
            kind: v.kind,
            count: v.count,
            complete: purge_bool(v.complete)?,
        });
    }
    let mut retained = purge::reserved(artifacts.len())?;
    for a in artifacts {
        retained.push(journal::ArtifactReceipt {
            kind: a.kind,
            member: purge_owned_binding(&a.member)?,
            identity: purge_owned_bytes(a.identity)?,
            entry_id: if a.has_entry == 1 {
                Some(a.entry_id)
            } else {
                None
            },
            outcome: match a.outcome {
                1 => journal::Outcome::Removed,
                2 => journal::Outcome::Absent,
                _ => journal::Outcome::Failed,
            },
            error: purge_receipt_error(a.error)?,
            retained_store: if a.retained_store.len == 0 {
                None
            } else {
                Some(purge_owned_bytes(a.retained_store)?)
            },
        });
    }
    Ok(Some(journal::CleanupReceipt {
        owners,
        coverage: covered,
        artifacts: retained,
        stores,
    }))
}
unsafe fn purge_persistence_receipt(
    p: *const PurgePersistenceReceiptC,
    b: &mut purge::Budget,
) -> Result<Option<journal::PersistenceReceipt>, purge::PurgeError> {
    if p.is_null() {
        return Ok(None);
    }
    let v = purge_record(p)?;
    b.table::<journal::PersistenceReceipt>(1)?;
    Ok(Some(journal::PersistenceReceipt {
        completed_actions: v.completed_actions,
        action_receipt_high_water: v.action_receipt_high_water,
        cleanup_receipt_sequence: v.cleanup_receipt_sequence,
        owners: purge_owner_coverage(v.owners, v.owners_count, b)?,
        stores: purge_store_receipts(v.stores, v.stores_count, b)?,
    }))
}

mod purge_inventory;
mod purge_subjects;

mod purge_intake;

#[cfg(test)]
mod purge_snapshot_review_tests {
    use super::*;

    #[test]
    fn review_snapshot_owned_width_overflow_refuses_before_conversion() {
        // One real 64KiB backing is safely aliased by initialized C spans.
        // No huge allocation, dangling pointer, or allocation-failure injection.
        // The baseline reaches the final invalid bool; the corrected preflight
        // rejects owned fixed widths before conversion allocates anything.
        let bytes = vec![b'x'; purge::MAX_IDENTITY];
        let mut members: Vec<PurgeMemberC> = vec![unsafe { std::mem::zeroed() }; 512];
        let mut remaining =
            purge::MAX_PAYLOAD - members.len() * std::mem::size_of::<PurgeMemberC>();
        assert!(std::mem::size_of::<purge::MemberRecord>() > std::mem::size_of::<PurgeMemberC>());
        for member in &mut members {
            for span in [
                &mut member.binding.posting_source,
                &mut member.name,
                &mut member.key,
            ] {
                let count = remaining.min(bytes.len());
                remaining -= count;
                *span = PurgeBytesC {
                    data: bytes.as_ptr(),
                    len: count as u32,
                };
            }
        }
        assert_eq!(remaining, 0);
        members.last_mut().unwrap().complete_inventory = 2;
        let mut snapshot: PurgeSnapshotC = unsafe { std::mem::zeroed() };
        snapshot.members = members.as_ptr();
        snapshot.members_count = members.len() as u32;
        let (result, allocations) =
            crate::allocation_probe::measure(|| unsafe { purge_snapshot(&snapshot) });
        assert_eq!(
            result.err(),
            Some(purge::PurgeError::LimitExceeded),
            "PR108_F2_OWNED_WIDTH_PREFLIGHT"
        );
        assert_eq!(
            allocations.calls, 0,
            "ABI preflight allocated before rejecting fixed owned widths"
        );
    }

    fn exact_owned_width<T>() {
        let width = std::mem::size_of::<T>();
        assert!(width > 1);
        let mut exact = purge::Budget::new();
        exact.add(purge::MAX_PAYLOAD - width).unwrap();
        exact.table::<T>(1).unwrap();
        assert_eq!(exact.add(1), Err(purge::PurgeError::LimitExceeded));
        let mut short = purge::Budget::new();
        short.add(purge::MAX_PAYLOAD - width + 1).unwrap();
        assert_eq!(short.table::<T>(1), Err(purge::PurgeError::LimitExceeded));
        let mut overflow = purge::Budget::new();
        assert_eq!(
            overflow.table::<T>(usize::MAX / width + 1),
            Err(purge::PurgeError::LimitExceeded)
        );
    }
    #[test]
    fn review_all_converted_owned_record_widths_have_checked_boundaries() {
        // Existing pure Budget API: actual Rust widths, no 64MiB allocation.
        exact_owned_width::<purge::MemberBinding>();
        exact_owned_width::<purge::MemberRecord>();
        exact_owned_width::<purge::SourceValidation>();
        exact_owned_width::<purge::CustodyReceipt>();
        exact_owned_width::<purge::ParentMembership>();
        exact_owned_width::<purge::AnchorComparison>();
        exact_owned_width::<purge::DirectBinding>();
        exact_owned_width::<purge::PairMembershipReceipt>();
        exact_owned_width::<purge::PairCustodyReceipt>();
        exact_owned_width::<purge::RootRecord>();
        exact_owned_width::<purge::NamespaceProof>();
        exact_owned_width::<purge::InventoryEntry>();
        exact_owned_width::<purge::OwnershipReceipt>();
        exact_owned_width::<purge::OutsideClaim>();
    }
    #[test]
    fn review_snapshot_small_owned_records_and_nested_bytes_still_convert() {
        let identity = b"root";
        let roots = [PurgeRootC {
            id: 1,
            descriptor_identity: PurgeBytesC {
                data: identity.as_ptr(),
                len: 4,
            },
            configured_container: 1,
        }];
        let mut snapshot: PurgeSnapshotC = unsafe { std::mem::zeroed() };
        snapshot.roots = roots.as_ptr();
        snapshot.roots_count = 1;
        let owned = unsafe { purge_snapshot(&snapshot) }.unwrap();
        assert_eq!(owned.roots.len(), 1);
        assert_eq!(owned.roots[0].id, 1);
        assert_eq!(owned.roots[0].descriptor_identity, b"root");
        assert!(owned.roots[0].configured_container);
        assert_ne!(
            owned.roots[0].descriptor_identity.as_ptr(),
            identity.as_ptr()
        );
    }
}

// ============================================================================
// Daemon Lifecycle Coordination C ABI (Requirement R2, Milestone M5)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_register_lifecycle_hooks(
    hooks: *const crate::lifecycle::LifecycleHooks,
) {
    if let Some(hooks) = hooks.as_ref() {
        crate::lifecycle::register_hooks(*hooks);
    }
}

#[cfg(test)]
mod lifecycle_ffi_tests {
    #[test]
    fn test_lifecycle_ffi_queries() {
        let _guard = crate::lifecycle::TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mgr = crate::lifecycle::LifecycleManager::global();
        mgr.set_phase(1.into());
        assert_eq!(mgr.get_phase() as u32, 1);
        mgr.set_phase(0.into());
        assert_eq!(mgr.get_phase() as u32, 0);

        assert!(!mgr.is_reloading());
        assert!(!mgr.is_stopping());

        mgr.notify_signal(0);
    }
}

// ============================================================================
// DupeArticleFallback, ParDamage, DupeCoordinator & QueueCoordinator C ABI
// ============================================================================

#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_op(
    op: c_int,
    a1: c_int,
    p1: usize,
    host_cb: Option<crate::queuesubsys_host::NzbgetRsQueueSubsysHostFn>,
) -> c_int {
    let _cancel_guard = PosixCancelGuard::disable();
    crate::dupefallback::dupe_fallback_static_op(op, a1, p1, host_cb)
}

#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_coord_exec(
    op: c_int,
    a1: c_int,
    p1: usize,
    p2: usize,
    p3: usize,
    s1: *const c_char,
    s2: *const c_char,
    host_cb: Option<crate::queuesubsys_host::NzbgetRsQueueSubsysHostFn>,
) -> usize {
    let _cancel_guard = PosixCancelGuard::disable();
    crate::dupecoord::dupe_coord_exec(op, a1, p1, p2, p3, s1, s2, host_cb)
}

#[cfg(unix)]
#[no_mangle]
pub extern "C" fn nzbget_rs_queue_coord_new() -> *mut crate::queuecoord::NzbgetRsQueueCoordinator {
    Box::into_raw(Box::new(crate::queuecoord::NzbgetRsQueueCoordinator::new()))
}

#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_queue_coord_free(
    ptr: *mut crate::queuecoord::NzbgetRsQueueCoordinator,
) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}

#[cfg(unix)]
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nzbget_rs_queue_coord_exec(
    ptr: *const crate::queuecoord::NzbgetRsQueueCoordinator,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    p3: usize,
    s1: *const c_char,
    ints: *const c_int,
    host_cb: Option<crate::queuesubsys_host::NzbgetRsQueueSubsysHostFn>,
    ctx: *mut std::ffi::c_void,
) -> usize {
    let Some(coord) = ptr.as_ref() else {
        return 0;
    };
    let _cancel_guard = PosixCancelGuard::disable();
    coord.exec(op, a1, a2, p1, p2, p3, s1, ints, host_cb, ctx)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_new(
    base_dir: *const c_char,
    threads_to_use: c_int,
    mem_to_use_mb: c_int,
    par_quick: c_int,
    par_scan_full: c_int,
    par_ignore_ext: *const c_char,
) -> *mut crate::parchecker::RustParRepairer {
    let dir_path = cstr_to_path_buf(base_dir);
    let ignore_bytes = if par_ignore_ext.is_null() {
        Vec::new()
    } else {
        CStr::from_ptr(par_ignore_ext).to_bytes().to_vec()
    };
    Box::into_raw(Box::new(crate::parchecker::RustParRepairer::new(
        dir_path,
        threads_to_use,
        mem_to_use_mb,
        par_quick != 0,
        par_scan_full != 0,
        ignore_bytes,
    )))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_free(
    handle: *mut crate::parchecker::RustParRepairer,
) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_cancel(
    handle: *const crate::parchecker::RustParRepairer,
) {
    if let Some(r) = handle.as_ref() {
        r.cancel();
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_preprocess(
    handle: *mut crate::parchecker::RustParRepairer,
    par_filename: *const c_char,
    cbs: *const crate::parchecker::NzbgetRsParCallbacks,
) -> c_int {
    let Some(r) = handle.as_mut() else {
        return crate::parchecker::PAR_RESULT_LOGIC_ERROR;
    };
    if par_filename.is_null() {
        return crate::parchecker::PAR_RESULT_INVALID_ARGS;
    }
    let pf_path = cstr_to_path_buf(par_filename);
    r.preprocess(&pf_path, cbs.as_ref())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_process(
    handle: *mut crate::parchecker::RustParRepairer,
    do_repair: c_int,
    cbs: *const crate::parchecker::NzbgetRsParCallbacks,
) -> c_int {
    let Some(r) = handle.as_mut() else {
        return crate::parchecker::PAR_RESULT_LOGIC_ERROR;
    };
    let empty_cbs = crate::parchecker::NzbgetRsParCallbacks {
        ctx: std::ptr::null_mut(),
        on_filename: None,
        on_progress: None,
        on_done: None,
        on_quick_verify: None,
        on_quick_verified_inc: None,
    };
    let cbs_ref = cbs.as_ref().unwrap_or(&empty_cbs);
    r.process(do_repair != 0, cbs_ref)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_load_extra_par2_file(
    handle: *mut crate::parchecker::RustParRepairer,
    path: *const c_char,
    cbs: *const crate::parchecker::NzbgetRsParCallbacks,
) -> c_int {
    let Some(r) = handle.as_mut() else {
        return 0;
    };
    if path.is_null() {
        return 0;
    }
    let p_path = cstr_to_path_buf(path);
    c_int::from(r.load_extra_par2_file(&p_path, cbs.as_ref()))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_verify_extra_file(
    handle: *mut crate::parchecker::RustParRepairer,
    path: *const c_char,
    is_split_fragment: c_int,
    cbs: *const crate::parchecker::NzbgetRsParCallbacks,
) -> c_int {
    let Some(r) = handle.as_mut() else {
        return 0;
    };
    if path.is_null() {
        return 0;
    }
    let p_path = cstr_to_path_buf(path);
    let empty_cbs = crate::parchecker::NzbgetRsParCallbacks {
        ctx: std::ptr::null_mut(),
        on_filename: None,
        on_progress: None,
        on_done: None,
        on_quick_verify: None,
        on_quick_verified_inc: None,
    };
    let cbs_ref = cbs.as_ref().unwrap_or(&empty_cbs);
    c_int::from(r.verify_extra_file(&p_path, is_split_fragment != 0, cbs_ref))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_missing_file_count(
    handle: *const crate::parchecker::RustParRepairer,
) -> u32 {
    handle.as_ref().map_or(0, |r| r.missing_file_count())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_damaged_file_count(
    handle: *const crate::parchecker::RustParRepairer,
) -> u32 {
    handle.as_ref().map_or(0, |r| r.damaged_file_count())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_missing_block_count(
    handle: *const crate::parchecker::RustParRepairer,
) -> u32 {
    handle.as_ref().map_or(0, |r| r.missing_block_count())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_recovery_block_count(
    handle: *const crate::parchecker::RustParRepairer,
) -> u32 {
    handle.as_ref().map_or(0, |r| r.recovery_block_count())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_block_size(
    handle: *const crate::parchecker::RustParRepairer,
) -> i64 {
    handle.as_ref().map_or(0, |r| r.block_size())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_creator(
    handle: *const crate::parchecker::RustParRepairer,
) -> RsBuf {
    let Some(r) = handle.as_ref() else {
        return into_buf(Vec::new());
    };
    into_buf(r.creator().into_bytes())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_repairer_source_file_count(
    handle: *const crate::parchecker::RustParRepairer,
) -> usize {
    handle.as_ref().map_or(0, |r| r.source_files().len())
}


#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_parse_message(
    text: *const c_char,
    unpack_extended_dir: *const c_char,
    is_seven_zip: c_int,
    out_info: *mut crate::unpack::NzbgetRsUnpackMessageInfo,
) {
    if out_info.is_null() {
        return;
    }
    let t = if text.is_null() {
        b""
    } else {
        CStr::from_ptr(text).to_bytes()
    };
    let d = if unpack_extended_dir.is_null() {
        b""
    } else {
        CStr::from_ptr(unpack_extended_dir).to_bytes()
    };
    *out_info = crate::unpack::parse_unpack_message(t, d, is_seven_zip != 0);
}

// ============================================================================
// QueueCoordinator, ArticleWriter, HistoryCoordinator, QueueEditor &
// DownloadInfo Hot-Path FFI (`rust/src/queuehotpath.rs`)
// ============================================================================

// ============================================================================
// DiskState, PrePostProcessor, Scanner & TwinCheck Hot-Path FFI (`rust/src/diskstatepp.rs`)
// ============================================================================

// ============================================================================
// DupeSearch, Fleet, DonorHealth, Newznab, FeedCoordinator, FeedInfo &
// FeedFile Hot-Path FFI (`rust/src/dupesearch_nzb.rs` / `rust/src/feed.rs`)
// ============================================================================

// ============================================================================
// StatMeter, WebProcessor, UrlCoordinator, DirectRenamer, NzbFile,
// QueueScript, PostScript, ScanScript, ScriptConfig & ScriptController FFI
// (`rust/src/statmeter.rs` / `rust/src/extension.rs`)
// ============================================================================

// ============================================================================
// XmlRpc, RemoteServer, Connection, WebDownloader, Options,
// Log & NString hot-path FFI
// ============================================================================

// ============================================================================
// System, NServ, DupeSearch, Connect, PostRenamer & SystemHealth Hot-Path FFI
// ============================================================================

// ============================================================================
// Consolidated ArticleDownloader & ServerPool Hot-Path FFI (`rust/src/nntp.rs`)
// ============================================================================

// ============================================================================
// ArticleWriter, ArticleCache & MemoryMonitor FFI (`rust/src/artwritercore.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_build_output_filenames(
    tmp_dir: *const c_char,
    dest_dir: *const c_char,
    sep: c_char,
    file_id: c_int,
    part_num: c_int,
    direct_write: c_int,
    force_direct_write: c_int,
    existing_output_filename: *const c_char,
    output_initialized: c_int,
    file_filename: *const c_char,
    out_result_filename: *mut RsBuf,
    out_temp_filename: *mut RsBuf,
    out_output_filename: *mut RsBuf,
    out_has_output_filename: *mut c_int,
    out_update_file_info_output_filename: *mut c_int,
) -> c_int {
    let t_dir = if tmp_dir.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(tmp_dir).to_bytes()
    };
    let d_dir = if dest_dir.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(dest_dir).to_bytes()
    };
    let existing = if existing_output_filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(existing_output_filename).to_bytes()
    };
    let fname = if file_filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(file_filename).to_bytes()
    };
    let Some(built) = crate::artwritercore::article_writer_build_output_filenames(
        t_dir,
        d_dir,
        sep as u8,
        file_id,
        part_num,
        direct_write != 0,
        force_direct_write != 0,
        existing,
        output_initialized != 0,
        fname,
        |path_bytes| crate::fsops::file_exists(path_bytes, &crate::fsops::posix_stat),
    ) else {
        return 0;
    };
    if !out_result_filename.is_null() {
        *out_result_filename = into_buf(built.result_filename);
    }
    if !out_temp_filename.is_null() {
        *out_temp_filename = into_buf(built.temp_filename);
    }
    if !out_has_output_filename.is_null() {
        *out_has_output_filename = i32::from(built.output_filename.is_some());
    }
    if !out_output_filename.is_null() {
        *out_output_filename = into_buf(built.output_filename.unwrap_or_default());
    }
    if !out_update_file_info_output_filename.is_null() {
        *out_update_file_info_output_filename = i32::from(built.update_file_info_output_filename);
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_execute_setup_output_file(
    dest_dir: *const c_char,
    dest_dir_len: usize,
    filename: *const c_char,
    filename_len: usize,
    sep: c_char,
    raw_article: c_int,
    direct_write: c_int,
    force_direct_write: c_int,
    output_filename: *const c_char,
    output_filename_len: usize,
    cb_ctx: *mut std::ffi::c_void,
    open_file_cb: Option<
        unsafe extern "C" fn(
            ctx: *mut std::ffi::c_void,
            path: *const c_char,
            read_write: c_int,
        ) -> c_int,
    >,
    out_final_path: *mut RsBuf,
    out_temp_path: *mut RsBuf,
    out_updated_output_filename: *mut RsBuf,
) -> c_int {
    let d_dir = if dest_dir.is_null() || dest_dir_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(dest_dir.cast::<u8>(), dest_dir_len)
    };
    let fname = if filename.is_null() || filename_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(filename.cast::<u8>(), filename_len)
    };
    let out_fn = if output_filename.is_null() || output_filename_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(output_filename.cast::<u8>(), output_filename_len)
    };
    let mut force_dirs_err = Vec::new();
    let res = crate::artwritercore::article_writer_execute_setup_output_file(
        d_dir,
        fname,
        sep as u8,
        raw_article != 0,
        direct_write != 0,
        force_direct_write != 0,
        out_fn,
        |a, b| a == b,
        |dir_bytes, fname_bytes| {
            let c_dir = CString::new(dir_bytes).unwrap_or_default();
            let c_fn = CString::new(fname_bytes).unwrap_or_default();
            crate::fsops::make_unique_filename(&c_dir, &c_fn, &crate::fsops::posix_stat)
        },
        |path_bytes| crate::fsops::file_exists(path_bytes, &crate::fsops::posix_stat),
        |path_bytes| {
            crate::fsops::force_directories(
                path_bytes,
                &mut force_dirs_err,
                &crate::fsops::posix_stat,
            )
        },
        |path_bytes| CString::new(path_bytes).is_ok_and(|c| libc::unlink(c.as_ptr()) == 0),
        |path_bytes, read_write| {
            let Some(cb) = open_file_cb else {
                return false;
            };
            let mut c_path = Vec::with_capacity(path_bytes.len() + 1);
            c_path.extend_from_slice(path_bytes);
            c_path.push(0);
            cb(cb_ctx, c_path.as_ptr().cast(), i32::from(read_write)) != 0
        },
        |path_bytes| {
            if let Ok(c_path) = CString::new(path_bytes) {
                let _ = libc::mkdir(
                    c_path.as_ptr(),
                    libc::S_IRWXU | libc::S_IRWXG | libc::S_IRWXO,
                );
            }
            crate::fsops::directory_exists(path_bytes, &crate::fsops::posix_stat)
        },
    );
    if !out_final_path.is_null() {
        *out_final_path = into_buf(res.final_path);
    }
    if !out_temp_path.is_null() {
        *out_temp_path = into_buf(res.temp_path);
    }
    if !out_updated_output_filename.is_null() {
        *out_updated_output_filename = into_buf(res.updated_output_filename);
    }
    if let Some(cb) = open_file_cb {
        if let Some(pdir) = res.parent_dir_failed {
            let msg = crate::artwritercore::join_c_slices(&[
                b"Could not create directory ",
                &pdir,
                b": ",
                &force_dirs_err,
            ]);
            let c_msg = nul(&msg);
            cb(cb_ctx, c_msg.as_ptr().cast(), -1);
        }
        if !res.ok && res.error_kind != 0 {
            let err_msg = crate::fsops::error_message(crate::fsops::last_errno());
            let verb: &[u8] = match res.error_kind {
                1 => b"create file",
                2 => b"open file",
                _ => b"create directory",
            };
            let msg = crate::artwritercore::join_c_slices(&[
                b"Could not ",
                verb,
                b" ",
                &res.error_path,
                b": ",
                &err_msg,
            ]);
            let c_msg = nul(&msg);
            cb(cb_ctx, c_msg.as_ptr().cast(), -1);
        }
    }
    i32::from(res.ok)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_execute_cleanup_old_data(
    direct_write: c_int,
    nzb_dest_dir: *const c_char,
    nzb_dest_dir_len: usize,
    final_output_path: *const c_char,
    final_output_path_len: usize,
    output_filename: *const c_char,
    output_filename_len: usize,
    is_hard_linked: c_int,
    inter_dir_empty: c_int,
    sep1: c_char,
    sep2: c_char,
    dupe_fallback_rounds: *const c_int,
    result_filenames: *const *const c_char,
    articles_len: usize,
    log_ctx: *mut std::ffi::c_void,
    log_cb: Option<
        unsafe extern "C" fn(ctx: *mut std::ffi::c_void, is_debug: c_int, msg: *const c_char),
    >,
    out_clear_output_filename: *mut c_int,
) {
    let d_dir = if nzb_dest_dir.is_null() || nzb_dest_dir_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(nzb_dest_dir.cast::<u8>(), nzb_dest_dir_len)
    };
    let final_path = if final_output_path.is_null() || final_output_path_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(final_output_path.cast::<u8>(), final_output_path_len)
    };
    let out_fn = if output_filename.is_null() || output_filename_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(output_filename.cast::<u8>(), output_filename_len)
    };
    let mut articles = Vec::with_capacity(articles_len);
    if !dupe_fallback_rounds.is_null() && !result_filenames.is_null() && articles_len > 0 {
        let rounds = std::slice::from_raw_parts(dupe_fallback_rounds, articles_len);
        let rfs = std::slice::from_raw_parts(result_filenames, articles_len);
        for (&r, &rf_ptr) in rounds.iter().zip(rfs.iter()) {
            let rf = if rf_ptr.is_null() {
                None
            } else {
                Some(CStr::from_ptr(rf_ptr).to_bytes())
            };
            articles.push((r, rf));
        }
    }
    let mut move_error = Vec::new();
    let res = crate::artwritercore::article_writer_execute_cleanup_old_data(
        direct_write != 0,
        d_dir,
        final_path,
        out_fn,
        is_hard_linked != 0,
        inter_dir_empty != 0,
        sep1 as u8,
        sep2 as u8,
        &articles,
        |a, b| a == b,
        |path_bytes| crate::fsops::file_exists(path_bytes, &crate::fsops::posix_stat),
        |path_bytes| CString::new(path_bytes).is_ok_and(|c| libc::unlink(c.as_ptr()) == 0),
        |from_bytes, to_bytes| {
            let Ok(c_from) = CString::new(from_bytes) else {
                return false;
            };
            let Ok(c_to) = CString::new(to_bytes) else {
                return false;
            };
            if crate::fsops::move_file(&c_from, &c_to) {
                true
            } else {
                let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                move_error = crate::fsops::error_message(err);
                false
            }
        },
        |dir_bytes| {
            let c_dir = CString::new(dir_bytes).ok();
            let mut browser = crate::fsops::PosixDirBrowser::new(c_dir.as_deref(), true);
            browser.next_ptr().is_null()
        },
        |dir_bytes| CString::new(dir_bytes).is_ok_and(|c| crate::fsops::remove_directory(&c)),
    );
    if let Some(cb) = log_cb {
        if res.move_failed {
            let msg = crate::artwritercore::join_c_slices(&[
                b"Could not move file ",
                out_fn,
                b" to ",
                final_path,
                b": ",
                &move_error,
            ]);
            let c_msg = nul(&msg);
            cb(log_ctx, 0, c_msg.as_ptr().cast());
        }
        if res.checked_old_dest_dir.is_some() {
            let msg = crate::artwritercore::join_c_slices(&[b"Checking old dir for: ", out_fn]);
            let c_msg = nul(&msg);
            cb(log_ctx, 1, c_msg.as_ptr().cast());
        }
        if let Some(del_dir) = res.deleted_old_dest_dir {
            let msg = crate::artwritercore::join_c_slices(&[b"Deleting old dir: ", &del_dir]);
            let c_msg = nul(&msg);
            cb(log_ctx, 1, c_msg.as_ptr().cast());
        }
    }
    if !out_clear_output_filename.is_null() {
        *out_clear_output_filename = i32::from(res.clear_output_filename);
    }
}

#[repr(C)]
pub struct NzbgetRsProcessArticleItem {
    pub segment_offset: i64,
    pub segment_size: i32,
    pub crc: u32,
    pub part_number: i32,
    pub dupe_fallback_round: i32,
    pub segment_content: *const u8,
    pub result_filename: *const c_char,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_process_articles(
    items: *const NzbgetRsProcessArticleItem,
    items_len: usize,
    total_articles_count: usize,
    info_filename: *const c_char,
    info_filename_len: usize,
    final_output_path: *const c_char,
    final_output_path_len: usize,
    raw_article: c_int,
    direct_write: c_int,
    skip_disk_write: c_int,
    is_yenc: c_int,
    ctx: *mut std::ffi::c_void,
    io_cb: Option<
        unsafe extern "C" fn(
            ctx: *mut std::ffi::c_void,
            op: c_int,
            val: i64,
            ptr: *const std::ffi::c_void,
            len: usize,
        ) -> i64,
    >,
    out_commit_error: *mut c_int,
    out_failed_delta: *mut c_int,
) -> u32 {
    let info_fn = if info_filename.is_null() || info_filename_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(info_filename.cast::<u8>(), info_filename_len)
    };
    let final_path = if final_output_path.is_null() || final_output_path_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(final_output_path.cast::<u8>(), final_output_path_len)
    };
    let mut rs_items = Vec::with_capacity(items_len);
    if !items.is_null() && items_len > 0 {
        for raw in std::slice::from_raw_parts(items, items_len) {
            let content = if raw.segment_content.is_null() {
                None
            } else {
                Some(std::slice::from_raw_parts(
                    raw.segment_content,
                    raw.segment_size.max(0) as usize,
                ))
            };
            let rf = if raw.result_filename.is_null() {
                None
            } else {
                Some(CStr::from_ptr(raw.result_filename).to_bytes())
            };
            rs_items.push(crate::artwritercore::ArticleWriterProcessItem {
                segment_offset: raw.segment_offset,
                segment_size: raw.segment_size,
                crc: raw.crc,
                part_number: raw.part_number,
                dupe_fallback_round: raw.dupe_fallback_round,
                segment_content: content,
                result_filename: rf,
            });
        }
    }
    let (crc, commit_error, failed_delta) = crate::artwritercore::article_writer_process_articles(
        &rs_items,
        total_articles_count,
        info_fn,
        final_path,
        raw_article != 0,
        direct_write != 0,
        skip_disk_write != 0,
        is_yenc != 0,
        || io_cb.map_or(-1, |cb| cb(ctx, 0, 0, std::ptr::null(), 0)),
        |off| io_cb.is_some_and(|cb| cb(ctx, 1, off, std::ptr::null(), 0) != 0),
        |buf| io_cb.map_or(0, |cb| cb(ctx, 2, 0, buf.as_ptr().cast(), buf.len())),
        |idx| {
            if let Some(cb) = io_cb {
                let _ = cb(ctx, 3, idx as i64, std::ptr::null(), 0);
            }
        },
        |msg| {
            if let Some(cb) = io_cb {
                let c_msg = nul(msg);
                let _ = cb(ctx, 4, 0, c_msg.as_ptr().cast(), 0);
            }
        },
    );
    if !out_commit_error.is_null() {
        *out_commit_error = c_int::from(commit_error);
    }
    if !out_failed_delta.is_null() {
        *out_failed_delta = failed_delta;
    }
    crc
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_move_completed_files(
    dest_dir: *const c_char,
    old_dest_dir: *const c_char,
    completed_filenames: *const *const c_char,
    completed_len: usize,
    ctx: *mut std::ffi::c_void,
    cb: Option<
        unsafe extern "C" fn(ctx: *mut std::ffi::c_void, err_msg_or_null: *const c_char) -> c_int,
    >,
) -> c_int {
    let d_dir = if dest_dir.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(dest_dir).to_bytes()
    };
    let o_dir = if old_dest_dir.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(old_dest_dir).to_bytes()
    };
    let mut fnames = Vec::with_capacity(completed_len);
    if !completed_filenames.is_null() && completed_len > 0 {
        for &p in std::slice::from_raw_parts(completed_filenames, completed_len) {
            fnames.push(if p.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(p).to_bytes()
            });
        }
    }
    c_int::from(crate::artwritercore::article_writer_move_completed_files(
        d_dir,
        o_dir,
        &fnames,
        |msg| {
            if let Some(f) = cb {
                let c_msg = nul(msg);
                let _ = f(ctx, c_msg.as_ptr().cast());
            }
        },
        || cb.is_some_and(|f| f(ctx, std::ptr::null()) != 0),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_create_output_file(
    output_filename: *const c_char,
    size: i64,
    out_err: *mut RsBuf,
) -> c_int {
    let out_fn = if output_filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(output_filename).to_bytes()
    };
    match crate::artwritercore::article_writer_create_output_file(out_fn, size) {
        Ok(()) => 1,
        Err(err) => {
            if !out_err.is_null() {
                *out_err = into_buf(err);
            }
            0
        }
    }
}

#[repr(C)]
pub struct NzbgetRsFlushArticleItem {
    pub segment_offset: i64,
    pub segment_size: i32,
    pub segment_content: *const u8,
    pub result_filename: *const c_char,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_flush_cache(
    items: *const NzbgetRsFlushArticleItem,
    items_len: usize,
    direct_write: c_int,
    skip_disk_write: c_int,
    output_filename: *const c_char,
    output_filename_len: usize,
    ctx: *mut std::ffi::c_void,
    cb: Option<
        unsafe extern "C" fn(
            ctx: *mut std::ffi::c_void,
            op: c_int,
            idx: usize,
            msg: *const c_char,
        ) -> c_int,
    >,
    out_flushed_articles: *mut c_int,
    out_flushed_size: *mut i64,
) {
    let out_fn = if output_filename.is_null() || output_filename_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(output_filename.cast::<u8>(), output_filename_len)
    };
    let mut rs_items = Vec::with_capacity(items_len);
    if !items.is_null() && items_len > 0 {
        for raw in std::slice::from_raw_parts(items, items_len) {
            let content = if raw.segment_content.is_null() {
                &[][..]
            } else {
                std::slice::from_raw_parts(raw.segment_content, raw.segment_size.max(0) as usize)
            };
            let rf = if raw.result_filename.is_null() {
                None
            } else {
                Some(CStr::from_ptr(raw.result_filename).to_bytes())
            };
            rs_items.push(crate::artwritercore::ArticleWriterFlushItem {
                segment_offset: raw.segment_offset,
                segment_size: raw.segment_size,
                segment_content: content,
                result_filename: rf,
            });
        }
    }
    let (flushed_articles, flushed_size) = crate::artwritercore::article_writer_flush_cache(
        &rs_items,
        direct_write != 0,
        skip_disk_write != 0,
        out_fn,
        || cb.is_some_and(|f| f(ctx, 0, 0, std::ptr::null()) != 0),
        |idx| {
            if let Some(f) = cb {
                let _ = f(ctx, 1, idx, std::ptr::null());
            }
        },
        |msg| {
            if let Some(f) = cb {
                let c_msg = nul(msg);
                let _ = f(ctx, 2, 0, c_msg.as_ptr().cast());
            }
        },
    );
    if !out_flushed_articles.is_null() {
        *out_flushed_articles = flushed_articles;
    }
    if !out_flushed_size.is_null() {
        *out_flushed_size = flushed_size;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_post_process_rename_and_hardlink(
    original_filename: *const c_char,
    original_filename_len: usize,
    current_filename: *const c_char,
    nzb_dest_dir: *const c_char,
    nzb_dest_dir_len: usize,
    file_output_filename: *const c_char,
    file_output_filename_len: usize,
    writer_output_filename: *const c_char,
    writer_output_filename_len: usize,
    is_hard_linked: c_int,
    inter_dir_empty: c_int,
    ctx: *mut std::ffi::c_void,
    log_err_cb: Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void, msg: *const c_char)>,
    out_updated_output_filename: *mut RsBuf,
    out_has_updated_output_filename: *mut c_int,
) -> c_int {
    let orig_fn = if original_filename.is_null() || original_filename_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(original_filename.cast::<u8>(), original_filename_len)
    };
    let cur_fn = if current_filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(current_filename).to_bytes()
    };
    let dest_dir = if nzb_dest_dir.is_null() || nzb_dest_dir_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(nzb_dest_dir.cast::<u8>(), nzb_dest_dir_len)
    };
    let file_ofn = if file_output_filename.is_null() || file_output_filename_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(file_output_filename.cast::<u8>(), file_output_filename_len)
    };
    let writer_ofn = if writer_output_filename.is_null() || writer_output_filename_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(
            writer_output_filename.cast::<u8>(),
            writer_output_filename_len,
        )
    };
    let (updated_ofn, clear_and_return) =
        crate::artwritercore::article_writer_post_process_rename_and_hardlink(
            orig_fn,
            cur_fn,
            dest_dir,
            file_ofn,
            writer_ofn,
            is_hard_linked != 0,
            inter_dir_empty != 0,
            |msg| {
                if let Some(f) = log_err_cb {
                    let c_msg = nul(msg);
                    f(ctx, c_msg.as_ptr().cast());
                }
            },
        );
    if !out_has_updated_output_filename.is_null() {
        *out_has_updated_output_filename = c_int::from(updated_ofn.is_some());
    }
    if !out_updated_output_filename.is_null() {
        *out_updated_output_filename = into_buf(updated_ofn.unwrap_or_default());
    }
    c_int::from(clear_and_return)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_finish_disk_ops(
    success: c_int,
    skip_disk_write: c_int,
    raw_article: c_int,
    direct_write: c_int,
    has_cached_data: c_int,
    temp_filename: *const c_char,
    temp_filename_len: usize,
    result_filename: *const c_char,
    result_filename_len: usize,
    ctx: *mut std::ffi::c_void,
    log_err_cb: Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void, msg: *const c_char)>,
) {
    let tmp_fn = if temp_filename.is_null() || temp_filename_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(temp_filename.cast::<u8>(), temp_filename_len)
    };
    let res_fn = if result_filename.is_null() || result_filename_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(result_filename.cast::<u8>(), result_filename_len)
    };
    crate::artwritercore::article_writer_finish_disk_ops(
        success != 0,
        skip_disk_write != 0,
        raw_article != 0,
        direct_write != 0,
        has_cached_data != 0,
        tmp_fn,
        res_fn,
        |msg| {
            if let Some(f) = log_err_cb {
                let c_msg = nul(msg);
                f(ctx, c_msg.as_ptr().cast());
            }
        },
    );
}

// ============================================================================
// QueueCoordinator & Options consolidated FFI exports
// ============================================================================

// ============================================================================
// DiskState Consolidated Validation & State FFI (`rust/src/diskstatepp.rs`)
// ============================================================================

// ============================================================================
// Consolidated WebDownloader & StatMeter FFI (`rust/src/webdownload.rs`, `rust/src/statmeter.rs`)
// ============================================================================

// ============================================================================
// DiskState Consolidated Parsing & Matching FFI (`rust/src/diskstatepp.rs`)
// ============================================================================

// ============================================================================
// DupeSearch Consolidated FFI (`rust/src/dupesearch_nzb.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cmdline_format_usage(
    base_name: *const c_char,
    path_sep: c_char,
    include_daemon: c_int,
) -> RsBuf {
    let name = if base_name.is_null() {
        b""
    } else {
        CStr::from_ptr(base_name).to_bytes()
    };
    into_buf(crate::cmdline::format_usage(
        name,
        path_sep as u8,
        include_daemon != 0,
    ))
}


#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_log_rotate_log_file(
    log_file: *const c_char,
    sep: c_char,
    cur_time: i64,
    rotate_log: c_int,
    ctx: *mut std::ffi::c_void,
    on_info_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) -> RsBuf {
    let path = input(log_file);
    #[cfg(unix)]
    {
        into_buf(crate::systemhealth::log_rotate_log_file(
            path,
            sep as u8,
            cur_time,
            rotate_log,
            |msg| {
                if let Some(cb) = on_info_cb {
                    let mut cstr = msg.to_vec();
                    cstr.push(0);
                    cb(ctx, cstr.as_ptr().cast());
                }
            },
        ))
    }
    #[cfg(not(unix))]
    {
        let _ = (sep, cur_time, rotate_log, ctx, on_info_cb);
        into_buf(path.to_vec())
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_find_default_config_file(
    app_dir: *const c_char,
    sep: c_char,
    out_found: *mut c_int,
) -> RsBuf {
    if !out_found.is_null() {
        *out_found = 0;
    }
    let dir = if app_dir.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(app_dir).to_bytes()
    };
    match crate::options::find_default_config_file(dir, sep as u8, |path| {
        crate::fsops::file_exists(path.to_bytes(), &crate::fsops::posix_stat)
    }) {
        Some(found) => {
            if !out_found.is_null() {
                *out_found = 1;
            }
            into_buf(found)
        }
        None => into_buf(Vec::new()),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_load_config_file(
    path: *const c_char,
    ctx: *mut std::ffi::c_void,
    on_line_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
) -> c_int {
    if path.is_null() {
        return 0;
    }
    let c_path = CStr::from_ptr(path);
    i32::from(crate::options::load_config_file_lines(
        c_path,
        |line_no, line| {
            if let Some(cb) = on_line_cb {
                cb(ctx, line_no, line.as_ptr());
            }
        },
    ))
}


// =========================================================================
// End-to-End MoveController, CleanupController, and RenameController FFI
// =========================================================================

fn bytes_to_cstr_buf(bytes: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(bytes.len() + 1);
    for &b in bytes {
        if b != 0 {
            buf.push(b);
        }
    }
    buf.push(0);
    buf
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_move_controller_move_files(
    inter_dir: *const c_char,
    dest_dir: *const c_char,
    ctx: *mut std::ffi::c_void,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
    rename_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *const c_char)>,
) -> c_int {
    let inter_path =
        crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(inter_dir));
    let dest_path =
        crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(dest_dir));
    let ok = crate::cleanup::move_files(
        &inter_path,
        &dest_path,
        || {
            if let Some(cb) = is_stopped_cb {
                cb(ctx) != 0
            } else {
                false
            }
        },
        |kind, msg| {
            if let Some(cb) = log_cb {
                let c_msg = bytes_to_cstr_buf(msg);
                cb(ctx, kind as c_int, c_msg.as_ptr() as *const c_char);
            }
        },
        |old_rel, new_rel| {
            if let Some(cb) = rename_cb {
                let c_old = bytes_to_cstr_buf(old_rel);
                let c_new = bytes_to_cstr_buf(new_rel);
                cb(
                    ctx,
                    c_old.as_ptr() as *const c_char,
                    c_new.as_ptr() as *const c_char,
                );
            }
        },
    );
    i32::from(ok)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_move_controller_remove_stale_hardlinks(
    hard_link_path: *const c_char,
    dest_dir: *const c_char,
    ctx: *mut std::ffi::c_void,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
) {
    let hl_path =
        crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(hard_link_path));
    let dst_path =
        crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(dest_dir));
    crate::cleanup::remove_stale_hardlinks(&hl_path, &dst_path, |kind, msg| {
        if let Some(cb) = log_cb {
            let c_msg = bytes_to_cstr_buf(msg);
            cb(ctx, kind as c_int, c_msg.as_ptr() as *const c_char);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cleanup_controller_run(
    dest_dir: *const c_char,
    final_dir: *const c_char,
    ext_cleanup_disk: *const c_char,
    ctx: *mut std::ffi::c_void,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
    out_cleanup_status: *mut c_int,
) -> c_int {
    let dst_path =
        crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(dest_dir));
    let fin_path =
        crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(final_dir));
    let ext_bytes = crate::diskstate_format::cstr_to_bytes(ext_cleanup_disk);
    let (outcome, status) =
        crate::cleanup::run_cleanup(&dst_path, &fin_path, ext_bytes, |kind, msg| {
            if let Some(cb) = log_cb {
                let c_msg = bytes_to_cstr_buf(msg);
                cb(ctx, kind as c_int, c_msg.as_ptr() as *const c_char);
            }
        });
    if !out_cleanup_status.is_null() {
        *out_cleanup_status = status;
    }
    outcome
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_post_renamer_run(
    is_post_unpack: c_int,
    meta_name: *const c_char,
    nzb_name: *const c_char,
    dst_dir: *const c_char,
    ignore_ext: *const c_char,
    ctx: *mut std::ffi::c_void,
    info_name_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
    rename_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *const c_char)>,
) -> c_int {
    let meta_bytes = crate::diskstate_format::cstr_to_bytes(meta_name);
    let nzb_bytes = crate::diskstate_format::cstr_to_bytes(nzb_name);
    let dst_path =
        crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(dst_dir));
    let ignore_bytes = crate::diskstate_format::cstr_to_bytes(ignore_ext);

    crate::cleanup::run_post_renamer(
        is_post_unpack != 0,
        meta_bytes,
        nzb_bytes,
        &dst_path,
        ignore_bytes,
        |info| {
            if let Some(cb) = info_name_cb {
                let c_info = bytes_to_cstr_buf(info);
                cb(ctx, c_info.as_ptr() as *const c_char);
            }
        },
        || {
            if let Some(cb) = is_stopped_cb {
                cb(ctx) != 0
            } else {
                false
            }
        },
        |kind, msg| {
            if let Some(cb) = log_cb {
                let c_msg = bytes_to_cstr_buf(msg);
                cb(ctx, kind as c_int, c_msg.as_ptr() as *const c_char);
            }
        },
        |old_name, new_name| {
            if let Some(cb) = rename_cb {
                let c_old = bytes_to_cstr_buf(old_name);
                let c_new = bytes_to_cstr_buf(new_name);
                cb(
                    ctx,
                    c_old.as_ptr() as *const c_char,
                    c_new.as_ptr() as *const c_char,
                );
            }
        },
    )
}

// ============================================================================
// SystemHealth (`rust/src/systemhealth.rs`)
// ============================================================================

unsafe fn bytes_from_ptr_len<'a>(ptr: *const c_char, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ptr.cast::<u8>(), len)
    }
}

unsafe fn path_from_ptr_len(ptr: *const c_char, len: usize) -> std::path::PathBuf {
    if ptr.is_null() || len == 0 {
        std::path::PathBuf::new()
    } else {
        crate::cleanup::bytes_to_path_buf(std::slice::from_raw_parts(ptr.cast::<u8>(), len))
    }
}

#[repr(C)]
pub struct NzbgetRsHealthServerInput {
    pub id: c_int,
    pub active: c_int,
    pub name: *const c_char,
    pub name_len: usize,
    pub level: c_int,
    pub optional: c_int,
    pub group: c_int,
    pub host: *const c_char,
    pub host_len: usize,
    pub port: c_int,
    pub username: *const c_char,
    pub username_len: usize,
    pub password: *const c_char,
    pub password_len: usize,
    pub tls: c_int,
    pub cipher: *const c_char,
    pub cipher_len: usize,
    pub max_connections: c_int,
    pub retention: c_int,
    pub cert_level: u32,
    pub ip_version: c_int,
}

#[repr(C)]
pub struct NzbgetRsHealthTaskInput {
    pub id: c_int,
    pub hours: c_int,
    pub minutes: c_int,
    pub week_days_bits: c_int,
}

#[repr(C)]
pub struct NzbgetRsHealthCategoryInput {
    pub name: *const c_char,
    pub name_len: usize,
    pub unpack: c_int,
}

#[repr(C)]
pub struct NzbgetRsHealthFeedInput {
    pub id: c_int,
    pub name: *const c_char,
    pub name_len: usize,
    pub url: *const c_char,
    pub url_len: usize,
    pub interval: c_int,
    pub cert_level: u32,
}

#[repr(C)]
pub struct NzbgetRsSystemHealthConfig {
    pub main_dir: *const c_char,
    pub main_dir_len: usize,
    pub dest_dir: *const c_char,
    pub dest_dir_len: usize,
    pub inter_dir: *const c_char,
    pub inter_dir_len: usize,
    pub nzb_dir: *const c_char,
    pub nzb_dir_len: usize,
    pub queue_dir: *const c_char,
    pub queue_dir_len: usize,
    pub web_dir: *const c_char,
    pub web_dir_len: usize,
    pub temp_dir: *const c_char,
    pub temp_dir_len: usize,
    pub script_dir_ptrs: *const *const c_char,
    pub script_dir_lens: *const usize,
    pub script_dir_count: usize,
    pub config_template: *const c_char,
    pub config_template_len: usize,
    pub log_file: *const c_char,
    pub log_file_len: usize,
    pub options_log_file: *const c_char,
    pub options_log_file_len: usize,
    pub cert_store: *const c_char,
    pub cert_store_len: usize,
    pub cert_store_path: *const c_char,
    pub cert_store_path_len: usize,
    pub config_file: *const c_char,
    pub config_file_len: usize,
    pub lock_file: *const c_char,
    pub lock_file_len: usize,
    pub write_log: c_int,
    pub cert_check: c_int,
    pub daemon_mode: c_int,
    pub servers: *const NzbgetRsHealthServerInput,
    pub server_count: usize,
    pub output_mode: c_int,
    pub update_interval: c_int,
    pub curses_nzb_name: c_int,
    pub curses_group: c_int,
    pub curses_time: c_int,
    pub rotate_log: c_int,
    pub log_buffer: c_int,
    pub crash_dump: c_int,
    pub time_correction: c_int,
    pub article_retries: c_int,
    pub hc_dupe: c_int,
    pub dupe_search: c_int,
    pub article_interval: c_int,
    pub article_timeout: c_int,
    pub article_read_chunk_size: c_int,
    pub url_retries: c_int,
    pub url_interval: c_int,
    pub url_timeout: c_int,
    pub url_connections: c_int,
    pub remote_timeout: c_int,
    pub download_rate: c_int,
    pub monthly_quota: c_int,
    pub quota_start_day: c_int,
    pub daily_quota: c_int,
    pub flush_queue: c_int,
    pub continue_partial: c_int,
    pub propagation_delay: c_int,
    pub article_cache: c_int,
    pub direct_write: c_int,
    pub write_buffer: c_int,
    pub file_naming: c_int,
    pub post_strategy: c_int,
    pub disk_space: c_int,
    pub nzb_cleanup_disk: c_int,
    pub keep_history: c_int,
    pub feed_history: c_int,
    pub skip_write: c_int,
    pub raw_article: c_int,
    pub control_ip: *const c_char,
    pub control_ip_len: usize,
    pub control_port: c_int,
    pub control_username: *const c_char,
    pub control_username_len: usize,
    pub control_password: *const c_char,
    pub control_password_len: usize,
    pub restricted_username: *const c_char,
    pub restricted_username_len: usize,
    pub restricted_password: *const c_char,
    pub restricted_password_len: usize,
    pub add_username: *const c_char,
    pub add_username_len: usize,
    pub add_password: *const c_char,
    pub add_password_len: usize,
    pub form_auth: c_int,
    pub secure_control: c_int,
    pub secure_cert: *const c_char,
    pub secure_cert_len: usize,
    pub secure_key: *const c_char,
    pub secure_key_len: usize,
    pub secure_port: c_int,
    pub cert_store_path_empty: c_int,
    pub daemon_username: *const c_char,
    pub daemon_username_len: usize,
    pub umask: c_int,
    pub append_category_dir: c_int,
    pub nzb_dir_interval: c_int,
    pub nzb_dir_file_age: c_int,
    pub dupe_check: c_int,
    pub health_check: c_int,
    pub crc_check: c_int,
    pub par_check: c_int,
    pub par_repair: c_int,
    pub par_quick: c_int,
    pub par_buffer: c_int,
    pub par_threads: c_int,
    pub hw_threads: usize,
    pub par_rename: c_int,
    pub rar_rename: c_int,
    pub direct_rename: c_int,
    pub hard_linking: c_int,
    pub par_time_limit: c_int,
    pub unpack: c_int,
    pub direct_unpack: c_int,
    pub unpack_pause_queue: c_int,
    pub unrar_path: *const c_char,
    pub unrar_path_len: usize,
    pub seven_zip_path: *const c_char,
    pub seven_zip_path_len: usize,
    pub ext_cleanup_disk: *const c_char,
    pub ext_cleanup_disk_len: usize,
    pub unpack_pass_file: *const c_char,
    pub unpack_pass_file_len: usize,
    pub unpack_ignore_ext: *const c_char,
    pub unpack_ignore_ext_len: usize,
    pub tasks: *const NzbgetRsHealthTaskInput,
    pub task_count: usize,
    pub categories: *const NzbgetRsHealthCategoryInput,
    pub category_count: usize,
    pub feeds: *const NzbgetRsHealthFeedInput,
    pub feed_count: usize,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_system_health_diagnose(
    cfg: *const NzbgetRsSystemHealthConfig,
    format_mode: c_int,
    ctx: *mut std::ffi::c_void,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
) -> RsBuf {
    let Some(c) = cfg.as_ref() else {
        return RsBuf {
            data: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        };
    };

    let main_p = path_from_ptr_len(c.main_dir, c.main_dir_len);
    let dest_p = path_from_ptr_len(c.dest_dir, c.dest_dir_len);
    let inter_p = path_from_ptr_len(c.inter_dir, c.inter_dir_len);
    let nzb_p = path_from_ptr_len(c.nzb_dir, c.nzb_dir_len);
    let queue_p = path_from_ptr_len(c.queue_dir, c.queue_dir_len);
    let web_p = path_from_ptr_len(c.web_dir, c.web_dir_len);
    let temp_p = path_from_ptr_len(c.temp_dir, c.temp_dir_len);
    let mut script_paths = Vec::with_capacity(c.script_dir_count);
    if !c.script_dir_ptrs.is_null() && !c.script_dir_lens.is_null() && c.script_dir_count > 0 {
        let ptrs = std::slice::from_raw_parts(c.script_dir_ptrs, c.script_dir_count);
        let lens = std::slice::from_raw_parts(c.script_dir_lens, c.script_dir_count);
        for i in 0..c.script_dir_count {
            script_paths.push(path_from_ptr_len(ptrs[i], lens[i]));
        }
    }
    let script_refs: Vec<&std::path::Path> = script_paths.iter().map(|p| p.as_path()).collect();
    let cfg_tpl_p = path_from_ptr_len(c.config_template, c.config_template_len);
    let log_p = path_from_ptr_len(c.log_file, c.log_file_len);
    let opt_log_p = path_from_ptr_len(c.options_log_file, c.options_log_file_len);
    let cert_p = path_from_ptr_len(c.cert_store, c.cert_store_len);
    let cert_path_p = path_from_ptr_len(c.cert_store_path, c.cert_store_path_len);
    let cfg_p = path_from_ptr_len(c.config_file, c.config_file_len);
    let lock_p = path_from_ptr_len(c.lock_file, c.lock_file_len);
    let secure_cert_p = path_from_ptr_len(c.secure_cert, c.secure_cert_len);
    let secure_key_p = path_from_ptr_len(c.secure_key, c.secure_key_len);
    let unrar_p = path_from_ptr_len(c.unrar_path, c.unrar_path_len);
    let seven_zip_p = path_from_ptr_len(c.seven_zip_path, c.seven_zip_path_len);
    let pass_p = path_from_ptr_len(c.unpack_pass_file, c.unpack_pass_file_len);

    let mut servers = Vec::with_capacity(c.server_count);
    if !c.servers.is_null() && c.server_count > 0 {
        for s in std::slice::from_raw_parts(c.servers, c.server_count) {
            servers.push(crate::systemhealth::HealthServerInput {
                id: s.id,
                section: crate::systemhealth::NewsServerSectionInput {
                    active: s.active != 0,
                    name: bytes_from_ptr_len(s.name, s.name_len),
                    level: s.level,
                    optional: s.optional != 0,
                    group: s.group,
                    host: bytes_from_ptr_len(s.host, s.host_len),
                    port: s.port,
                    username: bytes_from_ptr_len(s.username, s.username_len),
                    password: bytes_from_ptr_len(s.password, s.password_len),
                    tls: s.tls != 0,
                    cipher: bytes_from_ptr_len(s.cipher, s.cipher_len),
                    max_connections: s.max_connections,
                    retention: s.retention,
                    cert_level: s.cert_level,
                    ip_version: s.ip_version,
                    dupe_search: c.dupe_search != 0,
                },
            });
        }
    }

    let mut tasks = Vec::with_capacity(c.task_count);
    if !c.tasks.is_null() && c.task_count > 0 {
        for t in std::slice::from_raw_parts(c.tasks, c.task_count) {
            tasks.push(crate::systemhealth::HealthTaskInput {
                id: t.id,
                hours: t.hours,
                minutes: t.minutes,
                week_days_bits: t.week_days_bits,
            });
        }
    }

    let mut categories = Vec::with_capacity(c.category_count);
    if !c.categories.is_null() && c.category_count > 0 {
        for cat in std::slice::from_raw_parts(c.categories, c.category_count) {
            categories.push(crate::systemhealth::HealthCategoryInput {
                name: bytes_from_ptr_len(cat.name, cat.name_len),
                unpack: cat.unpack != 0,
            });
        }
    }

    let mut feeds = Vec::with_capacity(c.feed_count);
    if !c.feeds.is_null() && c.feed_count > 0 {
        for f in std::slice::from_raw_parts(c.feeds, c.feed_count) {
            feeds.push(crate::systemhealth::HealthFeedInput {
                id: f.id,
                name: bytes_from_ptr_len(f.name, f.name_len),
                url: bytes_from_ptr_len(f.url, f.url_len),
                interval: f.interval,
                cert_level: f.cert_level,
            });
        }
    }

    let input = crate::systemhealth::SystemHealthDiagnoseInput {
        paths: crate::systemhealth::PathsSectionInput {
            main_dir: &main_p,
            dest_dir: &dest_p,
            inter_dir: &inter_p,
            nzb_dir: &nzb_p,
            queue_dir: &queue_p,
            web_dir: &web_p,
            temp_dir: &temp_p,
            script_dirs: &script_refs,
            config_template: &cfg_tpl_p,
            log_file: &log_p,
            cert_store: &cert_p,
            cert_store_path: &cert_path_p,
            config_file: &cfg_p,
            lock_file: &lock_p,
            write_log: c.write_log,
            cert_check: c.cert_check != 0,
            daemon_mode: c.daemon_mode != 0,
        },
        servers: &servers,
        output_mode: c.output_mode,
        update_interval: c.update_interval,
        curses_nzb_name: c.curses_nzb_name != 0,
        curses_group: c.curses_group != 0,
        curses_time: c.curses_time != 0,
        rotate_log: c.rotate_log,
        log_buffer: c.log_buffer,
        crash_dump: c.crash_dump != 0,
        time_correction: c.time_correction,
        article_retries: c.article_retries,
        hc_dupe: c.hc_dupe != 0,
        dupe_search: c.dupe_search != 0,
        article_interval: c.article_interval,
        article_timeout: c.article_timeout,
        article_read_chunk_size: c.article_read_chunk_size,
        url_retries: c.url_retries,
        url_interval: c.url_interval,
        url_timeout: c.url_timeout,
        url_connections: c.url_connections,
        remote_timeout: c.remote_timeout,
        download_rate: c.download_rate,
        monthly_quota: c.monthly_quota,
        quota_start_day: c.quota_start_day,
        daily_quota: c.daily_quota,
        flush_queue: c.flush_queue != 0,
        continue_partial: c.continue_partial != 0,
        propagation_delay: c.propagation_delay,
        article_cache: c.article_cache,
        direct_write: c.direct_write != 0,
        write_buffer: c.write_buffer,
        file_naming: c.file_naming,
        post_strategy: c.post_strategy,
        disk_space: c.disk_space,
        nzb_cleanup_disk: c.nzb_cleanup_disk != 0,
        keep_history: c.keep_history,
        feed_history: c.feed_history,
        skip_write: c.skip_write != 0,
        raw_article: c.raw_article != 0,
        security: crate::systemhealth::SecuritySectionInput {
            control_ip: bytes_from_ptr_len(c.control_ip, c.control_ip_len),
            control_port: c.control_port,
            control_username: bytes_from_ptr_len(c.control_username, c.control_username_len),
            control_password: bytes_from_ptr_len(c.control_password, c.control_password_len),
            restricted_username: bytes_from_ptr_len(
                c.restricted_username,
                c.restricted_username_len,
            ),
            restricted_password: bytes_from_ptr_len(
                c.restricted_password,
                c.restricted_password_len,
            ),
            add_username: bytes_from_ptr_len(c.add_username, c.add_username_len),
            add_password: bytes_from_ptr_len(c.add_password, c.add_password_len),
            form_auth: c.form_auth != 0,
            secure_control: c.secure_control != 0,
            secure_cert: &secure_cert_p,
            secure_key: &secure_key_p,
            secure_port: c.secure_port,
            cert_check: c.cert_check != 0,
            cert_store_path_empty: c.cert_store_path_empty != 0,
            daemon_mode: c.daemon_mode != 0,
            daemon_username: bytes_from_ptr_len(c.daemon_username, c.daemon_username_len),
            umask: c.umask,
        },
        append_category_dir: c.append_category_dir != 0,
        nzb_dir_interval: c.nzb_dir_interval,
        nzb_dir_file_age: c.nzb_dir_file_age,
        dupe_check: c.dupe_check != 0,
        health_check: c.health_check,
        crc_check: c.crc_check != 0,
        par_check: c.par_check,
        par_repair: c.par_repair != 0,
        par_quick: c.par_quick != 0,
        par_buffer: c.par_buffer,
        par_threads: c.par_threads,
        hw_threads: c.hw_threads,
        par_rename: c.par_rename != 0,
        rar_rename: c.rar_rename != 0,
        direct_rename: c.direct_rename != 0,
        hard_linking: c.hard_linking != 0,
        par_time_limit: c.par_time_limit,
        unpack: c.unpack != 0,
        direct_unpack: c.direct_unpack != 0,
        unpack_pause_queue: c.unpack_pause_queue != 0,
        unrar_path: &unrar_p,
        seven_zip_path: &seven_zip_p,
        ext_cleanup_disk: bytes_from_ptr_len(c.ext_cleanup_disk, c.ext_cleanup_disk_len),
        unpack_pass_file: &pass_p,
        unpack_ignore_ext: bytes_from_ptr_len(c.unpack_ignore_ext, c.unpack_ignore_ext_len),
        options_log_file: &opt_log_p,
        tasks: &tasks,
        categories: &categories,
        feeds: &feeds,
    };

    let bytes = crate::systemhealth::diagnose_system_health(&input, format_mode, |sev, msg| {
        if let Some(cb) = log_cb {
            let c_msg = nul(msg);
            cb(ctx, sev, c_msg.as_ptr().cast());
        }
    });
    if bytes.is_empty() {
        RsBuf {
            data: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    } else {
        into_buf(bytes)
    }
}

// ============================================================================
// Extension & Script Subsystem End-to-End FFI (`rust/src/extension.rs`, `rust/src/options.rs`)
// ============================================================================

pub type OptionsConvertUpdateCb = Option<
    unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        idx: usize,
        name: *const c_char,
        value: *const c_char,
    ),
>;

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_convert_old_options(
    names: *const *const c_char,
    values: *const *const c_char,
    count: usize,
    ctx: *mut std::ffi::c_void,
    update_cb: OptionsConvertUpdateCb,
) {
    let mut entries = Vec::with_capacity(count + 1);
    let mut orig_values = Vec::with_capacity(count);
    if !names.is_null() && !values.is_null() {
        let n_slice = std::slice::from_raw_parts(names, count);
        let v_slice = std::slice::from_raw_parts(values, count);
        for i in 0..count {
            let n_cstr = if n_slice[i].is_null() {
                c""
            } else {
                CStr::from_ptr(n_slice[i])
            };
            let v_cstr = if v_slice[i].is_null() {
                c""
            } else {
                CStr::from_ptr(v_slice[i])
            };
            orig_values.push(v_cstr.to_bytes().to_vec());
            entries.push((n_cstr.to_owned(), v_cstr.to_owned()));
        }
    }
    crate::options::convert_old_options(&mut entries);
    if let Some(cb) = update_cb {
        for (i, (name, val)) in entries.iter().enumerate() {
            if i >= count || val.to_bytes() != orig_values[i].as_slice() {
                cb(ctx, i, name.as_ptr(), val.as_ptr());
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_mgr_prepare_update(
    ext_name: *const c_char,
    found: c_int,
    use_count: i64,
    target_path: *const c_char,
    archive_path: *const c_char,
    out_backup_path: *mut RsBuf,
) -> RsBuf {
    let en = if ext_name.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(ext_name).to_bytes()
    };
    let tp = if target_path.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(target_path).to_bytes()
    };
    let ap = if archive_path.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(archive_path).to_bytes()
    };
    let (backup, err) = crate::extension::ext_mgr_prepare_update(en, found != 0, use_count, tp, ap);
    if !out_backup_path.is_null() {
        *out_backup_path = into_buf(backup);
    }
    match err {
        Some(e) => into_buf(e),
        None => RsBuf {
            data: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_mgr_finish_update(
    target_path: *const c_char,
    backup_path: *const c_char,
    archive_path: *const c_char,
    install_err: *const c_char,
) -> RsBuf {
    let tp = if target_path.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(target_path).to_bytes()
    };
    let bp = if backup_path.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(backup_path).to_bytes()
    };
    let ap = if archive_path.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(archive_path).to_bytes()
    };
    let ie = if install_err.is_null() {
        None
    } else {
        Some(CStr::from_ptr(install_err).to_bytes())
    };
    match crate::extension::ext_mgr_finish_update(tp, bp, ap, ie) {
        Some(e) => into_buf(e),
        None => RsBuf {
            data: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_mgr_delete_extension(
    ext_name: *const c_char,
    found: c_int,
    use_count: i64,
    target_path: *const c_char,
) -> RsBuf {
    let en = if ext_name.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(ext_name).to_bytes()
    };
    let tp = if target_path.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(target_path).to_bytes()
    };
    match crate::extension::ext_mgr_delete_extension(en, found != 0, use_count, tp) {
        Some(e) => into_buf(e),
        None => RsBuf {
            data: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_mgr_install_extension(
    file_path: *const c_char,
    dest_path: *const c_char,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
    log_ctx: *mut std::ffi::c_void,
) -> RsBuf {
    let fp = if file_path.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(file_path).to_bytes()
    };
    let dp = if dest_path.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(dest_path).to_bytes()
    };
    match crate::extension::ext_mgr_install_extension(fp, dp, |kind, msg| {
        if let Some(cb) = log_cb {
            let mut buf: Vec<u8> = msg.bytes().filter(|&b| b != 0).collect();
            buf.push(0);
            cb(log_ctx, kind, buf.as_ptr().cast::<c_char>());
        }
    }) {
        Some(err) => into_buf(err),
        None => RsBuf {
            data: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        },
    }
}

pub type NzbScriptSetEnvSpecialCb = Option<
    unsafe extern "C" fn(
        ctx: *mut std::ffi::c_void,
        prefix: *const c_char,
        name: *const c_char,
        value: *const c_char,
    ),
>;

pub type NzbScriptPrepareOptionsCb =
    Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void, strip_prefix: *const c_char)>;

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_script_prepare_env(
    script_name: *const c_char,
    has_parameters: c_int,
    param_names: *const *const c_char,
    param_values: *const *const c_char,
    param_count: usize,
    ctx: *mut std::ffi::c_void,
    set_env_special_cb: NzbScriptSetEnvSpecialCb,
    prepare_options_cb: NzbScriptPrepareOptionsCb,
) {
    let sn = if script_name.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(script_name).to_bytes()
    };
    let mut names = Vec::with_capacity(param_count);
    let mut vals = Vec::with_capacity(param_count);
    if !param_names.is_null() && !param_values.is_null() {
        let n_slice = std::slice::from_raw_parts(param_names, param_count);
        let v_slice = std::slice::from_raw_parts(param_values, param_count);
        for i in 0..param_count {
            let n_cstr = if n_slice[i].is_null() {
                c""
            } else {
                CStr::from_ptr(n_slice[i])
            };
            let v_cstr = if v_slice[i].is_null() {
                c""
            } else {
                CStr::from_ptr(v_slice[i])
            };
            names.push(n_cstr);
            vals.push(v_cstr);
        }
    }
    crate::extension::nzb_script_prepare_env(
        sn,
        has_parameters != 0,
        &names,
        &vals,
        |prefix, name, val| {
            if let Some(cb) = set_env_special_cb {
                cb(ctx, prefix.as_ptr(), name.as_ptr(), val.as_ptr());
            }
        },
        |strip_prefix| {
            if let Some(cb) = prepare_options_cb {
                cb(ctx, strip_prefix.as_ptr());
            }
        },
    );
}

pub type ScriptSetEnvVarCb = Option<
    unsafe extern "C" fn(ctx: *mut std::ffi::c_void, name: *const c_char, value: *const c_char),
>;

pub type ScriptSetIntEnvVarCb =
    Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void, name: *const c_char, value: c_int)>;

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nzbget_rs_scan_script_prepare_env(
    nzb_filename: *const c_char,
    base_filename: *const c_char,
    url: *const c_char,
    nzb_name: *const c_char,
    category: *const c_char,
    priority: c_int,
    add_top: c_int,
    add_paused: c_int,
    dupe_key: *const c_char,
    dupe_score: c_int,
    dupe_mode: c_int,
    directory: *const c_char,
    path_sep: c_char,
    ctx: *mut std::ffi::c_void,
    set_env_cb: ScriptSetEnvVarCb,
    set_int_env_cb: ScriptSetIntEnvVarCb,
) {
    let cstr_or_empty = |p: *const c_char| -> &CStr {
        if p.is_null() {
            c""
        } else {
            CStr::from_ptr(p)
        }
    };
    let dir_bytes = cstr_or_empty(directory).to_bytes();
    crate::extension::scan_script_prepare_env(
        cstr_or_empty(nzb_filename),
        cstr_or_empty(base_filename),
        cstr_or_empty(url),
        cstr_or_empty(nzb_name),
        cstr_or_empty(category),
        priority,
        add_top != 0,
        add_paused != 0,
        cstr_or_empty(dupe_key),
        dupe_score,
        dupe_mode,
        dir_bytes,
        path_sep as u8,
        |name, val| {
            if let Some(cb) = set_env_cb {
                cb(ctx, name.as_ptr(), val.as_ptr());
            }
        },
        |name, val| {
            if let Some(cb) = set_int_env_cb {
                cb(ctx, name.as_ptr(), val);
            }
        },
    );
}

#[repr(C)]
pub struct NzbgetRsPostScriptEnv {
    pub nzb_id: c_int,
    pub nzb_name: *const c_char,
    pub dest_dir: *const c_char,
    pub filename: *const c_char,
    pub queued_filename: *const c_char,
    pub url: *const c_char,
    pub final_dir: *const c_char,
    pub category: *const c_char,
    pub health: c_int,
    pub critical_health: c_int,
    pub dupe_key: *const c_char,
    pub dupe_score: c_int,
    pub dupe_mode: c_int,
    pub status: *const c_char,
    pub script_status: c_int,
    pub par_status: c_int,
    pub unpack_status: c_int,
    pub delete_status: c_int,
    pub mark_status: c_int,
    pub total_articles: c_int,
    pub success_articles: c_int,
    pub failed_articles: c_int,
    pub server_ids: *const c_int,
    pub server_success_articles: *const c_int,
    pub server_failed_articles: *const c_int,
    pub server_count: usize,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_post_script_prepare_env(
    env: *const NzbgetRsPostScriptEnv,
    ctx: *mut std::ffi::c_void,
    set_env_cb: ScriptSetEnvVarCb,
    set_int_env_cb: ScriptSetIntEnvVarCb,
) {
    if env.is_null() {
        return;
    }
    let e = &*env;
    let cstr_or_empty = |p: *const c_char| -> &CStr {
        if p.is_null() {
            c""
        } else {
            CStr::from_ptr(p)
        }
    };
    let srv_ids = if e.server_ids.is_null() || e.server_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(e.server_ids, e.server_count)
    };
    let srv_succ = if e.server_success_articles.is_null() || e.server_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(e.server_success_articles, e.server_count)
    };
    let srv_fail = if e.server_failed_articles.is_null() || e.server_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(e.server_failed_articles, e.server_count)
    };
    let inp = crate::extension::PostScriptEnvInput {
        nzb_id: e.nzb_id,
        nzb_name: cstr_or_empty(e.nzb_name),
        dest_dir: cstr_or_empty(e.dest_dir),
        filename: cstr_or_empty(e.filename),
        queued_filename: cstr_or_empty(e.queued_filename),
        url: cstr_or_empty(e.url),
        final_dir: cstr_or_empty(e.final_dir),
        category: cstr_or_empty(e.category),
        health: e.health,
        critical_health: e.critical_health,
        dupe_key: cstr_or_empty(e.dupe_key),
        dupe_score: e.dupe_score,
        dupe_mode: e.dupe_mode,
        status: cstr_or_empty(e.status),
        script_status: e.script_status,
        par_status: e.par_status,
        unpack_status: e.unpack_status,
        delete_status: e.delete_status,
        mark_status: e.mark_status,
        total_articles: e.total_articles,
        success_articles: e.success_articles,
        failed_articles: e.failed_articles,
        server_ids: srv_ids,
        server_success_articles: srv_succ,
        server_failed_articles: srv_fail,
    };
    crate::extension::post_script_prepare_env(
        &inp,
        |name, val| {
            if let Some(cb) = set_env_cb {
                cb(ctx, name.as_ptr(), val.as_ptr());
            }
        },
        |name, val| {
            if let Some(cb) = set_int_env_cb {
                cb(ctx, name.as_ptr(), val);
            }
        },
    );
}

// ============================================================================
// NzbFileParser, UrlCoordinator & DirectRenamer End-to-End FFI
// ============================================================================

pub type NzbgetRsNzbFileParser = crate::nzbfile::NzbFileParser;
pub type NzbgetRsAllocFileIdCb = Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>;

#[repr(C)]
pub struct NzbgetRsNzbFileSummary {
    pub file_count: c_int,
    pub total_articles: c_int,
    pub failed_articles: c_int,
    pub cur_failed_articles: c_int,
    pub size: i64,
    pub remaining_size: i64,
    pub failed_size: i64,
    pub cur_failed_size: i64,
    pub par_size: i64,
    pub par_failed_size: i64,
    pub par_cur_failed_size: i64,
    pub remaining_par_count: c_int,
    pub full_content_hash: u32,
    pub filtered_content_hash: u32,
    pub many_dupe_files: c_int,
    pub password: *const c_char,
    pub meta_name: *const c_char,
    pub meta_title: *const c_char,
    pub category: *const c_char,
    pub kept_files_count: usize,
    pub messages_count: usize,
}

#[repr(C)]
pub struct NzbgetRsNzbKeptFileView {
    pub id: c_int,
    pub subject: *const c_char,
    pub filename: *const c_char,
    pub time: c_int,
    pub total_articles: c_int,
    pub missed_articles: c_int,
    pub size: i64,
    pub remaining_size: i64,
    pub missed_size: i64,
    pub par_file: c_int,
    pub cut_short: c_int,
    pub groups_ptrs: *const *const c_char,
    pub groups_count: usize,
    pub article_parts: *const c_int,
    pub article_sizes: *const i64,
    pub article_msg_ids: *const *const c_char,
    pub articles_count: usize,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzbfile_parser_new(
    file_name: *const c_char,
) -> *mut NzbgetRsNzbFileParser {
    let fn_bytes = if file_name.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(file_name).to_bytes()
    };
    Box::into_raw(Box::new(crate::nzbfile::NzbFileParser::new(fn_bytes)))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzbfile_parser_free(parser: *mut NzbgetRsNzbFileParser) {
    if !parser.is_null() {
        drop(Box::from_raw(parser));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzbfile_parser_parse_file(
    parser: *mut NzbgetRsNzbFileParser,
    par_ignore_ext: *const c_char,
    ct: *const RsCtype,
    alloc_ctx: *mut std::ffi::c_void,
    alloc_file_id_cb: NzbgetRsAllocFileIdCb,
    on_parsed_subject: Option<unsafe extern "C" fn(*const c_char, *const c_char)>,
    sort_indices: Option<unsafe extern "C" fn(*const *const c_char, *mut usize, usize)>,
    out_summary: *mut NzbgetRsNzbFileSummary,
) -> c_int {
    if parser.is_null() || out_summary.is_null() {
        return 0;
    }
    let Some(ct_view) = rs_ctype(ct) else {
        return 0;
    };
    let pie_bytes = if par_ignore_ext.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(par_ignore_ext).to_bytes()
    };
    let p = &mut *parser;
    let sort_cb = sort_indices.map(|cb| {
        move |filenames: &[*const c_char], indices: &mut [usize]| {
            cb(filenames.as_ptr(), indices.as_mut_ptr(), indices.len());
        }
    });
    let ok = p.parse_file(
        pie_bytes,
        &ct_view,
        || {
            if let Some(cb) = alloc_file_id_cb {
                cb(alloc_ctx)
            } else {
                0
            }
        },
        |subj, fname| {
            if let Some(cb) = on_parsed_subject {
                cb(subj.as_ptr(), fname.as_ptr());
            }
        },
        sort_cb,
    );
    *out_summary = NzbgetRsNzbFileSummary {
        file_count: p.file_count,
        total_articles: p.total_articles,
        failed_articles: p.failed_articles,
        cur_failed_articles: p.cur_failed_articles,
        size: p.size,
        remaining_size: p.remaining_size,
        failed_size: p.failed_size,
        cur_failed_size: p.cur_failed_size,
        par_size: p.par_size,
        par_failed_size: p.par_failed_size,
        par_cur_failed_size: p.par_cur_failed_size,
        remaining_par_count: p.remaining_par_count,
        full_content_hash: p.full_content_hash,
        filtered_content_hash: p.filtered_content_hash,
        many_dupe_files: c_int::from(p.many_dupe_files),
        password: p.password.as_ptr(),
        meta_name: p.meta_name.as_ptr(),
        meta_title: p.meta_title.as_ptr(),
        category: p.category.as_ptr(),
        kept_files_count: p.files.len(),
        messages_count: p.messages.len(),
    };
    c_int::from(ok)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzbfile_parser_get_message(
    parser: *const NzbgetRsNzbFileParser,
    index: usize,
    out_kind: *mut c_int,
    out_text: *mut *const c_char,
) -> c_int {
    let Some(p) = parser.as_ref() else {
        return 0;
    };
    let Some(msg) = p.messages.get(index) else {
        return 0;
    };
    if !out_kind.is_null() {
        *out_kind = msg.kind;
    }
    if !out_text.is_null() {
        *out_text = msg.text.as_ptr();
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzbfile_parser_get_file(
    parser: *const NzbgetRsNzbFileParser,
    index: usize,
    out_view: *mut NzbgetRsNzbKeptFileView,
) -> c_int {
    if out_view.is_null() {
        return 0;
    }
    let Some(p) = parser.as_ref() else {
        return 0;
    };
    let Some(f) = p.files.get(index) else {
        return 0;
    };
    *out_view = NzbgetRsNzbKeptFileView {
        id: f.id,
        subject: f.subject.as_ptr(),
        filename: f.filename.as_ptr(),
        time: f.time,
        total_articles: f.total_articles,
        missed_articles: f.missed_articles,
        size: f.size,
        remaining_size: f.remaining_size,
        missed_size: f.missed_size,
        par_file: c_int::from(f.par_file),
        cut_short: c_int::from(f.cut_short),
        groups_ptrs: f.group_ptrs.as_ptr(),
        groups_count: f.group_ptrs.len(),
        article_parts: f.article_parts.as_ptr(),
        article_sizes: f.article_sizes.as_ptr(),
        article_msg_ids: f.article_msg_id_ptrs.as_ptr(),
        articles_count: f.article_parts.len(),
    };
    1
}

// ============================================================================
// DownloadInfo, DupeProbe & DupeCoordinator End-to-End FFI
// ============================================================================


// ============================================================================
// DirectUnpack & UnpackController (`rust/src/unpack.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_queue_editor_visit_category_ext_diff(
    old_ext: *const c_char,
    new_ext: *const c_char,
    ctx: *mut std::ffi::c_void,
    cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *const c_char)>,
) {
    let Some(cb) = cb else { return };
    crate::queueeditor::visit_category_ext_diff(input(old_ext), input(new_ext), |name, val| {
        let mut c_name = Vec::with_capacity(name.len() + 1);
        c_name.extend_from_slice(name);
        c_name.push(0);
        let mut c_val = Vec::with_capacity(val.len() + 1);
        c_val.extend_from_slice(val);
        c_val.push(0);
        cb(ctx, c_name.as_ptr().cast(), c_val.as_ptr().cast());
    });
}

// =========================================================================
// Consolidated RepairController (`repair.rs`), ContentMap (`streamrepair.rs`),
// and TwinCheck (`twin.rs`) FFI
// =========================================================================
unsafe fn cstr_to_path_buf(ptr: *const c_char) -> std::path::PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(input(ptr)))
    }
    #[cfg(not(unix))]
    {
        std::path::PathBuf::from(String::from_utf8_lossy(input(ptr)).into_owned())
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_regex_new(
    pattern: *const c_char,
    match_buf_size: c_int,
) -> *mut std::ffi::c_void {
    let rx = if pattern.is_null() {
        crate::util::PosixRegEx::new_with(false, match_buf_size)
    } else {
        #[cfg(unix)]
        {
            crate::util::PosixRegEx::compile(CStr::from_ptr(pattern), match_buf_size)
        }
        #[cfg(not(unix))]
        {
            crate::util::PosixRegEx::new_with(false, match_buf_size)
        }
    };
    Box::into_raw(Box::new(rx)).cast()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_regex_free(handle: *mut std::ffi::c_void) {
    if !handle.is_null() {
        drop(Box::from_raw(handle.cast::<crate::util::PosixRegEx>()));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_regex_is_valid(handle: *const std::ffi::c_void) -> c_int {
    if handle.is_null() {
        return 0;
    }
    let rx = &*handle.cast::<crate::util::PosixRegEx>();
    if rx.is_valid() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_regex_match(
    handle: *mut std::ffi::c_void,
    str_ptr: *const c_char,
) -> c_int {
    if handle.is_null() || str_ptr.is_null() {
        return 0;
    }
    let rx = &mut *handle.cast::<crate::util::PosixRegEx>();
    #[cfg(unix)]
    {
        c_int::from(rx.match_cstr(CStr::from_ptr(str_ptr)))
    }
    #[cfg(not(unix))]
    {
        let _ = rx;
        0
    }
}

#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dir_browser_new(
    path: *const c_char,
    snapshot: c_int,
) -> *mut std::ffi::c_void {
    let p = (!path.is_null()).then(|| CStr::from_ptr(path));
    Box::into_raw(Box::new(crate::fsops::PosixDirBrowser::new(
        p,
        snapshot != 0,
    )))
    .cast()
}

#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dir_browser_next(
    handle: *mut std::ffi::c_void,
) -> *const c_char {
    if handle.is_null() {
        return std::ptr::null();
    }
    let browser = &mut *handle.cast::<crate::fsops::PosixDirBrowser>();
    browser.next_ptr()
}

#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dir_browser_free(handle: *mut std::ffi::c_void) {
    if !handle.is_null() {
        drop(Box::from_raw(
            handle.cast::<crate::fsops::PosixDirBrowser>(),
        ));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_alloc_buf(data: *const c_char, len: usize) -> RsBuf {
    into_buf(bytes(data, len).to_vec())
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_mgr_load_extensions(
    script_dir: *const c_char,
    script_order: *const c_char,
    path_sep: c_char,
    ctx: *mut std::ffi::c_void,
    on_error: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
    on_extension: Option<
        unsafe extern "C" fn(
            ctx: *mut std::ffi::c_void,
            entry: *const c_char,
            entry_len: usize,
            location: *const c_char,
            location_len: usize,
            root_dir: *const c_char,
            root_dir_len: usize,
            name: *const c_char,
            name_len: usize,
            display_name: *const c_char,
            display_name_len: usize,
            queue_events: *const c_char,
            queue_events_len: usize,
            task_time: *const c_char,
            task_time_len: usize,
            post_script: c_int,
            scan_script: c_int,
            queue_script: c_int,
            scheduler_script: c_int,
            feed_script: c_int,
            json_ptr: *const c_char,
            json_len: usize,
            xml_ptr: *const c_char,
            xml_len: usize,
        ),
    >,
) -> RsBuf {
    let dir_bytes = input(script_dir);
    let order_bytes = input(script_order);
    let loaded = match crate::extension::ext_mgr_load_extensions_with(
        dir_bytes,
        order_bytes,
        path_sep as u8,
        |err_msg| {
            if let Some(cb) = on_error {
                let c_msg = nul(err_msg);
                cb(ctx, c_msg.as_ptr().cast());
            }
        },
    ) {
        Ok(list) => list,
        Err(err) => return into_buf(err.into_bytes()),
    };
    if let Some(cb) = on_extension {
        for ext in &loaded {
            let json = crate::extension::extension_to_json(ext);
            let xml = crate::extension::extension_to_xml(ext);
            cb(
                ctx,
                ext.entry.as_ptr().cast(),
                ext.entry.len(),
                ext.location.as_ptr().cast(),
                ext.location.len(),
                ext.root_dir.as_ptr().cast(),
                ext.root_dir.len(),
                ext.name.as_ptr().cast(),
                ext.name.len(),
                ext.display_name.as_ptr().cast(),
                ext.display_name.len(),
                ext.queue_events.as_ptr().cast(),
                ext.queue_events.len(),
                ext.task_time.as_ptr().cast(),
                ext.task_time.len(),
                i32::from(ext.post_script),
                i32::from(ext.scan_script),
                i32::from(ext.queue_script),
                i32::from(ext.scheduler_script),
                i32::from(ext.feed_script),
                json.as_ptr().cast(),
                json.len(),
                xml.as_ptr().cast(),
                xml.len(),
            );
        }
    }
    RsBuf {
        data: std::ptr::null_mut(),
        len: 0,
        cap: 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_mgr_resolve_extension_path(
    ext_loc_ptr: *const c_char,
    ext_loc_len: usize,
    ext_entry_ptr: *const c_char,
    ext_entry_len: usize,
    all_locs_ptrs: *const *const c_char,
    all_locs_lens: *const usize,
    all_locs_count: usize,
) -> RsBuf {
    let mut locs = Vec::with_capacity(all_locs_count);
    for i in 0..all_locs_count {
        if !all_locs_ptrs.is_null() && !all_locs_lens.is_null() {
            locs.push(bytes(*all_locs_ptrs.add(i), *all_locs_lens.add(i)));
        }
    }
    let resolved = crate::extension::ext_mgr_resolve_extension_path(
        bytes(ext_loc_ptr, ext_loc_len),
        bytes(ext_entry_ptr, ext_entry_len),
        &locs,
    );
    into_buf(resolved.to_vec())
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_system_info_format(
    mode: c_int,
    uts_sysname: *const c_char,
    uts_release: *const c_char,
    uts_machine: *const c_char,
    ssl_lib_name: *const c_char,
    ssl_raw_version: *const c_char,
    ssl_extract_after_space: c_int,
    pub_ip_ptr: *const c_char,
    pub_ip_len: usize,
    priv_ip_ptr: *const c_char,
    priv_ip_len: usize,
) -> RsBuf {
    into_buf(crate::systemhealth::system_info_format(
        mode,
        input(uts_sysname),
        input(uts_release),
        input(uts_machine),
        input(ssl_lib_name),
        input(ssl_raw_version),
        ssl_extract_after_space != 0,
        bytes(pub_ip_ptr, pub_ip_len),
        bytes(priv_ip_ptr, priv_ip_len),
    ))
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_server_pool_select_connection(
    start_level: c_int,
    levels_ptr: *const i32,
    levels_len: usize,
    srv_ids: *const i32,
    srv_groups: *const i32,
    srv_norm_levels: *const i32,
    srv_active: *const u8,
    srv_optional: *const u8,
    srv_blocked: *const u8,
    srv_count: usize,
    conn_srv_indices: *const usize,
    conn_in_use: *const u8,
    conn_connected: *const u8,
    conn_count: usize,
    has_want: c_int,
    want_id: i32,
    want_group: i32,
    ign_ids: *const i32,
    ign_groups: *const i32,
    ign_norm_levels: *const i32,
    ign_count: usize,
    rand_val: c_int,
    out_unblock_indices: *mut usize,
    out_unblock_count: *mut usize,
    out_conn_index: *mut usize,
    out_chosen_level: *mut i32,
) -> c_int {
    let levels = if levels_ptr.is_null() || levels_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(levels_ptr, levels_len)
    };
    let mut servers = Vec::with_capacity(srv_count);
    for i in 0..srv_count {
        servers.push(crate::nntp::ServerPoolServerView {
            id: *srv_ids.add(i),
            group: *srv_groups.add(i),
            norm_level: *srv_norm_levels.add(i),
            active: *srv_active.add(i) != 0,
            optional: *srv_optional.add(i) != 0,
            blocked: *srv_blocked.add(i) != 0,
        });
    }
    let mut conns = Vec::with_capacity(conn_count);
    for i in 0..conn_count {
        conns.push(crate::nntp::ServerPoolConnView {
            server_index: *conn_srv_indices.add(i),
            in_use: *conn_in_use.add(i) != 0,
            connected: *conn_connected.add(i) != 0,
        });
    }
    let want = if has_want != 0 {
        Some((want_id, want_group))
    } else {
        None
    };
    let mut ignores = Vec::with_capacity(ign_count);
    for i in 0..ign_count {
        ignores.push((*ign_ids.add(i), *ign_groups.add(i), *ign_norm_levels.add(i)));
    }
    let mut unblock = Vec::new();
    let res = crate::nntp::server_pool_select_connection(
        start_level,
        levels,
        &servers,
        &conns,
        want,
        &ignores,
        rand_val,
        &mut unblock,
    );
    if !out_unblock_count.is_null() {
        *out_unblock_count = unblock.len();
    }
    if !out_unblock_indices.is_null() {
        for (i, &idx) in unblock.iter().enumerate() {
            *out_unblock_indices.add(i) = idx;
        }
    }
    if let Some((conn_idx, chosen_level)) = res {
        if !out_conn_index.is_null() {
            *out_conn_index = conn_idx;
        }
        if !out_chosen_level.is_null() {
            *out_chosen_level = chosen_level;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_script_config_read_cipher_snapshot(
    filename: *const c_char,
    out_bytes: *mut RsBuf,
) -> c_int {
    if filename.is_null() || out_bytes.is_null() {
        return 0;
    }
    let fn_bytes = CStr::from_ptr(filename).to_bytes();
    match crate::extension::script_config_read_cipher_snapshot(fn_bytes) {
        Some(buf) => {
            *out_bytes = into_buf(buf);
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_script_config_commit_cipher_patch(
    filename: *const c_char,
    expected_ptr: *const c_char,
    expected_len: usize,
    replacement_ptr: *const c_char,
    replacement_len: usize,
    ctx: *mut std::ffi::c_void,
    cancel_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    on_dir_sync_fail_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
) -> c_int {
    if filename.is_null() {
        return crate::extension::RsCipherCommit::Failed as c_int;
    }
    let fn_bytes = CStr::from_ptr(filename).to_bytes();
    let expected = bytes(expected_ptr, expected_len);
    let replacement = bytes(replacement_ptr, replacement_len);
    let commit = crate::extension::script_config_commit_cipher_patch(
        fn_bytes,
        expected,
        replacement,
        || cancel_cb.is_some_and(|cb| unsafe { cb(ctx) != 0 }),
        || {
            if let Some(cb) = on_dir_sync_fail_cb {
                unsafe { cb(ctx) };
            }
        },
    );
    commit as c_int
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NzbgetRsOptionsHost {
    pub ctx: *mut c_void,
    pub find_entry: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            *mut *const c_char,
            *mut *const c_char,
            *mut c_int,
        ) -> c_int,
    >,
    pub set_entry_raw_value:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
    pub set_option: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
    pub set_config_line: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
    pub get_config_line: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
    pub config_error: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub config_warn: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub file_exists: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int>,
    pub ensure_directory:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_char, usize) -> c_int>,
    pub add_news_server: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            c_int,
            *const c_char,
            *const c_char,
            c_int,
            c_int,
            *const c_char,
            *const c_char,
            c_int,
            c_int,
            *const c_char,
            c_int,
            c_int,
            c_int,
            c_int,
            c_int,
            u32,
        ),
    >,
    pub add_feed: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            *const c_char,
            *const c_char,
            c_int,
            *const c_char,
            c_int,
            c_int,
            *const c_char,
            c_int,
            c_int,
            *const c_char,
            u32,
        ),
    >,
    pub add_task:
        Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int, c_int, *const c_char)>,
    pub add_category: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            *const c_char,
            c_int,
            *const c_char,
            *const *const c_char,
            usize,
        ),
    >,
}

struct FfiOptionsHost<'a>(&'a NzbgetRsOptionsHost);

impl crate::options::OptionsHost for FfiOptionsHost<'_> {
    fn find_entry(
        &mut self,
        name: &CStr,
    ) -> Option<(Option<std::ffi::CString>, Option<std::ffi::CString>, i32)> {
        let cb = self.0.find_entry?;
        let mut val_ptr: *const c_char = std::ptr::null();
        let mut def_ptr: *const c_char = std::ptr::null();
        let mut line_no: c_int = 0;
        let found = unsafe {
            cb(
                self.0.ctx,
                name.as_ptr(),
                &mut val_ptr,
                &mut def_ptr,
                &mut line_no,
            )
        };
        if found == 0 {
            return None;
        }
        let val = if val_ptr.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(val_ptr) }.to_owned())
        };
        let def = if def_ptr.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(def_ptr) }.to_owned())
        };
        Some((val, def, line_no))
    }

    fn set_entry_raw_value(&mut self, name: &CStr, value: &CStr) {
        if let Some(cb) = self.0.set_entry_raw_value {
            unsafe { cb(self.0.ctx, name.as_ptr(), value.as_ptr()) };
        }
    }

    fn set_option(&mut self, name: &CStr, value: &CStr) {
        if let Some(cb) = self.0.set_option {
            unsafe { cb(self.0.ctx, name.as_ptr(), value.as_ptr()) };
        }
    }

    fn set_config_line(&mut self, line: i32) {
        if let Some(cb) = self.0.set_config_line {
            unsafe { cb(self.0.ctx, line) };
        }
    }

    fn get_config_line(&self) -> i32 {
        match self.0.get_config_line {
            Some(cb) => unsafe { cb(self.0.ctx) },
            None => 0,
        }
    }

    fn config_error(&mut self, msg: &CStr) {
        if let Some(cb) = self.0.config_error {
            unsafe { cb(self.0.ctx, msg.as_ptr()) };
        }
    }

    fn config_warn(&mut self, msg: &CStr) {
        if let Some(cb) = self.0.config_warn {
            unsafe { cb(self.0.ctx, msg.as_ptr()) };
        }
    }

    fn file_exists(&self, path: &CStr) -> bool {
        match self.0.file_exists {
            Some(cb) => unsafe { cb(self.0.ctx, path.as_ptr()) != 0 },
            None => false,
        }
    }

    fn ensure_directory(&mut self, dir: &CStr) -> Result<(), std::ffi::CString> {
        let Some(cb) = self.0.ensure_directory else {
            return Ok(());
        };
        let mut err_buf = [0 as c_char; 1024];
        let ok = unsafe {
            cb(
                self.0.ctx,
                dir.as_ptr(),
                err_buf.as_mut_ptr(),
                err_buf.len(),
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            err_buf[err_buf.len() - 1] = 0;
            let err_cstr = unsafe { CStr::from_ptr(err_buf.as_ptr()) };
            Err(err_cstr.to_owned())
        }
    }

    fn add_news_server(
        &mut self,
        id: i32,
        active: bool,
        name: Option<&CStr>,
        host: Option<&CStr>,
        port: i32,
        ip_version: i32,
        user: Option<&CStr>,
        pass: Option<&CStr>,
        join_group: bool,
        tls: bool,
        cipher: Option<&CStr>,
        max_connections: i32,
        retention: i32,
        level: i32,
        group: i32,
        optional: bool,
        cert_verif_level: u32,
    ) {
        if let Some(cb) = self.0.add_news_server {
            unsafe {
                cb(
                    self.0.ctx,
                    id,
                    c_int::from(active),
                    name.map_or(std::ptr::null(), CStr::as_ptr),
                    host.map_or(std::ptr::null(), CStr::as_ptr),
                    port,
                    ip_version,
                    user.map_or(std::ptr::null(), CStr::as_ptr),
                    pass.map_or(std::ptr::null(), CStr::as_ptr),
                    c_int::from(join_group),
                    c_int::from(tls),
                    cipher.map_or(std::ptr::null(), CStr::as_ptr),
                    max_connections,
                    retention,
                    level,
                    group,
                    c_int::from(optional),
                    cert_verif_level,
                )
            };
        }
    }

    fn add_feed(
        &mut self,
        id: i32,
        name: Option<&CStr>,
        url: Option<&CStr>,
        interval: i32,
        filter: Option<&CStr>,
        backlog: bool,
        pause_nzb: bool,
        category: Option<&CStr>,
        category_source: i32,
        priority: i32,
        extensions: Option<&CStr>,
        cert_verif_level: u32,
    ) {
        if let Some(cb) = self.0.add_feed {
            unsafe {
                cb(
                    self.0.ctx,
                    id,
                    name.map_or(std::ptr::null(), CStr::as_ptr),
                    url.map_or(std::ptr::null(), CStr::as_ptr),
                    interval,
                    filter.map_or(std::ptr::null(), CStr::as_ptr),
                    c_int::from(backlog),
                    c_int::from(pause_nzb),
                    category.map_or(std::ptr::null(), CStr::as_ptr),
                    category_source,
                    priority,
                    extensions.map_or(std::ptr::null(), CStr::as_ptr),
                    cert_verif_level,
                )
            };
        }
    }

    fn add_task(
        &mut self,
        id: i32,
        hours: i32,
        minutes: i32,
        week_days_bits: i32,
        command: i32,
        param: Option<&CStr>,
    ) {
        if let Some(cb) = self.0.add_task {
            unsafe {
                cb(
                    self.0.ctx,
                    id,
                    hours,
                    minutes,
                    week_days_bits,
                    command,
                    param.map_or(std::ptr::null(), CStr::as_ptr),
                )
            };
        }
    }

    fn add_category(
        &mut self,
        name: &CStr,
        dest_dir: Option<&CStr>,
        unpack: bool,
        extensions: Option<&CStr>,
        aliases: &[std::ffi::CString],
    ) {
        if let Some(cb) = self.0.add_category {
            let ptrs: Vec<*const c_char> = aliases.iter().map(|a| a.as_ptr()).collect();
            unsafe {
                cb(
                    self.0.ctx,
                    name.as_ptr(),
                    dest_dir.map_or(std::ptr::null(), CStr::as_ptr),
                    c_int::from(unpack),
                    extensions.map_or(std::ptr::null(), CStr::as_ptr),
                    ptrs.as_ptr(),
                    ptrs.len(),
                )
            };
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_init_defaults(
    disable_curses: c_int,
    set_opt: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
    ctx: *mut c_void,
) {
    let Some(cb) = set_opt else { return };
    crate::options::init_defaults_with(disable_curses != 0, |name, val| {
        cb(ctx, name.as_ptr(), val.as_ptr());
    });
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_expand_variables(
    initial_value: *const c_char,
    get_opt: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *const c_char>,
    ctx: *mut c_void,
) -> RsBuf {
    let init_slice = input(initial_value);
    let expanded = crate::options::expand_option_variables(init_slice, |var_name| {
        let cb = get_opt?;
        let ptr = cb(ctx, var_name.as_ptr());
        if ptr.is_null() {
            None
        } else {
            Some(CStr::from_ptr(ptr).to_bytes().to_vec())
        }
    });
    into_buf(expanded)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_split_and_convert(
    option: *const c_char,
    out_name: *mut RsBuf,
    out_value: *mut RsBuf,
) -> c_int {
    if option.is_null() {
        return 0;
    }
    let Some((name, val)) = crate::options::split_and_convert_option_string(CStr::from_ptr(option))
    else {
        return 0;
    };
    if !out_name.is_null() {
        *out_name = into_buf(name.into_bytes());
    }
    if !out_value.is_null() {
        *out_value = into_buf(val.into_bytes());
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_resolve_tool_path(
    value: *const c_char,
    len: usize,
) -> RsBuf {
    let slice = if value.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(value.cast::<u8>(), len)
    };
    into_buf(crate::options::resolve_tool_path_option(slice))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_category_alias_list_matches(
    aliases: *const *const c_char,
    aliases_len: usize,
    name: *const c_char,
    signed_char: c_int,
) -> c_int {
    if aliases.is_null() || aliases_len == 0 || name.is_null() {
        return 0;
    }
    let slice_ptrs = std::slice::from_raw_parts(aliases, aliases_len);
    let alias_slices: Vec<&[u8]> = slice_ptrs.iter().map(|&p| input(p)).collect();
    c_int::from(crate::options::category_alias_list_matches(
        &alias_slices,
        input(name),
        signed_char != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_check_dir(
    host: *const NzbgetRsOptionsHost,
    no_disk_access: c_int,
    option_name: *const c_char,
    parent_dir: *const c_char,
    allow_empty: c_int,
    create: c_int,
    out_has_dir: *mut c_int,
) -> RsBuf {
    if host.is_null() || option_name.is_null() {
        if !out_has_dir.is_null() {
            *out_has_dir = 0;
        }
        return into_buf(Vec::new());
    }
    let mut adapter = FfiOptionsHost(&*host);
    let parent_opt = if parent_dir.is_null() {
        None
    } else {
        Some(CStr::from_ptr(parent_dir))
    };
    match crate::options::check_dir_host(
        &mut adapter,
        no_disk_access != 0,
        CStr::from_ptr(option_name),
        parent_opt,
        allow_empty != 0,
        create != 0,
    ) {
        Some(dir) => {
            if !out_has_dir.is_null() {
                *out_has_dir = 1;
            }
            into_buf(dir.into_bytes())
        }
        None => {
            if !out_has_dir.is_null() {
                *out_has_dir = 0;
            }
            into_buf(Vec::new())
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_check_script_dir(
    host: *const NzbgetRsOptionsHost,
    no_disk_access: c_int,
    main_dir: *const c_char,
    out_has_dir: *mut c_int,
) -> RsBuf {
    if host.is_null() {
        if !out_has_dir.is_null() {
            *out_has_dir = 0;
        }
        return into_buf(Vec::new());
    }
    let mut adapter = FfiOptionsHost(&*host);
    let main_opt = if main_dir.is_null() {
        None
    } else {
        Some(CStr::from_ptr(main_dir))
    };
    match crate::options::check_script_dir_host(&mut adapter, no_disk_access != 0, main_opt) {
        Some(dir) => {
            if !out_has_dir.is_null() {
                *out_has_dir = 1;
            }
            into_buf(dir.into_bytes())
        }
        None => {
            if !out_has_dir.is_null() {
                *out_has_dir = 0;
            }
            into_buf(Vec::new())
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_init_scalars(
    host: *const NzbgetRsOptionsHost,
    out_scalars: *mut crate::options::NzbgetRsParsedOptions,
    out_has_mem_path: *mut c_int,
    out_mem_path: *mut RsBuf,
    out_has_hl_ext: *mut c_int,
    out_hl_ext: *mut RsBuf,
    out_has_dupe_url: *mut c_int,
    out_dupe_url: *mut RsBuf,
    out_has_dupe_key: *mut c_int,
    out_dupe_key: *mut RsBuf,
    out_warn_dupe_no_url: *mut c_int,
) {
    if host.is_null() || out_scalars.is_null() {
        return;
    }
    let mut adapter = FfiOptionsHost(&*host);
    let mut mem_path = None;
    let mut hl_ext = None;
    let mut dupe_url = None;
    let mut dupe_key = None;
    let mut warn_no_url = false;
    let scalars = crate::options::init_scalar_options_host(
        &mut adapter,
        &mut mem_path,
        &mut hl_ext,
        &mut dupe_url,
        &mut dupe_key,
        &mut warn_no_url,
    );
    *out_scalars = scalars;
    if !out_warn_dupe_no_url.is_null() {
        *out_warn_dupe_no_url = c_int::from(warn_no_url);
    }
    let write_opt_buf = |opt: Option<std::ffi::CString>, has: *mut c_int, buf: *mut RsBuf| {
        if let Some(c) = opt {
            if !has.is_null() {
                *has = 1;
            }
            if !buf.is_null() {
                *buf = into_buf(c.into_bytes());
            }
        } else {
            if !has.is_null() {
                *has = 0;
            }
            if !buf.is_null() {
                *buf = into_buf(Vec::new());
            }
        }
    };
    write_opt_buf(mem_path, out_has_mem_path, out_mem_path);
    write_opt_buf(hl_ext, out_has_hl_ext, out_hl_ext);
    write_opt_buf(dupe_url, out_has_dupe_url, out_dupe_url);
    write_opt_buf(dupe_key, out_has_dupe_key, out_dupe_key);
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nzbget_rs_options_check_options(
    host: *const NzbgetRsOptionsHost,
    scalars: *mut crate::options::NzbgetRsParsedOptions,
    no_disk_access: c_int,
    is_32bit: c_int,
    disable_parcheck: c_int,
    disable_curses: c_int,
    disable_tls: c_int,
    have_x509_check_host: c_int,
    cert_store_empty: c_int,
    web_dir: *const c_char,
    config_template: *const c_char,
    unpack_pass_file: *const c_char,
    out_ct_changed: *mut c_int,
) -> RsBuf {
    if host.is_null() || scalars.is_null() {
        if !out_ct_changed.is_null() {
            *out_ct_changed = 0;
        }
        return into_buf(Vec::new());
    }
    let mut adapter = FfiOptionsHost(&*host);
    let mut ct_opt = if config_template.is_null() {
        None
    } else {
        Some(CStr::from_ptr(config_template).to_owned())
    };
    let orig_ct = ct_opt.clone();
    let upf_opt = if unpack_pass_file.is_null() {
        None
    } else {
        Some(CStr::from_ptr(unpack_pass_file))
    };
    let mut local_scalars = *scalars;
    crate::options::check_options_host(
        &mut adapter,
        &mut local_scalars,
        no_disk_access != 0,
        is_32bit != 0,
        disable_parcheck != 0,
        disable_curses != 0,
        disable_tls != 0,
        have_x509_check_host != 0,
        cert_store_empty != 0,
        input(web_dir),
        &mut ct_opt,
        upf_opt,
    );
    *scalars = local_scalars;
    if ct_opt != orig_ct {
        if !out_ct_changed.is_null() {
            *out_ct_changed = 1;
        }
        into_buf(ct_opt.unwrap_or_default().into_bytes())
    } else {
        if !out_ct_changed.is_null() {
            *out_ct_changed = 0;
        }
        into_buf(Vec::new())
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_init_servers(
    host: *const NzbgetRsOptionsHost,
    disable_tls: c_int,
    scalars: *mut crate::options::NzbgetRsParsedOptions,
) {
    if host.is_null() || scalars.is_null() {
        return;
    }
    let mut adapter = FfiOptionsHost(&*host);
    let mut local_scalars = *scalars;
    crate::options::init_servers_host(&mut adapter, disable_tls != 0, &mut local_scalars);
    *scalars = local_scalars;
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_init_categories(
    host: *const NzbgetRsOptionsHost,
    no_disk_access: c_int,
    dest_dir: *const c_char,
) {
    if host.is_null() {
        return;
    }
    let mut adapter = FfiOptionsHost(&*host);
    let dd_opt = if dest_dir.is_null() {
        None
    } else {
        Some(CStr::from_ptr(dest_dir))
    };
    crate::options::init_categories_host(&mut adapter, no_disk_access != 0, dd_opt);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_init_feeds(host: *const NzbgetRsOptionsHost) {
    if host.is_null() {
        return;
    }
    let mut adapter = FfiOptionsHost(&*host);
    crate::options::init_feeds_host(&mut adapter);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_init_scheduler(host: *const NzbgetRsOptionsHost) {
    if host.is_null() {
        return;
    }
    let mut adapter = FfiOptionsHost(&*host);
    crate::options::init_scheduler_host(&mut adapter);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_options_create_scheduler_task(
    host: *const NzbgetRsOptionsHost,
    id: c_int,
    time: *const c_char,
    week_days: *const c_char,
    command: c_int,
    param: *const c_char,
) {
    if host.is_null() {
        return;
    }
    let mut adapter = FfiOptionsHost(&*host);
    let time_opt = if time.is_null() {
        None
    } else {
        Some(CStr::from_ptr(time))
    };
    let wd_opt = if week_days.is_null() {
        None
    } else {
        Some(CStr::from_ptr(week_days))
    };
    let param_opt = if param.is_null() {
        None
    } else {
        Some(CStr::from_ptr(param))
    };
    crate::options::create_scheduler_task_host(
        &mut adapter,
        id,
        time_opt,
        wd_opt,
        command,
        param_opt,
    );
}

#[cfg(test)]
pub(crate) type DonorNextArticleFn = Option<
    unsafe extern "C" fn(
        *mut std::ffi::c_void,
        *mut c_int,
        *mut i64,
        *mut i64,
        *mut *const u8,
        *mut usize,
    ) -> c_int,
>;

#[cfg(test)]
pub(crate) type DonorBeginMemberBatchFn =
    Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const i32, usize)>;

#[cfg(test)]
pub(crate) struct NzbgetRsCrossPackSession(crate::streamrepair::CrossPackRepairSession);

#[cfg(test)]
pub(crate) type DonorFetchMemberPartFn = Option<
    unsafe extern "C" fn(
        *mut std::ffi::c_void,
        i32,
        i32,
        *mut c_int,
        *mut i64,
        *mut i64,
        *mut *const u8,
        *mut usize,
    ) -> c_int,
>;

#[cfg(test)]
struct FfiCrossPackDonorEnv {
    ctx: *mut std::ffi::c_void,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    fetch_part_cb: DonorFetchMemberPartFn,
    begin_prefetch_cb: DonorBeginMemberBatchFn,
    next_batch_cb: DonorNextArticleFn,
    cancel_batch_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    begin_materialize_member_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32)>,
    set_progress_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, i32)>,
    set_materializing_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32)>,
    add_recovered_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64, i32)>,
    sync_target_holes_cb: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const usize,
            *const crate::dupefallback::NzbgetRsByteRange,
            usize,
        ),
    >,
    emit_message_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
    emit_extractor_log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
    emit_debug_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
}

#[cfg(test)]
impl crate::streamrepair::CrossPackDonorEnv for FfiCrossPackDonorEnv {
    fn is_stopped(&mut self) -> bool {
        self.is_stopped_cb
            .is_some_and(|cb| unsafe { cb(self.ctx) != 0 })
    }

    fn has_batch_fetcher(&self) -> bool {
        self.begin_prefetch_cb.is_some() && self.next_batch_cb.is_some()
    }

    fn fetch_donor_part(
        &mut self,
        member_index: i32,
        part_index: i32,
    ) -> Option<crate::streamrepair::FetchedDonorArticle> {
        let cb = self.fetch_part_cb?;
        let mut success: c_int = 0;
        let mut offset: i64 = 0;
        let mut file_size: i64 = 0;
        let mut data_ptr: *const u8 = std::ptr::null();
        let mut data_len: usize = 0;
        let ok = unsafe {
            cb(
                self.ctx,
                member_index,
                part_index,
                &mut success,
                &mut offset,
                &mut file_size,
                &mut data_ptr,
                &mut data_len,
            )
        };
        if ok == 0 {
            return None;
        }
        let data = if data_ptr.is_null() || data_len == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(data_ptr, data_len).to_vec() }
        };
        Some(crate::streamrepair::FetchedDonorArticle {
            success: success != 0,
            offset,
            file_size,
            data,
        })
    }

    fn begin_donor_prefetch(&mut self, member_index: i32, part_indices: &[i32]) {
        if let Some(cb) = self.begin_prefetch_cb {
            unsafe {
                cb(
                    self.ctx,
                    member_index,
                    part_indices.as_ptr(),
                    part_indices.len(),
                );
            }
        }
    }

    fn next_donor_prefetch(&mut self) -> Option<crate::streamrepair::FetchedDonorArticle> {
        let cb = self.next_batch_cb?;
        let mut success: c_int = 0;
        let mut offset: i64 = 0;
        let mut file_size: i64 = 0;
        let mut data_ptr: *const u8 = std::ptr::null();
        let mut data_len: usize = 0;
        let has_next = unsafe {
            cb(
                self.ctx,
                &mut success,
                &mut offset,
                &mut file_size,
                &mut data_ptr,
                &mut data_len,
            )
        };
        if has_next == 0 {
            return None;
        }
        let data = if data_ptr.is_null() || data_len == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(data_ptr, data_len).to_vec() }
        };
        Some(crate::streamrepair::FetchedDonorArticle {
            success: success != 0,
            offset,
            file_size,
            data,
        })
    }

    fn cancel_donor_prefetch(&mut self) {
        if let Some(cb) = self.cancel_batch_cb {
            unsafe { cb(self.ctx) };
        }
    }

    fn begin_materialize_member(&mut self, member_index: i32) {
        if let Some(cb) = self.begin_materialize_member_cb {
            unsafe { cb(self.ctx, member_index) };
        }
    }

    fn next_materialize_article(&mut self) -> Option<crate::streamrepair::FetchedDonorArticle> {
        self.next_donor_prefetch()
    }

    fn cancel_materialize_member(&mut self) {
        self.cancel_donor_prefetch();
    }

    fn set_progress(&mut self, label: &[u8], stage_progress: i32) {
        let Some(cb) = self.set_progress_cb else {
            return;
        };
        let mut buf: Vec<u8> = label.iter().copied().filter(|&b| b != 0).collect();
        buf.push(0);
        unsafe { cb(self.ctx, buf.as_ptr() as *const c_char, stage_progress) };
    }

    fn set_materializing(&mut self, delta: i32) {
        if let Some(cb) = self.set_materializing_cb {
            unsafe { cb(self.ctx, delta) };
        }
    }

    fn add_recovered(&mut self, bytes: i64, holes: i32) {
        if let Some(cb) = self.add_recovered_cb {
            unsafe { cb(self.ctx, bytes, holes) };
        }
    }

    fn sync_target_holes(&mut self, target_holes: &[Vec<crate::dupefallback::NzbgetRsByteRange>]) {
        let Some(cb) = self.sync_target_holes_cb else {
            return;
        };
        let mut counts = Vec::with_capacity(target_holes.len());
        let mut flat = Vec::new();
        for th in target_holes {
            counts.push(th.len());
            flat.extend_from_slice(th);
        }
        unsafe {
            cb(self.ctx, counts.as_ptr(), flat.as_ptr(), counts.len());
        }
    }

    fn emit_message_bytes(&mut self, kind: crate::streamrepair::TargetMessageKind, text: &[u8]) {
        let Some(cb) = self.emit_message_cb else {
            return;
        };
        let mut buf: Vec<u8> = text.iter().copied().filter(|&b| b != 0).collect();
        buf.push(0);
        unsafe { cb(self.ctx, kind as i32, buf.as_ptr() as *const c_char) };
    }

    fn emit_extractor_log(&mut self, kind: i32, msg: &str) {
        if let Some(cb) = self.emit_extractor_log_cb {
            let mut buf: Vec<u8> = msg.bytes().filter(|&b| b != 0).collect();
            buf.push(0);
            unsafe { cb(self.ctx, kind, buf.as_ptr() as *const c_char) };
        }
    }

    fn on_rar_volume_parsed(
        &mut self,
        member_name: &[u8],
        vol: &crate::rarreader::Volume,
        files: &[crate::rarreader::RarFile],
    ) {
        let Some(cb) = self.emit_debug_cb else {
            return;
        };
        for mut line in crate::streamrepair::format_rar_volume_debug_lines(member_name, vol, files)
        {
            line.push(0);
            unsafe { cb(self.ctx, line.as_ptr() as *const c_char) };
        }
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn nzbget_rs_cross_pack_session_new(
    dest_dir: *const c_char,
    member_names: *const *const c_char,
    member_count: usize,
    target_filenames: *const *const c_char,
    target_decoded_sizes: *const i64,
    target_patch_eligible: *const u8,
    target_hole_counts: *const usize,
    target_holes_flat: *const crate::dupefallback::NzbgetRsByteRange,
    target_count: usize,
    target_password: *const c_char,
    ctx: *mut std::ffi::c_void,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    emit_message_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
    emit_debug_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) -> *mut NzbgetRsCrossPackSession {
    let mut member_names_vec = Vec::with_capacity(member_count);
    if !member_names.is_null() && member_count > 0 {
        for &ptr in std::slice::from_raw_parts(member_names, member_count) {
            member_names_vec.push(input(ptr));
        }
    }
    let mut targets_vec = Vec::with_capacity(target_count);
    if target_count > 0
        && !target_filenames.is_null()
        && !target_decoded_sizes.is_null()
        && !target_patch_eligible.is_null()
        && !target_hole_counts.is_null()
    {
        let fnames = std::slice::from_raw_parts(target_filenames, target_count);
        let dsizes = std::slice::from_raw_parts(target_decoded_sizes, target_count);
        let peligible = std::slice::from_raw_parts(target_patch_eligible, target_count);
        let hcounts = std::slice::from_raw_parts(target_hole_counts, target_count);
        let total_holes: usize = hcounts.iter().copied().sum();
        let flat = if target_holes_flat.is_null() || total_holes == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(target_holes_flat, total_holes)
        };
        let mut cursor = 0usize;
        for t in 0..target_count {
            let cnt = hcounts[t];
            let end = cursor.saturating_add(cnt).min(flat.len());
            targets_vec.push(crate::streamrepair::CrossPackTargetInput {
                filename: input(fnames[t]).to_vec(),
                decoded_file_size: dsizes[t],
                patch_eligible: peligible[t] != 0,
                holes: flat[cursor..end].to_vec(),
            });
            cursor = end;
        }
    }
    let pw = if target_password.is_null() {
        None
    } else {
        let s = input(target_password);
        (!s.is_empty()).then_some(s)
    };
    let aborted = || is_stopped_cb.is_some_and(|cb| unsafe { cb(ctx) != 0 });
    let mut emit_msg = |kind: crate::streamrepair::TargetMessageKind, text: &[u8]| {
        if let Some(cb) = emit_message_cb {
            let mut buf: Vec<u8> = text.iter().copied().filter(|&b| b != 0).collect();
            buf.push(0);
            unsafe { cb(ctx, kind as i32, buf.as_ptr() as *const c_char) };
        }
    };
    let rar_vol_cb = |member_name: &[u8],
                      vol: &crate::rarreader::Volume,
                      files: &[crate::rarreader::RarFile]| {
        if let Some(cb) = emit_debug_cb {
            for mut line in
                crate::streamrepair::format_rar_volume_debug_lines(member_name, vol, files)
            {
                line.push(0);
                unsafe { cb(ctx, line.as_ptr() as *const c_char) };
            }
        }
    };
    let rar_vol_ref: Option<
        &dyn Fn(&[u8], &crate::rarreader::Volume, &[crate::rarreader::RarFile]),
    > = emit_debug_cb.map(|_| {
        &rar_vol_cb as &dyn Fn(&[u8], &crate::rarreader::Volume, &[crate::rarreader::RarFile])
    });
    let Some(session) = crate::streamrepair::CrossPackRepairSession::new(
        input(dest_dir),
        &member_names_vec,
        &targets_vec,
        pw,
        true,
        Some(&aborted),
        &mut emit_msg,
        rar_vol_ref,
    ) else {
        return std::ptr::null_mut();
    };
    Box::into_raw(Box::new(NzbgetRsCrossPackSession(session)))
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_cross_pack_session_free(session: *mut NzbgetRsCrossPackSession) {
    if !session.is_null() {
        drop(Box::from_raw(session));
    }
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_cross_pack_session_has_inner_holes(
    session: *const NzbgetRsCrossPackSession,
) -> c_int {
    let Some(s) = session.as_ref() else {
        return 0;
    };
    c_int::from(s.0.has_inner_holes())
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn nzbget_rs_cross_pack_session_process_donor(
    session: *mut NzbgetRsCrossPackSession,
    donor_queued_filename: *const c_char,
    donor_info_name: *const c_char,
    donor_password: *const c_char,
    donor_file_names: *const *const c_char,
    donor_file_sizes: *const i64,
    donor_article_counts: *const usize,
    donor_article_sizes_flat: *const i32,
    donor_file_count: usize,
    allow_decompress: c_int,
    ctx: *mut std::ffi::c_void,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    fetch_part_cb: DonorFetchMemberPartFn,
    begin_prefetch_cb: DonorBeginMemberBatchFn,
    next_batch_cb: DonorNextArticleFn,
    cancel_batch_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    begin_materialize_member_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32)>,
    set_progress_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, i32)>,
    set_materializing_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32)>,
    add_recovered_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64, i32)>,
    sync_target_holes_cb: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const usize,
            *const crate::dupefallback::NzbgetRsByteRange,
            usize,
        ),
    >,
    emit_message_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
    emit_extractor_log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
    emit_debug_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) {
    let Some(s) = session.as_mut() else {
        return;
    };
    let mut file_names_vec = Vec::with_capacity(donor_file_count);
    if !donor_file_names.is_null() && donor_file_count > 0 {
        for &ptr in std::slice::from_raw_parts(donor_file_names, donor_file_count) {
            file_names_vec.push(input(ptr));
        }
    }
    let file_sizes_slice = if donor_file_sizes.is_null() || donor_file_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(donor_file_sizes, donor_file_count)
    };
    let mut article_sizes_vec = Vec::with_capacity(donor_file_count);
    if !donor_article_counts.is_null() && donor_file_count > 0 {
        let counts = std::slice::from_raw_parts(donor_article_counts, donor_file_count);
        let total_arts: usize = counts.iter().copied().sum();
        let flat = if donor_article_sizes_flat.is_null() || total_arts == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(donor_article_sizes_flat, total_arts)
        };
        let mut cursor = 0usize;
        for &cnt in counts {
            let end = cursor.saturating_add(cnt).min(flat.len());
            article_sizes_vec.push(flat[cursor..end].to_vec());
            cursor = end;
        }
    }
    let mut env = FfiCrossPackDonorEnv {
        ctx,
        is_stopped_cb,
        fetch_part_cb,
        begin_prefetch_cb,
        next_batch_cb,
        cancel_batch_cb,
        begin_materialize_member_cb,
        set_progress_cb,
        set_materializing_cb,
        add_recovered_cb,
        sync_target_holes_cb,
        emit_message_cb,
        emit_extractor_log_cb,
        emit_debug_cb,
    };
    s.0.process_donor(
        input(donor_queued_filename),
        input(donor_info_name),
        input(donor_password),
        &file_names_vec,
        file_sizes_slice,
        &article_sizes_vec,
        allow_decompress != 0,
        &mut env,
    );
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_system_info_fetch_network(
    host: *const c_char,
    use_tls: c_int,
    include_gzip: c_int,
    version_rev: *const c_char,
    ctx: *mut c_void,
    tls_cb: Option<
        unsafe extern "C" fn(
            ctx: *mut c_void,
            fd: c_int,
            host: *const c_char,
            req: *const c_char,
            req_len: usize,
            out_resp: *mut RsBuf,
        ) -> c_int,
    >,
    out_public_ip: *mut RsBuf,
    out_private_ip: *mut RsBuf,
    out_err: *mut RsBuf,
) -> c_int {
    let host_str = String::from_utf8_lossy(input(host));
    let ver_bytes = input(version_rev);
    match crate::systemhealth::system_info_fetch_network(
        &host_str,
        use_tls != 0,
        include_gzip != 0,
        ver_bytes,
        |fd, host_cstr, req| {
            let Some(cb) = tls_cb else {
                return Err("TLS callback missing".to_string());
            };
            let mut resp_buf = RsBuf {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            let ok = cb(
                ctx,
                fd,
                host_cstr.as_ptr(),
                req.as_ptr().cast(),
                req.len(),
                &mut resp_buf,
            );
            let vec = if resp_buf.data.is_null() {
                Vec::new()
            } else {
                Vec::from_raw_parts(resp_buf.data.cast::<u8>(), resp_buf.len, resp_buf.cap)
            };
            if ok != 0 {
                Ok(vec)
            } else {
                Err(String::from_utf8_lossy(&vec).into_owned())
            }
        },
    ) {
        Ok((pub_ip, priv_ip)) => {
            if !out_public_ip.is_null() {
                *out_public_ip = into_buf(pub_ip);
            }
            if !out_private_ip.is_null() {
                *out_private_ip = into_buf(priv_ip);
            }
            1
        }
        Err(err) => {
            if !out_err.is_null() {
                *out_err = into_buf(err.into_bytes());
            }
            0
        }
    }
}


#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_format_name(
    op: c_int,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    s5: *const c_char,
    flag: c_int,
    nzb_id: c_int,
) -> RsBuf {
    let out = match op {
        0 => crate::downloadinfo::make_nice_nzb_name(input(s1), flag != 0),
        1 => crate::downloadinfo::make_nice_url_name(
            if s1.is_null() {
                None
            } else {
                Some(CStr::from_ptr(s1))
            },
            input(s2),
        ),
        2 => crate::downloadinfo::build_final_dir_name(
            input(s1),
            input(s2),
            input(s3),
            flag != 0,
            input(s4),
            nzb_id,
        ),
        3 => crate::downloadinfo::build_dest_dir_name(
            input(s1),
            input(s2),
            input(s3),
            input(s4),
            flag != 0,
            input(s5),
            nzb_id,
        ),
        _ => Vec::new(),
    };
    into_buf(out)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_nzb_calc_health(
    is_critical: c_int,
    size: i64,
    par_size: i64,
    current_failed_size: i64,
    par_current_failed_size: i64,
    allow_estimation: c_int,
) -> c_int {
    if is_critical == 0 {
        crate::downloadinfo::calc_health(
            size,
            par_size,
            current_failed_size,
            par_current_failed_size,
        )
    } else {
        crate::downloadinfo::calc_critical_health(
            size,
            par_size,
            par_current_failed_size,
            allow_estimation != 0,
        )
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_nzb_is_dupe_success(
    mark_status: c_int,
    delete_status: c_int,
    par_status: c_int,
    unpack_status: c_int,
    url_status: c_int,
    health: c_int,
    critical_health_est: c_int,
    critical_health_exact: c_int,
    health_check_dupe: c_int,
) -> c_int {
    c_int::from(crate::downloadinfo::is_dupe_success(
        mark_status,
        delete_status,
        par_status,
        unpack_status,
        url_status,
        health,
        critical_health_est,
        critical_health_exact,
        health_check_dupe != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_nzb_make_text_status(
    kind: c_int,
    mark_status: c_int,
    delete_status: c_int,
    par_status: c_int,
    unpack_status: c_int,
    move_status: c_int,
    url_status: c_int,
    script_status: c_int,
    health: c_int,
    critical_health: c_int,
) -> *const c_char {
    crate::downloadinfo::make_text_status_cstr(
        kind,
        mark_status,
        delete_status,
        par_status,
        unpack_status,
        move_status,
        url_status,
        script_status,
        health,
        critical_health,
    )
    .as_ptr()
    .cast::<c_char>()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_completed_same_filename(
    a: *const c_char,
    b: *const c_char,
) -> c_int {
    c_int::from(crate::downloadinfo::completed_same_filename(
        input(a),
        input(b),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_calc_active_downloads_sec(
    old_active: c_int,
    new_active: c_int,
    is_nzb_kind: c_int,
    cur_time: i64,
    inout_download_sec: *mut c_int,
    inout_download_start_time: *mut i64,
    inout_download_start_sec: *mut c_int,
    out_changed: *mut c_int,
) {
    let (sec, start_time, start_sec, changed) = crate::downloadinfo::calc_active_downloads_sec(
        old_active,
        new_active,
        is_nzb_kind != 0,
        cur_time,
        if inout_download_sec.is_null() {
            0
        } else {
            *inout_download_sec
        },
        if inout_download_start_time.is_null() {
            0
        } else {
            *inout_download_start_time
        },
        if inout_download_start_sec.is_null() {
            0
        } else {
            *inout_download_start_sec
        },
    );
    if !inout_download_sec.is_null() {
        *inout_download_sec = sec;
    }
    if !inout_download_start_time.is_null() {
        *inout_download_start_time = start_time;
    }
    if !inout_download_start_sec.is_null() {
        *inout_download_start_sec = start_sec;
    }
    if !out_changed.is_null() {
        *out_changed = c_int::from(changed);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_update_completed_stats(
    file_success_size: i64,
    file_failed_size: i64,
    file_failed_articles: c_int,
    file_success_articles: c_int,
    file_extra_priority: c_int,
    file_par_file: c_int,
    file_paused: c_int,
    inout_success_size: *mut i64,
    inout_failed_size: *mut i64,
    inout_failed_articles: *mut c_int,
    inout_success_articles: *mut c_int,
    inout_extra_priority: *mut c_int,
    inout_par_success_size: *mut i64,
    inout_par_failed_size: *mut i64,
    inout_remaining_par_count: *mut c_int,
    inout_paused_file_count: *mut c_int,
) {
    crate::downloadinfo::update_completed_stats(
        file_success_size,
        file_failed_size,
        file_failed_articles,
        file_success_articles,
        file_extra_priority != 0,
        file_par_file != 0,
        file_paused != 0,
        &mut *inout_success_size,
        &mut *inout_failed_size,
        &mut *inout_failed_articles,
        &mut *inout_success_articles,
        &mut *inout_extra_priority,
        &mut *inout_par_success_size,
        &mut *inout_par_failed_size,
        &mut *inout_remaining_par_count,
        &mut *inout_paused_file_count,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_update_deleted_stats(
    file_size: i64,
    file_success_size: i64,
    file_failed_size: i64,
    file_missed_size: i64,
    file_total_articles: c_int,
    file_success_articles: c_int,
    file_failed_articles: c_int,
    file_missed_articles: c_int,
    file_remaining_size: i64,
    file_extra_priority: c_int,
    file_par_file: c_int,
    file_paused: c_int,
    inout_file_count: *mut c_int,
    inout_size: *mut i64,
    inout_cur_success_size: *mut i64,
    inout_failed_size: *mut i64,
    inout_failed_articles: *mut c_int,
    inout_cur_failed_size: *mut i64,
    inout_total_articles: *mut c_int,
    inout_cur_success_articles: *mut c_int,
    inout_cur_failed_articles: *mut c_int,
    inout_remaining_size: *mut i64,
    inout_extra_priority: *mut c_int,
    inout_remaining_par_count: *mut c_int,
    inout_par_size: *mut i64,
    inout_par_cur_success_size: *mut i64,
    inout_par_failed_size: *mut i64,
    inout_par_cur_failed_size: *mut i64,
    inout_paused_file_count: *mut c_int,
    inout_paused_size: *mut i64,
) {
    crate::downloadinfo::update_deleted_stats(
        file_size,
        file_success_size,
        file_failed_size,
        file_missed_size,
        file_total_articles,
        file_success_articles,
        file_failed_articles,
        file_missed_articles,
        file_remaining_size,
        file_extra_priority != 0,
        file_par_file != 0,
        file_paused != 0,
        &mut *inout_file_count,
        &mut *inout_size,
        &mut *inout_cur_success_size,
        &mut *inout_failed_size,
        &mut *inout_failed_articles,
        &mut *inout_cur_failed_size,
        &mut *inout_total_articles,
        &mut *inout_cur_success_articles,
        &mut *inout_cur_failed_articles,
        &mut *inout_remaining_size,
        &mut *inout_extra_priority,
        &mut *inout_remaining_par_count,
        &mut *inout_par_size,
        &mut *inout_par_cur_success_size,
        &mut *inout_par_failed_size,
        &mut *inout_par_cur_failed_size,
        &mut *inout_paused_file_count,
        &mut *inout_paused_size,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_util_find_executor_program(
    filename: *const c_char,
    custom_path: *const c_char,
    out_found: *mut c_int,
) -> RsBuf {
    match crate::util::util_find_executor_program(input(filename), input(custom_path)) {
        Some(v) => {
            if !out_found.is_null() {
                *out_found = 1;
            }
            into_buf(v)
        }
        None => {
            if !out_found.is_null() {
                *out_found = 0;
            }
            into_buf(Vec::new())
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_util_format_time(
    time_sec: i64,
    buffer: *mut c_char,
    bufsize: c_int,
) {
    if !buffer.is_null() && bufsize > 0 {
        let slice = std::slice::from_raw_parts_mut(buffer.cast::<u8>(), bufsize as usize);
        crate::util::util_format_time(time_sec, slice);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fs_string_op(
    op: c_int,
    path: *const c_char,
    out_ok: *mut c_int,
) -> RsBuf {
    let bytes = match op {
        0 => match crate::fsops::get_real_path(input(path)) {
            Some(b) => {
                if !out_ok.is_null() {
                    *out_ok = 1;
                }
                b
            }
            None => {
                if !out_ok.is_null() {
                    *out_ok = 0;
                }
                return into_buf(Vec::new());
            }
        },
        1 => crate::fsops::get_current_directory(),
        2 => {
            let opt = if path.is_null() {
                None
            } else {
                Some(CStr::from_ptr(path))
            };
            crate::fsops::expand_home_path_auto(opt)
        }
        3 => crate::fsops::expand_file_name(input(path)),
        4 => crate::fsops::get_exe_file_name(input(path)),
        5 => crate::fsops::make_unique_path(input(path)),
        _ => Vec::new(),
    };
    if !out_ok.is_null() && op != 0 {
        *out_ok = 1;
    }
    into_buf(bytes)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fs_op_with_err(
    op: c_int,
    p1: *const c_char,
    p2: *const c_char,
    val: i64,
    out_errmsg: *mut RsBuf,
) -> i64 {
    let mut errmsg = Vec::new();
    let res: i64 = match op {
        0 => {
            if p1.is_null() || val < 0 {
                0
            } else {
                i64::from(crate::fsops::allocate_file(
                    CStr::from_ptr(p1),
                    val,
                    &mut errmsg,
                ))
            }
        }
        1 => {
            if p1.is_null() || p2.is_null() {
                0
            } else {
                i64::from(crate::fsops::create_hard_link(
                    CStr::from_ptr(p1),
                    CStr::from_ptr(p2),
                    &crate::fsops::posix_stat,
                    &mut errmsg,
                ))
            }
        }
        2 => i64::from(crate::fsops::flush_file_buffers(val as i32, &mut errmsg)),
        3 => {
            let c1 = if p1.is_null() {
                c""
            } else {
                CStr::from_ptr(p1)
            };
            i64::from(crate::fsops::flush_dir_buffers(c1, val as u8, &mut errmsg))
        }
        4 => {
            if p1.is_null() || p2.is_null() {
                0
            } else {
                i64::from(crate::fsops::move_file(
                    CStr::from_ptr(p1),
                    CStr::from_ptr(p2),
                ))
            }
        }
        5 => {
            if p1.is_null() || p2.is_null() {
                libc::EINVAL as i64
            } else {
                match crate::fsops::move_file_no_overwrite(CStr::from_ptr(p1), CStr::from_ptr(p2)) {
                    Ok(()) => 0,
                    Err(e) => e as i64,
                }
            }
        }
        6 => {
            if p1.is_null() || *p1 == 0 {
                0
            } else {
                let slice = if p2.is_null() || val <= 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(p2.cast::<u8>(), val as usize)
                };
                i64::from(crate::fsops::save_buffer_into_file(
                    CStr::from_ptr(p1),
                    slice,
                ))
            }
        }
        7 => {
            crate::fsops::fix_exec_permission(input(p1));
            1
        }
        8 => {
            if p1.is_null() {
                -1
            } else {
                crate::fsops::open_cloexec_fd(CStr::from_ptr(p1), val as i32, 0, 1, 2) as i64
            }
        }
        9 => {
            if p1.is_null() {
                -1
            } else {
                crate::fsops::file_size(CStr::from_ptr(p1))
            }
        }
        _ => 0,
    };
    if !out_errmsg.is_null() {
        *out_errmsg = into_buf(errmsg);
    }
    res
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fs_load_file_into_buffer(
    filename: *const c_char,
    add_trailing_null: c_int,
    ctx: *mut std::ffi::c_void,
    reserve_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int) -> *mut u8>,
) -> c_int {
    if filename.is_null() {
        return 0;
    }
    let Some(cb) = reserve_cb else {
        return 0;
    };
    c_int::from(crate::fsops::load_file_into_buffer(
        CStr::from_ptr(filename),
        add_trailing_null != 0,
        &crate::fsops::posix_stat,
        |sz| cb(ctx, sz),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fs_get_disk_state(
    path: *const c_char,
    out_avail: *mut i64,
    out_total: *mut i64,
) -> c_int {
    if path.is_null() {
        return 0;
    }
    if let Some((avail, total)) = crate::fsops::get_disk_state(CStr::from_ptr(path)) {
        if !out_avail.is_null() {
            *out_avail = avail;
        }
        if !out_total.is_null() {
            *out_total = total;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_tokenizer_next(
    data_str: *mut c_char,
    separators: *const c_char,
    inout_save_ptr: *mut *mut c_char,
    inout_working: *mut c_int,
) -> *mut c_char {
    let cur = if *inout_working != 0 {
        *inout_save_ptr
    } else {
        data_str
    };
    *inout_working = 1;
    if cur.is_null() {
        *inout_save_ptr = std::ptr::null_mut();
        return std::ptr::null_mut();
    }
    let slice = CStr::from_ptr(cur).to_bytes();
    let seps = input(separators);
    if let Some((tok_start, tok_end, next_pos)) =
        crate::util::tokenizer_find_next_token(slice, seps)
    {
        *cur.add(tok_end) = 0;
        *inout_save_ptr = cur.add(next_pos);
        cur.add(tok_start)
    } else {
        *inout_save_ptr = cur.add(slice.len());
        std::ptr::null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_update_current_stats_step(
    acc: *mut crate::downloadinfo::RsDownloadCurrentStatsAcc,
    file_remaining_size: i64,
    file_success_articles: c_int,
    file_failed_articles: c_int,
    file_success_size: i64,
    file_failed_size: i64,
    file_extra_priority: c_int,
    file_paused: c_int,
    file_par_file: c_int,
) {
    if acc.is_null() {
        return;
    }
    crate::downloadinfo::update_current_stats_step(
        &mut *acc,
        file_remaining_size,
        file_success_articles,
        file_failed_articles,
        file_success_size,
        file_failed_size,
        file_extra_priority != 0,
        file_paused != 0,
        file_par_file != 0,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_calc_min_max_time(
    times: *const i64,
    count: usize,
    out_min: *mut i64,
    out_max: *mut i64,
) {
    let slice = if times.is_null() || count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(times, count)
    };
    let (min_t, max_t) = crate::downloadinfo::calc_min_max_time(slice);
    if !out_min.is_null() {
        *out_min = min_t;
    }
    if !out_max.is_null() {
        *out_max = max_t;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_server_stat_apply_op(
    stat_op: c_int,
    delta_success: c_int,
    delta_failed: c_int,
    inout_success: *mut c_int,
    inout_failed: *mut c_int,
) {
    let (s, f) = crate::downloadinfo::server_stat_apply_op(
        *inout_success,
        *inout_failed,
        delta_success,
        delta_failed,
        stat_op,
        0,
        1,
        2,
    );
    *inout_success = s;
    *inout_failed = f;
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_script_calc_total_status(
    statuses: *const c_int,
    count: usize,
) -> c_int {
    let slice = if statuses.is_null() || count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(statuses, count)
    };
    crate::downloadinfo::script_calc_total_status(slice)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_script_parse_nzb_cmd(
    scope: c_int,
    msg_text: *const c_char,
    event: c_int,
    qe_nzb_downloaded: c_int,
    out_key_off: *mut usize,
    out_key_len: *mut usize,
    out_val_off: *mut usize,
    out_int_val: *mut c_int,
) -> c_int {
    let bytes = if msg_text.is_null() {
        &[][..]
    } else {
        input(msg_text)
    };
    let (kind, k_off, k_len, v_off, i_val) = match scope {
        0 => {
            let r = crate::extension::parse_queue_script_cmd(bytes, event, qe_nzb_downloaded);
            (r.kind, r.key_offset, r.key_len, r.val_offset, 0)
        }
        1 => {
            let r = crate::extension::parse_post_script_cmd(bytes);
            (r.kind, r.key_offset, r.key_len, r.val_offset, 0)
        }
        2 => {
            let r = crate::extension::parse_scan_script_cmd(bytes);
            (r.kind, r.key_offset, r.key_len, r.val_offset, r.int_val)
        }
        _ => (0, 0, 0, 0, 0),
    };
    if !out_key_off.is_null() {
        *out_key_off = k_off;
    }
    if !out_key_len.is_null() {
        *out_key_len = k_len;
    }
    if !out_val_off.is_null() {
        *out_val_off = v_off;
    }
    if !out_int_val.is_null() {
        *out_int_val = i_val;
    }
    kind
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_post_script_analyse_exit_code(
    exit_code: c_int,
    par_status: c_int,
    ps_skipped: c_int,
    parcheck_enabled: c_int,
    sr_none: c_int,
    sr_failure: c_int,
    sr_success: c_int,
    out_msg_action: *mut c_int,
    out_req_par_check: *mut c_int,
) -> c_int {
    let (status, action, req) = crate::extension::analyse_post_script_exit_code(
        exit_code,
        par_status,
        ps_skipped,
        parcheck_enabled != 0,
        sr_none,
        sr_failure,
        sr_success,
    );
    if !out_msg_action.is_null() {
        *out_msg_action = action;
    }
    if !out_req_par_check.is_null() {
        *out_req_par_check = c_int::from(req);
    }
    status
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_exec(
    op: c_int,
    env: *const crate::diskstate_format::NzbgetRsDiskStateEnv,
) -> c_int {
    if env.is_null() {
        return 0;
    }
    crate::diskstate_format::diskstate_exec(op, &*env)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_server_pool_op(
    op: c_int,
    a: i64,
    b: i64,
    c: c_int,
    ptr1: *const c_int,
    ptr2: *const u8,
    len: usize,
    out_ptr: *mut c_int,
) -> c_int {
    match op {
        0 => c_int::from(crate::dupefallback::server_pool_is_blocked(a, b, c)),
        1 => {
            if ptr1.is_null() || ptr2.is_null() || out_ptr.is_null() || len == 0 {
                return 0;
            }
            let free_times = std::slice::from_raw_parts(ptr1 as *const i64, len);
            let in_use = std::slice::from_raw_parts(ptr2, len);
            let connected = std::slice::from_raw_parts(out_ptr as *const u8, len);
            c_int::from(crate::dupefallback::server_pool_level_should_close(
                in_use, connected, free_times, a, b as c_int,
            ))
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_server_pool_normalize_levels(
    levels: *const c_int,
    active: *const u8,
    max_conns: *const c_int,
    len: usize,
    out_sorted_indices: *mut usize,
    out_norm_levels: *mut c_int,
) -> c_int {
    if levels.is_null()
        || active.is_null()
        || max_conns.is_null()
        || out_sorted_indices.is_null()
        || out_norm_levels.is_null()
        || len == 0
    {
        return 0;
    }
    let lvls = std::slice::from_raw_parts(levels, len);
    let act = std::slice::from_raw_parts(active, len);
    let mc = std::slice::from_raw_parts(max_conns, len);
    let mut inputs = Vec::with_capacity(len);
    for i in 0..len {
        inputs.push(crate::dupefallback::NzbgetRsServerNormInput {
            level: lvls[i],
            active: i32::from(act[i]),
            max_connections: mc[i],
        });
    }
    let sorted_idx = std::slice::from_raw_parts_mut(out_sorted_indices, len);
    let norm_lvls = std::slice::from_raw_parts_mut(out_norm_levels, len);
    crate::dupefallback::server_pool_normalize_levels(&inputs, sorted_idx, norm_lvls)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_statmeter_op(
    op: c_int,
    loc_or_cur_time: i64,
    a_i64: i64,
    b_i64: i64,
    c_i64: i64,
    a_i32: c_int,
    b_i32: c_int,
    c_i32: c_int,
    slice_ptr: *const i64,
    slice_len: usize,
    out_i64_1: *mut i64,
    out_i64_2: *mut i64,
    out_i32: *mut c_int,
) -> c_int {
    match op {
        0 => {
            let bytes_per_days = if slice_ptr.is_null() || slice_len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(slice_ptr, slice_len)
            };
            let elapsed_slots = crate::statmeter::calc_month_slots_utc(loc_or_cur_time, b_i32);
            let (month_bytes, day_bytes) = crate::statmeter::calc_quota_usage(
                loc_or_cur_time,
                a_i32,
                bytes_per_days,
                elapsed_slots,
            );
            if !out_i64_1.is_null() {
                *out_i64_1 = month_bytes;
            }
            if !out_i64_2.is_null() {
                *out_i64_2 = day_bytes;
            }
            elapsed_slots
        }
        1 => {
            let (adj_clocks, delta, adj_dl, adj_off) = crate::statmeter::eval_interval_check(
                loc_or_cur_time,
                a_i64,
                b_i64,
                c_i64,
                a_i32 != 0,
            );
            if !out_i64_1.is_null() {
                *out_i64_1 = delta;
            }
            let mask = (c_int::from(adj_clocks))
                | (c_int::from(adj_dl) << 1)
                | (c_int::from(adj_off) << 2);
            if !out_i32.is_null() {
                *out_i32 = mask;
            }
            mask
        }
        2 => {
            let (reached, action) =
                crate::statmeter::eval_quota_status(a_i32, b_i32, a_i64, b_i64, c_i32 != 0);
            if !out_i32.is_null() {
                *out_i32 = action;
            }
            c_int::from(reached)
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_memory_and_cache_op(
    op: c_int,
    a: usize,
    b: usize,
    tier: c_int,
    flag1: c_int,
    flag2: c_int,
    flag3: c_int,
    inout1: *mut c_int,
    inout2: *mut c_int,
) -> c_int {
    match op {
        0 => {
            if inout1.is_null() || inout2.is_null() {
                return flag1;
            }
            let (next_tier, transitioned) =
                crate::artwritercore::memory_monitor_step(tier, flag1, &mut *inout1, &mut *inout2);
            next_tier | (c_int::from(transitioned) << 8)
        }
        1 => {
            let direct_write = !inout1.is_null() && *inout1 != 0;
            let (should_flush, flush_all, emergency) =
                crate::artwritercore::article_cache_should_flush(
                    a,
                    b,
                    tier,
                    flag1 != 0,
                    flag2,
                    flag3 != 0,
                    direct_write,
                );
            c_int::from(should_flush)
                | (c_int::from(flush_all) << 1)
                | (c_int::from(emergency) << 2)
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_dupe_probe_new(
    nzb_id: c_int,
    count_all: c_int,
    recovered_at_start: c_int,
) -> *mut crate::dupeprobe::NzbgetRsDupeProbe {
    Box::into_raw(Box::new(crate::dupeprobe::NzbgetRsDupeProbe::new(
        nzb_id,
        count_all != 0,
        recovered_at_start,
    )))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_dupe_probe_free(
    p: *mut crate::dupeprobe::NzbgetRsDupeProbe,
) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_dupe_probe_exec(
    p: *mut crate::dupeprobe::NzbgetRsDupeProbe,
    op: c_int,
    n: usize,
    a1: c_int,
    a2: c_int,
    p1: usize,
    s1: *const c_char,
    groups: *const *const c_char,
    out_idx: *mut usize,
    ctx: *mut c_void,
    host_cb: Option<crate::dupeprobe::DupeProbeHostFn>,
) -> c_int {
    match op {
        0 => {
            let idxs = crate::dupeprobe::sample_indexes(n, a1);
            if !out_idx.is_null() && !idxs.is_empty() {
                std::slice::from_raw_parts_mut(out_idx, idxs.len()).copy_from_slice(&idxs);
            }
            idxs.len() as c_int
        }
        1 => {
            let Some(probe) = p.as_mut() else {
                return 0;
            };
            let msg_id = if s1.is_null() {
                Vec::new()
            } else {
                input(s1).to_vec()
            };
            let mut grps = Vec::with_capacity(n);
            if !groups.is_null() && n > 0 {
                let slice = std::slice::from_raw_parts(groups, n);
                for &gp in slice {
                    if !gp.is_null() {
                        grps.push(input(gp).to_vec());
                    }
                }
            }
            probe.add_sample(msg_id, grps);
            1
        }
        2 => {
            let (Some(probe), Some(cb)) = (p.as_mut(), host_cb) else {
                return 0;
            };
            c_int::from(probe.register(p1, cb))
        }
        3 => {
            let (Some(probe), Some(cb)) = (p.as_mut(), host_cb) else {
                return 0;
            };
            probe.run(ctx, cb);
            1
        }
        4 => {
            let (Some(probe), Some(cb)) = (p.as_mut(), host_cb) else {
                return 0;
            };
            c_int::from(probe.check(a1, ctx, cb))
        }
        5 => {
            let Some(cb) = host_cb else {
                return 0;
            };
            let (existing, reached) = if !out_idx.is_null() {
                let ints = out_idx as *const c_int;
                (*ints, *ints.add(1))
            } else {
                (0, 0)
            };
            crate::dupeprobe::probe_recheck_locked(p1, a1, a2, existing, reached, ctx, cb);
            1
        }
        6 => {
            crate::dupeprobe::probe_reset();
            1
        }
        7 => crate::dupeprobe::probe_probing_for(a1),
        8 => {
            crate::dupeprobe::probe_stop_all();
            1
        }
        9 => {
            crate::dupeprobe::probe_wait_all();
            1
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_queue_editor_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    f1: i64,
    f2: i64,
    s1: *const c_char,
    s2: *const c_char,
    in1: *const c_int,
    len1: usize,
    in2: *const c_int,
    len2: usize,
    inout1: *mut c_int,
    inout2: *mut c_int,
) -> c_int {
    if op == 9 {
        if !s1.is_null() && !s2.is_null() && !in1.is_null() && !inout1.is_null() && len1 > 0 {
            let is_par = std::slice::from_raw_parts(s1 as *const u8, len1);
            let has_vol = std::slice::from_raw_parts(s2 as *const u8, len1);
            let sizes = std::slice::from_raw_parts(in1 as *const i64, len1);
            let out_pause = std::slice::from_raw_parts_mut(inout1 as *mut u8, len1);
            crate::queueeditor::select_pars_to_pause(is_par, has_vol, sizes, a != 0, out_pause);
        }
        return 0;
    }
    let b1 = if s1.is_null() { &[][..] } else { input(s1) };
    let b2 = if s2.is_null() { &[][..] } else { input(s2) };
    let sl1 = if in1.is_null() || len1 == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(in1, len1)
    };
    let sl2 = if in2.is_null() || len2 == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(in2, len2)
    };
    match op {
        0 => match crate::queueeditor::parse_sort_spec(b1) {
            Some((crit, order)) => {
                if !inout1.is_null() {
                    *inout1 = crit;
                }
                if !inout2.is_null() {
                    *inout2 = order;
                }
                1
            }
            None => 0,
        },
        1 => {
            let cmp = match b1.cmp(b2) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
            let p2 = if inout1.is_null() { 0 } else { *inout1 };
            c_int::from(crate::queueeditor::group_sorter_compare_values(
                a, b, 1, cmp, f1, f2, f1, f2, f1, f2, cmp, c, p2,
            ))
        }
        2 => {
            if inout1.is_null() {
                return 0;
            }
            let (work_offset, new_last) = crate::queueeditor::clamp_move_offset(a, b, c, *inout1);
            *inout1 = new_last;
            work_offset
        }
        3 => c_int::from(crate::queueeditor::filename_has_vol(b1)),
        4 => {
            let (new_target, new_before, offset) =
                crate::queueeditor::resolve_move_groups_target(a, sl1, sl2, b != 0, c);
            if !inout1.is_null() {
                *inout1 = new_target;
            }
            if !inout2.is_null() {
                *inout2 = c_int::from(new_before);
            }
            offset
        }
        5 => crate::queueeditor::calc_move_groups_to_offset(sl1, a, b, c != 0, len2),
        6 => c_int::from(crate::queueeditor::compare_inner_files(
            b1,
            a != 0,
            f1,
            b2,
            b != 0,
            f2,
            c != 0,
        )),
        7 => crate::queueeditor::parse_dupe_mode(b1).unwrap_or(-1),
        8 => {
            let is_group_del = (18..=21).contains(&a);
            if is_group_del && !inout1.is_null() {
                let (parking, avoid_hist, set_dupe, cleanup_disk) =
                    crate::queueeditor::eval_group_delete_flags(
                        a,
                        19,
                        20,
                        21,
                        b,
                        c != 0,
                        f1 as i32,
                    );
                *inout1 = 1
                    | (c_int::from(parking) << 1)
                    | (c_int::from(avoid_hist) << 2)
                    | (c_int::from(set_dupe) << 3)
                    | (c_int::from(cleanup_disk) << 4);
            } else if !inout1.is_null() {
                *inout1 = 0;
            }
            crate::queueeditor::map_group_to_file_action(a)
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_transient_retries_op(
    handle: *mut c_void,
    op: c_int,
    a: usize,
    b: i64,
    out_arr: *mut usize,
    out_cap: usize,
) -> isize {
    match op {
        0 => Box::into_raw(Box::new(crate::nntp::TransientRetries::new(
            a as i32, b as i32,
        ))) as isize,
        1 => {
            if !handle.is_null() {
                drop(Box::from_raw(handle as *mut crate::nntp::TransientRetries));
            }
            0
        }
        2 => {
            if handle.is_null() {
                return 0;
            }
            isize::from((&mut *(handle as *mut crate::nntp::TransientRetries)).register(a, b))
        }
        3 => {
            if handle.is_null() {
                return 0;
            }
            (&*(handle as *const crate::nntp::TransientRetries)).remained_retries(a) as isize
        }
        4 => {
            if !handle.is_null() {
                (&mut *(handle as *mut crate::nntp::TransientRetries))
                    .set_remained_retries(a, b as i32);
            }
            0
        }
        5 => {
            if handle.is_null() {
                return 1;
            }
            isize::from((&*(handle as *const crate::nntp::TransientRetries)).is_empty())
        }
        6 => {
            if handle.is_null() {
                return 0;
            }
            isize::from((&*(handle as *const crate::nntp::TransientRetries)).is_exhausted())
        }
        7 => {
            if handle.is_null() {
                return 0;
            }
            let tr = &*(handle as *const crate::nntp::TransientRetries);
            let waiting = tr.waiting_servers(b);
            let want_waiting = waiting.contains(&a);
            let mut written = 0usize;
            if !out_arr.is_null() && out_cap > 0 {
                written = waiting.len().min(out_cap);
                std::ptr::copy_nonoverlapping(waiting.as_ptr(), out_arr, written);
            }
            ((written as isize) << 1) | isize::from(want_waiting)
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_article_dl_op(
    op: c_int,
    a: c_int,
    response: *const c_char,
    begin_pos: i64,
    end_pos: i64,
    article_file_size: i64,
    expected_file_size: i64,
    dupe_expected_offset: i64,
    dupe_expected_end: i64,
    out_offset: *mut i64,
    out_size: *mut c_int,
) -> c_int {
    match op {
        0 => {
            let resp = if response.is_null() {
                None
            } else {
                Some(input(response))
            };
            crate::dupefallback::article_downloader_check_response(resp, a != 0)
        }
        1 => match crate::dupefallback::article_downloader_validate_yenc(
            begin_pos,
            end_pos,
            article_file_size,
            expected_file_size,
            a,
            dupe_expected_offset,
            dupe_expected_end,
        ) {
            Ok((offset, size)) => {
                if !out_offset.is_null() {
                    *out_offset = offset;
                }
                if !out_size.is_null() {
                    *out_size = size;
                }
                0
            }
            Err(code) => {
                if code == 4 || code == 5 {
                    if !out_offset.is_null() {
                        *out_offset = begin_pos - 1;
                    }
                    if !out_size.is_null() {
                        *out_size = (end_pos - begin_pos + 1) as c_int;
                    }
                }
                code
            }
        },
        _ => 0,
    }
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_twin_exec(
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    nntp_host: Option<crate::articlefetcher::NzbgetRsNntpFetchHostFn>,
    host_cb: Option<crate::twin::NzbgetRsTwinHostFn>,
    out_buf: *mut RsBuf,
) -> c_int {
    let _cancel_guard = PosixCancelGuard::disable();
    let (rc, bytes) =
        crate::twin::twin_exec(op, a1, a2, p1, p2, s1, s2, s3, s4, nntp_host, host_cb);
    if let Some(vec) = bytes {
        if !out_buf.is_null() {
            *out_buf = into_buf(vec);
        }
    }
    rc
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_socket_deadline_wait(
    fd: c_int,
    read: c_int,
    write: c_int,
    ctx: *mut c_void,
    rem_cb: Option<unsafe extern "C" fn(*mut c_void) -> i64>,
) -> c_int {
    let Some(cb) = rem_cb else {
        return 0;
    };
    c_int::from(crate::lifecycle::socket_deadline_wait(
        fd,
        read != 0,
        write != 0,
        || cb(ctx),
    ))
}

pub use crate::scanner::{NzbgetRsScanner, NzbgetRsScannerEnv};

#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_scanner_new() -> *mut NzbgetRsScanner {
    Box::into_raw(Box::new(NzbgetRsScanner::new()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_scanner_free(scanner: *mut NzbgetRsScanner) {
    if !scanner.is_null() {
        drop(unsafe { Box::from_raw(scanner) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_scanner_exec(
    scanner: *mut NzbgetRsScanner,
    op: c_int,
    env: *const NzbgetRsScannerEnv,
    a1: c_int,
    a2: c_int,
    a3: c_int,
) -> c_int {
    unsafe { crate::scanner::scanner_exec(scanner, op, env, a1, a2, a3) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_prepost_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    d: c_int,
    e: c_int,
    f: c_int,
    g: c_int,
    h: c_int,
    i: c_int,
    t1: i64,
    t2: i64,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    arr: *const c_int,
    len: usize,
    out1: *mut c_int,
    out2: *mut c_int,
    out3: *mut c_int,
    out4: *mut c_int,
    out5: *mut c_int,
    out6: *mut c_int,
) -> c_int {
    let s1_bytes = if s1.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(s1).to_bytes()
    };
    let s2_bytes = if s2.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(s2).to_bytes()
    };
    let s3_bytes = if s3.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(s3).to_bytes()
    };
    let stages = if arr.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(arr, len)
    };
    match op {
        0 => c_int::from(crate::prepost::should_complete_after_direct_rename(
            a != 0,
            b,
            3,
            c != 0,
            d,
            0,
            e != 0,
            f != 0,
            g != 0,
            h != 0,
            i != 0,
        )),
        1 => {
            let (route, enq_del, set_par_skip) = crate::prepost::classify_nzb_downloaded_entry(
                a != 0,
                b != 0,
                c != 0,
                d,
                2,
                4,
                e != 0,
                f != 0,
                g,
                0,
                h,
                1,
                2,
            );
            if !out1.is_null() {
                *out1 = c_int::from(enq_del);
            }
            if !out2.is_null() {
                *out2 = c_int::from(set_par_skip);
            }
            route
        }
        2 => {
            let (new_stage, req_par) =
                crate::prepost::sanitise_post_stage(a, 11, 12, 0, b != 0, c != 0, d, 1);
            if !out1.is_null() {
                *out1 = c_int::from(req_par);
            }
            new_stage
        }
        3 => crate::prepost::classify_file_event(
            a != 0,
            b != 0,
            c != 0,
            d,
            0,
            2,
            e != 0,
            f != 0,
            g != 0,
        ),
        4 => {
            let (new_status, call_dl) = crate::prepost::resolve_nzb_deleted_route(a, 0, 1, 2, 4);
            if !out1.is_null() {
                *out1 = c_int::from(call_dl);
            }
            new_status
        }
        5 => {
            let (add_hist, call_dupe, enq_del, need_save) = crate::prepost::eval_nzb_completed(
                a,
                2,
                b,
                c != 0,
                d != 0,
                e,
                2,
                f != 0,
                g,
                0,
                2,
                4,
                7,
                6,
            );
            if !out1.is_null() {
                *out1 = c_int::from(add_hist);
            }
            if !out2.is_null() {
                *out2 = c_int::from(call_dupe);
            }
            if !out3.is_null() {
                *out3 = c_int::from(enq_del);
            }
            if !out4.is_null() {
                *out4 = c_int::from(need_save);
            }
            c_int::from(add_hist)
        }
        6 => {
            let (act, new_force_full) = crate::prepost::classify_request_par_action(
                a != 0,
                b,
                1,
                c != 0,
                d != 0,
                e,
                3,
                f != 0,
                g != 0,
            );
            if !out1.is_null() {
                *out1 = c_int::from(new_force_full);
            }
            act
        }
        7 => {
            let (ready, upd_sec, is_comp, new_sec) =
                crate::prepost::eval_cleanup_job_state(a != 0, b != 0, c, 12, d != 0, t1, t2, e);
            if !out1.is_null() {
                *out1 = c_int::from(upd_sec);
            }
            if !out2.is_null() {
                *out2 = c_int::from(is_comp);
            }
            if !out3.is_null() {
                *out3 = new_sec;
            }
            c_int::from(ready)
        }
        8 => {
            let (can_run, allow_par) = crate::prepost::can_run_more_jobs(stages, 1, 4, 3, a);
            if !out1.is_null() {
                *out1 = c_int::from(allow_par);
            }
            c_int::from(can_run)
        }
        9 => crate::prepost::classify_skipped_par_health(a, 1, b, 3, c, d, t1, t2),
        10 => {
            let (want_unpack, unpack, start_rar_rename) =
                crate::prepost::eval_want_unpack_and_rar_rename(
                    a != 0,
                    s1_bytes,
                    b,
                    0,
                    c,
                    0,
                    d,
                    0,
                    e != 0,
                );
            if !out1.is_null() {
                *out1 = c_int::from(want_unpack);
            }
            if !out2.is_null() {
                *out2 = c_int::from(unpack);
            }
            if !out3.is_null() {
                *out3 = c_int::from(start_rar_rename);
            }
            c_int::from(start_rar_rename)
        }
        11 => {
            let opt_post_dl_ren = (t1 & 1) != 0;
            let post_dl_ren_status = ((t1 >> 1) & 0xFF) as i32;
            let opt_ren_after_unpack = ((t1 >> 9) & 1) != 0;
            let post_unpack_ren_status = ((t1 >> 10) & 0xFF) as i32;
            let dec = crate::prepost::eval_post_stages(
                a != 0,
                b,
                c,
                d,
                e,
                f != 0,
                g,
                h,
                s1_bytes,
                s2_bytes,
                crate::paths::PATH_SEPARATOR,
                i,
                opt_post_dl_ren,
                post_dl_ren_status,
                !s1_bytes.is_empty(),
                !s3_bytes.is_empty(),
                opt_ren_after_unpack,
                post_unpack_ren_status,
            );
            if !out1.is_null() {
                *out1 = dec.unpack;
            }
            if !out2.is_null() {
                *out2 = dec.skip_unpack_due_to_par;
            }
            if !out3.is_null() {
                *out3 = dec.cleanup;
            }
            if !out4.is_null() {
                *out4 = dec.move_inter;
            }
            if !out5.is_null() {
                *out5 = dec.post_download_renaming;
            }
            if !out6.is_null() {
                *out6 = dec.post_unpack_renaming;
            }
            dec.unpack
        }
        12 => c_int::from(crate::prepost::calc_post_pause_state(
            stages,
            a != 0,
            b != 0,
            c != 0,
        )),
        13 => crate::prepost::classify_file_downloaded_direct_unpack(
            a != 0,
            b != 0,
            c != 0,
            d != 0,
            e,
            0,
            1,
            f,
            1,
            g != 0,
        ),
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_prepost_delete_cleanup(
    cleanup_disk: c_int,
    delete_status: c_int,
    parking: c_int,
    skip_disk_write: c_int,
    opt_skip_write: c_int,
    dest_dir: *const c_char,
    final_dir: *const c_char,
    completed_files: *const *const c_char,
    completed_count: usize,
    ctx: *mut c_void,
    log_cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char, *const c_char)>,
) {
    let mode = crate::prepost::classify_delete_cleanup_mode(
        cleanup_disk != 0,
        delete_status,
        3,
        parking != 0,
        skip_disk_write != 0,
        opt_skip_write != 0,
    );
    if mode == 0 || dest_dir.is_null() {
        return;
    }
    let dest_cstr = CStr::from_ptr(dest_dir);
    let dest_bytes = dest_cstr.to_bytes();
    let sep = crate::paths::PATH_SEPARATOR;
    if mode == 1 {
        if crate::fsops::directory_exists(dest_bytes, &crate::fsops::posix_stat) {
            if let Some(cb) = log_cb {
                cb(ctx, 0, c"Deleting dir %s".as_ptr(), dest_dir);
            }
            let mut errmsg = Vec::new();
            if !crate::fsdelete::delete_directory_with_content(
                &mut crate::fsops::PosixFsOps,
                dest_bytes,
                sep,
                &mut errmsg,
            )
            .unwrap_or(false)
            {
                if let Some(cb) = log_cb {
                    if let Ok(err_c) = CString::new(errmsg) {
                        cb(ctx, 1, dest_dir, err_c.as_ptr());
                    }
                }
            }
        }
        return;
    }

    if !completed_files.is_null() && completed_count > 0 {
        let slice = std::slice::from_raw_parts(completed_files, completed_count);
        for &cf_ptr in slice {
            if cf_ptr.is_null() {
                continue;
            }
            let cf_bytes = CStr::from_ptr(cf_ptr).to_bytes();
            let mut full = Vec::with_capacity(dest_bytes.len() + 1 + cf_bytes.len());
            full.extend_from_slice(dest_bytes);
            full.push(sep);
            full.extend_from_slice(cf_bytes);
            if crate::fsops::file_exists(&full, &crate::fsops::posix_stat) {
                if let Some(cb) = log_cb {
                    cb(ctx, 0, c"Deleting file %s".as_ptr(), cf_ptr);
                }
                if let Ok(full_c) = CString::new(full) {
                    let _ = crate::fsops::delete_file(&full_c);
                }
            }
        }
    }

    let mut dir = crate::fsops::PosixDirBrowser::new(Some(dest_cstr), false);
    loop {
        let fn_ptr = dir.next_ptr();
        if fn_ptr.is_null() {
            break;
        }
        let fn_bytes = CStr::from_ptr(fn_ptr).to_bytes();
        if fn_bytes.ends_with(b".out.tmp") {
            let mut full = Vec::with_capacity(dest_bytes.len() + 1 + fn_bytes.len());
            full.extend_from_slice(dest_bytes);
            full.push(sep);
            full.extend_from_slice(fn_bytes);
            if let Some(cb) = log_cb {
                cb(ctx, 0, c"Deleting file %s".as_ptr(), fn_ptr);
            }
            if let Ok(full_c) = CString::new(full) {
                let _ = crate::fsops::delete_file(&full_c);
            }
        }
    }

    let _ = crate::fsdelete::delete_directory(&mut crate::fsops::PosixFsOps, dest_bytes, sep);
    if !final_dir.is_null() {
        let final_cstr = CStr::from_ptr(final_dir);
        let final_bytes = final_cstr.to_bytes();
        if final_bytes != dest_bytes
            && crate::fsops::directory_exists(final_bytes, &crate::fsops::posix_stat)
        {
            let _ =
                crate::fsdelete::delete_directory(&mut crate::fsops::PosixFsOps, final_bytes, sep);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_prepost_pick_next_job(
    candidates: *const crate::prepost::RsPostJobCandidate,
    len: usize,
    allow_par: c_int,
    has_active_unpack: c_int,
    pause_post_process: c_int,
) -> c_int {
    let slice = if candidates.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(candidates, len)
    };
    crate::prepost::pick_next_post_job(
        slice,
        allow_par != 0,
        has_active_unpack != 0,
        pause_post_process != 0,
    )
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_repair_ctrl_new(
    nntp_host: crate::articlefetcher::NzbgetRsNntpFetchHostFn,
) -> *mut crate::repair::NzbgetRsRepairController {
    Box::into_raw(Box::new(crate::repair::NzbgetRsRepairController::new(
        nntp_host,
    )))
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_repair_ctrl_free(
    ctrl: *mut crate::repair::NzbgetRsRepairController,
) {
    if !ctrl.is_null() {
        drop(Box::from_raw(ctrl));
    }
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_repair_ctrl_exec(
    ctrl: *const crate::repair::NzbgetRsRepairController,
    op: c_int,
    a1: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    out_str: *mut *const c_char,
    host_cb: Option<crate::repair::NzbgetRsRepairCtrlHostFn>,
    ctx: *mut c_void,
) -> c_int {
    let _cancel_guard = PosixCancelGuard::disable();
    crate::repair::repair_ctrl_exec(ctrl, op, a1, p1, p2, s1, out_str, host_cb, ctx)
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_stream_repair_ctrl_new(
    live_mode: c_int,
    nzb_id: c_int,
    nntp_host: crate::articlefetcher::NzbgetRsNntpFetchHostFn,
    probe_host: crate::dupeprobe::DupeProbeHostFn,
    tls_enabled: c_int,
    debug_enabled: c_int,
) -> *mut crate::streamrepair_coord::NzbgetRsStreamRepairCtrl {
    Box::into_raw(Box::new(
        crate::streamrepair_coord::NzbgetRsStreamRepairCtrl::new(
            live_mode != 0,
            nzb_id,
            nntp_host,
            probe_host,
            tls_enabled != 0,
            debug_enabled != 0,
        ),
    ))
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_stream_repair_ctrl_free(
    ctrl: *mut crate::streamrepair_coord::NzbgetRsStreamRepairCtrl,
) {
    if !ctrl.is_null() {
        drop(Box::from_raw(ctrl));
    }
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_stream_repair_ctrl_exec(
    ctrl: *const crate::streamrepair_coord::NzbgetRsStreamRepairCtrl,
    op: c_int,
    host_cb: Option<crate::streamrepair_coord::NzbgetRsStreamRepairHostFn>,
    ctx: *mut c_void,
) -> c_int {
    let _cancel_guard = PosixCancelGuard::disable();
    crate::streamrepair_coord::stream_repair_ctrl_exec(ctrl, op, host_cb, ctx)
}

#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_repair_ctrl_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    d: c_int,
    e: c_int,
    f: c_int,
    t1: i64,
    _t2: i64,
    _t3: i64,
    _t4: i64,
    s1: *const c_char,
    s2: *const c_char,
    _i32_arr: *const c_int,
    _u8_arr: *const u8,
    _i64_arr: *const i64,
    _len: usize,
    out1: *mut c_int,
    out2: *mut c_int,
    _out3: *mut c_int,
    _out4: *mut c_int,
    _out_arr: *mut c_int,
    _out_i64_arr: *mut i64,
) -> c_int {
    let s1_bytes = if s1.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(s1).to_bytes()
    };
    let s2_bytes = if s2.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(s2).to_bytes()
    };
    match op {
        14 => c_int::from(crate::unpack::has_completed_archive_ext(s1_bytes)),
        15 => c_int::from(crate::unpack::use_inter_dir(
            s1_bytes,
            s2_bytes,
            crate::paths::PATH_SEPARATOR,
        )),
        16 => {
            let (outcome, move_status) = crate::cleanup::decide_move_outcome(a != 0, b != 0);
            if !out1.is_null() {
                *out1 = move_status;
            }
            outcome
        }
        17 => crate::cleanup::rename_classify_run_outcome(a != 0, b),
        18 => {
            let (use_final, detect_missing) =
                crate::cleanup::rename_eval_par_config(a, b, c, d != 0);
            if !out1.is_null() {
                *out1 = c_int::from(detect_missing);
            }
            c_int::from(use_final)
        }
        19 => {
            let (init_status, req_extra, req_par_check) =
                crate::cleanup::rename_complete_action(a, b, c != 0, d, e != 0, f);
            if !out1.is_null() {
                *out1 = c_int::from(req_extra);
            }
            if !out2.is_null() {
                *out2 = c_int::from(req_par_check);
            }
            init_status
        }
        20 => c_int::from(crate::cleanup::rename_should_reset_par_status(t1)),
        21 => c_int::from(crate::unpack::is_unpack_space_error(a != 0, s1_bytes)),
        22 => {
            let (req_par, force_rep, st) = crate::unpack::eval_unpack_completed_failure(
                a != 0,
                b != 0,
                c,
                1,
                d != 0,
                (e & 1) != 0,
                (e & 2) != 0,
                (e & 4) != 0,
                (e & 8) != 0,
                (e & 16) != 0,
                (e & 32) != 0,
                (e & 64) != 0,
                (f & 1) != 0,
                (f & 2) != 0,
                (f & 4) != 0,
                (f & 8) != 0,
            );
            if !out1.is_null() {
                *out1 = c_int::from(req_par);
            }
            if !out2.is_null() {
                *out2 = c_int::from(force_rep);
            }
            st
        }
        23 => c_int::from(crate::extension::ext_is_in_list(s1_bytes, s2_bytes)),
        24 => {
            if !s1_bytes.is_empty()
                && s1_bytes[0] != b'*'
                && s1_bytes[s1_bytes.len() - 1] == b':'
                && (s2_bytes.eq_ignore_ascii_case(b"yes")
                    || s2_bytes.eq_ignore_ascii_case(b"on")
                    || s2_bytes == b"1")
            {
                (s1_bytes.len() - 1) as c_int
            } else {
                0
            }
        }
        25 => c_int::from(crate::extension::is_nzbget_internal_env_var(s1_bytes)),
        26 => {
            let (trimmed_len, mk, text_off) =
                crate::extension::classify_script_output_line(s1_bytes);
            if trimmed_len == 0 {
                return -1;
            }
            if !out1.is_null() {
                *out1 = trimmed_len as c_int;
            }
            if !out2.is_null() {
                *out2 = text_off as c_int;
            }
            mk
        }
        _ => 0,
    }
}

#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_stream_repair_ctrl_op(
    op: c_int,
    _a: c_int,
    _b: c_int,
    _c: c_int,
    _d: c_int,
    _e: c_int,
    _t1: i64,
    _t2: i64,
    _t3: i64,
    _f1: f64,
    s1: *const c_char,
    _cstr_arr: *const *const c_char,
    _i64_arr: *const i64,
    _len: usize,
    _out1: *mut c_int,
    _out2: *mut c_int,
    _i64_inout: *mut i64,
    out_buf: *mut RsBuf,
) -> c_int {
    let s1_bytes = if s1.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(s1).to_bytes()
    };
    if op == 10 {
        let norm = crate::extension::normalize_special_env_var_name(s1_bytes);
        if norm.as_slice() != s1_bytes {
            if !out_buf.is_null() {
                *out_buf = into_buf(norm);
            }
            return 1;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_dupesearch_new() -> *mut crate::dupesearch_coord::NzbgetRsDupeSearch {
    Box::into_raw(Box::new(crate::dupesearch_coord::NzbgetRsDupeSearch::new()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_dupesearch_free(
    ptr: *mut crate::dupesearch_coord::NzbgetRsDupeSearch,
) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_dupesearch_exec(
    ptr: *mut crate::dupesearch_coord::NzbgetRsDupeSearch,
    op: c_int,
    a1: c_int,
    p1: usize,
    p2: usize,
    host_ctx: *mut c_void,
    host_cb: Option<crate::dupesearch_coord::DupeHostFn>,
) {
    let Some(ds) = ptr.as_ref() else {
        return;
    };
    match op {
        0 => ds.stop(),
        1 => {
            if let Some(cb) = host_cb {
                ds.notify_name_changed(a1, host_ctx, cb);
            }
        }
        2 => {
            if let Some(cb) = host_cb {
                ds.download_queue_update(a1, p1, p2, host_ctx, cb);
            }
        }
        3 => {
            if let Some(cb) = host_cb {
                ds.run(host_ctx, cb);
            }
        }
        _ => {}
    }
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nzbget_rs_fleet_append(
    host_ctx: *mut c_void,
    host_cb: Option<crate::dupesearch_coord::DupeHostFn>,
    dupe_key: *const c_char,
    category: *const c_char,
    priority: c_int,
    timeout_sec: c_int,
    members: *const crate::xmlrpc_format::NzbgetRsRpcFleetReqMember,
    members_len: usize,
    emit_ctx: *mut c_void,
    emit_cb: Option<crate::xmlrpc_format::NzbgetRsXmlRpcEmitFn>,
) -> c_int {
    let (Some(hcb), Some(ecb)) = (host_cb, emit_cb) else {
        return 0;
    };
    crate::dupesearch_coord::fleet_append(
        host_ctx,
        hcb,
        dupe_key,
        category,
        priority,
        timeout_sec,
        members,
        members_len,
        emit_ctx,
        ecb,
    )
}

#[cfg(unix)]
#[repr(C)]
pub struct NzbgetRsConnEnv {
    pub ctx: *mut std::ffi::c_void,
    pub host: *const c_char,
    pub port: c_int,
    pub ip_version: c_int,
    pub timeout_sec: c_int,
    pub graceful: c_int,
    pub force_close: c_int,
    pub has_deadline: c_int,
    pub resolved_addrs: *const *const c_char,
    pub resolved_count: usize,
    pub socket_atomic: *mut c_int,
    pub status_atomic: *mut c_int,
    pub is_listener: *mut c_int,
    pub accept_busy: *mut c_int,
    pub tls_error: *mut c_int,
    pub buf_offset: *mut usize,
    pub buf_avail: *mut usize,
    pub total_bytes_read: *mut c_int,
    pub read_buf: *mut u8,
    pub read_buf_cap: usize,
    pub send_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, c_int) -> c_int>,
    pub recv_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut c_char, c_int) -> c_int>,
    pub error_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
    pub debug_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
    pub deadline_wait_cb:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, c_int, c_int) -> c_int>,
    pub deadline_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
}

#[cfg(unix)]
struct PosixCancelGuard {
    old_state: c_int,
}

#[cfg(unix)]
impl PosixCancelGuard {
    unsafe fn disable() -> Self {
        extern "C" {
            fn pthread_setcancelstate(state: c_int, oldstate: *mut c_int) -> c_int;
        }
        let mut old_state = libc::PTHREAD_CANCEL_ENABLE;
        pthread_setcancelstate(libc::PTHREAD_CANCEL_DISABLE, &mut old_state);
        Self { old_state }
    }
}

#[cfg(unix)]
impl Drop for PosixCancelGuard {
    fn drop(&mut self) {
        extern "C" {
            fn pthread_setcancelstate(state: c_int, oldstate: *mut c_int) -> c_int;
        }
        unsafe {
            pthread_setcancelstate(self.old_state, std::ptr::null_mut());
        }
    }
}

#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_conn_op(
    op: c_int,
    env: *const NzbgetRsConnEnv,
    buf_ptr: *mut c_char,
    buf_len: c_int,
    arg1: *const c_char,
    arg2: *const c_char,
    arg3: *const c_char,
    int_arg: c_int,
    out_int: *mut c_int,
) -> c_int {
    use std::sync::atomic::{AtomicI32, Ordering};
    let _cancel_guard = PosixCancelGuard::disable();
    if env.is_null() {
        return 0;
    }
    let e = &*env;
    if e.socket_atomic.is_null() || e.status_atomic.is_null() {
        return 0;
    }
    let sock_slot = AtomicI32::from_ptr(e.socket_atomic);
    let status_slot = AtomicI32::from_ptr(e.status_atomic);
    if op == 5 {
        crate::socket::cancel_socket(e.force_close != 0, sock_slot, status_slot);
        return 1;
    }
    let host = input(e.host);
    let mut is_listener = matches!(op, 0 | 2) && !e.is_listener.is_null() && *e.is_listener != 0;
    let mut accept_busy = op == 3 && !e.accept_busy.is_null() && *e.accept_busy != 0;
    let mut tls_error =
        matches!(op, 0 | 1 | 3 | 7 | 8 | 9 | 11) && !e.tls_error.is_null() && *e.tls_error != 0;

    let to_cstr_lossless = |msg: &[u8]| -> std::ffi::CString {
        let nul = msg.iter().position(|&b| b == 0).unwrap_or(msg.len());
        std::ffi::CString::from_vec_unchecked(msg[..nul].to_vec())
    };

    let on_error = |msg: &[u8]| {
        if let Some(cb) = e.error_cb {
            let c = to_cstr_lossless(msg);
            cb(e.ctx, c.as_ptr());
        }
    };
    let on_debug = |msg: &[u8]| {
        if let Some(cb) = e.debug_cb {
            let c = to_cstr_lossless(msg);
            cb(e.ctx, c.as_ptr());
        }
    };

    let res = match op {
        0 => c_int::from(crate::socket::bind_listener(
            host,
            e.port,
            e.ip_version,
            sock_slot,
            status_slot,
            &mut is_listener,
            &mut tls_error,
            on_error,
        )),
        1 => {
            let mut resolved_vec = Vec::with_capacity(e.resolved_count);
            if !e.resolved_addrs.is_null() && e.resolved_count > 0 {
                for i in 0..e.resolved_count {
                    let p = *e.resolved_addrs.add(i);
                    resolved_vec.push(input(p));
                }
            }
            c_int::from(crate::socket::connect_stream(
                host,
                e.port,
                e.ip_version,
                e.timeout_sec,
                &resolved_vec,
                e.has_deadline != 0,
                |fd, r, w| {
                    e.deadline_wait_cb.map_or(false, |cb| {
                        cb(e.ctx, fd, c_int::from(r), c_int::from(w)) != 0
                    })
                },
                || e.deadline_stopped_cb.map_or(false, |cb| cb(e.ctx) != 0),
                sock_slot,
                status_slot,
                &mut tls_error,
                on_error,
                on_debug,
            ))
        }
        2 => {
            crate::socket::disconnect_socket(
                host,
                e.graceful != 0,
                &mut is_listener,
                sock_slot,
                status_slot,
            );
            1
        }
        3 => crate::socket::accept_connection(
            host,
            e.timeout_sec,
            sock_slot,
            status_slot,
            &mut accept_busy,
            &mut tls_error,
            || e.deadline_stopped_cb.map_or(false, |cb| cb(e.ctx) != 0),
            on_error,
        ),
        4 => {
            let ra = crate::socket::remote_addr(host, sock_slot.load(Ordering::SeqCst));
            if !buf_ptr.is_null() && buf_len > 0 {
                let copy_len = ra.len().min((buf_len as usize).saturating_sub(1));
                if copy_len > 0 {
                    std::ptr::copy_nonoverlapping(ra.as_ptr(), buf_ptr.cast::<u8>(), copy_len);
                }
                *buf_ptr.add(copy_len) = 0;
                copy_len as c_int
            } else {
                0
            }
        }
        6 => {
            let data = if buf_ptr.is_null() || buf_len <= 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(buf_ptr.cast::<u8>(), buf_len as usize)
            };
            c_int::from(crate::socket::send_all(status_slot, data, |chunk| {
                e.send_cb.map_or(-1, |cb| {
                    cb(e.ctx, chunk.as_ptr().cast::<c_char>(), chunk.len() as c_int)
                })
            }))
        }
        7 => {
            if status_slot.load(Ordering::SeqCst) != crate::socket::CS_CONNECTED
                || buf_ptr.is_null()
                || buf_len <= 0
                || e.read_buf.is_null()
                || e.read_buf_cap == 0
                || e.buf_offset.is_null()
                || e.buf_avail.is_null()
                || e.total_bytes_read.is_null()
            {
                0
            } else {
                let rbuf = std::slice::from_raw_parts_mut(e.read_buf, e.read_buf_cap);
                let dst = std::slice::from_raw_parts_mut(buf_ptr.cast::<u8>(), buf_len as usize);
                let r = crate::socket::read_line(
                    host,
                    status_slot,
                    rbuf,
                    &mut *e.buf_offset,
                    &mut *e.buf_avail,
                    &mut *e.total_bytes_read,
                    dst,
                    &mut tls_error,
                    |chunk, te| {
                        let out = e.recv_cb.map_or(-1, |cb| {
                            cb(
                                e.ctx,
                                chunk.as_mut_ptr().cast::<c_char>(),
                                chunk.len() as c_int,
                            )
                        });
                        *te = !e.tls_error.is_null() && *e.tls_error != 0;
                        out
                    },
                    on_error,
                );
                match r {
                    Some(n) => {
                        if !out_int.is_null() {
                            *out_int = n as c_int;
                        }
                        1
                    }
                    None => {
                        if !out_int.is_null() {
                            *out_int = 0;
                        }
                        0
                    }
                }
            }
        }
        8 => {
            if buf_len <= 0 {
                1
            } else if buf_ptr.is_null()
                || e.read_buf.is_null()
                || e.buf_offset.is_null()
                || e.buf_avail.is_null()
                || e.total_bytes_read.is_null()
            {
                0
            } else {
                let rbuf = std::slice::from_raw_parts(e.read_buf, e.read_buf_cap);
                let dst = std::slice::from_raw_parts_mut(buf_ptr.cast::<u8>(), buf_len as usize);
                c_int::from(crate::socket::recv_exact(
                    host,
                    rbuf,
                    &mut *e.buf_offset,
                    &mut *e.buf_avail,
                    &mut *e.total_bytes_read,
                    dst,
                    &mut tls_error,
                    |chunk, te| {
                        let out = e.recv_cb.map_or(-1, |cb| {
                            cb(
                                e.ctx,
                                chunk.as_mut_ptr().cast::<c_char>(),
                                chunk.len() as c_int,
                            )
                        });
                        *te = !e.tls_error.is_null() && *e.tls_error != 0;
                        out
                    },
                    on_error,
                ))
            }
        }
        9 => {
            let prefix = input(arg1);
            let msg_arg = if arg2.is_null() {
                None
            } else {
                Some(input(arg2))
            };
            let err_msg = if arg3.is_null() {
                None
            } else {
                Some(input(arg3))
            };
            if let Some(formatted) = crate::socket::format_report_error(
                &mut tls_error,
                prefix,
                msg_arg,
                buf_len != 0,
                int_arg,
                err_msg,
            ) {
                on_error(&formatted);
            }
            1
        }
        10 => crate::socket::write_line(status_slot, input(arg1), |chunk| {
            e.send_cb.map_or(-1, |cb| {
                cb(e.ctx, chunk.as_ptr().cast::<c_char>(), chunk.len() as c_int)
            })
        }),
        11 => {
            if buf_ptr.is_null() || buf_len < 0 || e.total_bytes_read.is_null() {
                -1
            } else {
                let dst = if buf_len == 0 {
                    &mut [][..]
                } else {
                    std::slice::from_raw_parts_mut(buf_ptr.cast::<u8>(), buf_len as usize)
                };
                crate::socket::try_recv(
                    host,
                    &mut *e.total_bytes_read,
                    dst,
                    &mut tls_error,
                    |chunk, te| {
                        let out = e.recv_cb.map_or(-1, |cb| {
                            cb(
                                e.ctx,
                                chunk.as_mut_ptr().cast::<c_char>(),
                                chunk.len() as c_int,
                            )
                        });
                        *te = !e.tls_error.is_null() && *e.tls_error != 0;
                        out
                    },
                    on_error,
                )
            }
        }
        _ => 0,
    };

    if matches!(op, 0 | 2) && !e.is_listener.is_null() {
        *e.is_listener = c_int::from(is_listener);
    }
    if op == 3 && !e.accept_busy.is_null() {
        *e.accept_busy = c_int::from(accept_busy);
    }
    if matches!(op, 0 | 1 | 3 | 7 | 8 | 9 | 11) && !e.tls_error.is_null() {
        *e.tls_error = c_int::from(tls_error);
    }
    res
}

#[cfg(unix)]
#[repr(C)]
pub struct NzbgetRsWebDlEnv {
    pub ctx: *mut std::ffi::c_void,
    pub initial_url: *const c_char,
    pub output_filename: *const c_char,
    pub info_name: *const c_char,
    pub version: *const c_char,
    pub write_buffer_kb: c_int,
    pub url_retries: c_int,
    pub url_interval_sec: c_int,
    pub retry: c_int,
    pub force: c_int,
    pub tls_disabled: c_int,
    pub gzip_disabled: c_int,
    pub connect_cb:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, c_int, c_int) -> c_int>,
    pub free_conn_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    pub write_line_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
    pub read_line_cb: Option<
        unsafe extern "C" fn(*mut std::ffi::c_void, *mut c_char, c_int, *mut c_int) -> c_int,
    >,
    pub read_body_cb:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut c_char, c_int) -> c_int>,
    pub is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    pub is_paused_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    pub touch_time_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    pub set_status_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int)>,
    pub set_http_status_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int)>,
    pub set_orig_filename_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
    pub set_url_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
    pub on_header_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
    pub on_log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
}

#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_webdl_execute(
    op: c_int,
    max_redirects: c_int,
    env: *const NzbgetRsWebDlEnv,
) -> c_int {
    use crate::webdownload::{
        webdl_download_with_redirects, webdl_run_loop, WebDlConfig, WebDlIo, WebDlStatus,
    };
    let _cancel_guard = PosixCancelGuard::disable();
    if env.is_null() {
        return WebDlStatus::AdFailed as c_int;
    }
    let e = &*env;
    let cfg = WebDlConfig {
        initial_url: input(e.initial_url),
        output_filename: input(e.output_filename),
        info_name: input(e.info_name),
        version: input(e.version),
        write_buffer_kb: e.write_buffer_kb,
        url_retries: e.url_retries,
        url_interval_sec: e.url_interval_sec,
        retry: e.retry != 0,
        force: e.force != 0,
        tls_disabled: e.tls_disabled != 0,
        gzip_disabled: e.gzip_disabled != 0,
    };

    struct CWebDlIo<'a> {
        e: &'a NzbgetRsWebDlEnv,
    }

    impl WebDlIo for CWebDlIo<'_> {
        fn connect(&mut self, host: &CStr, port: i32, tls: bool) -> bool {
            unsafe {
                self.e.connect_cb.map_or(false, |cb| {
                    cb(self.e.ctx, host.as_ptr(), port, c_int::from(tls)) != 0
                })
            }
        }
        fn free_conn(&mut self) {
            unsafe {
                if let Some(cb) = self.e.free_conn_cb {
                    cb(self.e.ctx);
                }
            }
        }
        fn write_line(&mut self, line: &CStr) {
            unsafe {
                if let Some(cb) = self.e.write_line_cb {
                    cb(self.e.ctx, line.as_ptr());
                }
            }
        }
        fn read_line(&mut self, buf: &mut [u8]) -> Option<usize> {
            unsafe {
                let mut len: c_int = 0;
                let ok = self.e.read_line_cb.map_or(0, |cb| {
                    cb(
                        self.e.ctx,
                        buf.as_mut_ptr().cast::<c_char>(),
                        buf.len() as c_int,
                        &mut len,
                    )
                });
                if ok != 0 {
                    Some((len.max(0) as usize).min(buf.len().saturating_sub(1)))
                } else {
                    None
                }
            }
        }
        fn read_body_chunk(&mut self, buf: &mut [u8]) -> i32 {
            unsafe {
                self.e.read_body_cb.map_or(-1, |cb| {
                    cb(
                        self.e.ctx,
                        buf.as_mut_ptr().cast::<c_char>(),
                        buf.len() as c_int,
                    )
                })
            }
        }
        fn is_stopped(&mut self) -> bool {
            unsafe { self.e.is_stopped_cb.map_or(false, |cb| cb(self.e.ctx) != 0) }
        }
        fn is_paused(&mut self) -> bool {
            unsafe { self.e.is_paused_cb.map_or(false, |cb| cb(self.e.ctx) != 0) }
        }
        fn touch_time(&mut self) {
            unsafe {
                if let Some(cb) = self.e.touch_time_cb {
                    cb(self.e.ctx);
                }
            }
        }
        fn sleep_100ms(&mut self) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        fn set_status(&mut self, status: WebDlStatus) {
            unsafe {
                if let Some(cb) = self.e.set_status_cb {
                    cb(self.e.ctx, status as c_int);
                }
            }
        }
        fn set_http_status(&mut self, http_status: i32) {
            unsafe {
                if let Some(cb) = self.e.set_http_status_cb {
                    cb(self.e.ctx, http_status);
                }
            }
        }
        fn set_original_filename(&mut self, filename: &[u8]) {
            unsafe {
                if let Some(cb) = self.e.set_orig_filename_cb {
                    let nul = filename
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(filename.len());
                    let c = std::ffi::CString::from_vec_unchecked(filename[..nul].to_vec());
                    cb(self.e.ctx, c.as_ptr());
                }
            }
        }
        fn set_url(&mut self, url: &[u8]) {
            unsafe {
                if let Some(cb) = self.e.set_url_cb {
                    let nul = url.iter().position(|&b| b == 0).unwrap_or(url.len());
                    let c = std::ffi::CString::from_vec_unchecked(url[..nul].to_vec());
                    cb(self.e.ctx, c.as_ptr());
                }
            }
        }
        fn on_header(&mut self, line: &CStr) {
            unsafe {
                if let Some(cb) = self.e.on_header_cb {
                    cb(self.e.ctx, line.as_ptr());
                }
            }
        }
        fn on_log(&mut self, level: i32, msg: &[u8]) {
            unsafe {
                if let Some(cb) = self.e.on_log_cb {
                    let nul = msg.iter().position(|&b| b == 0).unwrap_or(msg.len());
                    let c = std::ffi::CString::from_vec_unchecked(msg[..nul].to_vec());
                    cb(self.e.ctx, level, c.as_ptr());
                }
            }
        }
    }

    let mut io = CWebDlIo { e };
    match op {
        0 => webdl_download_with_redirects(max_redirects, &cfg, &mut io) as c_int,
        1 => webdl_run_loop(&cfg, &mut io) as c_int,
        _ => WebDlStatus::AdFailed as c_int,
    }
}

// ---- WebProcessor / BinRpcProcessor / RemoteClient / Frontend FFI ----

#[repr(C)]
pub struct NzbgetRsWebServeEnv {
    pub ctx: *mut c_void,
    pub signature: *const c_char,
    pub remote_addr: *const c_char,
    pub version: *const c_char,
    pub web_dir: *const c_char,
    pub script_dirs: *const *const c_char,
    pub script_dirs_count: usize,
    pub control_username: *const c_char,
    pub control_password: *const c_char,
    pub restricted_username: *const c_char,
    pub restricted_password: *const c_char,
    pub add_username: *const c_char,
    pub add_password: *const c_char,
    pub authorized_ip: *const c_char,
    pub config_filename: *const c_char,
    pub shell_override: *const c_char,
    pub control_port: c_int,
    pub form_auth: c_int,
    pub gzip_enabled: c_int,
    pub json_p: c_int,
    pub debug_build: c_int,
    pub read_line_cb: Option<unsafe extern "C" fn(*mut c_void, *mut c_char, c_int) -> c_int>,
    pub recv_cb: Option<unsafe extern "C" fn(*mut c_void, *mut c_char, c_int) -> c_int>,
    pub send_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_int>,
    pub log_cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>,
    pub rpc_action_cb: Option<crate::xmlrpc_format::NzbgetRsXmlRpcActionFn>,
    pub rpc_collect_cb: Option<crate::xmlrpc_format::NzbgetRsXmlRpcCollectFn>,
}

unsafe fn to_nul_cvec(bytes: &[u8]) -> std::ffi::CString {
    let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::ffi::CString::from_vec_unchecked(bytes[..nul].to_vec())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_web_serve(op: c_int, env: *const NzbgetRsWebServeEnv) -> c_int {
    #[cfg(unix)]
    let _cancel_guard = PosixCancelGuard::disable();

    if op == 0 {
        crate::webserver::init_server_tokens(|msg| {
            if !env.is_null() {
                if let Some(cb) = (*env).log_cb {
                    let c = to_nul_cvec(msg);
                    cb((*env).ctx, 2, c.as_ptr());
                }
            }
        });
        return 1;
    }

    #[cfg(unix)]
    if op == 1 {
        let Some(e) = env.as_ref() else {
            return 0;
        };
        if e.signature.is_null() {
            return 0;
        }
        let sig = std::slice::from_raw_parts(e.signature.cast::<u8>(), 4);
        let mut script_vec = Vec::with_capacity(if e.script_dirs.is_null() {
            0
        } else {
            e.script_dirs_count
        });
        if !e.script_dirs.is_null() {
            for i in 0..e.script_dirs_count {
                script_vec.push(input(*e.script_dirs.add(i)));
            }
        }
        let opt_c = |p: *const c_char| -> Option<&[u8]> {
            (!p.is_null()).then(|| CStr::from_ptr(p).to_bytes())
        };
        let cfg = crate::webserver::WebServeConfig {
            signature: sig,
            remote_addr: input(e.remote_addr),
            version: input(e.version),
            web_dir: input(e.web_dir),
            script_dirs: &script_vec,
            credentials: crate::webserver::Credentials {
                control_username: opt_c(e.control_username),
                control_password: opt_c(e.control_password),
                restricted_username: opt_c(e.restricted_username),
                restricted_password: opt_c(e.restricted_password),
                add_username: opt_c(e.add_username),
                add_password: opt_c(e.add_password),
                authorized_ip: opt_c(e.authorized_ip),
            },
            control_port: e.control_port,
            form_auth: e.form_auth != 0,
            gzip_enabled: e.gzip_enabled != 0,
            debug_build: e.debug_build != 0,
        };
        let lower = crate::wildmask::Lower::Fold(&crate::util::wild_mask_fold_locale);

        let keep_alive = crate::webserver::serve_web_request(
            &cfg,
            &lower,
            |dst| {
                let Some(cb) = e.read_line_cb else {
                    return false;
                };
                cb(e.ctx, dst.as_mut_ptr().cast(), dst.len() as c_int) != 0
            },
            |dst| {
                let Some(cb) = e.recv_cb else {
                    return false;
                };
                cb(e.ctx, dst.as_mut_ptr().cast(), dst.len() as c_int) != 0
            },
            |src| {
                let Some(cb) = e.send_cb else {
                    return false;
                };
                cb(e.ctx, src.as_ptr().cast(), src.len() as c_int) != 0
            },
            |level, msg| {
                if let Some(cb) = e.log_cb {
                    let c = to_nul_cvec(msg);
                    cb(e.ctx, level, c.as_ptr());
                }
            },
            |req_body, http_method, user_access, url_c, emit| {
                let (resp, ct, safe) = crate::xmlrpc_format::serve_xmlrpc(
                    e.ctx,
                    req_body,
                    http_method,
                    user_access,
                    e.json_p != 0,
                    e.debug_build != 0,
                    url_c,
                    cfg.version,
                    opt_c(e.config_filename),
                    input(e.shell_override),
                    e.log_cb,
                    e.rpc_action_cb,
                    e.rpc_collect_cb,
                );
                let nul = resp.iter().position(|&b| b == 0).unwrap_or(resp.len());
                emit(&resp[..nul], ct, safe);
            },
        );
        return c_int::from(keep_alive);
    }

    0
}

pub type NzbgetRsBinRpcListPushNzbFn = unsafe extern "C" fn(
    *mut c_void,
    u32,
    u32,
    i64,
    i64,
    i64,
    u32,
    u32,
    u32,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
);
pub type NzbgetRsBinRpcListPushParamFn =
    unsafe extern "C" fn(*mut c_void, u32, *const c_char, *const c_char);
pub type NzbgetRsBinRpcListPushFileFn = unsafe extern "C" fn(
    *mut c_void,
    u32,
    u32,
    i64,
    i64,
    u32,
    u32,
    u32,
    *const c_char,
    *const c_char,
    *const c_char,
);
pub type NzbgetRsBinRpcListSetStateFn = unsafe extern "C" fn(
    *mut c_void,
    i64,
    u32,
    c_int,
    c_int,
    c_int,
    c_int,
    c_int,
    c_int,
    c_int,
    c_int,
    c_int,
    i64,
);
pub type NzbgetRsBinRpcLogBeginFn = unsafe extern "C" fn(*mut c_void, c_int, u32, *mut c_int);
pub type NzbgetRsBinRpcLogPushFn = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_char);
pub type NzbgetRsBinRpcPostPushFn = unsafe extern "C" fn(
    *mut c_void,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
);
pub type NzbgetRsBinRpcHistPushFn =
    unsafe extern "C" fn(*mut c_void, u32, u32, u32, i64, u32, u32, u32, u32, *const c_char);

#[repr(C)]
pub struct NzbgetRsBinRpcServeEnv {
    pub ctx: *mut c_void,
    pub remote_addr: *const c_char,
    pub control_user: *const c_char,
    pub control_password: *const c_char,
    pub version: *const c_char,
    pub control_port: c_int,
    pub recv_cb: Option<unsafe extern "C" fn(*mut c_void, *mut c_char, c_int) -> c_int>,
    pub send_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_int>,
    pub log_cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>,
    pub action_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            c_int,
            c_int,
            c_int,
            c_int,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
            c_int,
            *const i32,
            usize,
            *const *const c_char,
            usize,
        ) -> c_int,
    >,
    pub collect_list_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            c_int,
            *mut c_void,
            NzbgetRsBinRpcListPushNzbFn,
            NzbgetRsBinRpcListPushParamFn,
            NzbgetRsBinRpcListPushFileFn,
            NzbgetRsBinRpcListSetStateFn,
        ),
    >,
    pub collect_log_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            u32,
            *mut c_void,
            NzbgetRsBinRpcLogBeginFn,
            NzbgetRsBinRpcLogPushFn,
        ),
    >,
    pub collect_post_cb:
        Option<unsafe extern "C" fn(*mut c_void, *mut c_void, NzbgetRsBinRpcPostPushFn)>,
    pub collect_history_cb:
        Option<unsafe extern "C" fn(*mut c_void, c_int, *mut c_void, NzbgetRsBinRpcHistPushFn)>,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_serve(env: *const NzbgetRsBinRpcServeEnv) {
    #[cfg(unix)]
    {
        let _cancel_guard = PosixCancelGuard::disable();
        let Some(e) = env.as_ref() else {
            return;
        };

        unsafe extern "C" fn list_push_nzb(
            b_ptr: *mut c_void,
            id: u32,
            kind: u32,
            size: i64,
            rem_size: i64,
            paused_size: i64,
            paused_files: u32,
            rem_pars: u32,
            priority: u32,
            filename: *const c_char,
            name: *const c_char,
            dest_dir: *const c_char,
            category: *const c_char,
            queued_filename: *const c_char,
        ) {
            let b = &mut *b_ptr.cast::<crate::binrpc::BinRpcListBuilder>();
            b.push_nzb(
                id,
                kind,
                size,
                rem_size,
                paused_size,
                paused_files,
                rem_pars,
                priority,
                input(filename),
                input(name),
                input(dest_dir),
                input(category),
                input(queued_filename),
            );
        }

        unsafe extern "C" fn list_push_param(
            b_ptr: *mut c_void,
            owner_idx: u32,
            name: *const c_char,
            value: *const c_char,
        ) {
            let b = &mut *b_ptr.cast::<crate::binrpc::BinRpcListBuilder>();
            b.push_param(owner_idx, input(name), input(value));
        }

        unsafe extern "C" fn list_push_file(
            b_ptr: *mut c_void,
            id: u32,
            nzb_idx: u32,
            size: i64,
            rem_size: i64,
            paused: u32,
            fn_confirmed: u32,
            active_dl: u32,
            nzb_name: *const c_char,
            subject: *const c_char,
            filename: *const c_char,
        ) {
            let b = &mut *b_ptr.cast::<crate::binrpc::BinRpcListBuilder>();
            b.push_file(
                id,
                nzb_idx,
                size,
                rem_size,
                paused,
                fn_confirmed,
                active_dl,
                input(nzb_name),
                input(subject),
                input(filename),
            );
        }

        unsafe extern "C" fn list_set_state(
            b_ptr: *mut c_void,
            rem_size: i64,
            dl_rate: u32,
            dl_limit: c_int,
            dl_paused: c_int,
            post_paused: c_int,
            scan_paused: c_int,
            thread_cnt: c_int,
            post_jobs: c_int,
            up_sec: c_int,
            dn_sec: c_int,
            stand_by: c_int,
            all_bytes: i64,
        ) {
            let b = &mut *b_ptr.cast::<crate::binrpc::BinRpcListBuilder>();
            b.remaining_size = rem_size;
            b.download_rate = dl_rate;
            b.download_limit = dl_limit;
            b.download_paused = dl_paused;
            b.post_paused = post_paused;
            b.scan_paused = scan_paused;
            b.thread_count = thread_cnt;
            b.post_job_count = post_jobs;
            b.up_time_sec = up_sec;
            b.dn_time_sec = dn_sec;
            b.stand_by = stand_by;
            b.downloaded_bytes = all_bytes;
            b.state_populated = true;
        }

        struct LogCollectCtx<'a> {
            lines: i32,
            id_from: u32,
            nr_entries: &'a mut u32,
            enc: &'a mut crate::binrpc::WireEntryEncoder,
        }

        unsafe extern "C" fn log_begin(
            enc_ptr: *mut c_void,
            msg_count: c_int,
            first_id: u32,
            out_start: *mut c_int,
        ) {
            let lc = &mut *enc_ptr.cast::<LogCollectCtx<'_>>();
            let (start, nr) =
                crate::binrpc::calc_log_slice(msg_count, lc.lines, lc.id_from, first_id);
            *lc.nr_entries = nr.max(0) as u32;
            if !out_start.is_null() {
                *out_start = start;
            }
        }

        unsafe extern "C" fn log_push(
            enc_ptr: *mut c_void,
            id: u32,
            kind: u32,
            time: u32,
            text: *const c_char,
        ) {
            let lc = &mut *enc_ptr.cast::<LogCollectCtx<'_>>();
            let entry = crate::binrpc::LogEntry {
                id,
                kind,
                time,
                texts: [crate::binrpc::Text::new(input(text))],
            };
            lc.enc.push(&entry);
        }

        struct CountedEncCtx<'a> {
            nr_entries: &'a mut u32,
            enc: &'a mut crate::binrpc::WireEntryEncoder,
        }

        unsafe extern "C" fn post_push(
            enc_ptr: *mut c_void,
            id: u32,
            stage: u32,
            stage_prog: u32,
            file_prog: u32,
            start_sec: u32,
            stage_sec: u32,
            filename: *const c_char,
            name: *const c_char,
            dest_dir: *const c_char,
            label: *const c_char,
        ) {
            let ce = &mut *enc_ptr.cast::<CountedEncCtx<'_>>();
            *ce.nr_entries = ce.nr_entries.wrapping_add(1);
            let entry = crate::binrpc::PostEntry {
                id,
                stage,
                stage_progress: stage_prog,
                file_progress: file_prog,
                start_time: start_sec,
                stage_time: stage_sec,
                texts: [
                    crate::binrpc::Text::new(input(filename)),
                    crate::binrpc::Text::new(input(name)),
                    crate::binrpc::Text::new(input(dest_dir)),
                    crate::binrpc::Text::new(input(label)),
                ],
            };
            ce.enc.push(&entry);
        }

        unsafe extern "C" fn hist_push(
            enc_ptr: *mut c_void,
            id: u32,
            kind: u32,
            time: u32,
            size: i64,
            file_cnt: u32,
            par_status: u32,
            script_status: u32,
            url_status: u32,
            name: *const c_char,
        ) {
            let ce = &mut *enc_ptr.cast::<CountedEncCtx<'_>>();
            *ce.nr_entries = ce.nr_entries.wrapping_add(1);
            let size_hi = ((size >> 32) & 0xffff_ffff) as u32;
            let size_lo = (size & 0xffff_ffff) as u32;
            let entry = crate::binrpc::HistoryEntry {
                id,
                kind,
                time,
                size_lo,
                size_hi,
                file_count: file_cnt,
                par_status,
                script_status,
                url_status,
                texts: [crate::binrpc::Text::new(input(name))],
            };
            ce.enc.push(&entry);
        }

        crate::binrpc::serve_binrpc(
            input(e.remote_addr),
            input(e.control_user),
            input(e.control_password),
            input(e.version),
            e.control_port,
            |dst| {
                let Some(cb) = e.recv_cb else {
                    return false;
                };
                cb(e.ctx, dst.as_mut_ptr().cast(), dst.len() as c_int) != 0
            },
            |src| {
                let Some(cb) = e.send_cb else {
                    return false;
                };
                cb(e.ctx, src.as_ptr().cast(), src.len() as c_int) != 0
            },
            |kind, msg| {
                if let Some(cb) = e.log_cb {
                    let c = to_nul_cvec(msg);
                    cb(e.ctx, kind, c.as_ptr());
                }
            },
            |kind, a1, a2, a3, a4, s1, s2, s3, body, ids, names| {
                let Some(cb) = e.action_cb else {
                    return false;
                };
                let c1 = to_nul_cvec(s1);
                let c2 = to_nul_cvec(s2);
                let c3 = to_nul_cvec(s3);
                let mut body_buf: Vec<u8> = Vec::new();
                let (body_ptr, body_len) = if body.is_empty() {
                    (c"".as_ptr(), 0)
                } else {
                    body_buf.reserve_exact(body.len() + 1);
                    body_buf.extend_from_slice(body);
                    body_buf.push(0);
                    (body_buf.as_ptr().cast::<c_char>(), body.len() as c_int)
                };
                let name_cvecs: Vec<std::ffi::CString> =
                    names.iter().map(|n| to_nul_cvec(n)).collect();
                let name_ptrs: Vec<*const c_char> = name_cvecs.iter().map(|c| c.as_ptr()).collect();
                cb(
                    e.ctx,
                    kind,
                    a1,
                    a2,
                    a3,
                    a4,
                    c1.as_ptr(),
                    c2.as_ptr(),
                    c3.as_ptr(),
                    body_ptr,
                    body_len,
                    if ids.is_empty() {
                        std::ptr::null()
                    } else {
                        ids.as_ptr()
                    },
                    ids.len(),
                    if name_ptrs.is_empty() {
                        std::ptr::null()
                    } else {
                        name_ptrs.as_ptr()
                    },
                    name_ptrs.len(),
                ) != 0
            },
            |want_files, want_state, builder| {
                if let Some(cb) = e.collect_list_cb {
                    cb(
                        e.ctx,
                        c_int::from(want_files),
                        c_int::from(want_state),
                        (builder as *mut crate::binrpc::BinRpcListBuilder).cast(),
                        list_push_nzb,
                        list_push_param,
                        list_push_file,
                        list_set_state,
                    );
                }
            },
            |lines, id_from, nr_entries, enc| {
                if let Some(cb) = e.collect_log_cb {
                    let mut lc = LogCollectCtx {
                        lines,
                        id_from,
                        nr_entries,
                        enc,
                    };
                    cb(
                        e.ctx,
                        lines,
                        id_from,
                        (&mut lc as *mut LogCollectCtx<'_>).cast(),
                        log_begin,
                        log_push,
                    );
                }
            },
            |nr_entries, enc| {
                if let Some(cb) = e.collect_post_cb {
                    let mut ce = CountedEncCtx { nr_entries, enc };
                    cb(e.ctx, (&mut ce as *mut CountedEncCtx<'_>).cast(), post_push);
                }
            },
            |show_hidden, nr_entries, enc| {
                if let Some(cb) = e.collect_history_cb {
                    let mut ce = CountedEncCtx { nr_entries, enc };
                    cb(
                        e.ctx,
                        c_int::from(show_hidden),
                        (&mut ce as *mut CountedEncCtx<'_>).cast(),
                        hist_push,
                    );
                }
            },
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_frontend_op(
    op: c_int,
    env: *const crate::ncurses_format::NzbgetRsFrontendEnv,
) -> c_int {
    #[cfg(unix)]
    let _cancel_guard = PosixCancelGuard::disable();
    crate::ncurses_format::frontend_op(op, env)
}

#[cfg(all(test, unix))]
mod conn_op_cancel_tests {
    use super::*;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicI32, Ordering};

    #[test]
    fn conn_op_cancel_touches_only_atomic_slots() {
        let mut pair = [0 as c_int; 2];
        assert_eq!(
            unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, pair.as_mut_ptr()) },
            0
        );
        let sock = AtomicI32::new(pair[0]);
        let status = AtomicI32::new(crate::socket::CS_CONNECTED);
        let dangling_c = NonNull::<c_char>::dangling().as_ptr();
        let dangling_i = NonNull::<c_int>::dangling().as_ptr();
        let dangling_u = NonNull::<usize>::dangling().as_ptr();
        let env = NzbgetRsConnEnv {
            ctx: std::ptr::null_mut(),
            host: dangling_c,
            port: 0,
            ip_version: 0,
            timeout_sec: 0,
            graceful: 0,
            force_close: 0,
            has_deadline: 0,
            resolved_addrs: std::ptr::null(),
            resolved_count: 0,
            socket_atomic: sock.as_ptr(),
            status_atomic: status.as_ptr(),
            is_listener: dangling_i,
            accept_busy: dangling_i,
            tls_error: dangling_i,
            buf_offset: dangling_u,
            buf_avail: dangling_u,
            total_bytes_read: dangling_i,
            read_buf: NonNull::<u8>::dangling().as_ptr(),
            read_buf_cap: 16,
            send_cb: None,
            recv_cb: None,
            error_cb: None,
            debug_cb: None,
            deadline_wait_cb: None,
            deadline_stopped_cb: None,
        };
        let rc = unsafe {
            nzbget_rs_conn_op(
                5,
                &env,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(rc, 1);
        assert_eq!(status.load(Ordering::SeqCst), crate::socket::CS_CANCELLED);
        unsafe {
            libc::close(pair[0]);
            libc::close(pair[1]);
        }
    }
}
