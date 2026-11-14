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
/// backend is null (built-in backend) or points to a complete callback table. Its context transfers
/// to Rust exactly once, including rejected/duplicate starts. Callbacks are
/// thread-safe for the documented single-worker use and must never unwind.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_tls_cipher_start(
    backend: *const tls_cipher_native::Io,
) -> RsBuf {
    // A null table selects the built-in OpenSSL backend.
    let io = if backend.is_null() {
        crate::tlssock::cipher::backend()
    } else {
        unsafe { *backend }
    };
    cipher_response(tls_cipher_native::start(io))
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
#[cfg(test)]
unsafe fn in_place(raw: *mut c_char, f: impl FnOnce(&mut [u8]) -> usize) {
    if raw.is_null() {
        return;
    }
    let len = CStr::from_ptr(raw).to_bytes().len();
    let n = f(std::slice::from_raw_parts_mut(raw.cast::<u8>(), len));
    *raw.add(n) = 0;
}

/// Util::MatchFileExt.
///
/// # Safety
/// The strings are null or NUL-terminated.
#[cfg(test)]
pub(super) unsafe extern "C" fn nzbget_rs_match_file_ext(
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
#[cfg(test)]
pub(super) unsafe extern "C" fn nzbget_rs_file_type(
    which: c_int,
    s: *const c_char,
    len: usize,
) -> c_int {
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
#[cfg(all(unix, test))]
unsafe fn nzbget_rs_force_directories(path: *const c_char, errmsg: *mut RsBuf) -> c_int {
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
            assert_eq!(
                crate::fsdelete::file_extension(b"a.\0b.c"),
                Some(&b".c"[..])
            );
        }
    }

    #[test]
    fn parrenamer_ffi_nulls() {
        let mut is_stopped = || false;
        let mut on_progress = |_: &[u8], _: i32| {};
        let mut on_message = |_: crate::parrenamer::Kind, _: &[u8]| {};
        let mut on_parred = |_: &[u8]| {};
        let mut on_renamed = |_: &[u8], _: &[u8]| {};
        let st = crate::parrenamer::execute_all(
            b"",
            b"",
            b"",
            false,
            b'/',
            &mut is_stopped,
            &mut on_progress,
            &mut on_message,
            &mut on_parred,
            &mut on_renamed,
        );
        assert_eq!(
            (
                st.renamed_count,
                st.has_missed_files,
                st.has_damaged_par_files
            ),
            (0, false, false)
        );
    }

    #[test]
    fn rarrenamer_nulls_and_borrowed_callback_strings() {
        struct Probe(Vec<(Vec<u8>, c_int)>);
        impl crate::rarrenamer::RunCallbacks for Probe {
            fn stopped(&mut self) -> Result<bool, ()> {
                Ok(false)
            }
            fn progress(&mut self, label: &[u8], stage: i32) -> Result<(), ()> {
                self.0.push((label.to_vec(), stage));
                Ok(())
            }
            fn message(&mut self, _kind: i32, _text: &[u8]) -> Result<(), ()> {
                Ok(())
            }
            fn register_renamed(&mut self, _old: &[u8], _new: &[u8]) -> Result<(), ()> {
                Ok(())
            }
        }
        let mut probe = Probe(Vec::new());
        let mut renamed = 0;
        assert!(crate::rarrenamer::execute_all(
            b"/nonexistent_dir_12345",
            b"job",
            b"",
            b"",
            true,
            &mut renamed,
            &mut probe,
        )
        .is_ok());
        assert_eq!(renamed, 0);
        assert_eq!(probe.0, vec![(b"Checkingrar-files for job".to_vec(), 0)]);
    }

    #[test]
    fn parparser_bounded_input_and_outputs() {
        assert!(crate::parparser::parse_par_filename(b"bad", true).is_none());
        assert_eq!(
            crate::parparser::parse_par_filename(b"a.vol+2.par2", true),
            Some((1, -1))
        );
        assert_eq!(
            crate::parparser::parse_par_filename(b"a.par2", true),
            Some((1, 0))
        );
        let mut name = [b'a'; crate::parparser::BSTRING_MAX];
        name[1018..].copy_from_slice(b".par2");
        assert_eq!(
            crate::parparser::parse_par_filename(&name, true),
            Some((1018, 0))
        );
        assert!(crate::parparser::same_par_collection(&name, &name, true));
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
static ACTIVE_QUEUE_COORD: std::sync::RwLock<
    Option<std::sync::Weak<crate::queuecoord::NzbgetRsQueueCoordinator>>,
> = std::sync::RwLock::new(None);

#[cfg(unix)]
pub(crate) fn set_active_queue_coord(
    opt: Option<std::sync::Weak<crate::queuecoord::NzbgetRsQueueCoordinator>>,
) {
    if let Ok(mut g) = ACTIVE_QUEUE_COORD.write() {
        *g = opt;
    }
}

#[cfg(unix)]
pub(crate) fn active_queue_coord(
) -> Option<std::sync::Arc<crate::queuecoord::NzbgetRsQueueCoordinator>> {
    ACTIVE_QUEUE_COORD
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

#[cfg(unix)]
pub(crate) fn queue_host() -> Option<crate::queuesubsys_host::QueueSubsysHost> {
    let cb = BUILD_ENV
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|e| e.queue_host_cb))?;
    Some(crate::queuesubsys_host::QueueSubsysHost::new(
        std::ptr::null_mut(),
        cb,
    ))
}

#[cfg(unix)]
pub(crate) fn active_queue_coord_and_host() -> Option<(
    std::sync::Arc<crate::queuecoord::NzbgetRsQueueCoordinator>,
    crate::queuesubsys_host::QueueSubsysHost,
)> {
    let arc = active_queue_coord()?;
    let qh = queue_host()?;
    Some((arc, qh))
}

#[cfg(unix)]
pub(crate) fn log_active_queue_coord_debug_info() {
    let Some((qc, qh)) = active_queue_coord_and_host() else {
        return;
    };
    qc.log_debug_info(&qh);
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
// ScriptConfig & ScriptController FFI
// (`rust/src/statmeter.rs` / `rust/src/configfile.rs` / `rust/src/procrun.rs`)
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
// SystemHealth (`rust/src/systemhealth.rs`)
// ============================================================================

#[cfg(unix)]
pub(crate) fn system_health_diagnose_bytes(format_mode: c_int) -> Vec<u8> {
    let Some(opts) = crate::options::active_options() else {
        return Vec::new();
    };
    let pool = crate::net::active_server_pool();
    let feed_coord = crate::feed::active_feed_coord();
    let sched = crate::scheduler::active_scheduler();
    let log = crate::log::active_log();
    let daemon_mode = opts.daemon_mode();
    let sc = opts.scalars();
    let to_path = |b: &[u8]| crate::cleanup::bytes_to_path_buf(b);

    let main_p = to_path(opts.str_bytes(2));
    let dest_p = to_path(opts.str_bytes(3));
    let inter_p = to_path(opts.str_bytes(4));
    let nzb_p = to_path(opts.str_bytes(7));
    let queue_p = to_path(opts.str_bytes(6));
    let web_p = to_path(opts.str_bytes(8));
    let temp_p = to_path(opts.str_bytes(5));
    let cfg_tpl_p = to_path(opts.str_bytes(9));
    let log_bytes = log
        .as_ref()
        .map_or_else(Vec::new, |l| l.log_filename_bytes());
    let log_p = to_path(&log_bytes);
    let opt_log_p = to_path(opts.str_bytes(27));
    let cert_bytes = opts.str_bytes(23);
    let cert_p = to_path(cert_bytes);
    let cfg_p = to_path(opts.str_bytes(0));
    let lock_p = to_path(opts.str_bytes(25));
    let secure_cert_p = to_path(opts.str_bytes(21));
    let secure_key_p = to_path(opts.str_bytes(22));
    let unrar_p = to_path(opts.str_bytes(38));
    let seven_zip_p = to_path(opts.str_bytes(39));
    let pass_p = to_path(opts.str_bytes(31));

    let raw_servers = pool
        .as_ref()
        .map_or_else(Vec::new, |p| p.collect_health_servers());
    let servers: Vec<crate::systemhealth::HealthServerInput<'_>> = raw_servers
        .iter()
        .map(|s| crate::systemhealth::HealthServerInput {
            id: s.id,
            section: crate::systemhealth::NewsServerSectionInput {
                active: s.active,
                name: s.name.as_slice(),
                level: s.level,
                optional: s.optional,
                group: s.group,
                host: s.host.as_slice(),
                port: s.port,
                username: s.username.as_slice(),
                password: s.password.as_slice(),
                tls: s.tls,
                cipher: s.cipher.as_slice(),
                max_connections: s.max_connections,
                retention: s.retention,
                cert_level: s.cert_level,
                ip_version: s.ip_version,
                dupe_search: sc.dupe_search != 0,
            },
        })
        .collect();

    let tasks = sched
        .as_ref()
        .map_or_else(Vec::new, |s| s.collect_health_tasks());

    let categories: Vec<crate::systemhealth::HealthCategoryInput<'_>> = opts
        .categories()
        .iter()
        .map(|cat| crate::systemhealth::HealthCategoryInput {
            name: cat.name.to_bytes(),
            unpack: cat.unpack,
        })
        .collect();

    let raw_feeds = feed_coord
        .as_ref()
        .map_or_else(Vec::new, |fc| fc.collect_health_feeds());
    let feeds: Vec<crate::systemhealth::HealthFeedInput<'_>> = raw_feeds
        .iter()
        .map(
            |(id, name, url, interval, cert_level)| crate::systemhealth::HealthFeedInput {
                id: *id,
                name: name.as_slice(),
                url: url.as_slice(),
                interval: *interval,
                cert_level: *cert_level,
            },
        )
        .collect();

    let diag_input = crate::systemhealth::SystemHealthDiagnoseInput {
        paths: crate::systemhealth::PathsSectionInput {
            main_dir: &main_p,
            dest_dir: &dest_p,
            inter_dir: &inter_p,
            nzb_dir: &nzb_p,
            queue_dir: &queue_p,
            web_dir: &web_p,
            temp_dir: &temp_p,
            config_template: &cfg_tpl_p,
            log_file: &log_p,
            cert_store: &cert_p,
            cert_store_path: &cert_p,
            config_file: &cfg_p,
            lock_file: &lock_p,
            write_log: sc.write_log,
            cert_check: sc.cert_check != 0,
            daemon_mode,
        },
        servers: &servers,
        output_mode: sc.output_mode,
        update_interval: sc.update_interval,
        curses_nzb_name: sc.curses_nzb_name != 0,
        curses_group: sc.curses_group != 0,
        curses_time: sc.curses_time != 0,
        rotate_log: sc.rotate_log,
        log_buffer: sc.log_buffer,
        crash_dump: sc.crash_dump != 0,
        time_correction: sc.time_correction,
        article_retries: sc.article_retries,
        hc_dupe: sc.health_check == 4,
        dupe_search: sc.dupe_search != 0,
        article_interval: sc.article_interval,
        article_timeout: sc.article_timeout,
        article_read_chunk_size: sc.article_read_chunk_size,
        url_retries: sc.url_retries,
        url_interval: sc.url_interval,
        url_timeout: sc.url_timeout,
        url_connections: sc.url_connections,
        remote_timeout: sc.remote_timeout,
        download_rate: sc.download_rate,
        monthly_quota: sc.monthly_quota,
        quota_start_day: sc.quota_start_day,
        daily_quota: sc.daily_quota,
        flush_queue: sc.flush_queue != 0,
        continue_partial: sc.continue_partial != 0,
        propagation_delay: sc.propagation_delay,
        article_cache: sc.article_cache,
        direct_write: sc.direct_write != 0,
        write_buffer: sc.write_buffer,
        file_naming: sc.file_naming,
        post_strategy: sc.post_strategy,
        disk_space: sc.disk_space,
        nzb_cleanup_disk: sc.nzb_cleanup_disk != 0,
        keep_history: sc.keep_history,
        feed_history: sc.feed_history,
        skip_write: sc.skip_write != 0,
        raw_article: sc.raw_article != 0,
        security: crate::systemhealth::SecuritySectionInput {
            control_ip: opts.str_bytes(14),
            control_port: sc.control_port,
            control_username: opts.str_bytes(15),
            control_password: opts.str_bytes(16),
            restricted_username: opts.str_bytes(17),
            restricted_password: opts.str_bytes(18),
            add_username: opts.str_bytes(19),
            add_password: opts.str_bytes(20),
            form_auth: sc.form_auth != 0,
            secure_control: sc.secure_control != 0,
            secure_cert: &secure_cert_p,
            secure_key: &secure_key_p,
            secure_port: sc.secure_port,
            cert_check: sc.cert_check != 0,
            cert_store_path_empty: cert_bytes.is_empty(),
            daemon_mode,
            daemon_username: opts.str_bytes(26),
            umask: sc.umask,
        },
        append_category_dir: sc.append_category_dir != 0,
        nzb_dir_interval: sc.nzb_dir_interval,
        nzb_dir_file_age: sc.nzb_dir_file_age,
        dupe_check: sc.dupe_check != 0,
        health_check: sc.health_check,
        crc_check: sc.crc_check != 0,
        par_check: sc.par_check,
        par_repair: sc.par_repair != 0,
        par_quick: sc.par_quick != 0,
        par_buffer: sc.par_buffer,
        par_threads: sc.par_threads,
        hw_threads: std::thread::available_parallelism().map_or(0, usize::from),
        par_rename: sc.par_rename != 0,
        rar_rename: sc.rar_rename != 0,
        direct_rename: sc.direct_rename != 0,
        hard_linking: sc.hard_linking != 0,
        par_time_limit: sc.par_time_limit,
        unpack: sc.unpack != 0,
        direct_unpack: sc.direct_unpack != 0,
        unpack_pause_queue: sc.unpack_pause_queue != 0,
        unrar_path: &unrar_p,
        seven_zip_path: &seven_zip_p,
        ext_cleanup_disk: opts.str_bytes(32),
        unpack_pass_file: &pass_p,
        unpack_ignore_ext: opts.str_bytes(35),
        options_log_file: &opt_log_p,
        tasks: &tasks,
        categories: &categories,
        feeds: &feeds,
    };

    crate::systemhealth::diagnose_system_health(&diag_input, format_mode, |sev, msg| {
        if let Some(lg) = log.as_deref() {
            if sev == 2 {
                lg.emit_bytes(2, Some(&opts), msg);
            } else if sev == 1 {
                lg.emit_bytes(1, Some(&opts), msg);
            }
        }
    })
}

// ============================================================================
// NzbFileParser, UrlCoordinator & DirectRenamer End-to-End FFI
// ============================================================================

// ============================================================================
// DownloadInfo, DupeProbe & DupeCoordinator End-to-End FFI
// ============================================================================

// =========================================================================
// Consolidated RepairController (`repair.rs`), ContentMap (`streamrepair.rs`),
// and TwinCheck (`twin.rs`) FFI
// =========================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_alloc_buf(data: *const c_char, len: usize) -> RsBuf {
    into_buf(bytes(data, len).to_vec())
}

#[cfg(unix)]
struct BuildEnvRuntime {
    ssl_lib_name: CString,
    ssl_raw_version: CString,
    tls_host_cb: crate::net::NzbgetRsConnHostFn,
    write_stdout_cb: Option<unsafe extern "C" fn(*const c_char, usize, c_int)>,
    queue_host_cb: Option<crate::queuesubsys_host::NzbgetRsQueueSubsysHostFn>,
}

static BUILD_FLAGS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static BUILD_VERSION: std::sync::OnceLock<CString> = std::sync::OnceLock::new();
static QUEUE_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(unix)]
static BUILD_ENV: std::sync::RwLock<Option<BuildEnvRuntime>> = std::sync::RwLock::new(None);

#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_build_env_init(
    flags: c_int,
    version: *const c_char,
    ssl_lib_name: *const c_char,
    ssl_raw_version: *const c_char,
    tls_host_cb: crate::net::NzbgetRsConnHostFn,
    write_stdout_cb: Option<unsafe extern "C" fn(*const c_char, usize, c_int)>,
    queue_host_cb: Option<crate::queuesubsys_host::NzbgetRsQueueSubsysHostFn>,
) {
    BUILD_FLAGS.store(flags, std::sync::atomic::Ordering::SeqCst);
    let _ = BUILD_VERSION.set(to_nul_cvec(input(version)));
    if let Ok(mut g) = BUILD_ENV.write() {
        *g = Some(BuildEnvRuntime {
            ssl_lib_name: to_nul_cvec(input(ssl_lib_name)),
            ssl_raw_version: to_nul_cvec(input(ssl_raw_version)),
            tls_host_cb,
            write_stdout_cb,
            queue_host_cb,
        });
    }
}

pub(crate) fn build_debug_enabled() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 1) != 0
}

pub(crate) fn build_tls_disabled() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 2) != 0
}

pub(crate) fn build_gzip_disabled() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 4) != 0
}

fn build_ssl_extract_after_space() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 8) != 0
}

pub(crate) fn build_parcheck_enabled() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 16) == 0
}

pub(crate) fn build_curses_disabled() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 32) != 0
}

pub(crate) fn is_queue_loaded() -> bool {
    QUEUE_LOADED.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(unix)]
pub(crate) fn script_host_cb() -> Option<crate::procrun::NzbgetRsScriptHostFn> {
    Some(crate::procrun::active_script_host_cb)
}

pub(crate) fn build_version_cstr() -> &'static CStr {
    BUILD_VERSION.get().map_or(c"", CString::as_c_str)
}

pub(crate) fn build_version_bytes() -> &'static [u8] {
    build_version_cstr().to_bytes()
}

/// Connection host callback: TLS, deadlines, stop checks and logging, all in Rust.
#[cfg(unix)]
pub(crate) fn tls_host_cb() -> crate::net::NzbgetRsConnHostFn {
    Some(crate::tlssock::conn_host)
}

/// The C++ callback registered at start-up. It now answers only operation 2:
/// whether the current C++ `Thread` was stopped.
#[cfg(unix)]
pub(crate) fn cpp_thread_host_cb() -> crate::net::NzbgetRsConnHostFn {
    BUILD_ENV
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|e| e.tls_host_cb))
}

#[cfg(unix)]
#[no_mangle]
pub extern "C" fn nzbget_rs_tls_init() {
    crate::tlssock::init();
}

/// # Safety
/// cert_store is null or a NUL-terminated path; empty or null loads no store.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_tls_init_options(cert_store: *const c_char) {
    let path = if cert_store.is_null() {
        &[][..]
    } else {
        unsafe { CStr::from_ptr(cert_store) }.to_bytes()
    };
    crate::tlssock::init_options(path);
}

#[cfg(unix)]
#[no_mangle]
pub extern "C" fn nzbget_rs_tls_final() {
    crate::tlssock::final_cleanup();
}

#[cfg(unix)]
#[no_mangle]
pub extern "C" fn nzbget_rs_tls_thread_stop() {
    if !build_tls_disabled() {
        crate::tlssock::stop_ssl_thread();
    }
}

#[cfg(unix)]
pub(crate) fn stop_ssl_thread() {
    if !build_tls_disabled() {
        crate::tlssock::stop_ssl_thread();
    }
}

#[cfg(unix)]
pub(crate) fn write_stdout_bytes(bytes: &[u8], flush: bool) {
    let cb = BUILD_ENV
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|e| e.write_stdout_cb));
    if let Some(f) = cb {
        unsafe {
            f(
                if bytes.is_empty() {
                    std::ptr::null()
                } else {
                    bytes.as_ptr().cast()
                },
                bytes.len(),
                c_int::from(flush),
            );
        }
    }
}

static THREAD_COUNT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);

std::thread_local! {
    static TL_RUST_THREAD_STOPPED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn current_thread_count() -> c_int {
    THREAD_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

pub(crate) fn is_rust_thread_stopped() -> bool {
    TL_RUST_THREAD_STOPPED.with(|c| {
        let ptr = c.get();
        if ptr == 0 {
            false
        } else {
            unsafe {
                (&*(ptr as *const std::sync::atomic::AtomicBool))
                    .load(std::sync::atomic::Ordering::SeqCst)
            }
        }
    })
}

pub(crate) struct RustThreadGuard {
    prev: usize,
}

impl RustThreadGuard {
    pub(crate) fn enter(stopped: &std::sync::atomic::AtomicBool) -> Self {
        THREAD_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let ptr = (stopped as *const std::sync::atomic::AtomicBool) as usize;
        let prev = TL_RUST_THREAD_STOPPED.with(|c| c.replace(ptr));
        Self { prev }
    }
}

impl Drop for RustThreadGuard {
    fn drop(&mut self) {
        TL_RUST_THREAD_STOPPED.with(|c| c.set(self.prev));
        THREAD_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_thread_count_op(op: c_int) -> c_int {
    match op {
        1 => THREAD_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1,
        -1 => THREAD_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) - 1,
        2 => c_int::from(is_rust_thread_stopped()),
        3 => {
            QUEUE_LOADED.store(true, std::sync::atomic::Ordering::SeqCst);
            1
        }
        4 => {
            QUEUE_LOADED.store(false, std::sync::atomic::Ordering::SeqCst);
            0
        }
        5 => c_int::from(is_queue_loaded()),
        _ => current_thread_count(),
    }
}

#[cfg(unix)]
pub(crate) unsafe fn format_system_info_bytes(mode: c_int, with_network: bool) -> Vec<u8> {
    let conn_host_cb = tls_host_cb();
    let (ssl_lib, ssl_raw) = BUILD_ENV
        .read()
        .ok()
        .and_then(|g| {
            g.as_ref().map(|e| {
                (
                    e.ssl_lib_name.to_bytes().to_vec(),
                    e.ssl_raw_version.to_bytes().to_vec(),
                )
            })
        })
        .unwrap_or_default();
    let (pub_ip, priv_ip) = if with_network {
        match crate::systemhealth::system_info_fetch_network(
            "ip.nzbget.com",
            !build_tls_disabled(),
            !build_gzip_disabled(),
            |fd, host_cstr, req| {
                let Some(cb) = conn_host_cb else {
                    return Err("TLS callback missing".to_string());
                };
                let suppress = std::sync::atomic::AtomicBool::new(true);
                let ptrs: [usize; 2] = [
                    0,
                    (&suppress as *const std::sync::atomic::AtomicBool) as usize,
                ];
                let mut out_int = [0i32; 2];
                let mut empty_cipher = [0 as c_char; 1];
                let tls_ptr = cb(
                    std::ptr::null_mut(),
                    4,
                    fd,
                    1,
                    ptrs.as_ptr() as usize,
                    host_cstr.as_ptr(),
                    c"".as_ptr(),
                    c"".as_ptr(),
                    empty_cipher.as_mut_ptr(),
                    out_int.as_mut_ptr(),
                ) as usize;
                if tls_ptr == 0 || out_int[0] == 0 {
                    if tls_ptr != 0 {
                        cb(
                            std::ptr::null_mut(),
                            7,
                            1,
                            0,
                            tls_ptr,
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        );
                    }
                    stop_ssl_thread();
                    return Err("TLS handshake failed".to_string());
                }
                let mut written = 0usize;
                while written < req.len() {
                    let n = cb(
                        std::ptr::null_mut(),
                        5,
                        (req.len() - written) as c_int,
                        0,
                        tls_ptr,
                        req[written..].as_ptr().cast(),
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                    if n <= 0 {
                        break;
                    }
                    written += n as usize;
                }
                let mut raw_resp = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = cb(
                        std::ptr::null_mut(),
                        6,
                        chunk.len() as c_int,
                        0,
                        tls_ptr,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        chunk.as_mut_ptr().cast(),
                        std::ptr::null_mut(),
                    );
                    if n <= 0 {
                        break;
                    }
                    raw_resp.extend_from_slice(&chunk[..(n as usize)]);
                }
                cb(
                    std::ptr::null_mut(),
                    7,
                    1,
                    0,
                    tls_ptr,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                stop_ssl_thread();
                Ok(raw_resp)
            },
        ) {
            Ok((pub_v, priv_v)) => (pub_v, priv_v),
            Err(err) => {
                if !err.is_empty() {
                    let mut m = b"Failed to get public and private IP: ".to_vec();
                    m.extend_from_slice(err.as_bytes());
                    crate::log::log_active_bytes(4, &m);
                }
                (Vec::new(), Vec::new())
            }
        }
    } else {
        (Vec::new(), Vec::new())
    };
    crate::systemhealth::system_info_format(
        mode,
        &[],
        &[],
        &[],
        &ssl_lib,
        &ssl_raw,
        build_ssl_extract_after_space(),
        &pub_ip,
        &priv_ip,
    )
}

pub struct NzbgetRsWorkState {
    resume_time: std::sync::atomic::AtomicI64,
    local_time_offset: std::sync::atomic::AtomicI32,
    speed_limit: std::sync::atomic::AtomicI32,
    temp_pause_download: std::sync::atomic::AtomicBool,
    fleet_hold: std::sync::atomic::AtomicI32,
    temp_pause_postprocess: std::sync::atomic::AtomicBool,
    pause_download: std::sync::atomic::AtomicBool,
    pause_post_process: std::sync::atomic::AtomicBool,
    pause_scan: std::sync::atomic::AtomicBool,
    pause_frontend: std::sync::atomic::AtomicBool,
    downloading: std::sync::atomic::AtomicBool,
    quota_reached: std::sync::atomic::AtomicBool,
    wait_lock: (std::sync::Mutex<u64>, std::sync::Condvar),
}

static ACTIVE_WORKSTATE: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsWorkState>>> =
    std::sync::RwLock::new(None);

pub(crate) fn active_workstate() -> Option<std::sync::Arc<NzbgetRsWorkState>> {
    ACTIVE_WORKSTATE
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

impl NzbgetRsWorkState {
    fn notify(&self) {
        self.wake_waiters();
        #[cfg(unix)]
        {
            crate::scheduler::wake_active_disk_service();
            crate::feed::notify_active_feed_work_state();
            crate::queuecoord::notify_from_workstate();
        }
    }

    pub(crate) fn wake_waiters(&self) {
        let mut g = self.wait_lock.0.lock().unwrap_or_else(|e| e.into_inner());
        *g = g.wrapping_add(1);
        self.wait_lock.1.notify_all();
    }

    pub(crate) fn wait_for_change(&self, timeout_ms: u64) {
        let g = self.wait_lock.0.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self
            .wait_lock
            .1
            .wait_timeout(g, std::time::Duration::from_millis(timeout_ms));
    }

    pub(crate) fn pause_download(&self) -> bool {
        self.pause_download
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn pause_post_process(&self) -> bool {
        self.pause_post_process
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn pause_scan(&self) -> bool {
        self.pause_scan.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn pause_frontend(&self) -> bool {
        self.pause_frontend
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn downloading(&self) -> bool {
        self.downloading.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn quota_reached(&self) -> bool {
        self.quota_reached.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn temp_pause_download(&self) -> bool {
        self.temp_pause_download
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn temp_pause_postprocess(&self) -> bool {
        self.temp_pause_postprocess
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn download_held(&self) -> bool {
        self.temp_pause_download
            .load(std::sync::atomic::Ordering::SeqCst)
            || self.fleet_hold.load(std::sync::atomic::Ordering::SeqCst) > 0
    }

    pub(crate) fn speed_limit(&self) -> i32 {
        self.speed_limit.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn resume_time(&self) -> i64 {
        self.resume_time.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn local_time_offset(&self) -> i32 {
        self.local_time_offset
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn set_pause_download(&self, val: bool) {
        self.pause_download
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_pause_post_process(&self, val: bool) {
        self.pause_post_process
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_pause_scan(&self, val: bool) {
        self.pause_scan
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_temp_pause_download(&self, val: bool) {
        self.temp_pause_download
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_temp_pause_postprocess(&self, val: bool) {
        self.temp_pause_postprocess
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_speed_limit(&self, val: i32) {
        self.speed_limit
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_resume_time(&self, val: i64) {
        self.resume_time
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_local_time_offset(&self, val: i32) {
        self.local_time_offset
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_quota_reached(&self, val: bool) {
        self.quota_reached
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_downloading(&self, val: bool) {
        self.downloading
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn hold_download_for_fleet(&self, hold: bool) {
        self.fleet_hold.fetch_add(
            if hold { 1 } else { -1 },
            std::sync::atomic::Ordering::SeqCst,
        );
        self.notify();
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_workstate_new() -> *mut NzbgetRsWorkState {
    let arc = std::sync::Arc::new(NzbgetRsWorkState {
        resume_time: std::sync::atomic::AtomicI64::new(0),
        local_time_offset: std::sync::atomic::AtomicI32::new(0),
        speed_limit: std::sync::atomic::AtomicI32::new(0),
        temp_pause_download: std::sync::atomic::AtomicBool::new(true),
        fleet_hold: std::sync::atomic::AtomicI32::new(0),
        temp_pause_postprocess: std::sync::atomic::AtomicBool::new(true),
        pause_download: std::sync::atomic::AtomicBool::new(false),
        pause_post_process: std::sync::atomic::AtomicBool::new(false),
        pause_scan: std::sync::atomic::AtomicBool::new(false),
        pause_frontend: std::sync::atomic::AtomicBool::new(false),
        downloading: std::sync::atomic::AtomicBool::new(false),
        quota_reached: std::sync::atomic::AtomicBool::new(false),
        wait_lock: (std::sync::Mutex::new(0), std::sync::Condvar::new()),
    });
    if let Ok(mut g) = ACTIVE_WORKSTATE.write() {
        *g = Some(std::sync::Arc::downgrade(&arc));
    }
    std::sync::Arc::into_raw(arc).cast_mut()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_workstate_free(ws: *mut NzbgetRsWorkState) {
    if !ws.is_null() {
        let arc = std::sync::Arc::from_raw(ws.cast_const());
        let wk = std::sync::Arc::downgrade(&arc);
        if let Ok(mut g) = ACTIVE_WORKSTATE.write() {
            if g.as_ref()
                .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
            {
                *g = None;
            }
        }
        drop(arc);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_workstate_ctl(
    ws: *const NzbgetRsWorkState,
    id: c_int,
    write: c_int,
    val: i64,
) -> i64 {
    let Some(s) = ws.as_ref() else {
        return 0;
    };
    use std::sync::atomic::Ordering::SeqCst;
    if write == 0 {
        return match id {
            0 => i64::from(s.pause_download.load(SeqCst)),
            1 => i64::from(s.pause_post_process.load(SeqCst)),
            2 => i64::from(s.pause_scan.load(SeqCst)),
            3 => i64::from(s.temp_pause_download.load(SeqCst)),
            4 => i64::from(s.download_held()),
            5 => i64::from(s.temp_pause_postprocess.load(SeqCst)),
            6 => i64::from(s.pause_frontend.load(SeqCst)),
            7 => s.speed_limit.load(SeqCst) as i64,
            8 => s.resume_time.load(SeqCst),
            9 => s.local_time_offset.load(SeqCst) as i64,
            10 => i64::from(s.quota_reached.load(SeqCst)),
            11 => i64::from(s.downloading.load(SeqCst)),
            _ => 0,
        };
    }
    match id {
        0 => s.pause_download.store(val != 0, SeqCst),
        1 => s.pause_post_process.store(val != 0, SeqCst),
        2 => s.pause_scan.store(val != 0, SeqCst),
        3 => s.temp_pause_download.store(val != 0, SeqCst),
        4 => {
            s.fleet_hold
                .fetch_add(if val != 0 { 1 } else { -1 }, SeqCst);
        }
        5 => s.temp_pause_postprocess.store(val != 0, SeqCst),
        7 => s.speed_limit.store(val as i32, SeqCst),
        8 => s.resume_time.store(val, SeqCst),
        9 => s.local_time_offset.store(val as i32, SeqCst),
        10 => s.quota_reached.store(val != 0, SeqCst),
        11 => s.downloading.store(val != 0, SeqCst),
        _ => return 0,
    }
    s.notify();
    0
}

static ACTIVE_SCRIPT_TEMPLATES: std::sync::Mutex<Vec<CString>> = std::sync::Mutex::new(Vec::new());

fn load_script_template_cstrings() -> Option<Vec<CString>> {
    let opts = crate::options::active_options()?;
    let path_bytes = opts.str_bytes(9);
    if path_bytes.is_empty() {
        return None;
    }
    let raw = std::fs::read(crate::cleanup::bytes_to_path_buf(path_bytes)).ok()?;
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let mut cbuf = Vec::with_capacity(end + 1);
    cbuf.extend_from_slice(&raw[..end]);
    cbuf.push(0);
    let cs = unsafe { CString::from_vec_with_nul_unchecked(cbuf) };
    Some(vec![cs])
}

#[cfg(unix)]
pub(crate) unsafe fn script_config_emit_templates(
    load_descriptions: bool,
    emit_ctx: *mut c_void,
    emit_cb: Option<crate::xmlrpc_format::NzbgetRsXmlRpcEmitFn>,
) -> c_int {
    let Some(emit) = emit_cb else {
        return 0;
    };
    if load_descriptions {
        let Some(loaded) = load_script_template_cstrings() else {
            return 0;
        };
        let ptrs: Vec<*const c_char> = loaded.iter().map(|c| c.as_ptr()).collect();
        emit(
            emit_ctx,
            12,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            ptrs.as_ptr().cast(),
            ptrs.len(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
        );
        1
    } else {
        let Ok(g) = ACTIVE_SCRIPT_TEMPLATES.lock() else {
            return 0;
        };
        let ptrs: Vec<*const c_char> = g.iter().map(|c| c.as_ptr()).collect();
        emit(
            emit_ctx,
            12,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            ptrs.as_ptr().cast(),
            ptrs.len(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
        );
        1
    }
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_script_config_exec(
    op: c_int,
    p1: *const c_char,
    n1: usize,
    p2: *const c_char,
    n2: usize,
    ctx: *mut std::ffi::c_void,
    cancel_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    out_buf: *mut RsBuf,
) -> c_int {
    match op {
        0 => match load_script_template_cstrings() {
            Some(v) => {
                if let Ok(mut g) = ACTIVE_SCRIPT_TEMPLATES.lock() {
                    *g = v;
                }
                1
            }
            None => {
                #[cfg(unix)]
                crate::log::log_active_bytes(2, b"Could not read configuration templates");
                0
            }
        },
        1 => {
            if out_buf.is_null() {
                return 0;
            }
            let Some(opts) = crate::options::active_options() else {
                return 0;
            };
            let fn_bytes = opts.str_bytes(0);
            if fn_bytes.is_empty() {
                return 0;
            }
            match crate::configfile::script_config_read_cipher_snapshot(fn_bytes) {
                Some(buf) => {
                    *out_buf = into_buf(buf);
                    1
                }
                None => 0,
            }
        }
        2 => {
            let Some(opts) = crate::options::active_options() else {
                return -1;
            };
            let fn_bytes = opts.str_bytes(0);
            if fn_bytes.is_empty() {
                return -1;
            }
            let expected = bytes(p1, n1);
            let replacement = bytes(p2, n2);
            let commit = crate::configfile::script_config_commit_cipher_patch(
                fn_bytes,
                expected,
                replacement,
                || cancel_cb.is_some_and(|cb| unsafe { cb(ctx) != 0 }),
                || {
                    #[cfg(unix)]
                    crate::log::log_active_bytes(
                        1,
                        b"TLS cipher settings were replaced, but the configuration directory could not be synchronized",
                    );
                },
            );
            match commit {
                crate::configfile::RsCipherCommit::Saved => 1,
                crate::configfile::RsCipherCommit::Unchanged => 0,
                crate::configfile::RsCipherCommit::Conflict => -2,
                crate::configfile::RsCipherCommit::Cancelled => -3,
                crate::configfile::RsCipherCommit::Failed => -1,
            }
        }
        _ => 0,
    }
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

#[cfg(unix)]
static ACTIVE_PREPOST: std::sync::RwLock<
    Option<std::sync::Weak<crate::prepost::NzbgetRsPrePostProcessor>>,
> = std::sync::RwLock::new(None);

#[cfg(unix)]
pub(crate) fn active_prepost() -> Option<std::sync::Arc<crate::prepost::NzbgetRsPrePostProcessor>> {
    ACTIVE_PREPOST
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_prepost_new(
    owner: usize,
) -> *mut crate::prepost::NzbgetRsPrePostProcessor {
    let arc = std::sync::Arc::new(crate::prepost::NzbgetRsPrePostProcessor::new(owner));
    if let Ok(mut g) = ACTIVE_PREPOST.write() {
        *g = Some(std::sync::Arc::downgrade(&arc));
    }
    std::sync::Arc::into_raw(arc).cast_mut()
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_prepost_free(
    ptr: *mut crate::prepost::NzbgetRsPrePostProcessor,
) {
    if !ptr.is_null() {
        let arc = std::sync::Arc::from_raw(ptr.cast_const());
        let wk = std::sync::Arc::downgrade(&arc);
        if let Ok(mut g) = ACTIVE_PREPOST.write() {
            if g.as_ref()
                .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
            {
                *g = None;
            }
        }
        drop(arc);
    }
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_prepost_exec(
    ptr: *const crate::prepost::NzbgetRsPrePostProcessor,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    p3: usize,
    s1: *const c_char,
    host_ctx: *mut c_void,
    host_cb: Option<crate::queuesubsys_host::NzbgetRsQueueSubsysHostFn>,
) -> usize {
    let (Some(proc_ref), Some(cb)) = (ptr.as_ref(), host_cb) else {
        return 0;
    };
    let _cancel_guard = PosixCancelGuard::disable();
    proc_ref.exec(op, a1, a2, p1, p2, p3, s1, host_ctx, cb)
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_par_checker_new() -> *mut crate::postproc::NzbgetRsParChecker {
    Box::into_raw(Box::new(crate::postproc::NzbgetRsParChecker::new()))
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_par_checker_free(
    ctrl: *mut crate::postproc::NzbgetRsParChecker,
) {
    if !ctrl.is_null() {
        drop(Box::from_raw(ctrl));
    }
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_par_checker_exec(
    ctrl: *const crate::postproc::NzbgetRsParChecker,
    op: c_int,
    a1: c_int,
    s1: *const c_char,
    host_cb: Option<crate::repair::NzbgetRsRepairCtrlHostFn>,
    ctx: *mut c_void,
) -> c_int {
    let _cancel_guard = PosixCancelGuard::disable();
    crate::postproc::par_checker_exec(ctrl, op, a1, s1, host_cb, ctx)
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_direct_unpack_new(
    nzb_id: c_int,
    thread_ptr: usize,
) -> *mut crate::postproc::NzbgetRsDirectUnpack {
    Box::into_raw(Box::new(crate::postproc::NzbgetRsDirectUnpack::new(
        nzb_id, thread_ptr,
    )))
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_direct_unpack_free(
    ctrl: *mut crate::postproc::NzbgetRsDirectUnpack,
) {
    if !ctrl.is_null() {
        drop(Box::from_raw(ctrl));
    }
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_direct_unpack_exec(
    ctrl: *const crate::postproc::NzbgetRsDirectUnpack,
    op: c_int,
    p1: usize,
    p2: usize,
) -> c_int {
    let Some(c) = ctrl.as_ref() else {
        return 0;
    };
    let _cancel_guard = PosixCancelGuard::disable();
    c.exec_active(op, p1, p2)
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_post_job_new(
    kind: c_int,
    thread_ptr: usize,
    post_ptr: usize,
    nzb_id: c_int,
) -> *mut crate::postproc::NzbgetRsPostJob {
    Box::into_raw(Box::new(crate::postproc::NzbgetRsPostJob::new(
        kind, thread_ptr, post_ptr, nzb_id,
    )))
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_post_job_free(job: *mut crate::postproc::NzbgetRsPostJob) {
    if !job.is_null() {
        drop(Box::from_raw(job));
    }
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_post_job_op(
    job: *const crate::postproc::NzbgetRsPostJob,
    op: c_int,
    a1: c_int,
    p1: usize,
) -> c_int {
    let _cancel_guard = PosixCancelGuard::disable();
    match job.as_ref() {
        Some(j) => j.exec(op, a1, p1),
        None => 0,
    }
}

// The `libc` crate exports these only for Apple targets; the values differ per platform.
#[cfg(all(unix, target_vendor = "apple"))]
const PTHREAD_CANCEL_ENABLE: c_int = 1;
#[cfg(all(unix, target_vendor = "apple"))]
const PTHREAD_CANCEL_DISABLE: c_int = 0;
#[cfg(all(unix, not(target_vendor = "apple")))]
const PTHREAD_CANCEL_ENABLE: c_int = 0;
#[cfg(all(unix, not(target_vendor = "apple")))]
const PTHREAD_CANCEL_DISABLE: c_int = 1;

#[cfg(unix)]
pub(crate) struct PosixCancelGuard {
    old_state: c_int,
}

#[cfg(unix)]
impl PosixCancelGuard {
    pub(crate) unsafe fn disable() -> Self {
        extern "C" {
            fn pthread_setcancelstate(state: c_int, oldstate: *mut c_int) -> c_int;
        }
        let mut old_state = PTHREAD_CANCEL_ENABLE;
        pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &mut old_state);
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

// ---- WebProcessor / BinRpcProcessor / RemoteClient / Frontend FFI ----

unsafe fn to_nul_cvec(bytes: &[u8]) -> std::ffi::CString {
    let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::ffi::CString::from_vec_unchecked(bytes[..nul].to_vec())
}

#[cfg(unix)]
unsafe extern "C" fn web_rpc_log_bridge(_ctx: *mut c_void, lvl: c_int, msg: *const c_char) {
    if lvl == 0 && !build_debug_enabled() {
        return;
    }
    let kind = match lvl {
        0 => 3,
        1 => 1,
        2 => 2,
        3 => 0,
        _ => 4,
    };
    crate::log::log_active_bytes(kind, input(msg));
}

#[cfg(unix)]
unsafe extern "C" fn web_rpc_action_bridge(
    _ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    a3: c_int,
    a4: c_int,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    body: *const c_char,
    body_len: usize,
    ids: *const i32,
    ids_len: usize,
    params: *const crate::xmlrpc_format::NzbgetRsRpcParamItem,
    params_len: usize,
    out_u32: *mut u32,
    out_f64: *mut f64,
    out_buf: *mut RsBuf,
) -> c_int {
    match op {
        1 => {
            let Some(ws) = active_workstate() else {
                return 0;
            };
            if a1 == 0 {
                let prev = c_int::from(ws.pause_download());
                ws.set_pause_download(true);
                return prev;
            }
            ws.set_pause_download(a2 != 0);
            0
        }
        2 => {
            let Some(ws) = active_workstate() else {
                return 0;
            };
            ws.set_resume_time(0);
            match a2 {
                0 => ws.set_pause_download(a1 != 0),
                1 => ws.set_pause_post_process(a1 != 0),
                2 => ws.set_pause_scan(a1 != 0),
                _ => return 0,
            }
            1
        }
        3 => {
            if let Some(ws) = active_workstate() {
                ws.set_resume_time(libc::time(std::ptr::null_mut()) as i64 + i64::from(a1));
            }
            1
        }
        4 => {
            if let Some(hooks) =
                crate::lifecycle::get_hooks().or_else(crate::lifecycle::resolve_hooks)
            {
                if let Some(exit_fn) = hooks.exit_proc {
                    exit_fn();
                    return 1;
                }
            }
            0
        }
        5 => {
            if let Some(hooks) =
                crate::lifecycle::get_hooks().or_else(crate::lifecycle::resolve_hooks)
            {
                if let Some(reload_fn) = hooks.reload {
                    reload_fn();
                    return 1;
                }
            }
            0
        }
        6 => {
            if let Some(lg) = crate::log::active_log() {
                lg.log_debug_info_active();
            }
            1
        }
        7 => {
            if let Some(ws) = active_workstate() {
                ws.set_speed_limit(a1);
            }
            1
        }
        8 => {
            if !out_buf.is_null() {
                *out_buf = into_buf(format_system_info_bytes(if a1 != 0 { 1 } else { 2 }, true));
            }
            1
        }
        9 => {
            if !crate::options::active_options()
                .is_some_and(|o| o.scalars().system_health_check != 0)
            {
                return 0;
            }
            if !out_buf.is_null() {
                *out_buf = into_buf(system_health_diagnose_bytes(if a1 != 0 { 1 } else { 2 }));
            }
            1
        }
        11 => {
            let add_top = (a1 & 1) != 0;
            let add_paused = (a1 & 2) != 0;
            let auto_cat = (a1 & 4) != 0;
            let is_url = (a1 & 8) != 0;
            let is_zip = (a1 & 16) != 0;
            let fn_b = if s1.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(s1).to_bytes()
            };
            let cat_b = if s2.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(s2).to_bytes()
            };
            let dk_b = if s3.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(s3).to_bytes()
            };
            let mut param_pairs: Vec<(&[u8], &[u8])> = Vec::new();
            if !params.is_null() && params_len > 0 {
                for item in std::slice::from_raw_parts(params, params_len) {
                    let k = if item.name.is_null() {
                        &[][..]
                    } else {
                        CStr::from_ptr(item.name).to_bytes()
                    };
                    let v = if item.value.is_null() {
                        &[][..]
                    } else {
                        CStr::from_ptr(item.value).to_bytes()
                    };
                    param_pairs.push((k, v));
                }
            }
            if is_url {
                let url_b = if body.is_null() {
                    &[][..]
                } else {
                    CStr::from_ptr(body).to_bytes()
                };
                let Some(nzb_id) = crate::urlcoord::add_rpc_url_active(
                    url_b,
                    fn_b,
                    cat_b,
                    auto_cat,
                    a2,
                    add_paused,
                    add_top,
                    dk_b,
                    a3,
                    a4,
                    &param_pairs,
                ) else {
                    return 0;
                };
                if !out_u32.is_null() {
                    *out_u32 = nzb_id;
                }
                return 1;
            }
            let data_b = if body.is_null() || body_len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(body.cast::<u8>(), body_len)
            };
            let Some(sc) = crate::scanner::active_scanner() else {
                return 0;
            };
            if is_zip {
                return c_int::from(
                    sc.add_archive_bytes_active(
                        fn_b,
                        cat_b,
                        auto_cat,
                        a2,
                        dk_b,
                        a3,
                        a4,
                        &param_pairs,
                        add_top,
                        add_paused,
                        data_b,
                    ) == crate::scanner::AS_SUCCESS,
                );
            }
            let (_, nzb_id) = sc.add_external_bytes_active(
                fn_b,
                cat_b,
                auto_cat,
                a2,
                dk_b,
                a3,
                a4,
                &param_pairs,
                add_top,
                add_paused,
                data_b,
            );
            if !out_u32.is_null() {
                *out_u32 = nzb_id as u32;
            }
            c_int::from(nzb_id > 0)
        }
        12 => {
            crate::log::clear_active_log();
            1
        }
        13 => {
            if let Some(sc) = crate::scanner::active_scanner() {
                sc.scan_nzb_dir_active(a1 != 0);
            }
            1
        }
        16 => {
            if let Some(fc) = crate::feed::active_feed_coord() {
                fc.fetch_feed_active(a1);
            }
            1
        }
        17 => {
            let Some(pool) = crate::net::active_server_pool() else {
                return 0;
            };
            c_int::from(
                crate::net::nzbget_rs_serverpool_ctl(
                    std::sync::Arc::as_ptr(&pool),
                    7,
                    0,
                    0,
                    ids as usize,
                    ids_len,
                ) != 0,
            )
        }
        18 => {
            let tdir =
                crate::options::active_options().and_then(|o| o.str_cstr(5).map(|s| s.to_owned()));
            let tdir_ptr = tdir.as_ref().map_or(c"".as_ptr(), |s| s.as_ptr());
            let mut buf = RsBuf {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            let ok = crate::net::nzbget_rs_webdl_read_temp_url(
                if s1.is_null() { c"".as_ptr() } else { s1 },
                if s2.is_null() { c"".as_ptr() } else { s2 },
                tdir_ptr,
                &mut buf,
            );
            if ok != 0 && !out_buf.is_null() {
                *out_buf = buf;
            } else {
                nzbget_rs_free(buf);
            }
            c_int::from(ok != 0)
        }
        19 => match crate::maintenance::check_updates_active() {
            Some(info) => {
                let nul = info.iter().position(|&b| b == 0).unwrap_or(info.len());
                if !out_buf.is_null() {
                    *out_buf = into_buf(info[..nul].to_vec());
                }
                1
            }
            None => 0,
        },
        20 => c_int::from(crate::maintenance::start_update_active(a1)),
        21 => {
            let Some(sm) = crate::statmeter::active_statmeter() else {
                return 0;
            };
            let tz = active_workstate().map_or(0, |w| w.local_time_offset());
            crate::statmeter::nzbget_rs_statmeter_rpc(
                std::sync::Arc::as_ptr(&sm),
                0,
                tz,
                a1,
                a2,
                0,
                0,
                0,
                std::ptr::null_mut(),
                None,
            )
        }
        23 => {
            let mut err = RsBuf {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            let ok = crate::net::nzbget_rs_serverpool_test(
                if s1.is_null() { c"".as_ptr() } else { s1 },
                a1,
                if s2.is_null() { c"".as_ptr() } else { s2 },
                if s3.is_null() { c"".as_ptr() } else { s3 },
                a2,
                if s4.is_null() { c"".as_ptr() } else { s4 },
                a3,
                a4 as u32,
                &mut err,
            );
            if ok == 0 && !err.data.is_null() && !out_buf.is_null() {
                *out_buf = err;
            } else {
                nzbget_rs_free(err);
            }
            ok
        }
        24 => {
            let Some(pool) = crate::net::active_server_pool() else {
                return 0;
            };
            let mut err_ptr: *const c_char = std::ptr::null();
            if crate::net::nzbget_rs_serverpool_ctl(
                std::sync::Arc::as_ptr(&pool),
                10,
                a1,
                0,
                (&mut err_ptr as *mut *const c_char) as usize,
                0,
            ) == 0
            {
                if !out_buf.is_null() && !err_ptr.is_null() {
                    *out_buf = into_buf(CStr::from_ptr(err_ptr).to_bytes().to_vec());
                }
                return 0;
            }
            let url_b = if s1.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(s1).to_bytes()
            };
            c_int::from(crate::urlcoord::add_testserver_url_active(a1, url_b))
        }
        10 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            let id_slice = if ids.is_null() || ids_len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(ids, ids_len)
            };
            let args_b = input(s1);
            c_int::from(qh.with_locked_queue(|dq_ptr| {
                qh.dq_edit_list(dq_ptr, Some(id_slice), None, 0, a1, args_b)
            }))
        }
        22 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            qh.raw(
                7,
                a1,
                0,
                out_buf as usize,
                0,
                s1,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) as c_int
        }
        25 => crate::net::nzbget_rs_net_speedtest_iter(body, body_len, a1, out_u32, out_f64),
        _ => 0,
    }
}

#[cfg(unix)]
unsafe fn emit_rpc_nzb(
    qh: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
    text_status_mode: i32,
    f_cb: impl FnOnce(
        &crate::queuesubsys_host::NzbSnap,
        *const c_char,
        &crate::xmlrpc_format::NzbgetRsRpcNzbInfoInput,
        &[crate::xmlrpc_format::NzbgetRsRpcParamItem],
        &[crate::xmlrpc_format::NzbgetRsRpcScriptItem],
        &[crate::xmlrpc_format::NzbgetRsRpcServerStatItem],
    ),
) {
    let n = qh.nzb_snap_mode(nzb_ptr, text_status_mode);
    let post_start_time = if n.post_info_ptr != 0 {
        qh.post_snap(n.post_info_ptr).start_time
    } else {
        0
    };
    let cur_time = if post_start_time != 0 {
        libc::time(std::ptr::null_mut()) as i64
    } else {
        0
    };
    let c_name = to_nul_cvec(&n.name);
    let c_url = to_nul_cvec(&n.url);
    let c_filename = to_nul_cvec(&n.filename);
    let c_dest_dir = to_nul_cvec(&n.dest_dir);
    let c_final_dir = to_nul_cvec(&n.final_dir);
    let c_category = to_nul_cvec(&n.category);
    let c_dupe_key = to_nul_cvec(&n.dupe_key);
    let c_text_status = to_nul_cvec(&n.text_status);

    let f = crate::xmlrpc_format::NzbgetRsRpcNzbInfoInput {
        id: n.id,
        kind: n.kind,
        name: c_name.as_ptr(),
        url: c_url.as_ptr(),
        filename: c_filename.as_ptr(),
        dest_dir: c_dest_dir.as_ptr(),
        final_dir: c_final_dir.as_ptr(),
        category: c_category.as_ptr(),
        par_status: n.par_status,
        unpack_status: n.unpack_status,
        move_status: n.move_status,
        script_status: n.script_total_status,
        delete_status: n.delete_status,
        mark_status: n.mark_status,
        url_status: n.url_status,
        size: n.size,
        file_count: n.file_count,
        min_time: n.min_time as c_int,
        max_time: n.max_time as c_int,
        total_articles: n.total_articles,
        current_success_articles: n.current_success_articles,
        current_failed_articles: n.current_failed_articles,
        dupe_recovered_articles: n.dupe_recovered_articles,
        dupe_recovered_bytes: n.dupe_recovered_bytes,
        dupe_recovered_holes: n.dupe_recovered_holes,
        live_repairing: c_int::from(n.has_live_repair_thread),
        health: n.health,
        critical_health: n.critical_health_exact,
        dupe_key: c_dupe_key.as_ptr(),
        dupe_score: n.dupe_score,
        dupe_mode: n.dupe_mode,
        downloaded_size: n.downloaded_size,
        download_sec: n.download_sec,
        post_total_sec: n.post_total_sec,
        has_post_start_time: c_int::from(post_start_time != 0),
        cur_time,
        post_start_time,
        par_sec: n.par_sec,
        repair_sec: n.repair_sec,
        unpack_sec: n.unpack_sec,
        message_count: n.message_count,
        cached_message_count: n.cached_message_count,
        extra_par_blocks: n.extra_par_blocks,
    };

    let plen = qh.nzb_params_len(nzb_ptr);
    let mut prm_cvecs = Vec::with_capacity(plen);
    for i in 0..plen {
        let (k, v) = qh.nzb_param_at(nzb_ptr, i);
        prm_cvecs.push((to_nul_cvec(&k), to_nul_cvec(&v)));
    }
    let prms: Vec<crate::xmlrpc_format::NzbgetRsRpcParamItem> = prm_cvecs
        .iter()
        .map(|(k, v)| crate::xmlrpc_format::NzbgetRsRpcParamItem {
            name: k.as_ptr(),
            value: v.as_ptr(),
        })
        .collect();

    let slen = qh.nzb_script_statuses_len(nzb_ptr);
    let mut scr_cvecs = Vec::with_capacity(slen);
    for i in 0..slen {
        let (st, sname) = qh.nzb_script_status_at(nzb_ptr, i);
        scr_cvecs.push((to_nul_cvec(&sname), st));
    }
    let scrs: Vec<crate::xmlrpc_format::NzbgetRsRpcScriptItem> = scr_cvecs
        .iter()
        .map(|(sname, st)| crate::xmlrpc_format::NzbgetRsRpcScriptItem {
            name: sname.as_ptr(),
            status: *st,
        })
        .collect();

    let stlen = qh.nzb_current_server_stats_len(nzb_ptr);
    let mut sts = Vec::with_capacity(stlen);
    for i in 0..stlen {
        let (sid, succ, fail) = qh.nzb_current_server_stat_at(nzb_ptr, i);
        sts.push(crate::xmlrpc_format::NzbgetRsRpcServerStatItem {
            server_id: sid,
            success_articles: succ,
            failed_articles: fail,
        });
    }

    f_cb(&n, c_text_status.as_ptr(), &f, &prms, &scrs, &sts);
}

#[cfg(unix)]
unsafe fn emit_rpc_post(
    qh: &crate::queuesubsys_host::QueueSubsysHost,
    post_ptr: usize,
    log_entries: c_int,
    post_queue: bool,
    emit_ctx: *mut c_void,
    emit_cb: crate::xmlrpc_format::NzbgetRsXmlRpcEmitFn,
) {
    if post_ptr == 0 {
        emit_cb(
            emit_ctx,
            5,
            c_int::from(post_queue),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
        );
        return;
    }
    let pi = qh.post_snap(post_ptr);
    let cur_time = libc::time(std::ptr::null_mut()) as i64;
    let c_label = to_nul_cvec(&pi.progress_label);
    let msgs = if log_entries > 0 {
        qh.nzb_cached_messages_slice(pi.nzb_ptr, 0, log_entries)
    } else {
        Vec::new()
    };
    let entries: Vec<crate::xmlrpc_format::NzbgetRsRpcLogEntry> = msgs
        .iter()
        .map(|m| crate::xmlrpc_format::NzbgetRsRpcLogEntry {
            id: m.0,
            kind: m.1,
            time: m.2,
            text: m.3.as_ptr(),
        })
        .collect();
    emit_cb(
        emit_ctx,
        5,
        c_int::from(post_queue),
        1,
        pi.stage_progress,
        0,
        0,
        0,
        cur_time,
        pi.stage_time,
        pi.start_time,
        c_label.as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null(),
        entries.as_ptr().cast(),
        entries.len(),
        std::ptr::null(),
        0,
        std::ptr::null(),
        0,
        std::ptr::null(),
        0,
    );
}

#[cfg(unix)]
unsafe extern "C" fn web_rpc_collect_bridge(
    _ctx: *mut c_void,
    kind: c_int,
    a1: c_int,
    a2: c_int,
    a3: c_int,
    a4: c_int,
    a5: c_int,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    s5: *const c_char,
    s6: *const c_char,
    p1: *const c_void,
    n1: usize,
    emit_ctx: *mut c_void,
    emit_cb: crate::xmlrpc_format::NzbgetRsXmlRpcEmitFn,
) -> c_int {
    match kind {
        1 => {
            let (post_job_count, url_count, remaining_size, forced_size) =
                if let Some(qh) = queue_host() {
                    qh.with_locked_queue(|dq_ptr| {
                        let mut pjc: c_int = 0;
                        let mut uc: c_int = 0;
                        let qlen = qh.queue_len(dq_ptr);
                        for i in 0..qlen {
                            let np = qh.queue_at(dq_ptr, i);
                            let n = qh.nzb_snap(np, false);
                            if n.has_post_info {
                                pjc += 1;
                            }
                            if n.kind == crate::queuesubsys_host::NK_URL {
                                uc += 1;
                            }
                        }
                        let (rem, forced) = qh.dq_calc_remaining_size(dq_ptr);
                        (pjc, uc, rem, forced)
                    })
                } else {
                    (0, 0, 0, 0)
                };
            let opts = crate::options::active_options();
            let ws = active_workstate();
            let sm = crate::statmeter::active_statmeter();
            let lg = crate::log::active_log();
            let pool = crate::net::active_server_pool();
            let fc = crate::feed::active_feed_coord();

            let (up_sec, dn_sec, all_bytes, stand_by) = sm
                .as_deref()
                .map_or((0, 0, 0, true), |s| s.total_stat_snapshot());
            let tz = ws.as_deref().map_or(0, |w| w.local_time_offset());
            let (month_bytes, day_bytes) = sm.as_deref().map_or((0, 0), |s| {
                s.calc_quota_usage(tz, opts.as_deref(), lg.as_deref())
            });

            let (free_disk, total_disk) = match opts.as_deref().and_then(|o| o.str_cstr(3)) {
                Some(p) if !p.to_bytes().is_empty() => {
                    crate::fsops::get_disk_state(p).unwrap_or((0, 0))
                }
                _ => (0, 0),
            };
            let inter_cstr = opts.as_deref().and_then(|o| o.str_cstr(4));
            let inter_empty = inter_cstr.map_or(true, |p| p.to_bytes().is_empty());
            let (free_inter, total_inter) = match inter_cstr {
                Some(p) if !inter_empty => crate::fsops::get_disk_state(p).unwrap_or((0, 0)),
                _ => (0, 0),
            };

            let mm_active = crate::articledl::memory_monitor_active();
            let cfg_cache_mb = opts.as_deref().map_or(0, |o| o.scalars().article_cache);
            let cfg_par_mb = opts.as_deref().map_or(0, |o| o.scalars().par_buffer);

            let sf = crate::xmlrpc_format::NzbgetRsRpcStatusInput {
                remaining_size,
                forced_size,
                article_cache: crate::articledl::article_cache_allocated() as i64,
                memory_pressure: if mm_active {
                    crate::articledl::memory_monitor_tier_name_c()
                } else {
                    c"NORMAL".as_ptr()
                },
                effective_article_cache_mb: if mm_active {
                    crate::articledl::memory_monitor_effective_cache_mb()
                } else {
                    cfg_cache_mb
                },
                effective_par_buffer_mb: if mm_active {
                    crate::articledl::memory_monitor_effective_par_buffer_mb(cfg_par_mb)
                } else {
                    cfg_par_mb
                },
                download_rate: sm.as_deref().map_or(0, |s| s.current_download_speed()),
                download_limit: ws.as_deref().map_or(0, |w| w.speed_limit()),
                thread_count: current_thread_count() - 1,
                post_job_count,
                url_count,
                up_time_sec: up_sec,
                download_time_sec: dn_sec,
                all_bytes,
                month_bytes,
                day_bytes,
                download_paused: c_int::from(ws.as_deref().is_some_and(|w| w.pause_download())),
                server_stand_by: c_int::from(stand_by),
                post_paused: c_int::from(ws.as_deref().is_some_and(|w| w.pause_post_process())),
                scan_paused: c_int::from(ws.as_deref().is_some_and(|w| w.pause_scan())),
                quota_reached: c_int::from(ws.as_deref().is_some_and(|w| w.quota_reached())),
                free_disk_space: free_disk,
                total_disk_space: total_disk,
                inter_dir_empty: c_int::from(inter_empty),
                free_inter_disk_space: free_inter,
                total_inter_disk_space: total_inter,
                server_time: libc::time(std::ptr::null_mut()) as c_int,
                resume_time: ws.as_deref().map_or(0, |w| w.resume_time() as c_int),
                feed_active: c_int::from(fc.as_deref().is_some_and(|f| f.has_active_downloads())),
                queued_scripts: 0,
            };

            let mut srvs: Vec<crate::xmlrpc_format::NzbgetRsRpcServerStatus> = Vec::new();
            if let Some(p) = pool.as_deref() {
                let cnt = p.server_count();
                srvs.resize_with(cnt, || crate::xmlrpc_format::NzbgetRsRpcServerStatus {
                    id: 0,
                    active: 0,
                });
                let filled = crate::net::nzbget_rs_serverpool_ctl(
                    p,
                    9,
                    0,
                    0,
                    srvs.as_mut_ptr() as usize,
                    cnt,
                ) as usize;
                srvs.truncate(filled.min(cnt));
            }

            emit_cb(
                emit_ctx,
                1,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                (&sf as *const crate::xmlrpc_format::NzbgetRsRpcStatusInput).cast(),
                0,
                srvs.as_ptr().cast(),
                srvs.len(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            );
            1
        }
        2 => {
            if a1 == 3 {
                let disk_msgs = crate::diskstate_format::load_active_nzb_messages(a2);
                if disk_msgs.is_empty() {
                    let Some(qh) = queue_host() else {
                        return 0;
                    };
                    let cached_msgs = qh.with_locked_queue(|dq_ptr| {
                        let np = qh.queue_find_by_id(dq_ptr, a2);
                        if np == 0 {
                            Vec::new()
                        } else {
                            qh.nzb_cached_messages_slice(np, a3, a4)
                        }
                    });
                    let entries: Vec<crate::xmlrpc_format::NzbgetRsRpcLogEntry> = cached_msgs
                        .iter()
                        .map(|m| crate::xmlrpc_format::NzbgetRsRpcLogEntry {
                            id: m.0,
                            kind: m.1,
                            time: m.2,
                            text: m.3.as_ptr(),
                        })
                        .collect();
                    emit_cb(
                        emit_ctx,
                        2,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                        entries.as_ptr().cast(),
                        entries.len(),
                        std::ptr::null(),
                        0,
                        std::ptr::null(),
                        0,
                        std::ptr::null(),
                        0,
                    );
                    return 1;
                }
                let mut start = disk_msgs.len() as i32;
                if a4 > 0 {
                    start = disk_msgs.len() as i32 - a4.min(disk_msgs.len() as i32);
                }
                if a3 > 0 {
                    start = (a3 - disk_msgs[0].0).max(0);
                }
                let mut entries = Vec::new();
                if start >= 0 && (start as usize) < disk_msgs.len() {
                    let ustart = start as usize;
                    entries.reserve(disk_msgs.len() - ustart);
                    for m in &disk_msgs[ustart..] {
                        entries.push(crate::xmlrpc_format::NzbgetRsRpcLogEntry {
                            id: m.0,
                            kind: m.1,
                            time: m.2,
                            text: m.3.as_ptr(),
                        });
                    }
                }
                emit_cb(
                    emit_ctx,
                    2,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    entries.as_ptr().cast(),
                    entries.len(),
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                );
                return 1;
            }
            if a1 == 1 {
                crate::maintenance::collect_maint_messages_active(a3, a4, emit_ctx, emit_cb);
            } else {
                let lg = crate::log::active_log();
                crate::log::nzbget_rs_log_collect(
                    1,
                    lg.as_deref().map_or(std::ptr::null(), |l| l as *const _),
                    a3,
                    a4,
                    0,
                    std::ptr::null_mut(),
                    emit_ctx,
                    emit_cb as usize,
                    0,
                );
            }
            1
        }
        3 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            qh.with_locked_queue(|dq_ptr| {
                let qlen = qh.queue_len(dq_ptr);
                for qi in 0..qlen {
                    let np = qh.queue_at(dq_ptr, qi);
                    let outer_n = qh.nzb_snap(np, false);
                    let c_outer_name = to_nul_cvec(&outer_n.name);
                    let c_outer_fn = to_nul_cvec(&outer_n.filename);
                    let c_outer_dest = to_nul_cvec(&outer_n.dest_dir);
                    let c_outer_cat = to_nul_cvec(&outer_n.category);
                    let flen = qh.nzb_file_len(np);
                    for fi in 0..flen {
                        let fp = qh.nzb_file_at(np, fi);
                        let f = qh.file_snap(fp);
                        let alt_n = if f.nzb_ptr == np {
                            None
                        } else {
                            Some(qh.nzb_snap(f.nzb_ptr, false))
                        };
                        let fnzb_id = alt_n.as_ref().map_or(outer_n.id, |an| an.id);
                        if !((a3 > 0 && a3 == fnzb_id)
                            || (a3 == 0 && (a1 == 0 || (a1 <= f.id && f.id <= a2))))
                        {
                            continue;
                        }
                        let denom = f.size - f.missed_size;
                        let progress = if (f.failed_size == 0 && f.success_size == 0) || denom <= 0
                        {
                            0
                        } else {
                            (1000 - f.remaining_size * 1000 / denom) as c_int
                        };
                        let alt_c = alt_n.as_ref().map(|an| {
                            (
                                to_nul_cvec(&an.name),
                                to_nul_cvec(&an.filename),
                                to_nul_cvec(&an.dest_dir),
                                to_nul_cvec(&an.category),
                                an.priority,
                            )
                        });
                        let c_subj = to_nul_cvec(&f.subject);
                        let c_ffn = to_nul_cvec(&f.filename);
                        let item = crate::xmlrpc_format::NzbgetRsRpcFileItem {
                            id: f.id,
                            file_size: f.size,
                            remaining_size: f.remaining_size,
                            post_time: f.time as c_int,
                            filename_confirmed: c_int::from(f.filename_confirmed),
                            paused: c_int::from(f.paused),
                            nzb_id: fnzb_id,
                            nzb_name: alt_c
                                .as_ref()
                                .map_or(c_outer_name.as_ptr(), |x| x.0.as_ptr()),
                            nzb_filename: alt_c
                                .as_ref()
                                .map_or(c_outer_fn.as_ptr(), |x| x.1.as_ptr()),
                            subject: c_subj.as_ptr(),
                            filename: c_ffn.as_ptr(),
                            dest_dir: alt_c
                                .as_ref()
                                .map_or(c_outer_dest.as_ptr(), |x| x.2.as_ptr()),
                            category: alt_c
                                .as_ref()
                                .map_or(c_outer_cat.as_ptr(), |x| x.3.as_ptr()),
                            priority: alt_c.as_ref().map_or(outer_n.priority, |x| x.4),
                            active_downloads: f.active_downloads,
                            progress,
                        };
                        emit_cb(
                            emit_ctx,
                            3,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            (&item as *const crate::xmlrpc_format::NzbgetRsRpcFileItem).cast(),
                            0,
                            std::ptr::null(),
                            0,
                            std::ptr::null(),
                            0,
                            std::ptr::null(),
                            0,
                        );
                    }
                }
            });
            1
        }
        4 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            qh.with_locked_queue(|dq_ptr| {
                let qlen = qh.queue_len(dq_ptr);
                for qi in 0..qlen {
                    let np = qh.queue_at(dq_ptr, qi);
                    let flen = qh.nzb_file_len(np) as c_int;
                    let mut post_ptr = 0usize;
                    emit_rpc_nzb(&qh, np, 0, |n, _, f, prms, scrs, sts| {
                        post_ptr = n.post_info_ptr;
                        let has_post = post_ptr != 0;
                        let post_stage = if has_post { n.post_stage } else { 0 };
                        let flags = (if has_post { 1 } else { 0 })
                            | ((post_stage & 0xff) << 4)
                            | ((n.direct_unpack_status & 0xff) << 12)
                            | ((n.kind & 0xff) << 20);
                        emit_cb(
                            emit_ctx,
                            4,
                            n.id,
                            flen,
                            n.remaining_par_count,
                            n.priority,
                            n.active_downloads,
                            flags,
                            n.remaining_size,
                            n.paused_size,
                            0,
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            (f as *const crate::xmlrpc_format::NzbgetRsRpcNzbInfoInput).cast(),
                            0,
                            prms.as_ptr().cast(),
                            prms.len(),
                            scrs.as_ptr().cast(),
                            scrs.len(),
                            sts.as_ptr().cast(),
                            sts.len(),
                        );
                    });
                    emit_rpc_post(&qh, post_ptr, a1, false, emit_ctx, emit_cb);
                }
            });
            1
        }
        5 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            qh.with_locked_queue(|dq_ptr| {
                let qlen = qh.queue_len(dq_ptr);
                for qi in 0..qlen {
                    let np = qh.queue_at(dq_ptr, qi);
                    let outer = qh.nzb_snap(np, false);
                    if outer.post_info_ptr == 0 {
                        continue;
                    }
                    let pi = qh.post_snap(outer.post_info_ptr);
                    emit_rpc_nzb(&qh, pi.nzb_ptr, 0, |pn, _, f, prms, scrs, sts| {
                        let c_pname = to_nul_cvec(&pn.name);
                        emit_cb(
                            emit_ctx,
                            6,
                            outer.id,
                            pi.stage,
                            pi.file_progress,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            c_pname.as_ptr(),
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            (f as *const crate::xmlrpc_format::NzbgetRsRpcNzbInfoInput).cast(),
                            0,
                            prms.as_ptr().cast(),
                            prms.len(),
                            scrs.as_ptr().cast(),
                            scrs.len(),
                            sts.as_ptr().cast(),
                            sts.len(),
                        );
                    });
                    emit_rpc_post(&qh, outer.post_info_ptr, a1, true, emit_ctx, emit_cb);
                }
            });
            1
        }
        6 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            qh.with_locked_queue(|dq_ptr| {
                let hlen = qh.history_len(dq_ptr);
                for hi in 0..hlen {
                    let h = qh.history_at(dq_ptr, hi);
                    if h.kind == crate::queuesubsys_host::HK_DUP && a1 == 0 {
                        continue;
                    }
                    let c_hname = to_nul_cvec(&h.name);
                    if h.kind == crate::queuesubsys_host::HK_NZB
                        || h.kind == crate::queuesubsys_host::HK_URL
                    {
                        let has_completed =
                            c_int::from(qh.nzb_completed_files_len(h.inner_ptr) > 0);
                        emit_rpc_nzb(&qh, h.inner_ptr, 2, |n, status_ptr, f, prms, scrs, sts| {
                            emit_cb(
                                emit_ctx,
                                7,
                                h.id,
                                n.parked_file_count,
                                has_completed,
                                h.time as c_int,
                                0,
                                0,
                                0,
                                0,
                                0,
                                c_hname.as_ptr(),
                                status_ptr,
                                std::ptr::null(),
                                std::ptr::null(),
                                (f as *const crate::xmlrpc_format::NzbgetRsRpcNzbInfoInput).cast(),
                                0,
                                prms.as_ptr().cast(),
                                prms.len(),
                                scrs.as_ptr().cast(),
                                scrs.len(),
                                sts.as_ptr().cast(),
                                sts.len(),
                            );
                        });
                    } else if h.kind == crate::queuesubsys_host::HK_DUP {
                        let d = qh.dup_snap(h.inner_ptr);
                        let c_dkey = to_nul_cvec(&d.dupe_key);
                        emit_cb(
                            emit_ctx,
                            8,
                            h.id,
                            h.time as c_int,
                            d.dupe_score,
                            d.dupe_mode,
                            d.status,
                            0,
                            d.size,
                            0,
                            0,
                            c_hname.as_ptr(),
                            c_dkey.as_ptr(),
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                            0,
                            std::ptr::null(),
                            0,
                            std::ptr::null(),
                            0,
                            std::ptr::null(),
                            0,
                        );
                    }
                }
            });
            1
        }
        7 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            qh.with_locked_queue(|dq_ptr| {
                let qlen = qh.queue_len(dq_ptr);
                for qi in 0..qlen {
                    let np = qh.queue_at(dq_ptr, qi);
                    let n = qh.nzb_snap(np, false);
                    if n.kind == crate::queuesubsys_host::NK_URL {
                        let c_fn = to_nul_cvec(&n.filename);
                        let c_url = to_nul_cvec(&n.url);
                        let c_name = to_nul_cvec(&n.name);
                        let c_cat = to_nul_cvec(&n.category);
                        emit_cb(
                            emit_ctx,
                            9,
                            n.id,
                            n.priority,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            c_fn.as_ptr(),
                            c_url.as_ptr(),
                            c_name.as_ptr(),
                            c_cat.as_ptr(),
                            std::ptr::null(),
                            0,
                            std::ptr::null(),
                            0,
                            std::ptr::null(),
                            0,
                            std::ptr::null(),
                            0,
                        );
                    }
                }
            });
            1
        }
        8 => crate::dupesearch_coord::fleet_append(
            std::ptr::null_mut(),
            crate::dupesearch_coord::native_dupe_host,
            s1,
            s2,
            a1,
            a2,
            p1.cast(),
            n1,
            emit_ctx,
            emit_cb,
        ),
        9 => {
            let opts = crate::options::active_options();
            let mut entries: Vec<crate::xmlrpc_format::NzbgetRsRpcConfigItem> = Vec::new();
            if let Some(o) = opts.as_deref() {
                o.for_each_option(|n, v, r| {
                    entries.push(crate::xmlrpc_format::NzbgetRsRpcConfigItem {
                        name: n.as_ptr(),
                        value: v.as_ptr(),
                        restricted: r,
                    });
                });
            }
            emit_cb(
                emit_ctx,
                11,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                entries.as_ptr().cast(),
                entries.len(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            );
            1
        }
        10 => script_config_emit_templates(a1 != 0, emit_ctx, Some(emit_cb)),
        11 => {
            let Some(fc) = crate::feed::active_feed_coord() else {
                return 0;
            };
            struct RpcFeedEmit {
                ctx: *mut c_void,
                cb: crate::xmlrpc_format::NzbgetRsXmlRpcEmitFn,
            }
            unsafe extern "C" fn on_feed_item(
                raw: *mut c_void,
                item: *const crate::xmlrpc_format::NzbgetRsRpcFeedItem,
            ) {
                let r = &*(raw as *const RpcFeedEmit);
                (r.cb)(
                    r.ctx,
                    13,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    item.cast(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                );
            }
            let mut rctx = RpcFeedEmit {
                ctx: emit_ctx,
                cb: emit_cb,
            };
            fc.emit_view_or_preview_active(
                a1,
                a2,
                a3,
                a4,
                a5,
                n1 as c_int,
                s1,
                s2,
                s3,
                s4,
                s5,
                s6,
                (&mut rctx as *mut RpcFeedEmit).cast(),
                Some(on_feed_item),
            )
        }
        12 => {
            let Some(sm) = crate::statmeter::active_statmeter() else {
                return 0;
            };
            let tz = active_workstate().map_or(0, |w| w.local_time_offset());
            crate::statmeter::nzbget_rs_statmeter_rpc(
                std::sync::Arc::as_ptr(&sm),
                1,
                tz,
                a1,
                a2,
                a3,
                a4,
                a5,
                emit_ctx,
                Some(emit_cb),
            )
        }
        _ => 0,
    }
}

#[cfg(unix)]
pub(crate) unsafe fn serve_web_conn(conn: &crate::net::NzbgetRsConn, sig: &[u8; 4]) -> bool {
    let _cancel_guard = PosixCancelGuard::disable();
    let opts = crate::options::active_options();
    let opt_str = |id: usize| -> Option<&[u8]> {
        opts.as_deref()
            .and_then(|o| o.str_cstr(id).map(CStr::to_bytes))
    };
    let remote_addr = conn.remote_addr_bytes();
    let version = build_version_bytes();
    let web_dir = opts.as_deref().map_or(&[][..], |o| o.str_bytes(8));
    let control_port = opts.as_deref().map_or(0, |o| o.scalars().control_port);
    let form_auth = opts.as_deref().is_some_and(|o| o.scalars().form_auth != 0);
    let json_p = opts.as_deref().is_some_and(|o| o.scalars().json_p != 0);
    let gzip_enabled = !build_gzip_disabled();
    let debug_build = build_debug_enabled();

    let cfg = crate::webserver::WebServeConfig {
        signature: sig,
        remote_addr: &remote_addr,
        version: &version,
        web_dir,
        credentials: crate::webserver::Credentials {
            control_username: opt_str(15),
            control_password: opt_str(16),
            restricted_username: opt_str(17),
            restricted_password: opt_str(18),
            add_username: opt_str(19),
            add_password: opt_str(20),
            authorized_ip: opt_str(24),
        },
        control_port,
        form_auth,
        gzip_enabled,
        debug_build,
    };
    let lower = crate::wildmask::Lower::Fold(&crate::util::wild_mask_fold_locale);

    crate::webserver::serve_web_request(
        &cfg,
        &lower,
        |dst| conn.read_line_web(dst),
        |dst| conn.recv_exact_web(dst),
        |src| conn.send_web(src),
        |level, msg| {
            if level == 0 && !debug_build {
                return;
            }
            let kind = match level {
                0 => 3,
                1 => 1,
                2 => 2,
                3 => 0,
                _ => 4,
            };
            crate::log::log_active_bytes(kind, msg);
        },
        |req_body, http_method, user_access, url_c, emit| {
            let (resp, ct, safe) = crate::xmlrpc_format::serve_xmlrpc(
                std::ptr::null_mut(),
                req_body,
                http_method,
                user_access,
                json_p,
                debug_build,
                url_c,
                &version,
                opt_str(0),
                Some(web_rpc_log_bridge),
                Some(web_rpc_action_bridge),
                Some(web_rpc_collect_bridge),
            );
            let nul = resp.iter().position(|&b| b == 0).unwrap_or(resp.len());
            emit(&resp[..nul], ct, safe);
        },
    )
}

#[cfg(unix)]
pub(crate) unsafe fn serve_binrpc_conn(conn: &crate::net::NzbgetRsConn) {
    let _cancel_guard = PosixCancelGuard::disable();

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
        let (start, nr) = crate::binrpc::calc_log_slice(msg_count, lc.lines, lc.id_from, first_id);
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

    let opts = crate::options::active_options();
    let remote_addr = conn.remote_addr_bytes();
    let control_user = opts.as_deref().map_or(&[][..], |o| o.str_bytes(15));
    let control_password = opts.as_deref().map_or(&[][..], |o| o.str_bytes(16));
    let control_port = opts.as_deref().map_or(0, |o| o.scalars().control_port);
    let version = build_version_bytes();

    crate::binrpc::serve_binrpc(
        &remote_addr,
        control_user,
        control_password,
        &version,
        control_port,
        |dst| conn.recv_exact_web(dst),
        |src| conn.send_web(src),
        |kind, msg| {
            crate::log::log_active_bytes(kind, msg);
        },
        |kind, a1, a2, a3, a4, s1, s2, s3, body, ids, names| {
            if kind == 1 {
                if (a1 & 8) != 0 {
                    return crate::urlcoord::add_binrpc_url_active(
                        body,
                        s1,
                        s2,
                        (a1 & 4) != 0,
                        a2,
                        (a1 & 2) != 0,
                        (a1 & 1) != 0,
                        s3,
                        a3,
                        a4,
                    );
                }
                let Some(sc) = crate::scanner::active_scanner() else {
                    return false;
                };
                return sc
                    .add_external_bytes_active(
                        s1,
                        s2,
                        (a1 & 4) != 0,
                        a2,
                        s3,
                        a3,
                        a4,
                        &[],
                        (a1 & 1) != 0,
                        (a1 & 2) != 0,
                        body,
                    )
                    .0
                    != crate::scanner::AS_FAILED;
            }
            if kind == 2 {
                if let Some(ws) = active_workstate() {
                    ws.set_resume_time(0);
                    match a2 {
                        1 => ws.set_pause_download(a1 != 0),
                        2 => ws.set_pause_post_process(a1 != 0),
                        3 => ws.set_pause_scan(a1 != 0),
                        _ => {}
                    }
                }
                return true;
            }
            if kind == 4 {
                if let Some(ws) = active_workstate() {
                    ws.set_speed_limit(a1);
                }
                return true;
            }
            if kind == 5 {
                if let Some(lg) = crate::log::active_log() {
                    lg.log_debug_info_active();
                }
                return true;
            }
            if kind == 6 {
                let Some(qh) = queue_host() else {
                    return false;
                };
                return qh.with_locked_queue(|dq_ptr| {
                    qh.dq_edit_list(
                        dq_ptr,
                        if ids.is_empty() { None } else { Some(ids) },
                        if names.is_empty() { None } else { Some(names) },
                        a2,
                        a1,
                        s1,
                    )
                });
            }
            if kind == 8 {
                if let Some(hooks) =
                    crate::lifecycle::get_hooks().or_else(crate::lifecycle::resolve_hooks)
                {
                    if let Some(exit_fn) = hooks.exit_proc {
                        exit_fn();
                    }
                }
                return true;
            }
            if kind == 9 {
                if let Some(hooks) =
                    crate::lifecycle::get_hooks().or_else(crate::lifecycle::resolve_hooks)
                {
                    if let Some(reload_fn) = hooks.reload {
                        reload_fn();
                    }
                }
                return true;
            }
            if kind == 13 {
                if let Some(sc) = crate::scanner::active_scanner() {
                    sc.scan_nzb_dir_active(a1 != 0);
                }
                return true;
            }
            false
        },
        |want_files, want_state, builder| {
            let mut post_jobs: c_int = 0;
            let mut rem_size: i64 = 0;
            if want_files || want_state {
                if let Some(qh) = queue_host() {
                    qh.with_locked_queue(|dq_ptr| {
                        let qlen = qh.queue_len(dq_ptr);
                        if want_files {
                            let mut queue_ptrs = Vec::with_capacity(qlen);
                            for i in 0..qlen {
                                queue_ptrs.push(qh.queue_at(dq_ptr, i));
                            }
                            let mut owner_index: u32 = 0;
                            for &np in &queue_ptrs {
                                owner_index += 1;
                                let n = qh.nzb_snap(np, false);
                                builder.push_nzb(
                                    n.id as u32,
                                    n.kind as u32,
                                    n.size,
                                    n.remaining_size,
                                    n.paused_size,
                                    n.paused_file_count as u32,
                                    n.remaining_par_count as u32,
                                    n.priority as u32,
                                    &n.filename,
                                    &n.name,
                                    &n.dest_dir,
                                    &n.category,
                                    &n.queued_filename,
                                );
                                let plen = qh.nzb_params_len(np);
                                for pi in 0..plen {
                                    let (pname, pval) = qh.nzb_param_at(np, pi);
                                    builder.push_param(owner_index, &pname, &pval);
                                }
                                let flen = qh.nzb_file_len(np);
                                for fi in 0..flen {
                                    let fp = qh.nzb_file_at(np, fi);
                                    let f = qh.file_snap(fp);
                                    let (nzb_index, fnzb_name_owned) = if f.nzb_ptr == np {
                                        (owner_index, None)
                                    } else {
                                        let mut idx: u32 = 0;
                                        for &qp in &queue_ptrs {
                                            idx += 1;
                                            if qp == f.nzb_ptr {
                                                break;
                                            }
                                        }
                                        (idx, Some(qh.nzb_snap(f.nzb_ptr, false).name))
                                    };
                                    let fnzb_name = fnzb_name_owned.as_deref().unwrap_or(&n.name);
                                    builder.push_file(
                                        f.id as u32,
                                        nzb_index,
                                        f.size,
                                        f.remaining_size,
                                        u32::from(f.paused),
                                        u32::from(f.filename_confirmed),
                                        f.active_downloads as u32,
                                        fnzb_name,
                                        &f.subject,
                                        &f.filename,
                                    );
                                }
                            }
                        }
                        if want_state {
                            for i in 0..qlen {
                                let np = qh.queue_at(dq_ptr, i);
                                if qh.nzb_snap(np, false).has_post_info {
                                    post_jobs += 1;
                                }
                            }
                            rem_size = qh.dq_calc_remaining_size(dq_ptr).0;
                        }
                    });
                }
            }
            if want_state {
                let sm = crate::statmeter::active_statmeter();
                let ws = active_workstate();
                let speed = sm.as_deref().map_or(0, |s| s.current_download_speed());
                let (up_sec, dn_sec, all_bytes, stand_by) = sm
                    .as_deref()
                    .map_or((0, 0, 0, true), |s| s.total_stat_snapshot());
                builder.remaining_size = rem_size;
                builder.download_rate = u32::try_from(speed).unwrap_or(0);
                builder.download_limit = ws.as_deref().map_or(0, |w| w.speed_limit());
                builder.download_paused =
                    c_int::from(ws.as_deref().is_some_and(|w| w.pause_download()));
                builder.post_paused =
                    c_int::from(ws.as_deref().is_some_and(|w| w.pause_post_process()));
                builder.scan_paused = c_int::from(ws.as_deref().is_some_and(|w| w.pause_scan()));
                builder.thread_count = current_thread_count() - 1;
                builder.post_job_count = post_jobs;
                builder.up_time_sec = up_sec;
                builder.dn_time_sec = dn_sec;
                builder.stand_by = c_int::from(stand_by);
                builder.downloaded_bytes = all_bytes;
                builder.state_populated = true;
            }
        },
        |lines, id_from, nr_entries, enc| {
            let mut lc = LogCollectCtx {
                lines,
                id_from,
                nr_entries,
                enc,
            };
            let lg = crate::log::active_log();
            crate::log::nzbget_rs_log_collect(
                2,
                lg.as_deref().map_or(std::ptr::null(), |l| l as *const _),
                0,
                0,
                0,
                std::ptr::null_mut(),
                (&mut lc as *mut LogCollectCtx<'_>).cast(),
                log_begin as *const () as usize,
                log_push as *const () as usize,
            );
        },
        |nr_entries, enc| {
            let Some(qh) = queue_host() else {
                return;
            };
            qh.with_locked_queue(|dq_ptr| {
                let cur_time = libc::time(std::ptr::null_mut()) as i64;
                let qlen = qh.queue_len(dq_ptr);
                for i in 0..qlen {
                    let np = qh.queue_at(dq_ptr, i);
                    let outer = qh.nzb_snap(np, false);
                    if outer.post_info_ptr == 0 {
                        continue;
                    }
                    let outer_id = outer.id as u32;
                    let pi = qh.post_snap(outer.post_info_ptr);
                    let pn = if pi.nzb_ptr == np {
                        outer
                    } else {
                        qh.nzb_snap(pi.nzb_ptr, false)
                    };
                    *nr_entries = nr_entries.wrapping_add(1);
                    let entry = crate::binrpc::PostEntry {
                        id: outer_id,
                        stage: pi.stage as u32,
                        stage_progress: pi.stage_progress as u32,
                        file_progress: pi.file_progress as u32,
                        start_time: if pi.start_time != 0 {
                            (cur_time - pi.start_time) as u32
                        } else {
                            0
                        },
                        stage_time: if pi.stage_time != 0 {
                            (cur_time - pi.stage_time) as u32
                        } else {
                            0
                        },
                        texts: [
                            crate::binrpc::Text::new(&pn.filename),
                            crate::binrpc::Text::new(&pn.name),
                            crate::binrpc::Text::new(&pn.dest_dir),
                            crate::binrpc::Text::new(&pi.progress_label),
                        ],
                    };
                    enc.push(&entry);
                }
            });
        },
        |show_hidden, nr_entries, enc| {
            let Some(qh) = queue_host() else {
                return;
            };
            qh.with_locked_queue(|dq_ptr| {
                let hlen = qh.history_len(dq_ptr);
                for i in 0..hlen {
                    let h = qh.history_at(dq_ptr, i);
                    if h.kind == crate::queuesubsys_host::HK_DUP && !show_hidden {
                        continue;
                    }
                    let mut size: i64 = 0;
                    let mut file_cnt: u32 = 0;
                    let mut par_status: u32 = 0;
                    let mut script_status: u32 = 0;
                    let mut url_status: u32 = 0;
                    if h.kind == crate::queuesubsys_host::HK_NZB {
                        let n = qh.nzb_snap(h.inner_ptr, false);
                        size = n.size;
                        file_cnt = n.file_count as u32;
                        par_status = n.par_status as u32;
                        script_status = n.script_total_status as u32;
                    } else if h.kind == crate::queuesubsys_host::HK_DUP {
                        size = qh.dup_snap(h.inner_ptr).size;
                    } else if h.kind == crate::queuesubsys_host::HK_URL {
                        url_status = qh.nzb_snap(h.inner_ptr, false).url_status as u32;
                    }
                    *nr_entries = nr_entries.wrapping_add(1);
                    let size_hi = ((size >> 32) & 0xffff_ffff) as u32;
                    let size_lo = (size & 0xffff_ffff) as u32;
                    let entry = crate::binrpc::HistoryEntry {
                        id: h.id as u32,
                        kind: h.kind as u32,
                        time: (h.time as i32) as u32,
                        size_lo,
                        size_hi,
                        file_count: file_cnt,
                        par_status,
                        script_status,
                        url_status,
                        texts: [crate::binrpc::Text::new(&h.name)],
                    };
                    enc.push(&entry);
                }
            });
        },
    );
}

#[cfg(unix)]
pub(crate) struct FrontendHandle {
    mode: c_int,
    use_color: bool,
    exit_cb: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[cfg(unix)]
impl FrontendHandle {
    pub(crate) fn new(
        mode: c_int,
        exit_cb: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        if mode != 3 {
            crate::log::log_active_bytes(3, b"Creating Frontend");
        }
        let use_color = if mode == 2 {
            unsafe { crate::ncurses_format::frontend_op(0, std::ptr::null()) > 0 }
        } else {
            true
        };
        Self {
            mode,
            use_color,
            exit_cb,
            stopped: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            running: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            handle: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn start(&self) {
        let mut g = self.handle.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_some() {
            return;
        }
        self.stopped
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mode = self.mode;
        let use_color = self.use_color;
        let exit_cb = self.exit_cb.clone();
        let stopped = self.stopped.clone();
        let running = self.running.clone();
        if let Ok(h) = std::thread::Builder::new()
            .name("frontend".into())
            .spawn(move || {
                struct RunningReset(std::sync::Arc<std::sync::atomic::AtomicBool>);
                impl Drop for RunningReset {
                    fn drop(&mut self) {
                        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                let _reset = RunningReset(running);
                let _tg = RustThreadGuard::enter(&stopped);
                run_frontend_thread(mode, use_color, &stopped, exit_cb.as_ref());
            })
        {
            *g = Some(h);
        } else {
            self.running
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub(crate) fn stop(&self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(ws) = active_workstate() {
            ws.wake_waiters();
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn join(&self) {
        let h = self.handle.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(h) = h {
            let _ = h.join();
        }
    }
}

#[cfg(unix)]
impl Drop for FrontendHandle {
    fn drop(&mut self) {
        self.stop();
        self.join();
        if self.mode == 2 {
            unsafe {
                crate::ncurses_format::frontend_op(1, std::ptr::null());
            }
            write_stdout_bytes(b"\n", true);
        }
    }
}

#[cfg(unix)]
fn run_frontend_thread(
    mode: c_int,
    use_color: bool,
    stopped: &std::sync::atomic::AtomicBool,
    exit_cb: Option<&std::sync::Arc<dyn Fn() + Send + Sync>>,
) {
    if mode == 0 || mode == 1 {
        crate::log::log_active_bytes(3, b"Entering LoggableFrontend-loop");
    } else if mode == 2 {
        crate::log::log_active_bytes(3, b"Entering NCursesFrontend-loop");
    }

    let opts = if mode != 3 {
        crate::options::active_options()
    } else {
        None
    };

    struct FeBridgeCtx<'a> {
        mode: c_int,
        stopped: &'a std::sync::atomic::AtomicBool,
        exit_cb: Option<&'a std::sync::Arc<dyn Fn() + Send + Sync>>,
    }

    unsafe extern "C" fn fe_stdout_cb(
        _raw: *mut c_void,
        ptr: *const c_char,
        len: usize,
        do_flush: c_int,
    ) {
        let slice = if ptr.is_null() || len == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(ptr.cast::<u8>(), len)
        };
        write_stdout_bytes(slice, do_flush != 0);
    }

    unsafe extern "C" fn fe_action_cb(
        raw: *mut c_void,
        act_op: c_int,
        a1: c_int,
        a2: c_int,
        a3: c_int,
    ) -> i64 {
        let br = &*raw.cast::<FeBridgeCtx<'_>>();
        match act_op {
            0 => i64::from(br.stopped.load(std::sync::atomic::Ordering::SeqCst)),
            1 => {
                if br.mode != 3 && active_workstate().is_some_and(|w| w.pause_frontend()) {
                    if let Some(ws) = active_workstate() {
                        ws.wait_for_change(2000);
                    }
                } else if a1 > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(a1 as u64));
                }
                0
            }
            2 => {
                br.stopped.store(true, std::sync::atomic::Ordering::SeqCst);
                if let Some(ws) = active_workstate() {
                    ws.wake_waiters();
                }
                0
            }
            3 => {
                if a2 != 0 {
                    crate::log::log_active_bytes(
                        0,
                        if a1 != 0 {
                            b"Pausing download"
                        } else {
                            b"Unpausing download"
                        },
                    );
                }
                if let Some(ws) = active_workstate() {
                    ws.set_resume_time(0);
                    ws.set_pause_download(a1 != 0);
                }
                0
            }
            4 => {
                if let Some(ws) = active_workstate() {
                    ws.set_speed_limit(a1);
                }
                0
            }
            5 => {
                let Some(qh) = queue_host() else {
                    return 0;
                };
                let arg = a2.to_string();
                let mut ok = false;
                qh.with_locked_queue(|dq_ptr| {
                    if dq_ptr != 0 {
                        ok = qh.dq_edit_entry(dq_ptr, a3, a1, Some(arg.as_bytes()));
                    }
                });
                i64::from(ok)
            }
            6 => {
                if let Some(cb) = br.exit_cb {
                    cb();
                }
                0
            }
            7 => libc::time(std::ptr::null_mut()) as i64,
            _ => 0,
        }
    }

    unsafe extern "C" fn fe_summary_cb(_raw: *mut c_void, s: *mut crate::binrpc::FrontendSummary) {
        if s.is_null() {
            return;
        }
        let out = &mut *s;
        let sm = crate::statmeter::active_statmeter();
        let ws = active_workstate();
        let (up_sec, dn_sec, all_bytes, stand_by) = sm
            .as_deref()
            .map_or((0, 0, 0, false), |m| m.total_stat_snapshot());
        out.current_download_speed = sm.as_deref().map_or(0, |m| m.current_download_speed());
        out.pause_download = c_int::from(ws.as_deref().is_some_and(|w| w.pause_download()));
        out.download_limit = ws.as_deref().map_or(0, |w| w.speed_limit());
        out.thread_count = current_thread_count();
        out.up_time_sec = up_sec;
        out.dn_time_sec = dn_sec;
        out.all_bytes = all_bytes;
        out.stand_by = c_int::from(stand_by);
        out.post_job_count = 0;
        out.remaining_size = 0;
        if let Some(qh) = queue_host() {
            qh.with_locked_queue(|dq_ptr| {
                if dq_ptr == 0 {
                    return;
                }
                let q_len = qh.queue_len(dq_ptr);
                let mut post_jobs = 0;
                for idx in 0..q_len {
                    let nzb_ptr = qh.queue_at(dq_ptr, idx);
                    if nzb_ptr != 0 && qh.nzb_snap(nzb_ptr, false).has_post_info {
                        post_jobs += 1;
                    }
                }
                out.post_job_count = post_jobs;
                let (rem, _) = qh.dq_calc_remaining_size(dq_ptr);
                out.remaining_size = rem;
            });
        }
    }

    unsafe extern "C" fn fe_messages_cb(
        _raw: *mut c_void,
        tail_mode: c_int,
        arg: u32,
        out_skipped: *mut c_int,
        emit_ctx: *mut c_void,
        emit: crate::ncurses_format::FrontendEmitMsgFn,
    ) {
        let lg = crate::log::active_log();
        crate::log::nzbget_rs_log_collect(
            0,
            lg.as_deref().map_or(std::ptr::null(), |l| l as *const _),
            tail_mode,
            0,
            arg,
            out_skipped,
            emit_ctx,
            emit as usize,
            0,
        );
    }

    unsafe extern "C" fn fe_queue_cb(
        _raw: *mut c_void,
        emit_ctx: *mut c_void,
        emit_nzb: crate::ncurses_format::FrontendEmitNzbFn,
        emit_file: crate::ncurses_format::FrontendEmitFileFn,
    ) {
        let Some(qh) = queue_host() else {
            return;
        };
        qh.with_locked_queue(|dq_ptr| {
            if dq_ptr == 0 {
                return;
            }
            let q_len = qh.queue_len(dq_ptr);
            for idx in 0..q_len {
                let nzb_ptr = qh.queue_at(dq_ptr, idx);
                if nzb_ptr == 0 {
                    continue;
                }
                let ns = qh.nzb_snap(nzb_ptr, false);
                let name_c = to_nul_cvec(&ns.name);
                emit_nzb(
                    emit_ctx,
                    ns.id,
                    ns.priority,
                    ns.active_downloads,
                    ns.size,
                    ns.remaining_size,
                    ns.paused_size,
                    ns.paused_file_count,
                    ns.remaining_par_count,
                    name_c.as_ptr(),
                );
                let f_len = qh.nzb_file_len(nzb_ptr);
                for f_idx in 0..f_len {
                    let f_ptr = qh.nzb_file_at(nzb_ptr, f_idx);
                    if f_ptr == 0 {
                        continue;
                    }
                    let fs = qh.file_snap(f_ptr);
                    let fname_c = to_nul_cvec(&fs.filename);
                    emit_file(
                        emit_ctx,
                        idx,
                        fs.id,
                        fs.active_downloads,
                        c_int::from(fs.paused),
                        fs.size,
                        fs.remaining_size,
                        fname_c.as_ptr(),
                    );
                }
            }
        });
    }

    let mut br = FeBridgeCtx {
        mode,
        stopped,
        exit_cb,
    };
    let env = crate::ncurses_format::NzbgetRsFrontendEnv {
        ctx: (&mut br as *mut FeBridgeCtx<'_>).cast(),
        remote_mode: c_int::from(opts.as_deref().is_some_and(|o| o.remote_client_mode())),
        update_interval: opts.as_deref().map_or(100, |o| o.scalars().update_interval),
        use_color: c_int::from(use_color),
        show_nzbname: c_int::from(
            opts.as_deref()
                .is_some_and(|o| o.scalars().curses_nzb_name != 0),
        ),
        show_timestamp: c_int::from(
            opts.as_deref()
                .is_some_and(|o| o.scalars().curses_time != 0),
        ),
        group_files: c_int::from(
            opts.as_deref()
                .is_some_and(|o| o.scalars().curses_group != 0),
        ),
        time_correction: opts.as_deref().map_or(0, |o| o.scalars().time_correction),
        path_sep: crate::paths::PATH_SEPARATOR as u8,
        control_ip: opts
            .as_deref()
            .and_then(|o| o.str_cstr(14))
            .map_or(std::ptr::null(), CStr::as_ptr),
        control_port: opts.as_deref().map_or(0, |o| o.scalars().control_port),
        control_username: opts
            .as_deref()
            .and_then(|o| o.str_cstr(15))
            .map_or(std::ptr::null(), CStr::as_ptr),
        control_password: opts
            .as_deref()
            .and_then(|o| o.str_cstr(16))
            .map_or(std::ptr::null(), CStr::as_ptr),
        write_stdout_cb: Some(fe_stdout_cb),
        action_cb: Some(fe_action_cb),
        local_summary_cb: Some(fe_summary_cb),
        local_messages_cb: Some(fe_messages_cb),
        local_queue_cb: Some(fe_queue_cb),
    };
    unsafe {
        crate::ncurses_format::frontend_op(2 + mode, &env);
    }

    if mode == 0 || mode == 1 {
        crate::log::log_active_bytes(3, b"Exiting LoggableFrontend-loop");
    } else if mode == 2 {
        crate::log::log_active_bytes(3, b"Exiting NCursesFrontend-loop");
    }
}
