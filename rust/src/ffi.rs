//! C ABI. Strings come in as NUL-terminated C strings; results go out as a
//! buffer the caller copies and then frees with nzbget_rs_free.

use std::ffi::{c_char, c_int, c_void, CStr, CString};

#[path = "tlscipher/native.rs"]
pub(crate) mod tls_cipher_native;

#[no_mangle]
pub extern "C" fn nzbget_rs_tls_cipher_pin(protocol: c_int, bits: u32, flags: u32) -> c_int {
    use crate::tlscipher::{eligible, Protocol, Suite};
    let version = match protocol { 12 => Protocol::Tls12, 13 => Protocol::Tls13, _ => return 0 };
    if bits > u16::MAX.into() { return 0; }
    let suite = Suite { name: String::new(), algorithm: String::new(), protocol: version, security_bits: bits as u16,
        aead: flags & 1 != 0, certificate: flags & 2 != 0, ephemeral: flags & 4 != 0 };
    if eligible(&suite) { protocol } else { 0 }
}

fn cipher_response(result: Result<String, crate::tlscipher::Error>) -> RsBuf {
    match result {
        Ok(json) => into_buf(json.into_bytes()),
        Err(error) => {
            let mut message = Vec::new();
            crate::escape::json_encode(error.0.as_bytes(), &mut message);
            let mut out = b"{\"Error\":\"".to_vec(); out.extend(message); out.extend_from_slice(b"\"}"); into_buf(out)
        }
    }
}

/// # Safety
/// backend points to a complete non-null callback table. Its context transfers
/// to Rust exactly once, including rejected/duplicate starts. Callbacks are
/// thread-safe for the documented single-worker use and must never unwind.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_tls_cipher_start(backend: *const tls_cipher_native::Io) -> RsBuf {
    if backend.is_null() { return cipher_response(Err(crate::tlscipher::Error("Missing TLS backend."))); }
    cipher_response(tls_cipher_native::start(unsafe { *backend }))
}
/// # Safety
/// id is null or a readable NUL-terminated string; null/empty queries current job.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_tls_cipher_status(id: *const c_char) -> RsBuf {
    let id = if id.is_null() { None } else {
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
    if id.is_null() { return cipher_response(Err(crate::tlscipher::Error("Missing cipher job ID."))); }
    let Ok(id) = unsafe { CStr::from_ptr(id) }.to_str() else { return cipher_response(Err(crate::tlscipher::Error("Invalid cipher job ID."))); };
    cipher_response(tls_cipher_native::service().cancel(id).and_then(|()| tls_cipher_native::service().status(Some(id))))
}
#[no_mangle]
pub extern "C" fn nzbget_rs_tls_cipher_shutdown() { tls_cipher_native::service().shutdown(); }

#[cfg(test)]
mod tls_cipher_abi_tests {
    use super::*;
    #[test]
    fn cipher_pin_validates_protocol_security_and_authentication_flags() {
        assert_eq!(nzbget_rs_tls_cipher_pin(13, 128, 3), 13);
        assert_eq!(nzbget_rs_tls_cipher_pin(12, 128, 7), 12);
        for (protocol, bits, flags) in [(11, 256, 7), (12, 128, 3), (13, 64, 7), (13, 128, 1), (13, 128, 2), (13, u32::MAX, 7)] {
            assert_eq!(nzbget_rs_tls_cipher_pin(protocol, bits, flags), 0);
        }
    }
    #[test]
    fn invalid_utf8_status_id_is_an_error_instead_of_current_job() {
        let bytes = [255u8, 0];
        let response = unsafe { nzbget_rs_tls_cipher_status(bytes.as_ptr().cast()) };
        let text = unsafe { std::slice::from_raw_parts(response.data.cast::<u8>(), response.len) };
        assert!(text.starts_with(b"{\"Error\":"));
        unsafe { nzbget_rs_free(response); }
    }
}

#[repr(C)]
pub struct RsBuf {
    pub data: *mut c_char,
    pub len: usize,
    cap: usize,
}

pub(crate) fn into_buf(mut v: Vec<u8>) -> RsBuf {
    v.push(0);
    let len = v.len() - 1;
    let mut v = std::mem::ManuallyDrop::new(v);
    RsBuf { data: v.as_mut_ptr() as *mut c_char, len, cap: v.capacity() }
}

pub(crate) unsafe fn input<'a>(raw: *const c_char) -> &'a [u8] {
    if raw.is_null() { &[] } else { CStr::from_ptr(raw).to_bytes() }
}

// ---- twins and alts (twin.rs); lists cross the ABI as text lines ----

fn with_store<T>(f: impl FnOnce(&mut crate::twin::Store) -> T) -> Option<T> {
    let mut guard = crate::twin::STORE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_mut().map(f)
}

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

unsafe fn str_arg(raw: *const c_char) -> String {
    String::from_utf8_lossy(input(raw)).into_owned()
}

unsafe fn bytes_arg<'a>(data: *const u8, len: usize) -> &'a [u8] {
    if data.is_null() { &[] } else { std::slice::from_raw_parts(data, len) }
}

/// Opens (or reopens) the store of par2 file lists at `path`.
///
/// # Safety
/// `path` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_open(path: *const c_char) {
    let store = crate::twin::Store::open(path_arg(path));
    *crate::twin::STORE.lock().unwrap_or_else(|e| e.into_inner()) = Some(store);
}

/// Parses the par2 data, stores its file list for download `id` (bound to the
/// posting: "fingerprint#posting", when it has files) and returns "fingerprint\tcomplete" (an
/// empty fingerprint for none; complete is 1 or 0).
///
/// # Safety
/// `posting` is null or a valid NUL-terminated string; `data` points to `len`
/// readable bytes (or is null with `len` 0).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_put_par2(id: i32, posting: *const c_char, data: *const u8, len: usize)
    -> RsBuf {
    let (mut sigs, complete) = crate::twin::par2_set(bytes_arg(data, len));
    let posting = str_arg(posting);
    // a partial list adds to the posting's partial list from an earlier try (that
    // one may have seen articles this one didn't); a complete one replaces it
    if !complete {
        if let Some(earlier) = with_store(|store| store.partial_of(id, &posting).map(|l| l.sigs.clone())).flatten() {
            sigs = crate::twin::merge(&earlier, &sigs);
        }
    }
    let print = crate::twin::fingerprint(&sigs);
    if !sigs.is_empty() {
        // bound to the posting: its fingerprint and the posting's own id (a hash of
        // its message-ids), which differently named twins don't share
        let identity = format!("{print}#{posting}");
        let listed = crate::twin::Listed { sigs, complete, print: identity };
        with_store(|store| store.put(id, listed));
    }
    // "fingerprint\tcomplete": a partial list is fetched again
    into_buf(format!("{print}\t{}", u8::from(complete)).into_bytes())
}

/// What's stored of download `id` for fingerprint `print`: 0 nothing, 1 a
/// partial list, 2 a complete one.
///
/// # Safety
/// `print` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_has(id: i32, print: *const c_char) -> i32 {
    let print = str_arg(print);
    with_store(|store| {
        let listed = store.get(id, &print);
        if listed.sigs.is_empty() { 0 } else if listed.complete { 2 } else { 1 }
    })
    .unwrap_or(0)
}

/// Keeps the stored lists of the postings given as "id\tfingerprint" lines
/// (those in queue and history), drops the rest and rewrites the file.
///
/// # Safety
/// `lines` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_prune(lines: *const c_char) {
    let live: Vec<(i32, String)> = String::from_utf8_lossy(input(lines))
        .lines()
        .filter_map(|line| {
            let (id, print) = line.split_once('\t')?;
            Some((id.parse().ok()?, print.to_string()))
        })
        .collect();
    with_store(|store| store.prune(&live));
}

/// The fingerprint of `count` files given as md5 texts and lengths (the
/// names are not part of it).
///
/// # Safety
/// `md5s` points to `count` valid NUL-terminated strings (null entries are
/// empty) and `lengths` to `count` values; both may be null for 0.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_fingerprint(md5s: *const *const c_char, lengths: *const u64, count: usize)
    -> RsBuf {
    if count > 0 && (md5s.is_null() || lengths.is_null()) {
        return into_buf(Vec::new());
    }
    let files = (0..count).map(|i| (String::from_utf8_lossy(input(*md5s.add(i))), *lengths.add(i)));
    into_buf(crate::twin::fingerprint_cow(files).into_bytes())
}

/// The block size of the par2 set in the data, 0 if none.
///
/// # Safety
/// `data` points to `len` readable bytes (or is null with `len` 0).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par2_block_size(data: *const u8, len: usize) -> u64 {
    crate::twin::block_size(bytes_arg(data, len))
}

/// The recovery blocks a par2-volume's name gives ("x.vol03+04.par2": 4), -1 if none.
///
/// # Safety
/// `filename` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par2_volume_blocks(filename: *const c_char) -> i32 {
    crate::twin::volume_blocks_bytes(input(filename)).map_or(-1, |b| i32::try_from(b).unwrap_or(i32::MAX))
}

/// The label of download `dupe` against `primary` (each by id and fingerprint):
/// "twin", "twin:N/M", "alt", or "" when it can't be told.
///
/// # Safety
/// `primary_print` and `dupe_print` are null or valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_kind(primary: i32, primary_print: *const c_char, dupe: i32,
    dupe_print: *const c_char) -> RsBuf {
    let (primary_print, dupe_print) = (str_arg(primary_print), str_arg(dupe_print));
    let kind = with_store(|store| crate::twin::kind(store.get(primary, &primary_print), store.get(dupe, &dupe_print)))
        .unwrap_or_default();
    into_buf(kind.into_bytes())
}

/// The file of download `donor` holding the bytes of our file `target` (or
/// `target_alt`) of download `own` (each by id and fingerprint): its name; "-"
/// when the donor's complete list has no file with these bytes; "" when not known.
///
/// # Safety
/// Every pointer is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_match(own: i32, own_print: *const c_char, target: *const c_char,
    target_alt: *const c_char, donor: i32, donor_print: *const c_char) -> RsBuf {
    let (own_print, donor_print) = (str_arg(own_print), str_arg(donor_print));
    let (target, target_alt) = (str_arg(target), str_arg(target_alt));
    let found = with_store(|store| {
        crate::twin::match_by_content(store.get(own, &own_print), &target, &target_alt, store.get(donor, &donor_print))
    })
    .flatten();
    into_buf(match found {
        None => Vec::new(),
        Some(name) if name.is_empty() => b"-".to_vec(),
        Some(name) => name.into_bytes(),
    })
}

/// # Safety
/// `buf` came from an nzbget_rs_* function and wasn't freed yet.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_free(buf: RsBuf) {
    if !buf.data.is_null() {
        drop(Vec::from_raw_parts(buf.data as *mut u8, buf.len + 1, buf.cap));
    }
}

/// Base64-decodes `input` into `output` (WebUtil::DecodeBase64); a length of
/// 0 or less means up to the NUL, with the legacy uint32 length truncation.
/// `output` may be `input`.
///
/// # Safety
/// `input` is null or readable for its length (or through its NUL). Null input
/// returns zero without accessing output. Otherwise `output` is writable for
/// `len / 4 * 3` bytes and is `input` or doesn't overlap it. No terminator is
/// written. Both buffers remain caller-owned.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_decode_base64(input: *const c_char, length: c_int, output: *mut c_char) -> u32 {
    if input.is_null() {
        return 0;
    }
    // C++ stores strlen's result in uint32 before reading any quartets.
    let len = if length > 0 { length as usize } else { CStr::from_ptr(input).to_bytes().len() as u32 as usize };
    crate::decode::base64_in_place(input.cast(), len, output.cast()) as u32
}

/// Decodes a JSON string body in place (WebUtil::JsonDecode).
///
/// # Safety
/// `raw` is null or a writable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_json_decode(raw: *mut c_char) {
    if raw.is_null() {
        return;
    }
    let len = CStr::from_ptr(raw).to_bytes().len();
    let buf = std::slice::from_raw_parts_mut(raw.cast::<u8>(), len);
    let n = crate::decode::json_decode(buf);
    *raw.add(n) = 0;
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

/// ZLib::GZipLen: returns upper bound on gzip-compressed output size.
#[no_mangle]
pub extern "C" fn nzbget_rs_gzip_len(input_len: c_int) -> u32 {
    let n = input_len.max(0) as u32;
    n.saturating_add(n >> 12)
        .saturating_add(n >> 14)
        .saturating_add(n >> 25)
        .saturating_add(32)
}

/// ZLib::GZip: compresses `in_buf[..in_len]` into `out_buf[..out_len]` in gzip format via `flate2`.
///
/// # Safety
/// `in_buf` is readable for `in_len` bytes; `out_buf` is writable for `out_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_gzip_compress(
    in_buf: *const u8,
    in_len: c_int,
    out_buf: *mut u8,
    out_len: c_int,
) -> u32 {
    if out_buf.is_null() || out_len <= 0 || in_len < 0 || (in_len > 0 && in_buf.is_null()) {
        return 0;
    }
    let in_slice = if in_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(in_buf, in_len as usize)
    };
    let out_slice = std::slice::from_raw_parts_mut(out_buf, out_len as usize);
    let mut comp = flate2::Compress::new_gzip(flate2::Compression::default(), 15);
    match comp.compress(in_slice, out_slice, flate2::FlushCompress::Finish) {
        Ok(flate2::Status::StreamEnd) => comp.total_out() as u32,
        _ => 0,
    }
}

// ---- Terminal / TUI FFI (replacing C ncurses) ----

#[no_mangle]
pub extern "C" fn nzbget_rs_term_init() -> c_int {
    crate::ncurses_format::term_init()
}

#[no_mangle]
pub extern "C" fn nzbget_rs_term_shutdown() {
    crate::ncurses_format::term_shutdown()
}

#[no_mangle]
pub extern "C" fn nzbget_rs_term_refresh(row: c_int, col: c_int) {
    crate::ncurses_format::term_refresh(row, col);
}

#[no_mangle]
pub extern "C" fn nzbget_rs_term_read_key() -> c_int {
    crate::ncurses_format::term_read_key()
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

/// WebUtil::XmlDecode, in place (rust/src/text.rs).
///
/// # Safety
/// `raw` is null or a writable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xml_decode(raw: *mut c_char) {
    in_place(raw, |b| crate::text::xml_decode(b, &crate::util::byte_tolower_locale))
}

/// WebUtil::XmlFindTag: a pointer into `xml` and the value length, or null
/// (`value_length` untouched).
///
/// # Safety
/// `xml` and `tag` are null or NUL-terminated; `value_length` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xml_find_tag(xml: *const c_char, tag: *const c_char, value_length: *mut c_int) -> *const c_char {
    if xml.is_null() || tag.is_null() || value_length.is_null() {
        return std::ptr::null();
    }
    match crate::webutil::xml_find_tag(input(xml), input(tag)) {
        Some((start, len)) => {
            // the C++ (int) of a pointer difference
            *value_length = len as c_int;
            xml.add(start)
        }
        None => std::ptr::null(),
    }
}

/// Util::FormatSize; free the result with nzbget_rs_free.
#[no_mangle]
pub extern "C" fn nzbget_rs_format_size(size: i64) -> RsBuf {
    into_buf(crate::util::format_size(size))
}

/// Util::ReduceStr, in place.
///
/// # Safety
/// `s` is null or a writable NUL-terminated string; `from` and `to` are null
/// or NUL-terminated. Operands may alias `s`; any null argument is a no-op.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_reduce_str(s: *mut c_char, from: *const c_char, to: *const c_char) {
    crate::util::reduce_str(s, from, to)
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

/// A FeedFilter's view of the C++ feed item (rust/src/feedfilter.rs).
#[repr(C)]
pub struct FeedItemCallbacks {
    pub user: *mut std::ffi::c_void,
    /// a field's text (null for a null C string) and number; `attr` is the
    /// attribute name for "attr-" fields
    pub field: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char, *mut *const c_char, *mut i64)>,
    /// GetSeason (0) or GetEpisode (1) after the title is parsed
    pub season_episode: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int) -> *const c_char>,
    pub apply: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const FeedOptions)>,
    pub set_match: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, c_int)>,
}

/// A matched rule's options for ApplyOptions; strings may be null.
#[repr(C)]
pub struct FeedOptions {
    pub has_pause: c_int,
    pub pause: c_int,
    pub has_category: c_int,
    pub category: *const c_char,
    pub has_priority: c_int,
    pub priority: c_int,
    pub has_add_priority: c_int,
    pub add_priority: c_int,
    pub has_dupe_score: c_int,
    pub dupe_score: c_int,
    pub has_add_dupe_score: c_int,
    pub add_dupe_score: c_int,
    pub has_build_dupe_key: c_int,
    /// rageid, tvdbid, tvmazeid, series
    pub ids: [*const c_char; 4],
    pub has_dupe_key: c_int,
    pub dupe_key: *const c_char,
    pub has_add_dupe_key: c_int,
    pub add_dupe_key: *const c_char,
    pub has_dupe_mode: c_int,
    pub dupe_mode: c_int,
}

struct CItem<'c> {
    cb: &'c FeedItemCallbacks,
}

unsafe fn c_text(p: *const c_char) -> Option<Vec<u8>> {
    (!p.is_null()).then(|| CStr::from_ptr(p).to_bytes().to_vec())
}

impl crate::feedfilter::Item for CItem<'_> {
    fn field(&mut self, field: crate::feedfilter::Field, attr: &[u8]) -> (Option<Vec<u8>>, i64) {
        let attr = std::ffi::CString::new(attr).unwrap_or_default();
        let mut s: *const c_char = std::ptr::null();
        let mut n: i64 = 0;
        unsafe {
            (self.cb.field.expect("callbacks validated"))(self.cb.user, field as c_int, attr.as_ptr(), &mut s, &mut n);
            (c_text(s), n)
        }
    }

    fn season_episode(&mut self, episode: bool) -> Option<Vec<u8>> {
        unsafe { c_text((self.cb.season_episode.expect("callbacks validated"))(self.cb.user, episode as c_int)) }
    }

    fn apply(&mut self, o: &crate::feedfilter::Applied<'_>) {
        // C strings for the call; None (a null C string) stays null
        let keep: Vec<Option<std::ffi::CString>> = [
            o.category.flatten(),
            o.dupe_key.flatten(),
            o.add_dupe_key.flatten(),
        ]
        .into_iter()
        .chain(o.build_dupe_key.unwrap_or_default())
        .map(|v| v.map(|b| std::ffi::CString::new(b).unwrap_or_default()))
        .collect();
        let ptr = |k: usize| keep[k].as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
        let flag = |b: bool| b as c_int;
        let opts = FeedOptions {
            has_pause: flag(o.pause.is_some()),
            pause: flag(o.pause.unwrap_or(false)),
            has_category: flag(o.category.is_some()),
            category: ptr(0),
            has_priority: flag(o.priority.is_some()),
            priority: o.priority.unwrap_or(0),
            has_add_priority: flag(o.add_priority.is_some()),
            add_priority: o.add_priority.unwrap_or(0),
            has_dupe_score: flag(o.dupe_score.is_some()),
            dupe_score: o.dupe_score.unwrap_or(0),
            has_add_dupe_score: flag(o.add_dupe_score.is_some()),
            add_dupe_score: o.add_dupe_score.unwrap_or(0),
            has_build_dupe_key: flag(o.build_dupe_key.is_some()),
            ids: [ptr(3), ptr(4), ptr(5), ptr(6)],
            has_dupe_key: flag(o.dupe_key.is_some()),
            dupe_key: ptr(1),
            has_add_dupe_key: flag(o.add_dupe_key.is_some()),
            add_dupe_key: ptr(2),
            has_dupe_mode: flag(o.dupe_mode.is_some()),
            dupe_mode: o.dupe_mode.map_or(0, |m| m as c_int),
        };
        unsafe { (self.cb.apply.expect("callbacks validated"))(self.cb.user, &opts) }
    }

    fn set_match(&mut self, status: i32, rule: i32) {
        unsafe { (self.cb.set_match.expect("callbacks validated"))(self.cb.user, status, rule) }
    }
}

/// FeedFilter(filter): a compiled filter; free it with
/// nzbget_rs_feed_filter_free.
///
/// # Safety
/// `filter` is null (an empty filter) or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_filter_new(filter: *const c_char) -> *mut crate::feedfilter::FeedFilter {
    Box::into_raw(Box::new(crate::feedfilter::FeedFilter::new(input(filter))))
}

/// # Safety
/// `filter` is null or from nzbget_rs_feed_filter_new, not yet freed and
/// not currently being matched.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_filter_free(filter: *mut crate::feedfilter::FeedFilter) {
    if !filter.is_null() {
        drop(Box::from_raw(filter));
    }
}

/// FeedFilter::Match.
///
/// # Safety
/// `filter` is null or an exclusively accessed handle from
/// nzbget_rs_feed_filter_new. `item` is null or a valid callback table for
/// the duration of this call. Callbacks must not unwind, reenter/free this
/// filter, or invalidate the table. Returned strings must be null or valid
/// NUL-terminated strings until the next callback; passed strings are only
/// borrowed for the callback. Regex handles must remain valid across calls.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_filter_match(filter: *mut crate::feedfilter::FeedFilter, item: *const FeedItemCallbacks) {
    if filter.is_null() || item.is_null() {
        return;
    }
    let cb = &*item;
    if cb.field.is_none() || cb.season_episode.is_none() || cb.apply.is_none() || cb.set_match.is_none()
    {
        return;
    }
    let mut c_item = CItem { cb };
    (*filter).matches(&mut c_item);
}

unsafe fn bytes<'a>(p: *const c_char, len: usize) -> &'a [u8] {
    if p.is_null() || len == 0 { &[] } else { std::slice::from_raw_parts(p.cast(), len) }
}

/// Deobfuscation::IsExcessivelyObfuscated (rust/src/deobfuscation.rs).
///
/// # Safety
/// `s` is null or readable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_is_excessively_obfuscated(s: *const c_char, len: usize) -> c_int {
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
        is_seven_zip_ext, is_rar_ext, is_rar_volume_ext, is_numeric_volume_ext, is_all_digits_ext, is_archive_ext,
        is_disc_structure_ext, is_disc_structure_dir, is_disc_descriptor_ext, is_disc_image_ext,
        is_generic_disc_image_ext, is_clutter_dir, is_clutter_file, is_parity_ext, is_video_ext, is_audio_ext,
        is_subtitle_ext, is_nfo_ext, is_book_ext, is_image_ext, is_sample_stem, is_seven_zip_file, is_rar_file,
        is_archive_file, is_sample_file, is_known_collection_ext,
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
pub unsafe extern "C" fn nzbget_rs_path_text(op: c_int, s: *const c_char, len: usize, flag: c_int) -> RsBuf {
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
            if name.is_empty() && path.len() == b.len() { usize::MAX } else { path.len() }
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

/// WebProcessor::ParseHeaders for one line (rust/src/webserver.rs): the kind
/// (0 other, 1 Content-Length, 2 credentials, 3 credentials too long,
/// 4 Accept-Encoding, 5 Origin, 6 Auth-Token cookie, 7 X-Forwarded-For,
/// 8 If-None-Match, 9 keep-alive, 10 end of headers), its value as offset
/// and length in the line, and a number (Content-Length, gzip).
///
/// # Safety
/// `line` is null (empty) or readable for `len` initialized bytes. Header
/// tests stop at the first NUL; fixed-offset values may start beyond it in
/// unusual locales. Outputs are null or writable and disjoint from inputs.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_web_header(
    line: *const c_char,
    len: usize,
    auth_info_empty: c_int,
    value_start: *mut usize,
    value_len: *mut usize,
    number: *mut c_int,
) -> c_int {
    use crate::webserver::Header;
    let l = bytes(line, len);
    let h = crate::webserver::header(l, auth_info_empty != 0);
    let (kind, value, n): (c_int, &[u8], c_int) = match h {
        Header::Other => (0, &[], 0),
        Header::ContentLength(n) => (1, &[], n),
        Header::Auth(v) => (2, v, 0),
        Header::AuthTooBig => (3, &[], 0),
        Header::AcceptEncoding { gzip } => (4, &[], gzip as c_int),
        Header::Origin(v) => (5, v, 0),
        Header::AuthToken(v) => (6, v, 0),
        Header::ForwardedFor(v) => (7, v, 0),
        Header::IfNoneMatch(v) => (8, v, 0),
        Header::KeepAlive => (9, &[], 0),
        Header::End => (10, &[], 0),
    };
    if !value_start.is_null() {
        // the value's place in the line, an empty value's too (an empty
        // "Authorization: Basic " decodes nothing, not the whole line)
        let at = value.as_ptr() as usize;
        let base = l.as_ptr() as usize;
        *value_start = if at >= base && at <= base + l.len() { at - base } else { l.len() };
    }
    if !value_len.is_null() {
        *value_len = value.len();
    }
    if !number.is_null() {
        *number = n;
    }
    kind
}

/// WebProcessor::ParseUrl: the URL to dispatch, or with `redirect` set the
/// location to redirect to; `auth` gets the credentials of the URL (data
/// null for none). Free both with nzbget_rs_free.
///
/// # Safety
/// `url` is null (empty) or readable for `len` bytes, stopping at its first
/// NUL. `redirect` and `auth` are null or writable, disjoint from the input
/// and each other. The caller owns returned buffers; free with nzbget_rs_free.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_web_parse_url(url: *const c_char, len: usize, redirect: *mut c_int, auth: *mut RsBuf) -> RsBuf {
    let none = || RsBuf { data: std::ptr::null_mut(), len: 0, cap: 0 };
    if !auth.is_null() {
        *auth = none();
    }
    match crate::webserver::parse_url(bytes(url, len)) {
        crate::webserver::Url::Redirect(loc) => {
            if !redirect.is_null() {
                *redirect = 1;
            }
            into_buf(loc)
        }
        crate::webserver::Url::Go { url, auth: a } => {
            if !redirect.is_null() {
                *redirect = 0;
            }
            if let (Some(a), false) = (a, auth.is_null()) {
                *auth = into_buf(a);
            }
            into_buf(url)
        }
    }
}

/// CheckCredentials' options and inputs (null strings for unset options).
#[repr(C)]
pub struct WebCredentialsC {
    /// control, restricted, add: username, password
    pub users: [*const c_char; 6],
    pub authorized_ip: *const c_char,
    pub remote_addr: *const c_char,
    pub auth_info: *const c_char,
    pub auth_token: *const c_char,
    pub server_tokens: [*const c_char; 3],
}

/// CheckCredentials' result.
#[repr(C)]
pub struct WebCheckC {
    pub authorized: c_int,
    /// EUserAccess, or -1 to leave it
    pub access: c_int,
    /// where m_authInfo is cut (its ':'), or -1
    pub auth_cut: isize,
    pub warn: c_int,
}

unsafe fn opt(p: *const c_char) -> Option<&'static [u8]> {
    (!p.is_null()).then(|| CStr::from_ptr(p).to_bytes())
}

/// WebProcessor::CheckCredentials.
///
/// # Safety
/// `input` is null or readable; its strings are null or NUL-terminated.
/// `out` is null or writable and disjoint from all input storage. Null input denies access.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_web_check_credentials(input: *const WebCredentialsC, out: *mut WebCheckC) {
    if out.is_null() {
        return;
    }
    *out = WebCheckC { authorized: 0, access: -1, auth_cut: -1, warn: 0 };
    if input.is_null() {
        return;
    }
    let i = &*input;
    let lower = crate::wildmask::Lower::Fold(&crate::util::wild_mask_fold_locale);
    let o = crate::webserver::Credentials {
        control_username: opt(i.users[0]),
        control_password: opt(i.users[1]),
        restricted_username: opt(i.users[2]),
        restricted_password: opt(i.users[3]),
        add_username: opt(i.users[4]),
        add_password: opt(i.users[5]),
        authorized_ip: opt(i.authorized_ip),
    };
    let tokens = [opt(i.server_tokens[0]).unwrap_or_default(), opt(i.server_tokens[1]).unwrap_or_default(), opt(i.server_tokens[2]).unwrap_or_default()];
    let r = crate::webserver::check_credentials(
        &o,
        opt(i.remote_addr).unwrap_or_default(),
        opt(i.auth_info).unwrap_or_default(),
        opt(i.auth_token).unwrap_or_default(),
        tokens,
        &lower,
    );
    *out = WebCheckC {
        authorized: r.authorized as c_int,
        access: r.access.unwrap_or(-1),
        auth_cut: r.auth_cut.map_or(-1, |k| k as isize),
        warn: r.warn as c_int,
    };
}

/// Decoder (rust/src/decoder.rs) with built-in Rust `nntmux-yenc` + `crc32fast` (or custom callbacks).
#[no_mangle]
pub extern "C" fn nzbget_rs_decoder_new(decode: Option<crate::decoder::DecodeFn>, crc: Option<crate::decoder::CrcFn>) -> *mut crate::decoder::Decoder {
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
    if let Some(d) = d.as_mut() { d.clear(); }
}

/// Decoder::DecodeBuffer.
///
/// # Safety
/// `d` is null or a live, exclusively borrowed handle from nzbget_rs_decoder_new; `buffer` is writable as the C++
/// Decoder needed (see Decoder::decode_buffer), disjoint from `d`. NULL
/// input or a negative length is a no-op. Panics abort at this C ABI boundary.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_decoder_decode(d: *mut crate::decoder::Decoder, buffer: *mut c_char, len: c_int) -> c_int {
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
    d.as_mut().map_or(crate::decoder::Status::UnknownError, |d| d.check()) as c_int
}

/// The decoder's settings and fields: `which` 0 crc check, 1 raw mode
/// (set to `value`).
///
/// # Safety
/// `d` is null or a live, exclusively borrowed handle from nzbget_rs_decoder_new.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_decoder_set(d: *mut crate::decoder::Decoder, which: c_int, value: c_int) {
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
pub unsafe extern "C" fn nzbget_rs_decoder_get(d: *mut crate::decoder::Decoder, which: c_int) -> i64 {
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
pub unsafe extern "C" fn nzbget_rs_decoder_filename(d: *mut crate::decoder::Decoder) -> *const c_char {
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
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_scheduler_check(
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
    let tasks: &mut [crate::scheduler::Task] =
        if count == 0 { &mut [] } else { std::slice::from_raw_parts_mut(tasks, count) };
    // Keep the retry transactional, including each task's last execution time.
    let mut updated_tasks = tasks.to_vec();
    let mut updated_last_check = *last_check;
    let r = crate::scheduler::check_tasks(&mut updated_tasks, &mut updated_last_check, current, current_offset, last_check_offset, |time| {
        let mut fields = std::mem::MaybeUninit::uninit();
        gmtime(time, fields.as_mut_ptr());
        fields.assume_init()
    });
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

/// The request from `p` through its NUL, writable.
unsafe fn request_buf<'a>(p: *mut c_char) -> &'a mut [u8] {
    let len = CStr::from_ptr(p).to_bytes().len();
    std::slice::from_raw_parts_mut(p.cast::<u8>(), len + 1)
}

/// XmlCommand::PrepareParams for a JSON POST: the read position after
/// `"params"`, or `request` emptied (and returned) without it.
///
/// # Safety
/// `request` is null or a writable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rpc_skip_to_params(request: *mut c_char) -> *mut c_char {
    if request.is_null() {
        return request;
    }
    request.add(crate::rpcparams::skip_to_params(request_buf(request)))
}

/// XmlCommand::NextParamAsInt (`what` 0), NextParamAsBool (1, as 0/1 in
/// `*int_value`) and NextParamAsStr (2, a pointer into the request in
/// `*str_value`): parses the next parameter at `*request` in place and moves
/// `*request` past it. Returns 1 with a value, else 0 (outputs untouched).
///
/// # Safety
/// Non-null `request` points to null or to a pointer into a writable
/// NUL-terminated request; non-null `int_value`/`str_value` are writable for
/// the kinds that use them. The pointer slots, value output and request buffer
/// must be disjoint. All storage stays caller-owned; returned strings borrow
/// the request. Panics abort rather than unwinding across the ABI.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rpc_next_param(
    request: *mut *mut c_char,
    get: c_int,
    json: c_int,
    what: c_int,
    int_value: *mut c_int,
    str_value: *mut *mut c_char,
) -> c_int {
    use crate::rpcparams::{Kind, Raw};
    if request.is_null() || (*request).is_null()
        || !matches!(what, 0..=2)
        || (what != 2 && int_value.is_null())
        || (what == 2 && str_value.is_null())
    {
        return 0;
    }
    let base = *request;
    // read in place: JSON and GET values without measuring the rest of the request
    let raw = Raw::new(base.cast::<u8>());
    let kind = if get != 0 {
        Kind::Get
    } else if json != 0 {
        Kind::Json
    } else {
        Kind::Xml
    };
    let (pos, ok) = match what {
        0 => {
            let (pos, v) = raw.next_int(kind);
            if let Some(v) = v {
                *int_value = v;
            }
            (pos, v.is_some())
        }
        1 => {
            let (pos, v) = raw.next_bool(kind, json != 0);
            if let Some(v) = v {
                *int_value = v as c_int;
            }
            (pos, v.is_some())
        }
        2 => {
            let (pos, v) = raw.next_str(kind);
            if let Some(v) = v {
                *str_value = base.add(v);
            }
            (pos, v.is_some())
        }
        _ => (0, false),
    };
    *request = base.add(pos);
    ok as c_int
}

/// XmlRpcProcessor::Execute: the protocol of an RPC URL (0 if none).
///
/// # Safety
/// `url` is null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rpc_protocol(url: *const c_char) -> c_int {
    crate::rpcroute::protocol(input(url))
}

/// XmlRpcProcessor::Dispatch's parsing: writes the method name (NUL-terminated,
/// at most 99 bytes) to `method_name` (100 bytes), where the parameters start to
/// `*params` (into `url` for GET, else `request`) and the JSON-RPC id to
/// `*id`/`*id_len` (null if none).
///
/// # Safety
/// `url` and `request` are null or NUL-terminated; the outputs are non-null,
/// writable and disjoint, with 100 bytes at `method_name`. Returned pointers
/// borrow the inputs. Panics abort rather than unwinding across the ABI.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rpc_route(
    url: *const c_char,
    request: *const c_char,
    get: c_int,
    protocol: c_int,
    method_name: *mut c_char,
    params: *mut *const c_char,
    id: *mut *const c_char,
    id_len: *mut c_int,
) {
    let r = crate::rpcroute::route(input(url), input(request), get != 0, protocol);
    std::ptr::copy_nonoverlapping(r.method.as_ptr().cast::<c_char>(), method_name, r.method.len());
    *method_name.add(r.method.len()) = 0;
    *params = match r.params {
        // a null URL has no parameters to point into (and no arithmetic on null)
        Some(_) if url.is_null() => std::ptr::null(),
        Some(at) => url.add(at),
        None => request,
    };
    match r.id {
        Some((at, len)) => {
            *id = request.add(at);
            *id_len = len as c_int;
        }
        None => {
            *id = std::ptr::null();
            *id_len = 0;
        }
    }
}

/// XmlRpcProcessor::BuildResponse: the text before and after a response; free
/// both with nzbget_rs_free.
///
/// # Safety
/// `callback` and `id` are null or NUL-terminated; `head` and `tail` are non-null,
/// writable and disjoint. Each returned buffer is independently Rust-owned.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rpc_envelope(
    protocol: c_int,
    fault: c_int,
    callback: *const c_char,
    id: *const c_char,
    head: *mut RsBuf,
    tail: *mut RsBuf,
) {
    let callback = (!callback.is_null()).then(|| input(callback));
    let id = (!id.is_null()).then(|| input(id));
    let (h, t) = crate::rpcroute::envelope(protocol, fault != 0, callback, id);
    *head = into_buf(h);
    *tail = into_buf(t);
}

/// XmlRpcProcessor::Access: the access a request runs with (rpcroute::access).
#[no_mangle]
pub extern "C" fn nzbget_rs_rpc_access(protocol: c_int, access: c_int) -> c_int {
    crate::rpcroute::access(protocol, access)
}

/// XmlRpcProcessor::Decide: 0 runs the command, 1 answers with an error inside
/// the JSON-P callback, 2 answers with a plain error; `code` and `text` get
/// that error (0 and null when it runs). Null outputs discard that result.
/// `text` is static.
///
/// # Safety
/// Nonnull `code` and `text` must be aligned and valid for writes of their
/// respective types, and their storage must not overlap. The returned text
/// is NUL-terminated and must not be modified or freed.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rpc_decide(
    protocol: c_int,
    http_method: c_int,
    safe: c_int,
    error: c_int,
    jsonp: c_int,
    code: *mut c_int,
    text: *mut *const c_char,
) -> c_int {
    let d = crate::rpcroute::decide(protocol, http_method, safe != 0, error != 0, jsonp != 0);
    if !code.is_null() {
        code.write(d.code);
    }
    if !text.is_null() {
        text.write(d.text.map_or(std::ptr::null(), CStr::as_ptr));
    }
    d.dispatch
}

/// `len` bytes at `data` (empty when null or zero).
unsafe fn span<'a>(data: *const c_char, len: usize) -> &'a [u8] {
    if data.is_null() || len == 0 { &[] } else { std::slice::from_raw_parts(data.cast::<u8>(), len) }
}

/// Util::SplitCommandLine: the words, each NUL-terminated, one after the
/// other (`len` covers them all); free with nzbget_rs_free.
///
/// # Safety
/// `s` is null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_split_command_line(s: *const c_char) -> RsBuf {
    let mut v = Vec::new();
    for word in crate::util::split_command_line(input(s)) {
        v.extend_from_slice(&word);
        v.push(0);
    }
    into_buf(v)
}

/// Util::TrimRight(char*) (`right_only`) or Util::Trim(char*): trims CR, LF,
/// space and tab in place and returns where the text starts.
///
/// # Safety
/// `s` is null or a writable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_trim_line(s: *mut c_char, right_only: c_int) -> *mut c_char {
    if s.is_null() {
        return s;
    }
    let text = CStr::from_ptr(s).to_bytes();
    let len = text.len();
    let (start, end) = if right_only != 0 { (0, crate::util::trim_right_line(text)) } else { crate::util::trim_line(text) };
    // The legacy helper zeroes every removed byte, not just the new terminator.
    // The shared CStr borrow ends before these writes.
    std::ptr::write_bytes(s.add(end), 0, len - end);
    s.add(start)
}

/// ServerVolume::CalcSlots: the slots of a local time; updates `*first_day`.
///
/// # Safety
/// `first_day` and `slots` are null (a no-op) or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_volume_calc_slots(loc_cur_time: i64, first_day: *mut c_int, slots: *mut crate::statmeter::Slots) {
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
        if p.is_null() || len == 0 { &mut [] } else { std::slice::from_raw_parts_mut(p, len) }
    }
    let Some(slots) = slots.as_ref().copied() else { return };
    crate::statmeter::add_stats(
        array(seconds, seconds_len), array(minutes, minutes_len), array(hours, hours_len), &slots,
        last_min_slot, last_hour_slot, loc_cur_time, loc_data_time, bytes,
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
pub unsafe extern "C" fn nzbget_rs_option_restricted(name: *const c_char, signed_char: c_int) -> c_int {
    (!name.is_null() && crate::options::option_restricted(CStr::from_ptr(name), signed_char != 0)) as c_int
}

/// NzbgetRsNntpIo: the C++ connection for the NNTP exchange. Each callback
/// returns 0, or -1 when it threw (the exchange stops; C++ rethrows).
#[repr(C)]
pub struct NntpIo {
    pub ctx: *mut std::ffi::c_void,
    pub write_line: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub read_line: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut *mut c_char) -> c_int>,
    pub report_error: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *const c_char) -> c_int>,
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
        if p.is_null() { c"" } else { unsafe { CStr::from_ptr(p) } }
    }
}

fn nntp_status(r: c_int) -> Result<(), crate::nntp::Abort> {
    if r == 0 { Ok(()) } else { Err(crate::nntp::Abort) }
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
    let mut auth = crate::nntp::Auth { error: *auth_error != 0, rejected: *auth_rejected != 0 };
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
    nntp_run(io, auth_error, auth_rejected, |io, auth| crate::nntp::request(io, auth, req), |a| *answer = a)
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
    nntp_run(io, auth_error, auth_rejected, crate::nntp::handshake, |r| *result = r)
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
    nntp_run(io, auth_error, auth_rejected, |io, auth| crate::nntp::join_group(io, auth, group), |(a, j)| {
        *answer = a;
        *joined = j as c_int;
    })
}

/// NzbgetRsCmdlineSink: the C++ CommandLineParser's fields. Each callback
/// returns 0, or -1 when it threw (parsing stops; C++ rethrows).
#[repr(C)]
pub struct CmdlineSink {
    pub getopt_long: Option<unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int>,
    pub ctx: *mut std::ffi::c_void,
    pub set_int: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, c_int) -> c_int>,
    pub set_str: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char) -> c_int>,
    pub steal: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, c_int) -> c_int>,
    pub push_option: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub push_id: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int) -> c_int>,
    pub push_name: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub error: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
}

struct FfiCmdline<'a>(&'a CmdlineSink);

fn cmd_status(r: c_int) -> Result<(), crate::cmdline::Abort> {
    if r == 0 { Ok(()) } else { Err(crate::cmdline::Abort) }
}

impl crate::cmdline::Sink for FfiCmdline<'_> {
    unsafe fn getopt_long(&mut self, argc: c_int, argv: *mut *mut c_char) -> c_int {
        match self.0.getopt_long {
            Some(f) => f(argc, argv),
            None => crate::cmdline::default_getopt_long(argc, argv),
        }
    }
    fn set_int(&mut self, field: i32, value: i32) -> Result<(), crate::cmdline::Abort> {
        let f = self.0.set_int.ok_or(crate::cmdline::Abort)?;
        cmd_status(unsafe { f(self.0.ctx, field, value) })
    }
    fn set_str(&mut self, field: i32, value: *const c_char) -> Result<(), crate::cmdline::Abort> {
        let f = self.0.set_str.ok_or(crate::cmdline::Abort)?;
        cmd_status(unsafe { f(self.0.ctx, field, value) })
    }
    fn steal(&mut self, field: i32, index: c_int) -> Result<(), crate::cmdline::Abort> {
        let f = self.0.steal.ok_or(crate::cmdline::Abort)?;
        cmd_status(unsafe { f(self.0.ctx, field, index) })
    }
    fn push_option(&mut self, value: *const c_char) -> Result<(), crate::cmdline::Abort> {
        let f = self.0.push_option.ok_or(crate::cmdline::Abort)?;
        cmd_status(unsafe { f(self.0.ctx, value) })
    }
    fn push_id(&mut self, id: i32) -> Result<(), crate::cmdline::Abort> {
        let f = self.0.push_id.ok_or(crate::cmdline::Abort)?;
        cmd_status(unsafe { f(self.0.ctx, id) })
    }
    fn push_name(&mut self, value: *const c_char) -> Result<(), crate::cmdline::Abort> {
        let f = self.0.push_name.ok_or(crate::cmdline::Abort)?;
        cmd_status(unsafe { f(self.0.ctx, value) })
    }
    fn error(&mut self, msg: &CStr) -> Result<(), crate::cmdline::Abort> {
        let f = self.0.error.ok_or(crate::cmdline::Abort)?;
        cmd_status(unsafe { f(self.0.ctx, msg.as_ptr()) })
    }
}

/// CommandLineParser's constructor: parses `argv` (permuted in place by
/// getopt, entries nulled when the sink steals them) into the sink. 0, or -1
/// when a callback threw.
///
/// # Safety
/// `argv` points to `argc` C strings and a trailing null sentinel, writable
/// as getopt permutes them.
/// NULL inputs/entries and missing callbacks return -1. Only consumed entries
/// may be nulled by `steal`; it keeps their strings alive until the destination
/// is overwritten or parsing finishes. String callbacks copy borrowed data
/// if retaining it; no callback may
/// unwind. `sink` is valid; getopt globals are not used concurrently.
/// Rust panics abort at this ABI boundary; they never unwind into C++.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cmdline_parse(argc: c_int, argv: *mut *mut c_char, use_long: c_int, sink: *const CmdlineSink) -> c_int {
    let Some(sink) = sink.as_ref() else { return -1 };
    if argv.is_null() || argc < 1 {
        return -1;
    }
    if (0..argc).any(|i| (*argv.add(i as usize)).is_null())
        || sink.set_int.is_none() || sink.set_str.is_none() || sink.steal.is_none()
        || sink.push_option.is_none() || sink.push_id.is_none()
        || sink.push_name.is_none() || sink.error.is_none()
    {
        return -1;
    }
    crate::cmdline::with_getopt_lock(|| match crate::cmdline::parse(argc, argv, use_long != 0, &mut FfiCmdline(sink)) {
        Ok(()) => 0,
        Err(_) => -1,
    })
}

#[cfg(test)]
mod cmdline_tests {
    use super::*;
    use std::ffi::{c_void, CString};

    struct Calls {
        count: usize,
        fail_at: usize,
        argv: *mut *mut c_char,
    }

    unsafe fn called(ctx: *mut c_void) -> c_int {
        let calls = &mut *ctx.cast::<Calls>();
        calls.count += 1;
        if calls.count == calls.fail_at { -1 } else { 0 }
    }
    unsafe extern "C" fn integer(ctx: *mut c_void, _: c_int, _: c_int) -> c_int { called(ctx) }
    unsafe extern "C" fn string(ctx: *mut c_void, _: c_int, _: *const c_char) -> c_int { called(ctx) }
    unsafe extern "C" fn id(ctx: *mut c_void, _: c_int) -> c_int { called(ctx) }
    unsafe extern "C" fn text(ctx: *mut c_void, _: *const c_char) -> c_int { called(ctx) }
    unsafe extern "C" fn steal(ctx: *mut c_void, _: c_int, index: c_int) -> c_int {
        let result = called(ctx);
        if result == 0 {
            // The owning CString remains alive, as the C++ destination would.
            *(*ctx.cast::<Calls>()).argv.add(index as usize) = std::ptr::null_mut();
        }
        result
    }

    #[test]
    fn cmdline_rejects_invalid_inputs_and_stops_at_each_failed_callback() {
        let mut calls = Calls { count: 0, fail_at: usize::MAX, argv: std::ptr::null_mut() };
        let mut sink = CmdlineSink {
            getopt_long: None, ctx: (&mut calls as *mut Calls).cast(),
            set_int: Some(integer), set_str: Some(string), steal: Some(steal),
            push_option: Some(text), push_id: Some(id), push_name: Some(text), error: Some(text),
        };
        let mut null_arg = [std::ptr::null_mut()];
        let mut null_value = [c"nzbget".as_ptr().cast_mut(), std::ptr::null_mut()];
        unsafe {
            assert_eq!(nzbget_rs_cmdline_parse(2, null_value.as_mut_ptr(), 0, &sink), -1);
            assert_eq!(nzbget_rs_cmdline_parse(1, null_arg.as_mut_ptr(), 0, &sink), -1);
            assert_eq!(nzbget_rs_cmdline_parse(1, std::ptr::null_mut(), 0, &sink), -1);
            assert_eq!(nzbget_rs_cmdline_parse(0, null_arg.as_mut_ptr(), 0, &sink), -1);
            assert_eq!(nzbget_rs_cmdline_parse(-1, null_arg.as_mut_ptr(), 0, &sink), -1);
            assert_eq!(nzbget_rs_cmdline_parse(1, null_arg.as_mut_ptr(), 0, std::ptr::null()), -1);
        }
        assert_eq!(calls.count, 0);
        for args in [
            vec!["nzbget", "-A", "N", "name", "-o", "X=1", "-c", "conf", "file"],
            vec!["nzbget", "-E", "GN", "D", "name"],
            vec!["nzbget", "-E", "G", "D", "1,3-5"],
            vec!["nzbget", "-E", "G", "invalid"],
        ] {
            let strings: Vec<_> = args.iter().map(|a| CString::new(*a).unwrap()).collect();
            let mut fail_at = 1;
            loop {
                let mut argv: Vec<_> = strings.iter().map(|a| a.as_ptr().cast_mut()).collect();
                argv.push(std::ptr::null_mut());
                calls.count = 0;
                calls.fail_at = fail_at;
                calls.argv = argv.as_mut_ptr();
                let result = unsafe { nzbget_rs_cmdline_parse(args.len() as c_int, argv.as_mut_ptr(), 0, &sink) };
                if result == 0 {
                    assert_eq!(calls.count + 1, fail_at);
                    break;
                }
                assert_eq!(calls.count, fail_at, "callbacks continued after failure");
                fail_at += 1;
                assert!(fail_at < 100);
            }
        }
        let mut argv = [c"nzbget".as_ptr().cast_mut(), std::ptr::null_mut()];
        calls.count = 0;
        unsafe {
            sink.error = None;
            assert_eq!(nzbget_rs_cmdline_parse(1, argv.as_mut_ptr(), 1, &sink), -1);
            assert_eq!(nzbget_rs_cmdline_parse(1, argv.as_mut_ptr(), 0, &sink), -1);
        }
        assert_eq!(calls.count, 0);
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
    (!path.is_null() && crate::fsops::directory_exists(input(path), &crate::fsops::posix_stat)) as c_int
}

/// FileSystem::ForceDirectories: 1 created or there, else 0 with the message
/// in `*errmsg` (free with nzbget_rs_free; empty on success).
///
/// # Safety
/// `path` is null or NUL-terminated; `errmsg` is null or writable and disjoint
/// from the input. Every non-null output is initialized, including on failure.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_force_directories(path: *const c_char, errmsg: *mut RsBuf) -> c_int {
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
    pub progress: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, c_int) -> c_int>,
    pub message: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char) -> c_int>,
    pub register_renamed:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *const c_char) -> c_int>,
}

struct FfiRarRenamerRun<'a>(&'a RarRenamerRunEnv);

impl crate::rarrenamer::RunCallbacks for FfiRarRenamerRun<'_> {
    fn stopped(&mut self) -> Result<bool, ()> {
        let Some(f) = self.0.stopped else { return Ok(false) };
        match unsafe { f(self.0.ctx) } {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(()),
        }
    }
    fn progress(&mut self, label: &[u8], stage_progress: i32) -> Result<(), ()> {
        let Some(f) = self.0.progress else { return Ok(()) };
        let l = c_string(label);
        if unsafe { f(self.0.ctx, l.as_ptr(), stage_progress) } == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
    fn message(&mut self, kind: i32, text: &[u8]) -> Result<(), ()> {
        let Some(f) = self.0.message else { return Ok(()) };
        let t = c_string(text);
        if unsafe { f(self.0.ctx, kind, t.as_ptr()) } == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
    fn register_renamed(&mut self, old_base: &[u8], new_base: &[u8]) -> Result<(), ()> {
        let Some(f) = self.0.register_renamed else { return Ok(()) };
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
    let info = if info_name.is_null() { &b"(null)"[..] } else { input(info_name) };
    let res = crate::rarrenamer::execute_all(
        input(dest_dir),
        info,
        input(password),
        input(ignore_ext),
        tls_enabled != 0,
        renamed_count,
        &mut FfiRarRenamerRun(env),
    );
    if res.is_ok() { 0 } else { -1 }
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
pub unsafe extern "C" fn nzbget_rs_rename_analyzer_finish(a: *mut crate::renameanalyzer::Analyzer, hash: *mut c_char) {
    let Some(a) = a.as_mut() else { return };
    if hash.is_null() {
        return;
    }
    let h = a.finish();
    std::ptr::copy_nonoverlapping(h.as_ptr().cast(), hash, h.len());
}

/// Reads the par2 hash at an index of DirectRenamer's FileHashList.
pub type ParHashAt = unsafe extern "C" fn(*mut std::ffi::c_void, usize, *mut *const c_char, *mut *const c_char);

/// DirectRenamer::BuildNewRegularName: the index of the par2 hash whose file
/// name the file gets, or -1 to keep its name.
///
/// # Safety
/// `old_name` and `hash16k` are null (empty) or NUL-terminated. `at` writes,
/// for every index below `count`, a filename and hash that are null (empty)
/// or NUL-terminated and stay valid until this returns; it must not unwind.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_direct_regular_name(old_name: *const c_char, hash16k: *const c_char, count: usize,
    at: Option<ParHashAt>, ctx: *mut std::ffi::c_void) -> isize {
    let as_c = |p: *const c_char| if p.is_null() { c"" } else { CStr::from_ptr(p) };
    let Some(at) = at else { return -1 };
    let hashes = (0..count).map(|i| {
        let (mut filename, mut hash) = (std::ptr::null(), std::ptr::null());
        at(ctx, i, &mut filename, &mut hash);
        (as_c(filename), as_c(hash))
    });
    crate::directrenamer::regular_name(as_c(old_name), as_c(hash16k), hashes).map_or(-1, |i| i as isize)
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
pub unsafe extern "C" fn nzbget_rs_direct_need_rename_pars(pars: *const ParName, count: usize) -> c_int {
    if pars.is_null() {
        return 0;
    }
    let pars: Vec<_> = std::slice::from_raw_parts(pars, count).iter().map(|p| {
        ((!p.filename.is_null()).then(|| CStr::from_ptr(p.filename)), if p.set_id.is_null() { c"" } else { CStr::from_ptr(p.set_id) })
    }).collect();
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
    register_renamed_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *const c_char)>,
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
            errmsg.write(into_buf(crate::util::fs_format_null_delete_errmsg(&last_err)));
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
/// # Safety
/// `filename`/`bak_part` are null or NUL-terminated; `new_name` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rename_bak(
    filename: *const c_char,
    bak_part: *const c_char,
    remove_old_extension: c_int,
    new_name: *mut RsBuf,
) -> c_int {
    if filename.is_null() || bak_part.is_null() {
        if !new_name.is_null() {
            new_name.write(into_buf(Vec::new()));
        }
        return 0;
    }
    let mut name = Vec::new();
    let done = crate::fsdelete::rename_bak(
        &mut crate::fsops::PosixFsOps,
        input(filename),
        input(bak_part),
        remove_old_extension != 0,
        &mut name,
    )
    .unwrap_or(false);
    if !new_name.is_null() {
        new_name.write(into_buf(name));
    }
    done as c_int
}

/// FileSystem::GetFileExtension: the offset of the last '.', or -1.
///
/// # Safety
/// `name` is null (with `len` 0) or `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_file_extension(name: *const c_char, len: usize) -> isize {
    let name = bytes_arg(name.cast(), len);
    crate::fsdelete::file_extension(name).map_or(-1, |ext| (name.len() - ext.len()) as isize)
}

/// BinRpcProcessor::Execute's check of a request base (BASE_SIZE bytes as
/// received): 1 when the credentials match, else 0; `kind` (null: not
/// written) gets the request type.
///
/// # Safety
/// `base` is readable for `len` bytes (fewer than BASE_SIZE: 0); the control
/// names are null (empty) or NUL-terminated; `kind` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_check(base: *const u8, len: usize, control_user: *const c_char,
    control_password: *const c_char, kind: *mut u32) -> c_int {
    let Some(base) = bytes_arg(base, len).get(..crate::binrpc::BASE_SIZE) else { return 0 };
    let base: &[u8; crate::binrpc::BASE_SIZE] = base.try_into().unwrap();
    // Finish reading inputs before writing an output that may overlap them.
    let request_type = crate::binrpc::request_type(base);
    let valid = crate::binrpc::credentials_ok(base, input(control_user), input(control_password));
    if !kind.is_null() {
        kind.write(request_type);
    }
    valid as c_int
}

/// The request type's name for the log ("Unknown" past the last type): a
/// static NUL-terminated string.
#[no_mangle]
pub extern "C" fn nzbget_rs_binrpc_name(kind: u32) -> *const c_char {
    crate::binrpc::request_name(kind).as_ptr()
}

/// Dispatch's size check against g_MessageRequestSizes: 1 when fine, 0 with
/// `expected`/`received` (null: not written) set on a size mismatch. A short
/// base or a missing table entry also returns 0, leaving outputs untouched.
///
/// # Safety
/// `base` is null or readable for `len` bytes; `sizes` is null or `count`
/// readable, aligned values; outputs are null or writable, aligned values.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_size_ok(base: *const u8, len: usize, sizes: *const u32, count: usize,
    expected: *mut u32, received: *mut u32) -> c_int {
    let Some(base) = bytes_arg(base, len).get(..crate::binrpc::BASE_SIZE) else { return 0 };
    let base: &[u8; crate::binrpc::BASE_SIZE] = base.try_into().unwrap();
    let sizes = if sizes.is_null() { &[][..] } else { std::slice::from_raw_parts(sizes, count) };
    let kind = crate::binrpc::request_type(base);
    if (1..=crate::binrpc::LAST_TYPE).contains(&kind) && sizes.len() <= kind as usize {
        return 0;
    }
    match crate::binrpc::size_error(base, sizes) {
        None => 1,
        Some((e, r)) => {
            if !expected.is_null() {
                expected.write(e);
            }
            if !received.is_null() {
                received.write(r);
            }
            0
        }
    }
}

/// SendBoolResponse's bytes (the 16-byte response, then the text and its
/// NUL); free with nzbget_rs_free.
///
/// # Safety
/// `text` is null (empty) or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_bool_response(success: c_int, text: *const c_char) -> RsBuf {
    into_buf(crate::binrpc::bool_response(success != 0, input(text)))
}

/// Download's URL test: 1 when `content` starts with "http:/" or "https:/"
/// in any case.
///
/// # Safety
/// `content` is null (empty) or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_is_url(content: *const c_char) -> c_int {
    crate::binrpc::is_url_cstr(content.cast()) as c_int
}

/// UrlDownloader::ProcessHeader's X-DNZB headers: 1 with `*start` and
/// `*length` the trimmed value of X-DNZB-Category in `line` (nothing is
/// allocated), 2 with `name` the parameter's name and `value` its value, 0
/// (outputs untouched) for another line.
///
/// # Safety
/// `line` is null (empty) or NUL-terminated; the outputs are null or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_url_dnzb_header(line: *const c_char, start: *mut usize, length: *mut usize,
    name: *mut RsBuf, value: *mut RsBuf) -> c_int {
    use crate::urlcoord::Dnzb;
    match crate::urlcoord::dnzb_header(input(line)) {
        Dnzb::None => 0,
        Dnzb::Category(from, to) => {
            if !start.is_null() {
                *start = from;
            }
            if !length.is_null() {
                *length = to - from;
            }
            1
        }
        Dnzb::Param(n, text) => {
            if !name.is_null() {
                *name = into_buf(n);
            }
            if !value.is_null() {
                *value = into_buf(text);
            }
            2
        }
    }
}

/// A response of `total` bytes (and a NUL after them for the C string
/// functions) written by `fill`, which gets room for them and returns the
/// bytes it wrote; an empty buffer for no bytes or more than an int's range.
/// The buffer isn't zeroed first.
///
/// # Safety
/// As the entries' strings in `fill`.
unsafe fn response(total: u64, fill: impl FnOnce(&mut [std::mem::MaybeUninit<u8>]) -> usize) -> RsBuf {
    if total == 0 || total > crate::binrpc::MAX_RESPONSE {
        return RsBuf { data: std::ptr::null_mut(), len: 0, cap: 0 };
    }
    let total = total as usize;
    let mut v: Vec<u8> = Vec::with_capacity(total + 1);
    let room = v.spare_capacity_mut();
    assert_eq!(fill(&mut room[..total]), total);
    room[total].write(0);
    v.set_len(total + 1);
    let mut v = std::mem::ManuallyDrop::new(v);
    RsBuf { data: v.as_mut_ptr() as *mut c_char, len: total, cap: v.capacity() }
}

/// An encoded response: owned_capacity is zero when data borrows the
/// caller's scratch buffer; otherwise free the equivalent RsBuf.
#[repr(C)]
pub struct BinResponse {
    pub data: *mut c_char,
    pub len: usize,
    pub owned_capacity: usize,
}

/// Uses scratch only when it holds the complete response and its final NUL.
/// All lengths retain response's limits; an undersized or null scratch falls
/// back to one owned allocation. The scratch buffer is never freed by Rust.
///
/// # Safety
/// scratch is null or writable for capacity bytes, and does not overlap
/// any entry or string read by fill. As response for fill.
unsafe fn response_into(total: u64, scratch: *mut c_char, capacity: usize,
    fill: impl FnOnce(&mut [std::mem::MaybeUninit<u8>]) -> usize) -> BinResponse {
    if total != 0 && total <= crate::binrpc::MAX_RESPONSE && !scratch.is_null() && total < capacity as u64 {
        let total = total as usize;
        let room = std::slice::from_raw_parts_mut(scratch.cast::<std::mem::MaybeUninit<u8>>(), total);
        assert_eq!(fill(room), total);
        scratch.add(total).write(0);
        BinResponse { data: scratch, len: total, owned_capacity: 0 }
    } else {
        let buf = response(total, fill);
        BinResponse { data: buf.data, len: buf.len, owned_capacity: buf.cap }
    }
}

unsafe fn batch<'a, E>(first: *const E, count: usize) -> &'a [E] {
    if first.is_null() { &[] } else { std::slice::from_raw_parts(first, count) }
}

/// Encodes a whole List response into caller scratch when it fits, or
/// one owned allocation otherwise. owned_capacity == 0 means borrowed;
/// otherwise free RsBuf {data, len, cap: owned_capacity}. Empty/oversized
/// responses have null data. See response_into for scratch requirements.
///
/// # Safety
/// Arrays and strings meet encode_list's requirements. scratch is null or
/// writable for capacity bytes, disjoint from all input arrays/strings.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_encode_list_into(nzbs: *const crate::binrpc::NzbEntry, nzb_count: usize,
    params: *const crate::binrpc::ParamEntry, param_count: usize, files: *const crate::binrpc::FileEntry,
    file_count: usize, scratch: *mut c_char, capacity: usize) -> BinResponse {
    use crate::binrpc::{encode, encoded_len};
    let (nzbs, params, files) = (batch(nzbs, nzb_count), batch(params, param_count), batch(files, file_count));
    let total = encoded_len(nzbs).saturating_add(encoded_len(params)).saturating_add(encoded_len(files));
    response_into(total, scratch, capacity, |out| {
        let mut at = encode(nzbs, out);
        at += encode(params, &mut out[at..]);
        at + encode(files, &mut out[at..])
    })
}

/// As encode_list_into, for a log response.
///
/// # Safety
/// As encode_list_into.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_encode_log_into(log: *const crate::binrpc::LogEntry, count: usize,
    scratch: *mut c_char, capacity: usize) -> BinResponse {
    let entries = batch(log, count);
    response_into(crate::binrpc::encoded_len(entries), scratch, capacity, |out| crate::binrpc::encode(entries, out))
}

/// As encode_list_into, for a post_queue response.
///
/// # Safety
/// As encode_list_into.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_encode_post_queue_into(jobs: *const crate::binrpc::PostEntry, count: usize,
    scratch: *mut c_char, capacity: usize) -> BinResponse {
    let entries = batch(jobs, count);
    response_into(crate::binrpc::encoded_len(entries), scratch, capacity, |out| crate::binrpc::encode(entries, out))
}

/// As encode_list_into, for a history response.
///
/// # Safety
/// As encode_list_into.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_encode_history_into(history: *const crate::binrpc::HistoryEntry, count: usize,
    scratch: *mut c_char, capacity: usize) -> BinResponse {
    let entries = batch(history, count);
    response_into(crate::binrpc::encoded_len(entries), scratch, capacity, |out| crate::binrpc::encode(entries, out))
}

/// The remote client's reading of one list-response entry at `*at`: 1 with
/// `nints` ints in `ints` (host order), `nfields` string starts in `starts`
/// (the strings' lengths in ints[fields[k]]) and `*at` past the entry; 0
/// (outputs untouched) when it or a string doesn't fit in `len`.
///
/// # Safety
/// `data` is readable for `len` (or null with 0); `at` is valid; `ints`,
/// `fields` and `starts` are valid, aligned arrays of their counts (or null
/// for 0). `ints` and `starts` are writable and may be uninitialized.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_read_entry(data: *const c_char, len: usize, at: *mut usize, ints: *mut u32,
    nints: usize, fields: *const usize, starts: *mut usize, nfields: usize) -> c_int {
    if at.is_null() || (nints > 0 && ints.is_null()) || (nfields > 0 && (fields.is_null() || starts.is_null())) {
        return 0;
    }
    let data = bytes_arg(data.cast(), len);
    let fields: &[usize] = if nfields > 0 { std::slice::from_raw_parts(fields, nfields) } else { &[] };
    // C callers pass uninitialized output arrays. Form slices only over
    // initialized storage, then write the caller's arrays after validation.
    let mut values = vec![0; nints];
    let mut offsets = vec![0; nfields];
    match crate::binrpc::read_entry(data, *at, &mut values, fields, &mut offsets) {
        Some(next) => {
            if nints > 0 {
                std::ptr::copy_nonoverlapping(values.as_ptr(), ints, nints);
            }
            if nfields > 0 {
                std::ptr::copy_nonoverlapping(offsets.as_ptr(), starts, nfields);
            }
            *at = next;
            1
        }
        None => 0,
    }
}

/// The remote client's log listing of the first `count` entries of `len`
/// bytes of data, as printed ("[INFO] text" lines, each with its newline;
/// free with nzbget_rs_free). It ends at an entry that doesn't fit the data;
/// unknown kinds have no line.
///
/// # Safety
/// `data` is readable for `len` (or null with 0).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_log_listing(data: *const c_char, len: usize, count: u32) -> RsBuf {
    into_buf(crate::binrpc::log_listing(bytes_arg(data.cast(), len), count))
}

/// The remote client's post-processing listing ("[id] name, stage, n%"; as
/// nzbget_rs_binrpc_log_listing).
///
/// # Safety
/// As nzbget_rs_binrpc_log_listing.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_post_queue_listing(data: *const c_char, len: usize, count: u32) -> RsBuf {
    into_buf(crate::binrpc::post_queue_listing(bytes_arg(data.cast(), len), count))
}

/// The remote client's history listing ("[id] name (...)"; as
/// nzbget_rs_binrpc_log_listing). Kinds it doesn't list have no line.
///
/// # Safety
/// As nzbget_rs_binrpc_log_listing.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_history_listing(data: *const c_char, len: usize, count: u32) -> RsBuf {
    into_buf(crate::binrpc::history_listing(bytes_arg(data.cast(), len), count))
}

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
mod binrpc_tests {
    use super::*;
    use crate::binrpc::{FileEntry, HistoryEntry, LogEntry, NzbEntry, ParamEntry, PostEntry, Text, BASE_SIZE};
    use std::ptr::{null, null_mut};

    fn log(id: u32, kind: u32, text: &[u8]) -> LogEntry {
        LogEntry { id, kind, time: 0, texts: [Text::new(text)] }
    }

    fn bytes(buf: &RsBuf) -> &[u8] {
        unsafe { bytes_arg(buf.data.cast(), buf.len) }
    }


    #[test]
    fn credential_output_may_overlap_inputs() {
        #[repr(C, align(4))]
        struct Base([u8; BASE_SIZE]);
        unsafe {
            let mut base = Base([0; BASE_SIZE]);
            base.0[8..12].copy_from_slice(&14u32.to_be_bytes());
            base.0[12..16].copy_from_slice(b"user");
            let ptr = base.0.as_mut_ptr();
            assert_eq!(nzbget_rs_binrpc_check(ptr, BASE_SIZE, c"user".as_ptr(), c"".as_ptr(),
                ptr.add(12).cast()), 1);
            assert_eq!(ptr.add(12).cast::<u32>().read(), 14);

            let mut control_password = u32::from_ne_bytes(*b"pw\0\0");
            base.0[44..46].copy_from_slice(b"pw");
            let password_ptr = &mut control_password as *mut u32;
            assert_eq!(nzbget_rs_binrpc_check(base.0.as_ptr(), BASE_SIZE, null(), password_ptr.cast(),
                password_ptr), 1);
            assert_eq!(control_password, 14);
        }
    }

    #[test]
    fn missing_sizes_fail_and_null_outputs_are_allowed() {
        unsafe {
            let mut base = [0u8; BASE_SIZE];
            base[8..12].copy_from_slice(&14u32.to_be_bytes());
            let sizes = [123u32; 15];
            let (mut expected, mut received) = (7, 9);
            for count in 0..15 {
                assert_eq!(nzbget_rs_binrpc_size_ok(base.as_ptr(), BASE_SIZE, sizes.as_ptr(), count,
                    &mut expected, &mut received), 0);
                assert_eq!((expected, received), (7, 9));
            }
            assert_eq!(nzbget_rs_binrpc_size_ok(base.as_ptr(), BASE_SIZE, null(), 15, null_mut(), null_mut()), 0);
            assert_eq!(nzbget_rs_binrpc_size_ok(base.as_ptr(), BASE_SIZE, sizes.as_ptr(), 15,
                null_mut(), null_mut()), 0);
            assert_eq!(nzbget_rs_binrpc_size_ok(base.as_ptr(), BASE_SIZE, sizes.as_ptr(), 15,
                &mut expected, &mut received), 0);
            assert_eq!((expected, received), (123, 0));
            base[4..8].copy_from_slice(&123u32.to_be_bytes());
            assert_eq!(nzbget_rs_binrpc_size_ok(base.as_ptr(), BASE_SIZE, sizes.as_ptr(), 15,
                null_mut(), null_mut()), 1);
            // Unknown types still bypass the table, just as Dispatch does.
            base[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
            assert_eq!(nzbget_rs_binrpc_size_ok(base.as_ptr(), BASE_SIZE, null(), 0, null_mut(), null_mut()), 1);
        }
    }

    #[test]
    fn null_and_short_inputs() {
        unsafe {
            let base = [0u8; BASE_SIZE];
            let mut kind = 42;
            for len in 0..BASE_SIZE {
                assert_eq!(nzbget_rs_binrpc_check(base.as_ptr(), len, null(), null(), &mut kind), 0);
                assert_eq!(kind, 42);
                assert_eq!(nzbget_rs_binrpc_size_ok(base.as_ptr(), len, null(), 0, null_mut(), null_mut()), 0);
            }
            assert_eq!(nzbget_rs_binrpc_check(null(), BASE_SIZE, null(), null(), &mut kind), 0);
            assert_eq!(kind, 42);
            assert_eq!(nzbget_rs_binrpc_size_ok(null(), BASE_SIZE, null(), 0, null_mut(), null_mut()), 0);
            assert_eq!(nzbget_rs_binrpc_check(base.as_ptr(), BASE_SIZE, null(), null(), null_mut()), 1);
            let response = nzbget_rs_binrpc_bool_response(-1, null());
            assert_eq!(std::slice::from_raw_parts(response.data.cast::<u8>(), response.len),
                [0x6e, 0x7a, 0x62, 0x31, 0, 0, 0, 16, 0, 0, 0, 1, 0, 0, 0, 1, 0]);
            nzbget_rs_free(response);
        }
    }

    #[test]
    fn client_entry_outputs_and_failures() {
        use std::mem::MaybeUninit;
        unsafe {
            let data = b"\0\0\0\x07\0\0\0\x04abc\0";
            let fields = [1usize];
            let mut at = 0;
            let mut ints = MaybeUninit::<[u32; 2]>::uninit();
            let mut starts = MaybeUninit::<[usize; 1]>::uninit();
            assert_eq!(nzbget_rs_binrpc_read_entry(data.as_ptr().cast(), data.len(), &mut at,
                ints.as_mut_ptr().cast(), 2, fields.as_ptr(), starts.as_mut_ptr().cast(), 1), 1);
            assert_eq!(ints.assume_init(), [7, 4]);
            assert_eq!(starts.assume_init(), [8]);
            assert_eq!(at, 12);

            let (mut ints, mut starts) = ([91, 92], [93]);
            let bad_length = b"\0\0\0\x07\xff\xff\xff\xffabc\0";
            let no_nul = b"\0\0\0\x07\0\0\0\x04abcd";
            for (bytes, offset, field) in [
                (&data[..7], 0, 1), (&data[..11], 0, 1), (&bad_length[..], 0, 1),
                (&no_nul[..], 0, 1), (&data[..], 0, 2), (&data[..], usize::MAX - 2, 1),
            ] {
                at = offset;
                assert_eq!(nzbget_rs_binrpc_read_entry(bytes.as_ptr().cast(), bytes.len(), &mut at,
                    ints.as_mut_ptr(), 2, &field, starts.as_mut_ptr(), 1), 0);
                assert_eq!((at, ints, starts), (offset, [91, 92], [93]));
            }

            at = 0;
            for (out_at, out_ints, in_fields, out_starts) in [
                (null_mut(), ints.as_mut_ptr(), fields.as_ptr(), starts.as_mut_ptr()),
                (&mut at, null_mut(), fields.as_ptr(), starts.as_mut_ptr()),
                (&mut at, ints.as_mut_ptr(), null(), starts.as_mut_ptr()),
                (&mut at, ints.as_mut_ptr(), fields.as_ptr(), null_mut()),
            ] {
                assert_eq!(nzbget_rs_binrpc_read_entry(data.as_ptr().cast(), data.len(), out_at,
                    out_ints, 2, in_fields, out_starts, 1), 0);
                assert_eq!((at, ints, starts), (0, [91, 92], [93]));
            }
            assert_eq!(nzbget_rs_binrpc_read_entry(null(), 0, &mut at, ints.as_mut_ptr(), 2,
                fields.as_ptr(), starts.as_mut_ptr(), 1), 0);
            assert_eq!((at, ints, starts), (0, [91, 92], [93]));
            assert_eq!(nzbget_rs_binrpc_read_entry(null(), 0, &mut at, null_mut(), 0, null(), null_mut(), 0), 1);

            // Outputs may reuse input storage after all input has been read.
            let mut words = [7u32.to_be(), 4u32.to_be(), 0];
            let mut field_and_start = 1usize;
            let p = words.as_mut_ptr();
            let s = &mut field_and_start as *mut usize;
            assert_eq!(nzbget_rs_binrpc_read_entry(p.cast(), 12, &mut at, p, 2, s, s, 1), 1);
            assert_eq!((words, field_and_start, at), ([7, 4, 0], 8, 12));
        }
    }

    #[test]
    fn binrpc_url_prefix_handles_null_short_and_mixed_case_strings() {
        unsafe {
            assert_eq!(nzbget_rs_binrpc_is_url(null()), 0);
            for source in [b"http:/".as_slice(), b"https:/", b"ftp:/", b"http:x", b"https:x", b"<?xml version=1><nzb>"] {
                for end in 0..=source.len() {
                    let input = std::ffi::CString::new(&source[..end]).unwrap();
                    assert_eq!(nzbget_rs_binrpc_is_url(input.as_ptr()) != 0, crate::binrpc::is_url(&source[..end]));
                }
            }
            for mask in 0..32 {
                let mut source = b"https:/".to_vec();
                for (at, ch) in source[..5].iter_mut().enumerate() {
                    if mask & (1 << at) != 0 { *ch = ch.to_ascii_uppercase(); }
                }
                let input = std::ffi::CString::new(source).unwrap();
                assert_eq!(nzbget_rs_binrpc_is_url(input.as_ptr()), 1);
            }
            let mut source = b"<?xml version=1><nzb>".to_vec();
            source.resize(1024 * 1024, b'x');
            source.push(0);
            assert_eq!(nzbget_rs_binrpc_is_url(source.as_ptr().cast()), 0);
        }
    }






    #[test]
    fn encoders_allocate_once_and_read_back() {
        unsafe {
            // nothing: an empty buffer that frees as any
            let none = nzbget_rs_binrpc_encode_log_into(null(), 5, null_mut(), 0);
            assert_eq!((none.data, none.len, none.owned_capacity), (null_mut(), 0, 0));
            let none = nzbget_rs_binrpc_encode_list_into(null(), 0, null(), 0, null(), 0, null_mut(), 0);
            assert_eq!(none.len, 0);

            let entries = [log(1, 2, b"hello"), log(2, 4, b""), log(3, 0, b"abcd")];
            let b = nzbget_rs_binrpc_encode_log_into(entries.as_ptr(), entries.len(), null_mut(), 0);
            let buf = RsBuf { data: b.data, len: b.len, cap: b.owned_capacity };
            assert_eq!(buf.len, 24 + 20 + 24);
            // the text is a C string past the data, too
            assert_eq!(*buf.data.add(buf.len), 0);
            let mut out = [std::mem::MaybeUninit::<LogEntry>::uninit(); 4];
            let mut at = 0;
            let n = crate::binrpc::read_many(bytes(&buf), &mut at, out.as_mut_ptr().cast::<LogEntry>(), 3);
            assert_eq!((n, at), (3, buf.len));
            assert_eq!(input(out[2].assume_init().texts[0].data.cast()), b"abcd");
            nzbget_rs_free(buf);

            // the three kinds of List in one buffer, in order
            let nzbs = [NzbEntry { id: 1, kind: 0, size_lo: 0, size_hi: 0, remaining_size_lo: 0, remaining_size_hi: 0,
                paused_size_lo: 0, paused_size_hi: 0, paused_count: 0, remaining_par_count: 0, priority: 0, matched: 1,
                texts: [Text::new(b"f"), Text::new(b"n"), Text::EMPTY, Text::EMPTY, Text::EMPTY] }];
            let params = [ParamEntry { nzb_index: 1, texts: [Text::new(b"k"), Text::new(b"v")] }];
            let files = [FileEntry { id: 9, nzb_index: 1, size_lo: 0, size_hi: 0, remaining_size_lo: 0, remaining_size_hi: 0,
                paused: 0, filename_confirmed: 0, active_downloads: 0, matched: 0, texts: [Text::new(b"s"), Text::EMPTY] }];
            let b = nzbget_rs_binrpc_encode_list_into(nzbs.as_ptr(), 1, params.as_ptr(), 1, files.as_ptr(), 1, null_mut(), 0);
            let buf = RsBuf { data: b.data, len: b.len, cap: b.owned_capacity };
            assert_eq!(buf.len, (68 + 8) + (12 + 4) + (48 + 4));
            let data = bytes(&buf);
            assert_eq!(&data[0..4], [0, 0, 0, 1]);
            assert_eq!(data[68..70], *b"f\0");
            assert_eq!(&data[76..80], [0, 0, 0, 1]);
            let (mut at, mut nz, mut pa, mut fi) = (0usize, MaybeUninitNzb::new(), MaybeUninitPar::new(), MaybeUninitFile::new());
            assert_eq!(crate::binrpc::read_many(data, &mut at, nz.0.as_mut_ptr().cast::<NzbEntry>(), 1), 1);
            assert_eq!(crate::binrpc::read_many(data, &mut at, pa.0.as_mut_ptr().cast::<ParamEntry>(), 1), 1);
            assert_eq!(crate::binrpc::read_many(data, &mut at, fi.0.as_mut_ptr().cast::<FileEntry>(), 1), 1);
            assert_eq!(at, buf.len);
            assert_eq!(nz.0[0].assume_init().matched, 1);
            assert_eq!(pa.0[0].assume_init().nzb_index, 1);
            assert_eq!(fi.0[0].assume_init().id, 9);
            nzbget_rs_free(buf);

            // the other two kinds
            let jobs = [PostEntry { id: 1, stage: 7, stage_progress: 100, file_progress: 0, start_time: 0, stage_time: 0,
                texts: [Text::EMPTY, Text::new(b"Name"), Text::EMPTY, Text::EMPTY] }];
            let b = nzbget_rs_binrpc_encode_post_queue_into(jobs.as_ptr(), 1, null_mut(), 0);
            let buf = RsBuf { data: b.data, len: b.len, cap: b.owned_capacity };
            assert_eq!(buf.len, 40 + 1 + 5 + 1 + 1);
            let listing = nzbget_rs_binrpc_post_queue_listing(buf.data, buf.len, 1);
            assert_eq!(bytes(&listing), b"[1] Name, Unpacking, 10%\n");
            nzbget_rs_free(listing);
            nzbget_rs_free(buf);
            let history = [HistoryEntry { id: 1, kind: 1, time: 0, size_lo: 2048, size_hi: 0, file_count: 2, par_status: 3,
                script_status: 3, url_status: 0, texts: [Text::new(b"Nzb")] }];
            let b = nzbget_rs_binrpc_encode_history_into(history.as_ptr(), 1, null_mut(), 0);
            let buf = RsBuf { data: b.data, len: b.len, cap: b.owned_capacity };
            assert_eq!(buf.len, 40 + 4);
            let listing = nzbget_rs_binrpc_history_listing(buf.data, buf.len, 1);
            assert_eq!(bytes(&listing), b"[1] Nzb (2 files, 2.00 KB, Par successful, Script successful)\n");
            nzbget_rs_free(listing);
            nzbget_rs_free(buf);
        }
    }

    struct MaybeUninitNzb([std::mem::MaybeUninit<NzbEntry>; 1]);
    struct MaybeUninitPar([std::mem::MaybeUninit<ParamEntry>; 1]);
    struct MaybeUninitFile([std::mem::MaybeUninit<FileEntry>; 1]);
    impl MaybeUninitNzb { fn new() -> Self { Self([std::mem::MaybeUninit::uninit()]) } }
    impl MaybeUninitPar { fn new() -> Self { Self([std::mem::MaybeUninit::uninit()]) } }
    impl MaybeUninitFile { fn new() -> Self { Self([std::mem::MaybeUninit::uninit()]) } }

    #[test]
    fn a_response_over_an_int_or_with_no_bytes_is_empty() {
        unsafe {
            let called = std::cell::Cell::new(false);
            for total in [0, crate::binrpc::MAX_RESPONSE + 1, u64::MAX] {
                let buf = response(total, |_| {
                    called.set(true);
                    0
                });
                assert_eq!((buf.data, buf.len, buf.cap), (null_mut(), 0, 0));
            }
            assert!(!called.get());
            let buf = response(5, |out| {
                assert_eq!(out.len(), 5);
                for (slot, byte) in out.iter_mut().zip(b"hello") {
                    slot.write(*byte);
                }
                5
            });
            assert_eq!(bytes(&buf), b"hello");
            assert_eq!(*buf.data.add(5), 0);
            nzbget_rs_free(buf);
        }
    }

    #[test]
    fn response_scratch_has_explicit_ownership_and_exact_boundaries() {
        unsafe {
            let entries = [log(1, 2, b"hello"), log(2, 0, b"")];
            let owned_resp = nzbget_rs_binrpc_encode_log_into(entries.as_ptr(), entries.len(), null_mut(), 0);
            let owned = RsBuf { data: owned_resp.data, len: owned_resp.len, cap: owned_resp.owned_capacity };
            let expected = bytes(&owned).to_vec();
            for capacity in [0, owned.len - 1, owned.len, owned.len + 1, owned.len + 9] {
                let mut scratch = vec![0xabu8; owned.len + 10];
                let b = nzbget_rs_binrpc_encode_log_into(entries.as_ptr(), entries.len(), scratch.as_mut_ptr().cast(), capacity);
                assert_eq!(bytes_arg(b.data.cast(), b.len), expected);
                assert_eq!(*b.data.add(b.len), 0);
                if capacity > owned.len {
                    assert_eq!(b.data, scratch.as_mut_ptr().cast());
                    assert_eq!(b.owned_capacity, 0);
                    assert_eq!(scratch[b.len + 1], 0xab);
                } else {
                    assert_ne!(b.data, scratch.as_mut_ptr().cast());
                    assert!(b.owned_capacity > 0);
                    assert!(scratch.iter().all(|&v| v == 0xab));
                    nzbget_rs_free(RsBuf { data: b.data, len: b.len, cap: b.owned_capacity });
                }
            }
            // A null buffer cannot be borrowed, regardless of its capacity.
            let b = nzbget_rs_binrpc_encode_log_into(entries.as_ptr(), entries.len(), null_mut(), usize::MAX);
            assert!(b.owned_capacity > 0);
            assert_eq!(bytes_arg(b.data.cast(), b.len), expected);
            nzbget_rs_free(RsBuf { data: b.data, len: b.len, cap: b.owned_capacity });
            let mut scratch = [0xabu8; 32];
            for total in [0, crate::binrpc::MAX_RESPONSE + 1, u64::MAX] {
                let b = response_into(total, scratch.as_mut_ptr().cast(), scratch.len(), |_| panic!("invalid size filled"));
                assert_eq!((b.data, b.len, b.owned_capacity), (null_mut(), 0, 0));
                assert!(scratch.iter().all(|&v| v == 0xab));
            }
            nzbget_rs_free(owned);
        }
    }

    #[test]
    fn listings_of_null_and_short_data_are_empty_strings() {
        unsafe {
            for listing in [nzbget_rs_binrpc_log_listing(null(), 0, 5), nzbget_rs_binrpc_post_queue_listing(null(), 0, 5),
                nzbget_rs_binrpc_history_listing(null(), 7, 5)] {
                assert_eq!(bytes(&listing), b"");
                assert_eq!(*listing.data, 0);
                nzbget_rs_free(listing);
            }
            let entries = [log(1, 0, b"x")];
            let b = nzbget_rs_binrpc_encode_log_into(entries.as_ptr(), 1, null_mut(), 0);
            let buf = RsBuf { data: b.data, len: b.len, cap: b.owned_capacity };
            let listing = nzbget_rs_binrpc_log_listing(buf.data, buf.len, 2);
            assert_eq!(bytes(&listing), b"[INFO] x\n");
            nzbget_rs_free(listing);
            nzbget_rs_free(buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twin_fingerprint_takes_arrays() {
        unsafe {
            let md5s = [b"abc\0".as_ptr().cast(), std::ptr::null(), b"abc\0".as_ptr().cast()];
            let lengths = [1u64, 7, 1];
            let result = nzbget_rs_twin_fingerprint(md5s.as_ptr(), lengths.as_ptr(), 3);
            let expected = crate::twin::fingerprint_of([("abc", 1), ("", 7)].into_iter());
            assert_eq!(input(result.data), expected.as_bytes()); // the repeated one counts once
            nzbget_rs_free(result);
            for (m, l, n) in [(std::ptr::null(), std::ptr::null(), 0), (std::ptr::null(), lengths.as_ptr(), 1)] {
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
        let poison = || RsBuf { data: null_mut(), len: usize::MAX, cap: usize::MAX };
        unsafe {
            let mut error = poison();
            assert_eq!(nzbget_rs_delete_directory_with_content(null(), b'/' as c_char, &mut error), 0);
            assert!(input(error.data).starts_with(b"could not delete (null): "));
            nzbget_rs_free(error);
            let mut name = poison();
            assert_eq!(nzbget_rs_rename_bak(null(), null(), 0, &mut name), 0);
            assert_eq!(name.len, 0);
            assert_eq!(input(name.data), b"");
            nzbget_rs_free(name);
            assert_eq!(nzbget_rs_delete_directory_with_content(null(), 0, null_mut()), 0);
            assert_eq!(nzbget_rs_rename_bak(null(), null(), 0, null_mut()), 0);
            assert_eq!(nzbget_rs_file_extension(null(), 0), -1);
            assert_eq!(nzbget_rs_file_extension(b"a.\0b.c".as_ptr().cast(), 6), 4);
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
            assert_eq!(nzbget_rs_par_renamer_list_parred_files(null(), 1024, null_mut(), None), 0);
            assert_eq!(nzbget_rs_par_renamer_load_par_entries(null(), null_mut(), None, None), 0);
        }
    }

    #[test]
    fn rarrenamer_nulls_and_borrowed_callback_strings() {
        use std::ptr::{null, null_mut};
        unsafe extern "C" fn progress(ctx: *mut std::ffi::c_void, label: *const c_char, stage: c_int) -> c_int {
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
            assert_eq!(nzbget_rs_rar_renamer_execute(null(), null(), null(), null(), null(), 1, null_mut()), -1);
            assert_eq!(nzbget_rs_rar_renamer_execute(&env, null(), null(), null(), null(), 1, null_mut()), -1);
            assert_eq!(nzbget_rs_rar_renamer_execute(&env, c"/nonexistent_dir_12345".as_ptr(), c"job".as_ptr(), null(), null(), 1, &mut renamed), 0);
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
            assert_eq!(nzbget_rs_parse_par_filename(c"a.vol+2.par2".as_ptr(), -1, p, p), 1);
            assert_eq!(output, -1); // blocks wins when outputs alias
            assert_eq!(nzbget_rs_parse_par_filename(c"a.par2".as_ptr(), 1, null_mut(), null_mut()), 1);
            assert_eq!(nzbget_rs_same_par_collection(null(), c"a.par2".as_ptr(), 1), 0);
            assert_eq!(nzbget_rs_same_par_collection(c"a.par2".as_ptr(), null(), 1), 0);

            let mut name = [b'a'; crate::parparser::BSTRING_MAX];
            name[1018..].copy_from_slice(b".par2");
            assert_eq!(nzbget_rs_parse_par_filename(name.as_ptr().cast(), 1, p, null_mut()), 1);
            assert_eq!(output, 1018);
            assert_eq!(nzbget_rs_same_par_collection(name.as_ptr().cast(), name.as_ptr().cast(), 1), 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn fsops_null_outputs_and_ownership() {
        unsafe {
            let mut msg = std::mem::MaybeUninit::uninit();
            assert_eq!(nzbget_rs_force_directories(std::ptr::null(), msg.as_mut_ptr()), 0);
            let msg = msg.assume_init();
            assert_eq!(msg.len, 0);
            assert_eq!(CStr::from_ptr(msg.data), c"");
            nzbget_rs_free(msg);
            assert_eq!(nzbget_rs_force_directories(c".".as_ptr(), std::ptr::null_mut()), 0);
            assert_eq!(nzbget_rs_file_exists(std::ptr::null()), 0);
            assert_eq!(nzbget_rs_directory_exists(std::ptr::null()), 0);
            assert_eq!(nzbget_rs_directory_exists(c".".as_ptr()), 1);
            let mut source = b"~/x\0".to_vec();
            let expanded = crate::fsops::expand_home_path(Some(CStr::from_ptr(source.as_ptr().cast())), Some(b""));
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
            ctx: buffer.as_mut_ptr().cast(), write_line: None, read_line: Some(greeting),
            report_error: None, debug: None, cancelled: None,
            user: null(), password: null(), name: null(), host: null(), conn_host: null(),
        };
        unsafe {
            let (mut error, mut rejected, mut result, mut joined) = (1, 1, -9, -9);
            let mut answer = null_mut();
            assert_eq!(nzbget_rs_nntp_request(null(), null(), null_mut(), null_mut(), null_mut()), -1);
            assert_eq!(nzbget_rs_nntp_handshake(null(), null_mut(), null_mut(), null_mut()), -1);
            assert_eq!(nzbget_rs_nntp_join_group(null(), null(), null_mut(), null_mut(), null_mut(), null_mut()), -1);
            assert_eq!(nzbget_rs_nntp_request(&io, null(), &mut error, &mut rejected, &mut answer), -1);
            assert_eq!(nzbget_rs_nntp_join_group(&io, null(), &mut error, &mut rejected, &mut answer, &mut joined), -1);
            assert_eq!(nzbget_rs_nntp_handshake(null(), &mut error, &mut rejected, &mut result), -1);
            assert_eq!((error, rejected, result, joined), (1, 1, -9, -9));

            // NULL configuration strings are empty; optional debug is absent.
            assert_eq!(nzbget_rs_nntp_handshake(&io, &mut error, &mut rejected, &mut result), 0);
            assert_eq!((error, rejected, result), (1, 1, 0));

            // Missing required write/read callbacks stop the exchange and leave
            // result outputs untouched, while writing back reached auth state.
            assert_eq!(nzbget_rs_nntp_request(&io, c"BODY x\r\n".as_ptr(), &mut error, &mut rejected, &mut answer), -1);
            assert_eq!((error, rejected), (0, 0));
            assert!(answer.is_null());
            io.read_line = None;
            result = -9;
            assert_eq!(nzbget_rs_nntp_handshake(&io, &mut error, &mut rejected, &mut result), -1);
            assert_eq!(result, -9);
        }
    }

    #[test]
    fn options_ffi_outputs() {
        use std::ptr::null_mut;
        unsafe {
            assert_eq!(nzbget_rs_validate_option_name(std::ptr::null(), None, null_mut()), 0);
            assert_eq!(nzbget_rs_validate_option_name(c"Server1.Host".as_ptr(), None, null_mut()), 1);
            assert_eq!(nzbget_rs_option_restricted(std::ptr::null(), 1), 0);
            assert_eq!(nzbget_rs_option_restricted(c"Server1.Password".as_ptr(), 1), 1);
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
            slots = Slots { sec: 0, min: -1, hour: 1, ..Slots::default() };
            nzbget_rs_volume_add(sec.as_mut_ptr().add(1), 1, min.as_mut_ptr().add(1), 1,
                hour.as_mut_ptr().add(1), 1, &slots, 0, 0, 0, 0, 5);
            assert_eq!(sec, [91, (1i64 << 40) + 5, 92]);
            assert_eq!(min, [93, 7, 94]);
            assert_eq!(hour, [95, 8, 96]);
            nzbget_rs_volume_add(sec.as_mut_ptr(), 3, null_mut(), 0,
                null_mut(), 0, null(), 0, 0, 60, 0, 5);
            assert_eq!(sec, [91, (1i64 << 40) + 5, 92]);
            for t in [i64::MIN, i64::MAX, i64::from(i32::MIN), -2_147_483_647, -61] {
                nzbget_rs_volume_calc_slots(t, &mut first, &mut slots);
                nzbget_rs_volume_add(null_mut(), usize::MAX, sec.as_mut_ptr().add(1), 1,
                    hour.as_mut_ptr().add(1), 0, &slots, 0, 0, t, 0, i64::MAX);
            }
            assert_eq!((sec[0], sec[2]), (91, 92));
            assert_eq!(hour, [95, 8, 96]);

            // The readable descriptor can occupy output storage. Read it
            // before constructing exclusive array references.
            let mut storage = [0i64; 3];
            let buffer = storage.as_mut_ptr();
            let descriptor = buffer.cast::<Slots>();
            descriptor.write(Slots::default());
            nzbget_rs_volume_add(buffer, 3, null_mut(), 0,
                null_mut(), 0, descriptor, 0, 0, 0, 0, 9);
            assert_eq!(storage, [9, 0, 0]);
        }
    }

    #[test]
    fn textutil_in_place_ranges_nulls_and_ownership() {
        unsafe {
            let null = std::ptr::null_mut();
            assert!(nzbget_rs_trim_line(null, 0).is_null());
            let buf = nzbget_rs_split_command_line(null);
            assert_eq!(buf.len, 0);
            assert_eq!(*buf.data, 0);
            nzbget_rs_free(buf);
            for (source, expected, offset) in [
                (b"! x \t\r\n\0!".as_slice(), b"! x\0\0\0\0\0!".as_slice(), 1),
                (b"! \t\r\n\0!", b"!\0\0\0\0\0!", 0),
                (b"!\0!", b"!\0!", 0),
            ] {
                for right_only in [0, 1] {
                    let mut buf = source.to_vec();
                    let base = buf.as_mut_ptr().add(1).cast();
                    let result = nzbget_rs_trim_line(base, right_only);
                    assert_eq!(result, base.add(if right_only == 0 { offset } else { 0 }));
                    assert_eq!(buf, expected);
                }
            }
            let mut source = b"a 'b c'\0".to_vec();
            let words = nzbget_rs_split_command_line(source.as_ptr().cast());
            source.fill(b'x');
            assert_eq!(bytes_arg(words.data.cast(), words.len), b"a\0b c\0");
            nzbget_rs_free(words);
        }
    }

    #[test]
    fn rpc_route_null_inputs_borrowing_and_method_capacity() {
        unsafe {
            let null = std::ptr::null();
            assert_eq!(nzbget_rs_rpc_protocol(null), 0);
            for get in [0, 1] {
                for protocol in [0, 1, 2, 3] {
                    let mut method = [0x55_u8; 102];
                    let mut params = c"sentinel".as_ptr();
                    let mut id = params;
                    let mut len = -1;
                    nzbget_rs_rpc_route(null, null, get, protocol,
                        method.as_mut_ptr().add(1).cast(), &mut params, &mut id, &mut len);
                    assert_eq!((method[0], method[1], method[101]), (0x55, 0, 0x55));
                    assert!(params.is_null() && id.is_null());
                    assert_eq!(len, 0);
                }
            }
            let mut url = b"/jsonrpc/".to_vec();
            url.extend_from_slice(&[b'x'; 200]);
            url.extend_from_slice(b"?a=1\0");
            let mut method = [0x55_u8; 102];
            let mut params = null;
            let mut id = null;
            let mut len = -1;
            nzbget_rs_rpc_route(url.as_ptr().cast(), null, 1, 2,
                method.as_mut_ptr().add(1).cast(), &mut params, &mut id, &mut len);
            assert_eq!(&method[1..100], &[b'x'; 99]);
            assert_eq!((method[0], method[100], method[101]), (0x55, 0, 0x55));
            assert_eq!(params, url.as_ptr().add(210).cast());
            assert!(id.is_null());
            let request = c"\"id\":},\"method\":\"\"";
            nzbget_rs_rpc_route(null, request.as_ptr(), 0, 2,
                method.as_mut_ptr().add(1).cast(), &mut params, &mut id, &mut len);
            assert_eq!(params, request.as_ptr());
            assert_eq!(id, request.as_ptr().add(5));
            assert_eq!(len, 0); // CString::Set will echo the suffix for length zero.
            assert_eq!(&method[..3], &[0x55, b'"', 0]);
        }
    }

    #[test]
    fn rpc_decide_outputs() {
        unsafe {
            assert_eq!(nzbget_rs_rpc_access(3, 0), 1);
            assert_eq!(nzbget_rs_rpc_access(2, 0), 0);
            let mut code = -1;
            let mut text = c"sentinel".as_ptr();
            assert_eq!(nzbget_rs_rpc_decide(3, 1, 0, 0, 1, &mut code, &mut text), 1);
            assert_eq!((code, CStr::from_ptr(text)),
                (4, c"Not safe procedure for JSON-P. Use JSON-RPC or XML-RPC with HTTP-Method POST instead"));
            assert_eq!(nzbget_rs_rpc_decide(2, 0, 0, 0, 0, &mut code, &mut text), 0);
            assert!(code == 0 && text.is_null());
            assert_eq!(nzbget_rs_rpc_decide(3, 0, 1, 0, 0, std::ptr::null_mut(), std::ptr::null_mut()), 1);
        }
    }

    #[test]
    fn rpc_envelope_owns_both_outputs() {
        unsafe {
            let mut cb = b"callback\0ignored".to_vec();
            let mut id = b"7\0ignored".to_vec();
            let mut head = std::mem::MaybeUninit::uninit();
            let mut tail = std::mem::MaybeUninit::uninit();
            nzbget_rs_rpc_envelope(3, 0, cb.as_ptr().cast(), id.as_ptr().cast(),
                head.as_mut_ptr(), tail.as_mut_ptr());
            let head = head.assume_init();
            let tail = tail.assume_init();
            cb.fill(b'x');
            id.fill(b'x');
            assert_eq!(input(head.data), b"callback({\n\"version\" : \"1.1\",\n\"id\" : 7,\n\"result\" : ");
            assert_eq!(input(head.data).len(), head.len);
            nzbget_rs_free(head);
            assert_eq!(input(tail.data), b"\n})");
            assert_eq!(tail.len, 3);
            nzbget_rs_free(tail);
        }
    }

    #[test]
    fn rpc_null_arguments_and_unknown_kind() {
        unsafe {
            assert!(nzbget_rs_rpc_skip_to_params(std::ptr::null_mut()).is_null());
            assert_eq!(nzbget_rs_rpc_next_param(std::ptr::null_mut(), 1, 1, 0,
                std::ptr::null_mut(), std::ptr::null_mut()), 0);
            let mut null = std::ptr::null_mut();
            let mut number = 12345;
            assert_eq!(nzbget_rs_rpc_next_param(&mut null, 1, 1, 0,
                &mut number, std::ptr::null_mut()), 0);
            assert_eq!(number, 12345);
            for (what, input) in [(0, b"a=12&b=3\0".as_slice()),
                (1, b"a=true&b=false\0"), (2, b"a=x%20y&b=z\0"), (-1, b"a=1\0"), (3, b"a=1\0")]
            {
                let mut buf = input.to_vec();
                let base = buf.as_mut_ptr().cast();
                let mut request = base;
                assert_eq!(nzbget_rs_rpc_next_param(&mut request, 1, 1, what,
                    std::ptr::null_mut(), std::ptr::null_mut()), 0);
                assert_eq!(request, base);
                assert_eq!(buf, input);
            }
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
        let original = Task { hours: 336, minutes: 0, week_days: 0, last_executed: 0 };
        let mut task = original;
        let mut last = 0;
        let mut reset = -1;
        let mut due = [usize::MAX; 17];
        for capacity in [0, 1, 9, 14] {
            let n = unsafe {
                nzbget_rs_scheduler_check(&mut task, 1, &mut last, 80_049_600, 0, 0,
                    if capacity == 0 { std::ptr::null_mut() } else { due.as_mut_ptr().add(1) },
                    capacity, &mut reset, Some(calendar))
            };
            assert_eq!(n, 15);
            assert_eq!(task, original);
            assert_eq!(last, 0);
            assert_eq!(reset, -1);
            assert_eq!(due, [usize::MAX; 17]);
        }
        let n = unsafe {
            nzbget_rs_scheduler_check(&mut task, 1, &mut last, 80_049_600, 0, 0,
                due.as_mut_ptr().add(1), 15, &mut reset, Some(calendar))
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
        let original = Task { hours: -1, minutes: 0, week_days: 0, last_executed: 0 };
        // Each required pointer may be absent. Rejection must be atomic.
        for missing in 0..5 {
            let mut task = original;
            let mut last = 123;
            let mut reset = -1;
            let mut due = [usize::MAX; 9];
            let n = unsafe {
                nzbget_rs_scheduler_check(
                    if missing == 0 { std::ptr::null_mut() } else { &mut task },
                    1,
                    if missing == 1 { std::ptr::null_mut() } else { &mut last },
                    456,
                    0,
                    0,
                    if missing == 2 { std::ptr::null_mut() } else { due.as_mut_ptr() },
                    due.len(),
                    if missing == 3 { std::ptr::null_mut() } else { &mut reset },
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
            assert_eq!(nzbget_rs_scheduler_check(&mut task, usize::MAX, &mut last, 456, 0, 0, due.as_mut_ptr(), due.len(), &mut reset, Some(calendar)), 0);
            assert_eq!(nzbget_rs_scheduler_check(&mut task, 1, &mut last, 456, 0, 0, due.as_mut_ptr(), usize::MAX, &mut reset, Some(calendar)), 0);
            assert_eq!(task, original);
            assert_eq!(last, 123);
            assert_eq!(reset, -1);
            assert_eq!(due, [usize::MAX; 9]);
            assert_eq!(nzbget_rs_scheduler_check(std::ptr::null_mut(), 0, &mut last, 456, 0, 0, std::ptr::null_mut(), 0, &mut reset, Some(calendar)), 0);
        }
        assert_eq!(last, 456);
        assert_eq!(reset, 0);
    }

    #[test]
    fn web_null_inputs_and_optional_outputs() {
        unsafe {
            let mut check = WebCheckC { authorized: 1, access: 2, auth_cut: 100, warn: 1 };
            nzbget_rs_web_check_credentials(std::ptr::null(), &mut check);
            assert_eq!((check.authorized, check.access, check.auth_cut, check.warn), (0, -1, -1, 0));
            nzbget_rs_web_check_credentials(std::ptr::null(), std::ptr::null_mut());
            let mut redirect = -1;
            let mut auth = RsBuf { data: std::ptr::null_mut(), len: 0, cap: 0 };
            let url = nzbget_rs_web_parse_url(std::ptr::null(), 42, &mut redirect, &mut auth);
            assert_eq!(redirect, 0);
            assert!(auth.data.is_null());
            assert_eq!(CStr::from_ptr(url.data).to_bytes(), b"");
            nzbget_rs_free(url);
            nzbget_rs_free(auth);
            let input = b"/nzbget\0/u:p/jsonrpc";
            let url = nzbget_rs_web_parse_url(input.as_ptr().cast(), input.len(), &mut redirect, std::ptr::null_mut());
            assert_eq!(redirect, 1);
            assert_eq!(CStr::from_ptr(url.data).to_bytes(), b"/nzbget/");
            nzbget_rs_free(url);
            let input = b"/u:p/jsonrpc";
            let url = nzbget_rs_web_parse_url(input.as_ptr().cast(), input.len(), std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(CStr::from_ptr(url.data).to_bytes(), b"/jsonrpc");
            nzbget_rs_free(url);
            let (mut start, mut len, mut number) = (99, 99, 99);
            assert_eq!(nzbget_rs_web_header(std::ptr::null(), 42, 1, &mut start, &mut len, &mut number), 10);
            assert_eq!((start, len, number), (0, 0, 0));
        }
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
            let mut raw = [b'a', crate::paths::ALT_PATH_SEPARATOR, 0, crate::paths::ALT_PATH_SEPARATOR];
            nzbget_rs_normalize_path_separators(raw.as_mut_ptr().cast());
            assert_eq!(raw, [b'a', crate::paths::PATH_SEPARATOR, 0, crate::paths::ALT_PATH_SEPARATOR]);

            // Length-bounded input need not have a terminator. The result
            // owns its bytes, including embedded NULs, and an extra terminator.
            let mut input = b"a\0b".to_vec();
            let first = nzbget_rs_path_text(3, input.as_ptr().cast(), input.len(), 0);
            let second = nzbget_rs_path_text(1, input.as_ptr().cast(), input.len(), 0);
            input.fill(b'x');
            drop(input);
            assert_eq!(std::slice::from_raw_parts(first.data.cast::<u8>(), first.len + 1), b"\"a\0b\"\0");
            assert_eq!(CStr::from_ptr(second.data), c"a");
            nzbget_rs_free(second);
            nzbget_rs_free(first);
            let path = b"a/b";
            assert_eq!(nzbget_rs_path_position(0, path.as_ptr().cast(), path.len()), 2);
            assert_eq!(nzbget_rs_path_position(1, path.as_ptr().cast(), path.len()), 1);
            assert_eq!(nzbget_rs_path_position(1, path.as_ptr().cast(), 1), usize::MAX);
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
                assert_eq!(nzbget_rs_is_excessively_obfuscated(std::ptr::null(), len), 0);
            }
            let mut input = b"prefix \"a\0b\" suffix".to_vec();
            let first = crate::deobfuscation::deobfuscate(&input);
            let second = crate::deobfuscation::deobfuscate(&input[..6]);
            input.fill(b'x');
            drop(input);
            assert_eq!(first, b"a\0b");
            assert_eq!(second, b"prefix");
            assert_eq!(nzbget_rs_is_excessively_obfuscated(b"abcx".as_ptr().cast(), 3), 1);
            assert_eq!(nzbget_rs_is_excessively_obfuscated(b"abcx".as_ptr().cast(), 4), 0);
        }
    }

    #[test]
    fn feed_filter_nulls_ownership_and_callback_order() {
        use std::ffi::c_void;
        #[derive(Default)]
        struct Probe {
            events: Vec<String>,
            category: Vec<u8>,
            priority: i64,
        }
        unsafe fn probe<'a>(user: *mut c_void) -> &'a mut Probe {
            &mut *user.cast::<Probe>()
        }
        unsafe extern "C" fn field(user: *mut c_void, f: c_int, _: *const c_char, s: *mut *const c_char, n: *mut i64) {
            let p = probe(user);
            p.events.push(format!("field {f}"));
            *s = std::ptr::null();
            *n = if f == 13 { p.priority } else { 0 };
        }
        unsafe extern "C" fn season(user: *mut c_void, episode: c_int) -> *const c_char {
            probe(user).events.push(format!("season {episode}"));
            c"02".as_ptr()
        }
        unsafe extern "C" fn apply(user: *mut c_void, o: *const FeedOptions) {
            let p = probe(user);
            let o = &*o;
            p.events.push("apply".into());
            if o.has_category != 0 {
                p.category = c_text(o.category).unwrap_or_default();
            }
            if o.has_priority != 0 { p.priority = o.priority as i64; }
        }
        unsafe extern "C" fn set_match(user: *mut c_void, status: c_int, rule: c_int) {
            probe(user).events.push(format!("match {status} {rule}"));
        }
        let mut p = Probe::default();
        let cb = FeedItemCallbacks {
            user: (&mut p as *mut Probe).cast(), field: Some(field),
            season_episode: Some(season),
            apply: Some(apply), set_match: Some(set_match),
        };
        unsafe {
            nzbget_rs_feed_filter_free(std::ptr::null_mut());
            nzbget_rs_feed_filter_match(std::ptr::null_mut(), std::ptr::null());
            let empty = nzbget_rs_feed_filter_new(std::ptr::null());
            nzbget_rs_feed_filter_match(empty, &cb);
            nzbget_rs_feed_filter_free(empty);
            assert_eq!(p.events, ["match 0 0"]);
            p.events.clear();

            let input = std::ffi::CString::new("O(c:${season},r:7): **%A: priority:=7").unwrap();
            let filter = nzbget_rs_feed_filter_new(input.as_ptr());
            drop(input);
            nzbget_rs_feed_filter_match(filter, std::ptr::null());
            // A zero-initialized C callback table and each missing callback
            // must be representable and rejected without invoking anything.
            let zero: FeedItemCallbacks = std::mem::zeroed();
            nzbget_rs_feed_filter_match(filter, &zero);
            for missing in 0..4 {
                let mut bad = FeedItemCallbacks { ..cb };
                match missing {
                    0 => bad.field = None,
                    1 => bad.season_episode = None,
                    2 => bad.apply = None,
                    _ => bad.set_match = None,
                }
                nzbget_rs_feed_filter_match(filter, &bad);
            }
            assert!(p.events.is_empty());
            nzbget_rs_feed_filter_match(filter, &cb);
            nzbget_rs_feed_filter_free(filter);
            assert_eq!(p.category, b"02");
            assert_eq!(p.events, ["field 0", "season 0", "match 1 1", "apply", "field 13", "match 1 2", "apply"]);
        }
    }

    #[test]
    fn util_ffi_nulls_ownership_and_table_without_callbacks() {
        unsafe {
            nzbget_rs_reduce_str(std::ptr::null_mut(), std::ptr::null(), std::ptr::null());
            let mut raw = *b"abc\0";
            nzbget_rs_reduce_str(raw.as_mut_ptr().cast(), std::ptr::null(), c"".as_ptr());
            assert_eq!(&raw, b"abc\0");
            let first = nzbget_rs_format_size(512);
            let second = nzbget_rs_format_size(12345);
            nzbget_rs_free(second);
            assert_eq!(std::slice::from_raw_parts(first.data.cast::<u8>(), first.len + 1), &b"512 B\0"[..]);
            nzbget_rs_free(first);
            for list in [c".NZB", c"*.NZ?"] {
                assert_eq!(nzbget_rs_match_file_ext(c"a.nzb".as_ptr(), list.as_ptr(), c",".as_ptr()), 1);
            }
            assert_eq!(nzbget_rs_match_file_ext(std::ptr::null(), std::ptr::null(), std::ptr::null()), 0);
        }
    }

    #[test]
    fn webutil_nulls_borrowing_and_ownership() {
        unsafe {
            let mut length = -7;
            for (text, name) in [(std::ptr::null(), c"x".as_ptr()),
                                 (c"".as_ptr(), std::ptr::null()),
                                 (c"".as_ptr(), c"x".as_ptr())] {
                assert!(nzbget_rs_xml_find_tag(text, name, &mut length).is_null());
                assert_eq!(length, -7);
            }
            assert!(nzbget_rs_xml_find_tag(c"".as_ptr(), c"x".as_ptr(), std::ptr::null_mut()).is_null());
            let xml = c"<x>abc</x>";
            assert_eq!(nzbget_rs_xml_find_tag(xml.as_ptr(), c"x".as_ptr(), &mut length), xml.as_ptr().add(3));
            assert_eq!(length, 3);
        }
    }

    #[test]
    fn text_null_inputs_and_callback() {
        unsafe {
            nzbget_rs_xml_decode(std::ptr::null_mut());
            let mut xml = *b"&#xA;\0tail";
            nzbget_rs_xml_decode(xml.as_mut_ptr().cast());
            assert_eq!(&xml[..2], b"\n\0");
        }
    }

    #[test]
    fn decoder_null_arguments_and_failed_value_preserve_length() {
        unsafe {
            assert_eq!(nzbget_rs_decode_base64(std::ptr::null(), 4, std::ptr::null_mut()), 0);
            nzbget_rs_json_decode(std::ptr::null_mut());
        }
    }

    #[test]
    fn decoder_in_place_buffers_and_borrowed_value_pointer() {
        unsafe {
            let mut base64 = *b"YWJj\0YQ==!";
            let ptr = base64.as_mut_ptr().cast();
            assert_eq!(nzbget_rs_decode_base64(ptr, 9, ptr), 4);
            assert_eq!(&base64, b"abca\0YQ==!");

            let mut json = *b"\\ud83d\\ude00\\u0000\0!";
            nzbget_rs_json_decode(json.as_mut_ptr().cast());
            assert_eq!(&json[..8], b"\xf0\x9f\x98\x80\xef\xbf\xbd\0");
            assert_eq!(json[19], b'!');
        }
    }

    #[test]
    fn null_buffer_can_be_freed() {
        unsafe { nzbget_rs_free(RsBuf { data: std::ptr::null_mut(), len: 0, cap: 0 }) };
    }
}

// ---- dupesearch (daemon/dupesearch/) ----

/// The bytes of a (pointer, length) argument; NULL is empty.
unsafe fn arg<'a>(data: *const c_char, len: usize) -> &'a [u8] {
    if data.is_null() || len == 0 { &[] } else { std::slice::from_raw_parts(data.cast(), len) }
}

/// ReleaseName::Normalize.
///
/// # Safety
/// `name` must be readable for `len` bytes (or NULL).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_releasename_normalize(name: *const c_char, len: usize) -> RsBuf {
    into_buf(crate::releasename::normalize(arg(name, len)))
}

/// ReleaseName::Readable.
///
/// # Safety
/// `name` must be readable for `len` bytes (or NULL).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_releasename_readable(name: *const c_char, len: usize) -> c_int {
    c_int::from(crate::releasename::readable(arg(name, len)))
}

/// ReleaseName::SameRelease.
///
/// # Safety
/// `a` and `b` must be readable for `alen` and `blen` bytes (or NULL).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_releasename_same_release(a: *const c_char, alen: usize, b: *const c_char,
                                                            blen: usize) -> c_int {
    c_int::from(crate::releasename::same_release(arg(a, alen), arg(b, blen)))
}

/// One search result across the C ABI, as Posting's grouping and ordering read it.
#[repr(C)]
pub struct NzbgetRsListing {
    pub link: *const c_char,
    pub link_len: usize,
    pub size: i64,
    pub grabs: i32,
    pub date: i64,
}

/// The strings of (pointer, length) arrays; NULL arrays are empty.
unsafe fn strings<'a>(data: *const *const c_char, lens: *const usize, n: usize) -> Vec<&'a [u8]> {
    if n == 0 || data.is_null() || lens.is_null() {
        return Vec::new();
    }
    let (data, lens) = (std::slice::from_raw_parts(data, n), std::slice::from_raw_parts(lens, n));
    data.iter().zip(lens).map(|(&p, &l)| arg(p, l)).collect()
}

unsafe fn listings<'a>(raw: *const NzbgetRsListing, n: usize) -> Vec<crate::posting::Listing<'a>> {
    if n == 0 || raw.is_null() {
        return Vec::new();
    }
    std::slice::from_raw_parts(raw, n)
        .iter()
        .map(|l| crate::posting::Listing { link: arg(l.link, l.link_len), size: l.size, grabs: l.grabs, date: l.date })
        .collect()
}

/// Writes groups of indices: `indices` (writable for n) gets them group after
/// group, `group_sizes` (writable for n) each group's length, `*groups` the
/// number of groups; returns the indices written.
unsafe fn write_groups(groups: Vec<Vec<usize>>, indices: *mut usize, group_sizes: *mut usize, count: *mut usize) -> usize {
    let mut written = 0;
    for (g, group) in groups.iter().enumerate() {
        *group_sizes.add(g) = group.len();
        for &i in group {
            *indices.add(written) = i;
            written += 1;
        }
    }
    *count = groups.len();
    written
}

/// Posting::MakeSketch: `out` (writable for 64) gets the sketch, `*out_len` its length.
///
/// # Safety
/// `ids`/`lens` readable for `n`; `out` writable for 64; `out_len` writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_posting_sketch(ids: *const *const c_char, lens: *const usize, n: usize,
                                                  out: *mut u32, out_len: *mut usize) {
    let sketch = crate::posting::make_sketch(strings(ids, lens, n));
    std::ptr::copy_nonoverlapping(sketch.as_ptr(), out, sketch.len());
    *out_len = sketch.len();
}

/// Posting::SameSketch.
///
/// # Safety
/// `a` and `b` readable for `na` and `nb` (NULL when 0).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_posting_same_sketch(a: *const u32, na: usize, b: *const u32, nb: usize) -> c_int {
    let s = |p: *const u32, n: usize| if n == 0 || p.is_null() { &[][..] } else { std::slice::from_raw_parts(p, n) };
    c_int::from(crate::posting::same_sketch(s(a, na), s(b, nb)))
}

/// Posting::SamePosting (both lists sorted).
///
/// # Safety
/// The arrays must be readable for their counts.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_posting_same_posting(a: *const *const c_char, alens: *const usize, na: usize,
                                                        b: *const *const c_char, blens: *const usize, nb: usize) -> c_int {
    c_int::from(crate::posting::same_posting(&strings(a, alens, na), &strings(b, blens, nb)))
}

/// Posting::ListingMismatch.
#[no_mangle]
pub extern "C" fn nzbget_rs_posting_listing_mismatch(listing_size: i64, nzb_bytes: i64) -> c_int {
    c_int::from(crate::posting::listing_mismatch(listing_size, nzb_bytes))
}

/// Posting::OrderPostings (see write_groups).
///
/// # Safety
/// `listings` readable for `n`; `indices` and `group_sizes` writable for `n`; `groups` writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_posting_order(raw: *const NzbgetRsListing, n: usize, pick_bytes: i64,
                                                 max_postings: c_int, indices: *mut usize, group_sizes: *mut usize,
                                                 groups: *mut usize) -> usize {
    write_groups(crate::posting::order_postings(&listings(raw, n), pick_bytes, max_postings), indices, group_sizes, groups)
}

// Native duplicate-discovery identity (rust/src/dupeidentity.rs).
#[repr(C)]
pub struct DupeIdentityInput {
    pub bytes: u64,
    pub name: *const c_char,
    pub name_len: usize,
    pub extension: *const c_char,
    pub extension_len: usize,
}

unsafe fn dupe_identity_input(raw: *const DupeIdentityInput) -> Option<crate::dupeidentity::Identity> {
    if raw.is_null() { return None; }
    let raw = &*raw;
    if raw.bytes == 0 { return None; }
    Some(crate::dupeidentity::Identity {
        bytes: raw.bytes,
        name: arg(raw.name, raw.name_len).to_vec(),
        extension: arg(raw.extension, raw.extension_len).to_vec(),
    })
}

/// # Safety
/// Names are readable for their lengths, or NULL (empty).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_candidate(title: *const c_char, title_len: usize, bytes: u64,
    name: *const c_char, name_len: usize, size: u64) -> c_int {
    c_int::from(crate::dupeidentity::candidate_ok(arg(title, title_len), bytes, arg(name, name_len), size, 0.0, true))
}

/// # Safety
/// Names readable for their lengths; identities NULL or valid with readable fields.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_lenient_ok(title: *const c_char, title_len: usize,
    main_name: *const c_char, main_len: usize, mine: *const DupeIdentityInput, theirs: *const DupeIdentityInput) -> c_int {
    let mine = dupe_identity_input(mine);
    let theirs = dupe_identity_input(theirs);
    c_int::from(crate::dupeidentity::lenient_ok(arg(title, title_len), arg(main_name, main_len), mine.as_ref(), theirs.as_ref()))
}

// Native duplicate-discovery watcher and per-search fetch budget.
#[repr(C)]
pub struct DupeWatchPeer {
    pub id: i32,
    pub score: i32,
    pub flags: u32,
    pub key: *const c_char,
    pub key_len: usize,
}

/// Eligibility with an unchanged identity and an existing search stamp, before
/// peer snapshots are needed. This scalar boundary does not read queue memory.
#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_watch_stamped(flags: u32, score: i32, now: i64,
    previous_score: i32, previous_time: i64) -> c_int {
    crate::dupewatch::decide(flags, score, None, now,
        Some(crate::dupewatch::SearchStamp { score: previous_score, time: previous_time }), false) as c_int
}

#[inline(never)]
unsafe fn dupe_watch_has_higher(pick: &DupeWatchPeer, peers: *const DupeWatchPeer, count: usize) -> bool {
    use crate::dupewatch::{self, Snapshot};
    let mine = Snapshot { id: pick.id, score: pick.score, flags: pick.flags, key: arg(pick.key, pick.key_len) };
    for peer in std::slice::from_raw_parts(peers, count) {
        if peer.id == pick.id || peer.score <= pick.score { continue; }
        let other = Snapshot { id: peer.id, score: peer.score, flags: peer.flags, key: arg(peer.key, peer.key_len) };
        if dupewatch::higher_peer(mine, other, true) { return true; }
    }
    false
}

/// # Safety
/// Peer records and key buffers valid for their counts; pick is non-NULL.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_watch(pick: *const DupeWatchPeer, peers: *const DupeWatchPeer,
    count: usize, now: i64, previous_score: i32, previous_time: i64, has_previous: c_int,
    identity_changed: c_int) -> c_int {
    use crate::dupewatch::{self, Decision};
    if pick.is_null() { return 0; }
    let pick = &*pick;
    // Changed identities never use the previous stamp; avoid irrelevant time
    // arithmetic as well as peer/key work on these common fast paths.
    let previous = if has_previous != 0 && identity_changed == 0 {
        Some(dupewatch::SearchStamp { score: previous_score, time: previous_time })
    } else { None };
    let preliminary = dupewatch::decide(pick.flags, pick.score, None, now, previous, identity_changed != 0);
    if preliminary != Decision::Search || count == 0 || peers.is_null() { return preliminary as c_int; }
    // Keep the key scan out of the entry's register-save/prologue fast path.
    if dupe_watch_has_higher(pick, peers, count) { return Decision::Skip as c_int; }
    Decision::Search as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_budget_new(max_donors: u32) -> *mut crate::dupewatch::FetchBudget {
    Box::into_raw(Box::new(crate::dupewatch::FetchBudget::from_max_donors(max_donors as usize)))
}

/// # Safety
/// Budget is a live pointer returned by nzbget_rs_dupe_budget_new; callers have joined users before freeing.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_budget_free(budget: *mut crate::dupewatch::FetchBudget) {
    if !budget.is_null() { drop(Box::from_raw(budget)); }
}

/// # Safety
/// Budget is live; concurrent calls are allowed, but free must not run concurrently.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_budget_spent(budget: *const crate::dupewatch::FetchBudget) -> c_int {
    if budget.is_null() { return 1; }
    c_int::from((*budget).spent())
}

/// # Safety
/// The decoded body is readable for len, or NULL (empty).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_par_block(data: *const c_char, len: usize) -> u64 {
    crate::twin::block_size(arg(data,len))
}
/// # Safety
/// Names readable for their lengths, or NULL (empty).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_outer_ok(title: *const c_char, title_len: usize, name: *const c_char, len: usize) -> c_int {
    let title=arg(title,title_len);let name=arg(name,len);
    c_int::from(!crate::dupeidentity::conflicts(title,name) && !crate::dupeidentity::other_title(title,name)
        && (!crate::releasename::readable(name) || crate::dupeidentity::consistent(title,name)))
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
fn purge_owned_table_overhead<C, R>(count: usize, budget: &mut purge::Budget) -> Result<(), purge::PurgeError> {
    let extra = std::mem::size_of::<R>().saturating_sub(std::mem::size_of::<C>());
    budget.add(count.checked_mul(extra).ok_or(purge::PurgeError::LimitExceeded)?)
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
    purge_owned_table_overhead::<PurgeComparisonC, purge::AnchorComparison>(comparisons.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeDirectC, purge::DirectBinding>(direct.len(), &mut b)?;
    purge_owned_table_overhead::<PurgePairC, purge::PairMembershipReceipt>(pairs.len(), &mut b)?;
    purge_owned_table_overhead::<PurgePairCustodyC, purge::PairCustodyReceipt>(pair_custody.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeRootC, purge::RootRecord>(roots.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeNamespaceC, purge::NamespaceProof>(namespaces.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeEntryC, purge::InventoryEntry>(entries.len(), &mut b)?;
    purge_owned_table_overhead::<PurgeOwnershipC, purge::OwnershipReceipt>(ownership.len(), &mut b)?;
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

mod purge_subjects;
mod purge_inventory;

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
        let mut remaining = purge::MAX_PAYLOAD - members.len() * std::mem::size_of::<PurgeMemberC>();
        assert!(std::mem::size_of::<purge::MemberRecord>() > std::mem::size_of::<PurgeMemberC>());
        for member in &mut members {
            for span in [&mut member.binding.posting_source, &mut member.name, &mut member.key] {
                let count = remaining.min(bytes.len()); remaining -= count;
                *span = PurgeBytesC { data: bytes.as_ptr(), len: count as u32 };
            }
        }
        assert_eq!(remaining, 0);
        members.last_mut().unwrap().complete_inventory = 2;
        let mut snapshot: PurgeSnapshotC = unsafe { std::mem::zeroed() };
        snapshot.members = members.as_ptr(); snapshot.members_count = members.len() as u32;
        let (result, allocations) = crate::allocation_probe::measure(|| unsafe { purge_snapshot(&snapshot) });
        assert_eq!(result.err(), Some(purge::PurgeError::LimitExceeded), "PR108_F2_OWNED_WIDTH_PREFLIGHT");
        assert_eq!(allocations.calls, 0, "ABI preflight allocated before rejecting fixed owned widths");
    }

    fn exact_owned_width<T>() {
        let width = std::mem::size_of::<T>(); assert!(width > 1);
        let mut exact = purge::Budget::new();
        exact.add(purge::MAX_PAYLOAD - width).unwrap();
        exact.table::<T>(1).unwrap();
        assert_eq!(exact.add(1), Err(purge::PurgeError::LimitExceeded));
        let mut short = purge::Budget::new();
        short.add(purge::MAX_PAYLOAD - width + 1).unwrap();
        assert_eq!(short.table::<T>(1), Err(purge::PurgeError::LimitExceeded));
        let mut overflow = purge::Budget::new();
        assert_eq!(overflow.table::<T>(usize::MAX / width + 1), Err(purge::PurgeError::LimitExceeded));
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
        let roots = [PurgeRootC { id: 1, descriptor_identity: PurgeBytesC { data: identity.as_ptr(), len: 4 }, configured_container: 1 }];
        let mut snapshot: PurgeSnapshotC = unsafe { std::mem::zeroed() };
        snapshot.roots = roots.as_ptr(); snapshot.roots_count = 1;
        let owned = unsafe { purge_snapshot(&snapshot) }.unwrap();
        assert_eq!(owned.roots.len(), 1); assert_eq!(owned.roots[0].id, 1);
        assert_eq!(owned.roots[0].descriptor_identity, b"root"); assert!(owned.roots[0].configured_container);
        assert_ne!(owned.roots[0].descriptor_identity.as_ptr(), identity.as_ptr());
    }
}

// ============================================================================
// Daemon Lifecycle Coordination C ABI (Requirement R2, Milestone M5)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_register_lifecycle_hooks(hooks: *const crate::lifecycle::LifecycleHooks) {
    if let Some(hooks) = hooks.as_ref() {
        crate::lifecycle::register_hooks(*hooks);
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_signal_notify(signum: std::ffi::c_int) {
    crate::lifecycle::LifecycleManager::global().notify_signal(signum);
}

#[cfg(test)]
mod lifecycle_ffi_tests {
    use super::*;

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

        nzbget_rs_signal_notify(0); // no-op for signum 0
    }
}

// ============================================================================
// DupeArticleFallback, ParDamage, DupeProbe, ServerPool, ArticleFetcher &
// ArticleDownloader Hot-Path C ABI (rust/src/dupefallback.rs)
// ============================================================================


#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_steps_hash(
    sizes: *const c_int,
    count: usize,
) -> u64 {
    if sizes.is_null() || count == 0 {
        return 0;
    }
    let slice = std::slice::from_raw_parts(sizes, count);
    crate::dupefallback::article_size_steps_hash(slice)
}






#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_match_donor_file(
    target: *const crate::dupefallback::NzbgetRsDonorFileSpec,
    target_nzb_filename: *const c_char,
    by_content: *const c_char,
    twin_allowed: c_int,
    twin_only: c_int,
    donor_files: *const crate::dupefallback::NzbgetRsDonorFileSpec,
    donor_count: usize,
) -> isize {
    if target.is_null() || (donor_count > 0 && donor_files.is_null()) {
        return -1;
    }
    let to_spec = |raw: &crate::dupefallback::NzbgetRsDonorFileSpec| -> crate::dupefallback::DonorFileSpec<'_> {
        let filename = if raw.filename.is_null() {
            &[][..]
        } else {
            CStr::from_ptr(raw.filename).to_bytes()
        };
        let subject = if raw.subject.is_null() {
            &[][..]
        } else {
            CStr::from_ptr(raw.subject).to_bytes()
        };
        let parts = if raw.parts.is_null() || raw.article_count == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(raw.parts, raw.article_count)
        };
        let sizes = if raw.sizes.is_null() || raw.article_count == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(raw.sizes, raw.article_count)
        };
        crate::dupefallback::DonorFileSpec {
            filename,
            subject,
            total_articles: raw.total_articles,
            size: raw.size,
            is_par_file: raw.is_par_file != 0,
            parts,
            sizes,
        }
    };
    let target_spec = to_spec(&*target);
    let target_nzb_fn = if target_nzb_filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(target_nzb_filename).to_bytes()
    };
    let by_content_bytes = if by_content.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(by_content).to_bytes()
    };
    let raw_donors = if donor_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(donor_files, donor_count)
    };
    let donor_specs: Vec<crate::dupefallback::DonorFileSpec<'_>> =
        raw_donors.iter().map(to_spec).collect();
    crate::dupefallback::match_donor_file(
        &target_spec,
        target_nzb_fn,
        by_content_bytes,
        twin_allowed != 0,
        twin_only != 0,
        &donor_specs,
    )
    .map_or(-1, |idx| idx as isize)
}










// ============================================================================
// Post-Repair & DupeCoordinator Hot-Path FFI (`rust/src/postrepair.rs`)
// ============================================================================


#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupecoord_select_dupe_backup(
    has_self_nzb: c_int,
    self_full_content_hash: u32,
    dupe_health: c_int,
    history: *const crate::dupecoord::NzbgetRsDupeHistoryCandidate,
    queued_filenames: *const *const c_char,
    history_count: usize,
    queue: *const crate::dupecoord::NzbgetRsDupeQueueCandidate,
    queue_count: usize,
) -> c_int {
    let h = if history_count == 0 || history.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(history, history_count)
    };
    let q = if queue_count == 0 || queue.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(queue, queue_count)
    };
    crate::dupecoord::select_dupe_backup(
        has_self_nzb != 0,
        self_full_content_hash,
        dupe_health != 0,
        h,
        q,
        |idx| {
            if queued_filenames.is_null() || idx >= history_count {
                return false;
            }
            let ptr = *queued_filenames.add(idx);
            !ptr.is_null() && crate::fsops::file_exists(input(ptr), &crate::fsops::posix_stat)
        },
    )
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
    c_int::from(r.verify_extra_file(
        &p_path,
        is_split_fragment != 0,
        cbs_ref,
    ))
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
pub extern "C" fn nzbget_rs_streaming_par_has_session(nzb_id: c_int) -> c_int {
    c_int::from(crate::parchecker::streaming_session_has_session(nzb_id))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_streaming_par_start(
    nzb_id: c_int,
    par2_path: *const c_char,
    base_dir: *const c_char,
) -> c_int {
    if par2_path.is_null() || base_dir.is_null() {
        return 0;
    }
    let Some(p_str) = CStr::from_ptr(par2_path).to_str().ok() else {
        return 0;
    };
    let Some(d_str) = CStr::from_ptr(base_dir).to_str().ok() else {
        return 0;
    };
    c_int::from(
        crate::parchecker::streaming_session_start(
            nzb_id,
            std::path::Path::new(p_str),
            std::path::Path::new(d_str),
            crate::parchecker::SessionOptions::default(),
        )
        .is_ok(),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_streaming_par_register_queued_volume(
    nzb_id: c_int,
    volume_id: u64,
    blocks: c_int,
    paused: c_int,
) -> c_int {
    c_int::from(crate::parchecker::streaming_session_register_queued_volume(
        nzb_id,
        volume_id,
        blocks,
        paused != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_streaming_par_on_article_finished_by_name(
    nzb_id: c_int,
    name: *const c_char,
    offset: u64,
    size: u64,
    crc32: u32,
    bytes: *const u8,
    bytes_len: usize,
) -> c_int {
    if name.is_null() {
        return 0;
    }
    let name_bytes = CStr::from_ptr(name).to_bytes();
    let payload = if bytes.is_null() || bytes_len == 0 {
        None
    } else {
        Some(std::slice::from_raw_parts(bytes, bytes_len))
    };
    c_int::from(
        crate::parchecker::streaming_session_on_article_finished_by_name(
            nzb_id, name_bytes, offset, size, crc32, payload,
        )
        .is_some_and(|r| r.is_ok()),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_streaming_par_on_article_failed_permanently_by_name(
    nzb_id: c_int,
    name: *const c_char,
    offset: u64,
    size: u64,
) -> c_int {
    if name.is_null() {
        return 0;
    }
    let name_bytes = CStr::from_ptr(name).to_bytes();
    c_int::from(
        crate::parchecker::streaming_session_on_article_failed_permanently_by_name(
            nzb_id, name_bytes, offset, size,
        )
        .is_some_and(|r| r.is_ok()),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_streaming_par_on_recovery_volume_arrived(
    nzb_id: c_int,
    path: *const c_char,
    queued_id: u64,
    has_queued_id: c_int,
) -> c_int {
    if path.is_null() {
        return 0;
    }
    let Some(p_str) = CStr::from_ptr(path).to_str().ok() else {
        return 0;
    };
    let qid = if has_queued_id != 0 {
        Some(queued_id)
    } else {
        None
    };
    c_int::from(
        crate::parchecker::streaming_session_on_recovery_volume_arrived(
            nzb_id,
            std::path::Path::new(p_str),
            qid,
        )
        .is_some_and(|r| r.is_ok()),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_streaming_par_take_unpause_ids(
    nzb_id: c_int,
    out_ids: *mut u64,
    max_ids: usize,
) -> usize {
    if out_ids.is_null() || max_ids == 0 {
        return 0;
    }
    let ids = crate::parchecker::streaming_session_take_unpause_ids_bounded(nzb_id, max_ids);
    let n = ids.len();
    if n > 0 {
        std::ptr::copy_nonoverlapping(ids.as_ptr(), out_ids, n);
    }
    n
}

#[no_mangle]
pub extern "C" fn nzbget_rs_streaming_par_forget(nzb_id: c_int) {
    crate::parchecker::streaming_session_forget(nzb_id);
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

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_generator_execute(
    data_dir: *const c_char,
    segment_size: c_int,
    log_cb: Option<unsafe extern "C" fn(c_int, *const c_char)>,
) {
    if data_dir.is_null() {
        return;
    }
    let path = path_arg(data_dir);
    let dir_bytes = input(data_dir);
    crate::nserv::execute_nzb_generator(&path, dir_bytes, segment_size, |kind, msg| {
        if let Some(cb) = log_cb {
            let mut c_msg = Vec::with_capacity(msg.len() + 1);
            c_msg.extend_from_slice(msg);
            c_msg.push(0);
            cb(kind as c_int, c_msg.as_ptr().cast());
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nntp_cache_new() -> *mut crate::nserv::NntpCache {
    Box::into_raw(Box::new(crate::nserv::NntpCache::new()))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nntp_cache_free(cache: *mut crate::nserv::NntpCache) {
    if !cache.is_null() {
        drop(Box::from_raw(cache));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nntp_processor_handle_line(
    server_id: c_int,
    data_dir: *const c_char,
    cache_dir: *const c_char,
    latency: c_int,
    speed: c_int,
    cache: *const crate::nserv::NntpCache,
    raw_line: *const c_char,
    ctx: *mut std::ffi::c_void,
    write_line_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
    send_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, c_int)>,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
) -> c_int {
    let data_dir_opt = (!data_dir.is_null()).then(|| input(data_dir)).filter(|b| !b.is_empty());
    let cache_dir_opt = (!cache_dir.is_null()).then(|| input(cache_dir)).filter(|b| !b.is_empty());
    let cache_ref = (!cache.is_null()).then(|| &*cache);
    let line_bytes = if raw_line.is_null() { &[][..] } else { input(raw_line) };

    let mut write_line_fn = |line: &[u8]| {
        if let Some(cb) = write_line_cb {
            let mut c_str = Vec::with_capacity(line.len() + 1);
            c_str.extend_from_slice(line);
            c_str.push(0);
            cb(ctx, c_str.as_ptr().cast());
        }
    };
    let mut send_fn = |buf: &[u8]| {
        if let Some(cb) = send_cb {
            cb(ctx, buf.as_ptr().cast(), buf.len() as c_int);
        }
    };
    let mut log_fn = |kind: i32, msg: &[u8]| {
        if let Some(cb) = log_cb {
            let mut c_str = Vec::with_capacity(msg.len() + 1);
            c_str.extend_from_slice(msg);
            c_str.push(0);
            cb(ctx, kind as c_int, c_str.as_ptr().cast());
        }
    };

    i32::from(crate::nserv::nntp_processor_handle_line(
        server_id,
        data_dir_opt,
        cache_dir_opt,
        latency,
        speed,
        cache_ref,
        line_bytes,
        &mut write_line_fn,
        &mut send_fn,
        &mut log_fn,
    ))
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nserv_parse_args(
    argc: c_int,
    argv: *const *const c_char,
    ctx: *mut std::ffi::c_void,
    set_fields_cb: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const c_char,
            *const c_char,
            *const c_char,
            c_int,
            c_int,
            *const c_char,
            *const c_char,
            *const c_char,
            c_int,
            c_int,
            c_int,
            c_int,
            c_int,
            c_int,
            c_int,
        ),
    >,
    push_opt_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) {
    let mut arg_slices: Vec<&[u8]> = Vec::new();
    if argc > 0 && !argv.is_null() {
        for i in 0..argc as usize {
            let p = *argv.add(i);
            arg_slices.push(if p.is_null() { b"" } else { input(p) });
        }
    }
    let mut parsed = crate::nserv::nserv_parse_args(&arg_slices);
    if let Some(cb) = set_fields_cb {
        let to_opt_ptr = |v: &mut Vec<u8>| -> *const c_char {
            if v.is_empty() {
                std::ptr::null()
            } else {
                v.push(0);
                v.as_ptr().cast()
            }
        };
        let data_ptr = to_opt_ptr(&mut parsed.data_dir);
        let cache_ptr = to_opt_ptr(&mut parsed.cache_dir);
        parsed.bind_address.push(0);
        let bind_ptr: *const c_char = parsed.bind_address.as_ptr().cast();
        let log_ptr = to_opt_ptr(&mut parsed.log_file);
        let cert_ptr = to_opt_ptr(&mut parsed.secure_cert);
        let key_ptr = to_opt_ptr(&mut parsed.secure_key);
        cb(
            ctx,
            data_ptr,
            cache_ptr,
            bind_ptr,
            parsed.first_port,
            parsed.instances,
            log_ptr,
            cert_ptr,
            key_ptr,
            c_int::from(parsed.generate_nzb),
            parsed.segment_size,
            c_int::from(parsed.quit),
            parsed.latency,
            parsed.speed,
            c_int::from(parsed.mem_cache),
            c_int::from(parsed.param_error),
        );
    }
    if let Some(cb) = push_opt_cb {
        for mut opt in parsed.cmd_opts {
            opt.push(0);
            cb(ctx, opt.as_ptr().cast());
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_network_speed_test_run(
    ctx: *mut std::ffi::c_void,
    execute_iter_cb: Option<
        unsafe extern "C" fn(
            ctx: *mut std::ffi::c_void,
            req_ptr: *const c_char,
            req_len: usize,
            timeout_sec: i64,
            out_downloaded: *mut u32,
            out_elapsed: *mut f64,
        ) -> c_int,
    >,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
    out_speed_mbps: *mut f64,
    out_err_msg: *mut RsBuf,
) -> c_int {
    if !out_err_msg.is_null() {
        *out_err_msg = RsBuf {
            data: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        };
    }
    if !out_speed_mbps.is_null() {
        *out_speed_mbps = 0.0;
    }
    let Some(iter_fn) = execute_iter_cb else {
        return 0;
    };
    let res = crate::webdownload::run_network_speed_test(
        |req, timeout_sec| {
            let mut downloaded: u32 = 0;
            let mut elapsed: f64 = 0.0;
            iter_fn(
                ctx,
                req.as_ptr().cast(),
                req.len(),
                timeout_sec,
                &mut downloaded,
                &mut elapsed,
            );
            (downloaded, elapsed)
        },
        |kind, msg| {
            if let Some(cb) = log_cb {
                let c_msg = nul(msg.as_bytes());
                cb(ctx, kind, c_msg.as_ptr().cast());
            }
        },
    );
    match res {
        Ok(speed) => {
            if !out_speed_mbps.is_null() {
                *out_speed_mbps = speed;
            }
            1
        }
        Err(err) => {
            if !out_err_msg.is_null() {
                *out_err_msg = into_buf(err.as_bytes().to_vec());
            }
            0
        }
    }
}

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
        unsafe extern "C" fn(ctx: *mut std::ffi::c_void, path: *const c_char, read_write: c_int) -> c_int,
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
            crate::fsops::force_directories(path_bytes, &mut force_dirs_err, &crate::fsops::posix_stat)
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
                let _ = libc::mkdir(c_path.as_ptr(), libc::S_IRWXU | libc::S_IRWXG | libc::S_IRWXO);
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
    cb: Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void, err_msg_or_null: *const c_char) -> c_int>,
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
pub unsafe extern "C" fn nzbget_rs_binrpc_status_summary(
    spec: *const crate::binrpc::StatusSummarySpec,
) -> RsBuf {
    if spec.is_null() {
        return into_buf(Vec::new());
    }
    into_buf(crate::binrpc::status_summary(&*spec))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ncurses_format_status_bar(
    spec: *const crate::ncurses_format::NcursesStatusSpec,
) -> RsBuf {
    if spec.is_null() {
        return into_buf(Vec::new());
    }
    into_buf(crate::ncurses_format::format_status_bar(&*spec))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ncurses_format_top_header(
    header: *const c_char,
    screen_width: c_int,
    up_time: c_int,
    time_sec: c_int,
) -> RsBuf {
    let hdr = if header.is_null() {
        b""
    } else {
        CStr::from_ptr(header).to_bytes()
    };
    into_buf(crate::ncurses_format::format_top_header(
        hdr,
        screen_width,
        up_time != 0,
        time_sec,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ncurses_format_file_queue_header(
    use_color: c_int,
    file_num: c_int,
    paused_files: c_int,
    remaining: i64,
    paused: i64,
) -> RsBuf {
    into_buf(crate::ncurses_format::format_file_queue_header(
        use_color != 0,
        file_num,
        paused_files,
        remaining,
        paused,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ncurses_format_group_queue_header(
    use_color: c_int,
    group_count: c_int,
    remaining: i64,
    paused: i64,
) -> RsBuf {
    into_buf(crate::ncurses_format::format_group_queue_header(
        use_color != 0,
        group_count,
        remaining,
        paused,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ncurses_format_file_row(
    spec: *const crate::ncurses_format::NcursesFileRowSpec,
    out_color: *mut c_int,
) -> RsBuf {
    if spec.is_null() {
        if !out_color.is_null() {
            *out_color = 0;
        }
        return into_buf(Vec::new());
    }
    let (row, color) = crate::ncurses_format::format_file_row(&*spec);
    if !out_color.is_null() {
        *out_color = color;
    }
    into_buf(row)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ncurses_format_group_row(
    spec: *const crate::ncurses_format::NcursesGroupRowSpec,
    inout_col_files: *mut c_int,
    inout_col_total: *mut c_int,
    inout_col_left: *mut c_int,
    out_color: *mut c_int,
) -> RsBuf {
    if spec.is_null() {
        if !out_color.is_null() {
            *out_color = 0;
        }
        return into_buf(Vec::new());
    }
    let mut wf = if inout_col_files.is_null() {
        0
    } else {
        *inout_col_files
    };
    let mut wt = if inout_col_total.is_null() {
        0
    } else {
        *inout_col_total
    };
    let mut wl = if inout_col_left.is_null() {
        0
    } else {
        *inout_col_left
    };
    let (row, color) = crate::ncurses_format::format_group_row(&*spec, &mut wf, &mut wt, &mut wl);
    if !inout_col_files.is_null() {
        *inout_col_files = wf;
    }
    if !inout_col_total.is_null() {
        *inout_col_total = wt;
    }
    if !inout_col_left.is_null() {
        *inout_col_left = wl;
    }
    if !out_color.is_null() {
        *out_color = color;
    }
    into_buf(row)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_colored_frontend_format_status_bar(
    stand_by: c_int,
    pause_download: c_int,
    current_download_speed: i64,
    remaining_size: i64,
    download_limit: i64,
    post_job_count: c_int,
    thread_count: c_int,
    control_seq: *const c_char,
) -> RsBuf {
    into_buf(crate::ncurses_format::format_colored_status_bar(
        stand_by,
        pause_download,
        current_download_speed,
        remaining_size,
        download_limit,
        post_job_count,
        thread_count,
        input(control_seq),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_frontend_format_log_message(
    colored: c_int,
    kind: c_int,
    text: *const c_char,
) -> RsBuf {
    into_buf(crate::ncurses_format::format_log_message_line(
        colored != 0,
        kind,
        input(text),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nserv_format_usage(base_name: *const c_char) -> RsBuf {
    into_buf(crate::cmdline::format_nserv_usage(input(base_name)))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_decode_log_response_entries(
    data: *const u8,
    len: usize,
    nr_entries: u32,
    ctx: *mut std::ffi::c_void,
    emit_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, u32, u32, u32, *const c_char)>,
) {
    if data.is_null() || len == 0 {
        return;
    }
    let Some(cb) = emit_cb else {
        return;
    };
    let slice = std::slice::from_raw_parts(data, len);
    crate::binrpc::decode_log_response_entries(slice, nr_entries, |id, kind, time, text_ptr| {
        cb(ctx, id, kind, time, text_ptr.cast());
    });
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_encode_edit_queue_trailing(
    text: *const c_char,
    ids: *const i32,
    ids_len: usize,
    names: *const *const c_char,
    names_len: usize,
    out_text_len: *mut c_int,
    out_names_len: *mut c_int,
) -> RsBuf {
    let text_opt = if text.is_null() {
        None
    } else {
        Some(input(text))
    };
    let ids_slice = if ids.is_null() || ids_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ids, ids_len)
    };
    let mut name_slices = Vec::with_capacity(if names.is_null() { 0 } else { names_len });
    if !names.is_null() {
        for i in 0..names_len {
            name_slices.push(input(*names.add(i)));
        }
    }
    let (buf, tlen, nlen) =
        crate::binrpc::encode_edit_queue_trailing(text_opt, ids_slice, &name_slices);
    if !out_text_len.is_null() {
        *out_text_len = tlen;
    }
    if !out_names_len.is_null() {
        *out_names_len = nlen;
    }
    into_buf(buf)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_file_listing_from_wire(
    data: *const c_char,
    len: usize,
    nr_nzb: u32,
    nr_ppp: u32,
    nr_files: u32,
    has_pattern: c_int,
) -> RsBuf {
    let slice = if data.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(data.cast::<u8>(), len)
    };
    into_buf(crate::binrpc::file_listing_from_wire(
        slice,
        nr_nzb,
        nr_ppp,
        nr_files,
        has_pattern != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_group_listing_from_wire(
    data: *const c_char,
    len: usize,
    nr_nzb: u32,
    nr_ppp: u32,
    nr_files: u32,
    has_pattern: c_int,
) -> RsBuf {
    let slice = if data.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(data.cast::<u8>(), len)
    };
    into_buf(crate::binrpc::group_listing_from_wire(
        slice,
        nr_nzb,
        nr_ppp,
        nr_files,
        has_pattern != 0,
    ))
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_borrowed_par2_mismatches(
    path: *const c_char,
    dest_dir: *const c_char,
    filename: *const c_char,
    statuses: *const i32,
    donor_ids: *const i32,
    fallback_rounds: *const i32,
    segment_offsets: *const i64,
    segment_sizes: *const i32,
    article_count: usize,
    out_indices: *mut i32,
    out_cap: usize,
) -> usize {
    if article_count == 0
        || statuses.is_null()
        || donor_ids.is_null()
        || fallback_rounds.is_null()
        || segment_offsets.is_null()
        || segment_sizes.is_null()
        || out_indices.is_null()
        || out_cap == 0
    {
        return 0;
    }
    let path_bytes = input(path);
    let dest_bytes = input(dest_dir);
    if path_bytes.is_empty() || dest_bytes.is_empty() {
        return 0;
    }
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let mismatches = crate::dupefallback::borrowed_par2_mismatches_with_crc(
        std::path::Path::new(OsStr::from_bytes(path_bytes)),
        std::path::Path::new(OsStr::from_bytes(dest_bytes)),
        input(filename),
        std::slice::from_raw_parts(statuses, article_count),
        std::slice::from_raw_parts(donor_ids, article_count),
        std::slice::from_raw_parts(fallback_rounds, article_count),
        std::slice::from_raw_parts(segment_offsets, article_count),
        std::slice::from_raw_parts(segment_sizes, article_count),
        crate::dupefallback::crc32_ieee,
    );
    let n = mismatches.len().min(out_cap);
    std::ptr::copy_nonoverlapping(mismatches.as_ptr(), out_indices, n);
    n
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nzbget_rs_df_build_newznab_queries(
    title_ptr: *const c_char,
    title_len: usize,
    imdb_ptr: *const c_char,
    imdb_len: usize,
    tvdb_ptr: *const c_char,
    tvdb_len: usize,
    ctx: *mut std::ffi::c_void,
    visit: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const *const c_char,
            *const usize,
            *const *const c_char,
            *const usize,
            usize,
        ),
    >,
) {
    let Some(cb) = visit else { return };
    let title = if title_ptr.is_null() || title_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(title_ptr.cast::<u8>(), title_len)
    };
    let imdb = if imdb_ptr.is_null() || imdb_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(imdb_ptr.cast::<u8>(), imdb_len)
    };
    let tvdb = if tvdb_ptr.is_null() || tvdb_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(tvdb_ptr.cast::<u8>(), tvdb_len)
    };
    for query in crate::dupesearch_nzb::build_newznab_queries(title, imdb, tvdb) {
        let mut key_ptrs = Vec::with_capacity(query.len());
        let mut key_lens = Vec::with_capacity(query.len());
        let mut val_ptrs = Vec::with_capacity(query.len());
        let mut val_lens = Vec::with_capacity(query.len());
        for (k, v) in &query {
            key_ptrs.push(k.as_ptr().cast::<c_char>());
            key_lens.push(k.len());
            val_ptrs.push(v.as_ptr().cast::<c_char>());
            val_lens.push(v.len());
        }
        cb(
            ctx,
            key_ptrs.as_ptr(),
            key_lens.as_ptr(),
            val_ptrs.as_ptr(),
            val_lens.as_ptr(),
            query.len(),
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_visit_par2_sigs(
    data: *const u8,
    len: usize,
    ctx: *mut std::ffi::c_void,
    visit: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            u64,
            *const c_char,
            usize,
            *const c_char,
            usize,
        ),
    >,
) {
    let Some(cb) = visit else { return };
    for s in crate::twin::par2_sigs(bytes_arg(data, len)) {
        let name: Vec<u8> = s
            .name
            .bytes()
            .map(|b| if matches!(b, b'\n' | b'\t' | b'\r') { b'_' } else { b })
            .collect();
        cb(
            ctx,
            s.length,
            s.md5.as_ptr().cast::<c_char>(),
            s.md5.len(),
            name.as_ptr().cast::<c_char>(),
            name.len(),
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_visit_nzb_entries(
    path: *const c_char,
    ctx: *mut std::ffi::c_void,
    on_entry: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            i64,
            i32,
            *const c_char,
            usize,
            *const c_char,
            usize,
        ),
    >,
    on_group: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, usize)>,
    on_segment: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64, *const c_char, usize)>,
) {
    let xml = std::fs::read(path_arg(path)).unwrap_or_default();
    let clean = |s: &str| s.replace(['\n', '\t', '\r'], "_");
    for e in crate::twin::read_nzb(&xml) {
        let fname = clean(&e.filename);
        let subj = clean(&e.subject);
        if let Some(cb) = on_entry {
            cb(
                ctx,
                e.size() as i64,
                i32::from(e.is_par2()),
                fname.as_ptr().cast::<c_char>(),
                fname.len(),
                subj.as_ptr().cast::<c_char>(),
                subj.len(),
            );
        }
        if let Some(cb) = on_group {
            for g in &e.groups {
                let g_clean = clean(g);
                cb(ctx, g_clean.as_ptr().cast::<c_char>(), g_clean.len());
            }
        }
        if let Some(cb) = on_segment {
            for (bytes, id) in &e.segments {
                let id_clean = clean(id);
                cb(ctx, *bytes as i64, id_clean.as_ptr().cast::<c_char>(), id_clean.len());
            }
        }
    }
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nzbget_rs_stream_build_donor_key(
    password: *const c_char,
    file_names_ptr: *const *const c_char,
    file_sizes_ptr: *const i64,
    file_is_par_ptr: *const u8,
    file_group_counts_ptr: *const usize,
    file_count: usize,
    groups_ptr: *const *const c_char,
    total_groups: usize,
    file_article_counts_ptr: *const usize,
    art_part_numbers_ptr: *const i32,
    art_sizes_ptr: *const i32,
    art_message_ids_ptr: *const *const c_char,
    total_articles: usize,
) -> RsBuf {
    let pwd = input(password);
    let mut file_names = Vec::with_capacity(file_count);
    if !file_names_ptr.is_null() {
        for i in 0..file_count {
            file_names.push(input(*file_names_ptr.add(i)));
        }
    }
    let file_sizes = if file_sizes_ptr.is_null() || file_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(file_sizes_ptr, file_count)
    };
    let file_is_par = if file_is_par_ptr.is_null() || file_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(file_is_par_ptr, file_count)
    };
    let file_group_counts = if file_group_counts_ptr.is_null() || file_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(file_group_counts_ptr, file_count)
    };
    let mut groups = Vec::with_capacity(total_groups);
    if !groups_ptr.is_null() {
        for i in 0..total_groups {
            groups.push(input(*groups_ptr.add(i)));
        }
    }
    let file_article_counts = if file_article_counts_ptr.is_null() || file_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(file_article_counts_ptr, file_count)
    };
    let art_part_numbers = if art_part_numbers_ptr.is_null() || total_articles == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(art_part_numbers_ptr, total_articles)
    };
    let art_sizes = if art_sizes_ptr.is_null() || total_articles == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(art_sizes_ptr, total_articles)
    };
    let mut art_message_ids = Vec::with_capacity(total_articles);
    if !art_message_ids_ptr.is_null() {
        for i in 0..total_articles {
            art_message_ids.push(input(*art_message_ids_ptr.add(i)));
        }
    }
    into_buf(crate::streamrepair::build_donor_key(
        pwd,
        &file_names,
        file_sizes,
        file_is_par,
        file_group_counts,
        &groups,
        file_article_counts,
        art_part_numbers,
        art_sizes,
        &art_message_ids,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_compare_par2_data(
    own_names_ptr: *const *const c_char,
    own_md5s_ptr: *const *const c_char,
    own_count: usize,
    own_print_ptr: *const c_char,
    twin_data_ptr: *const u8,
    twin_data_len: usize,
) -> i32 {
    let mut own_names = Vec::with_capacity(own_count);
    let mut own_md5s = Vec::with_capacity(own_count);
    if !own_names_ptr.is_null() && !own_md5s_ptr.is_null() {
        for i in 0..own_count {
            own_names.push(input(*own_names_ptr.add(i)));
            own_md5s.push(input(*own_md5s_ptr.add(i)));
        }
    }
    let own_print = input(own_print_ptr);
    let twin_data = if twin_data_ptr.is_null() || twin_data_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(twin_data_ptr, twin_data_len)
    };
    crate::twin::compare_twin_par2_data(&own_names, &own_md5s, own_print, twin_data)
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
    i32::from(crate::options::load_config_file_lines(c_path, |line_no, line| {
        if let Some(cb) = on_line_cb {
            cb(ctx, line_no, line.as_ptr());
        }
    }))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_util_write_atomic(
    path: *const c_char,
    path_len: usize,
    data: *const c_char,
    data_len: usize,
) -> c_int {
    let p = if path.is_null() || path_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(path as *const u8, path_len)
    };
    let d = if data.is_null() || data_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(data as *const u8, data_len)
    };
    c_int::from(crate::dupesearch_nzb::dupe_write_atomic_bytes(p, d))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_util_read_all(
    path: *const c_char,
    path_len: usize,
) -> RsBuf {
    let p = if path.is_null() || path_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(path as *const u8, path_len)
    };
    into_buf(crate::dupesearch_nzb::dupe_read_all_bytes(p))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dqh_dupesearch_visit_expand_queries(
    name: *const c_char,
    name_len: usize,
    ctx: *mut std::ffi::c_void,
    cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, usize)>,
) {
    let Some(cb) = cb else {
        return;
    };
    let n = if name.is_null() || name_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(name as *const u8, name_len)
    };
    crate::dupesearch_nzb::dupesearch_visit_expand_queries(n, |q| {
        cb(ctx, q.as_ptr() as *const c_char, q.len());
    });
}

#[repr(C)]
pub struct NzbgetRsParDamageQueuedFile {
    pub filename: *const c_char,
    pub confirmed: c_int,
    pub size: i64,
}

#[repr(C)]
pub struct NzbgetRsParDamageCompletedFile {
    pub filename: *const c_char,
    pub not_failed: c_int,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_damage_file_completed(
    nzb_id: c_int,
    is_par: c_int,
    failed_plus_missed: c_int,
    decoded_size: i64,
    success_articles: c_int,
    file_size: i64,
    filename: *const c_char,
    holes: *const crate::dupefallback::NzbgetRsByteRange,
    holes_count: usize,
) {
    let fname = if filename.is_null() {
        b""
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    let holes_slice = if holes.is_null() || holes_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(holes, holes_count)
    };
    crate::pardamage::file_completed(
        nzb_id,
        is_par != 0,
        failed_plus_missed,
        decoded_size,
        success_articles,
        file_size,
        fname,
        holes_slice,
    );
}

#[no_mangle]
pub extern "C" fn nzbget_rs_par_damage_has_damage(nzb_id: c_int) -> c_int {
    i32::from(crate::pardamage::has_damage(nzb_id))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_damage_judge(
    nzb_id: c_int,
    dest_dir: *const c_char,
    nzb_size: i64,
    nzb_par_size: i64,
    queued: *const NzbgetRsParDamageQueuedFile,
    queued_count: usize,
    completed: *const NzbgetRsParDamageCompletedFile,
    completed_count: usize,
    out_verdict: *mut crate::pardamage::ParDamageVerdictC,
) {
    if out_verdict.is_null() {
        return;
    }
    let dir_bytes = if dest_dir.is_null() {
        b""
    } else {
        CStr::from_ptr(dest_dir).to_bytes()
    };
    #[cfg(unix)]
    let dir_path = {
        use std::os::unix::ffi::OsStrExt;
        std::path::Path::new(std::ffi::OsStr::from_bytes(dir_bytes))
    };
    #[cfg(not(unix))]
    let dir_path = std::path::Path::new(std::str::from_utf8(dir_bytes).unwrap_or(""));

    let mut q_vec = Vec::with_capacity(queued_count);
    if !queued.is_null() && queued_count > 0 {
        for q in std::slice::from_raw_parts(queued, queued_count) {
            let fname = if q.filename.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(q.filename).to_bytes()
            };
            q_vec.push(crate::pardamage::QueuedParCandidate {
                filename: fname,
                confirmed: q.confirmed != 0,
                size: q.size,
            });
        }
    }

    let mut c_vec = Vec::with_capacity(completed_count);
    if !completed.is_null() && completed_count > 0 {
        for c in std::slice::from_raw_parts(completed, completed_count) {
            let fname = if c.filename.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(c.filename).to_bytes()
            };
            c_vec.push(crate::pardamage::CompletedParCandidate {
                filename: fname,
                not_failed: c.not_failed != 0,
            });
        }
    }

    *out_verdict = crate::pardamage::judge(
        nzb_id,
        dir_path,
        nzb_size,
        nzb_par_size,
        &q_vec,
        &c_vec,
    );
}

#[no_mangle]
pub extern "C" fn nzbget_rs_par_damage_first_report(nzb_id: c_int) -> c_int {
    i32::from(crate::pardamage::first_report(nzb_id))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_par_damage_forget(nzb_id: c_int) {
    crate::pardamage::forget(nzb_id);
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
    let dest_path = crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(dest_dir));
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
    let dst_path = crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(dest_dir));
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
    let dst_path = crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(dest_dir));
    let fin_path = crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(final_dir));
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
    let dst_path = crate::cleanup::bytes_to_path_buf(crate::diskstate_format::cstr_to_bytes(dst_dir));
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
    let (backup, err) =
        crate::extension::ext_mgr_prepare_update(en, found != 0, use_count, tp, ap);
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

#[repr(C)]
pub struct NzbgetRsQueueScriptEnv {
    pub nzb_name: *const c_char,
    pub nzb_id: c_int,
    pub filename: *const c_char,
    pub dest_dir: *const c_char,
    pub queued_filename: *const c_char,
    pub url: *const c_char,
    pub category: *const c_char,
    pub priority: c_int,
    pub dupe_key: *const c_char,
    pub dupe_score: c_int,
    pub dupe_mode: c_int,
    pub event: c_int,
    pub delete_status: c_int,
    pub url_status: c_int,
    pub mark_status: c_int,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_queue_script_prepare_env(
    env: *const NzbgetRsQueueScriptEnv,
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
    let inp = crate::extension::QueueScriptEnvInput {
        nzb_name: cstr_or_empty(e.nzb_name),
        nzb_id: e.nzb_id,
        filename: cstr_or_empty(e.filename),
        dest_dir: cstr_or_empty(e.dest_dir),
        queued_filename: cstr_or_empty(e.queued_filename),
        url: cstr_or_empty(e.url),
        category: cstr_or_empty(e.category),
        priority: e.priority,
        dupe_key: cstr_or_empty(e.dupe_key),
        dupe_score: e.dupe_score,
        dupe_mode: e.dupe_mode,
        event: e.event,
        delete_status: e.delete_status,
        url_status: e.url_status,
        mark_status: e.mark_status,
    };
    crate::extension::queue_script_prepare_env(
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















#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupecoord_files_on_disk(
    final_dir: *const c_char,
    dest_dir: *const c_char,
    ext_cleanup_disk: *const c_char,
) -> c_int {
    let f = if final_dir.is_null() { b"" } else { CStr::from_ptr(final_dir).to_bytes() };
    let d = if dest_dir.is_null() { b"" } else { CStr::from_ptr(dest_dir).to_bytes() };
    let e = if ext_cleanup_disk.is_null() { b"" } else { CStr::from_ptr(ext_cleanup_disk).to_bytes() };
    crate::dupecoord::files_on_disk(f, d, e) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupecoord_no_backup_reason(
    fails_over_flag: c_int,
    dupe_mode: c_int,
    history: *const crate::dupecoord::NzbgetRsDupeHistoryCandidate,
    queued_filenames: *const *const c_char,
    history_len: usize,
) -> RsBuf {
    let h = if history_len == 0 || history.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(history, history_len)
    };
    into_buf(crate::dupecoord::no_backup_reason(
        fails_over_flag != 0,
        dupe_mode,
        h,
        |idx| {
            if queued_filenames.is_null() || idx >= history_len {
                return false;
            }
            let ptr = *queued_filenames.add(idx);
            !ptr.is_null() && crate::fsops::file_exists(input(ptr), &crate::fsops::posix_stat)
        },
    ))
}



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


#[allow(clippy::too_many_arguments)]

// =========================================================================
// Consolidated RepairController (`repair.rs`), ContentMap (`streamrepair.rs`),
// and TwinCheck (`twin.rs`) FFI
// =========================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_sample_indices(
    primary_seg_counts: *const usize,
    primary_seg_bytes: *const i64,
    primary_seg_bytes_len: usize,
    primary_files: usize,
    dupe_seg_counts: *const usize,
    dupe_seg_bytes: *const i64,
    dupe_seg_bytes_len: usize,
    dupe_files: usize,
    sample_count: usize,
    out_file_indices: *mut usize,
    out_seg_indices: *mut usize,
) -> c_int {
    if primary_files == 0
        || dupe_files == 0
        || sample_count == 0
        || primary_seg_counts.is_null()
        || dupe_seg_counts.is_null()
        || out_file_indices.is_null()
        || out_seg_indices.is_null()
    {
        return 0;
    }
    let p_counts = std::slice::from_raw_parts(primary_seg_counts, primary_files);
    let d_counts = std::slice::from_raw_parts(dupe_seg_counts, dupe_files);
    let p_bytes = if primary_seg_bytes.is_null() || primary_seg_bytes_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(primary_seg_bytes, primary_seg_bytes_len)
    };
    let d_bytes = if dupe_seg_bytes.is_null() || dupe_seg_bytes_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(dupe_seg_bytes, dupe_seg_bytes_len)
    };
    if let Some(indices) =
        crate::twin::sample_indices(p_counts, p_bytes, d_counts, d_bytes, sample_count)
    {
        for (i, (f_idx, s_idx)) in indices.into_iter().enumerate() {
            *out_file_indices.add(i) = f_idx;
            *out_seg_indices.add(i) = s_idx;
        }
        1
    } else {
        0
    }
}

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

fn cstring_lossy(bytes: &[u8]) -> std::ffi::CString {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::ffi::CString::new(&bytes[..end]).unwrap_or_default()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupematcher_filter_dupe_sources(
    nzb_name: *const c_char,
    dest_dir: *const c_char,
    expected_size: i64,
    unrar_cmd: *const c_char,
    dupe_dirs: *const *const c_char,
    dupe_count: usize,
    out_indices: *mut usize,
    on_message: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>,
    ctx: *mut c_void,
) -> usize {
    if dest_dir.is_null() || dupe_dirs.is_null() || out_indices.is_null() || dupe_count == 0 {
        return 0;
    }
    let dir_path = cstr_to_path_buf(dest_dir);
    let nzb_name_bytes = input(nzb_name);
    let unrar_bytes = input(unrar_cmd);
    let mut dirs_vec = Vec::with_capacity(dupe_count);
    for &ptr in std::slice::from_raw_parts(dupe_dirs, dupe_count) {
        dirs_vec.push(input(ptr));
    }
    let usable = crate::pardamage::dupe_matcher_filter_dupe_sources(
        nzb_name_bytes,
        &dir_path,
        expected_size,
        unrar_bytes,
        &dirs_vec,
        |kind, msg| {
            if let Some(log_cb) = on_message {
                let c_msg = cstring_lossy(msg);
                log_cb(ctx, kind, c_msg.as_ptr());
            }
        },
    );
    let n = usable.len().min(dupe_count);
    if n > 0 {
        std::ptr::copy_nonoverlapping(usable.as_ptr(), out_indices, n);
    }
    n
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_archive_processor_process(
    archive_file: *const c_char,
    unpack_dir: *const c_char,
    dest_dir: *const c_char,
    processed_dir: *const c_char,
    broken_dir: *const c_char,
    config_action: c_int,
    ctx: *mut c_void,
    log_cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>,
    moved_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
) -> c_int {
    let archive_path = cstr_to_path_buf(archive_file);
    let unpack_path = cstr_to_path_buf(unpack_dir);
    let dest_path = cstr_to_path_buf(dest_dir);
    let processed_path = cstr_to_path_buf(processed_dir);
    let broken_path = cstr_to_path_buf(broken_dir);

    let ok = crate::cleanup::archive_processor_process(
        &archive_path,
        &unpack_path,
        &dest_path,
        &processed_path,
        &broken_path,
        config_action,
        |arc_path, _| {
            if let Some(cb) = log_cb {
                let archive_name = arc_path
                    .file_name()
                    .map(crate::cleanup::os_str_to_bytes)
                    .unwrap_or(b"");
                let ext = arc_path
                    .extension()
                    .map(crate::cleanup::os_str_to_bytes)
                    .unwrap_or(b"");
                let mut msg = b"Unpack: Could not process '".to_vec();
                msg.extend_from_slice(archive_name);
                msg.extend_from_slice(b"': Unsupported archive format: ");
                if !ext.is_empty() {
                    msg.push(b'.');
                    msg.extend_from_slice(ext);
                }
                let c_msg = cstring_lossy(&msg);
                cb(ctx, crate::cleanup::CleanupLogKind::Error as c_int, c_msg.as_ptr());
            }
            (false, false)
        },
        |kind, msg| {
            if let Some(cb) = log_cb {
                let c_msg = cstring_lossy(msg);
                cb(ctx, kind as c_int, c_msg.as_ptr());
            }
        },
        |moved_path| {
            if let Some(cb) = moved_cb {
                let c_path = cstring_lossy(moved_path);
                cb(ctx, c_path.as_ptr());
            }
        },
    );
    i32::from(ok)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_now_ms() -> i64 {
    crate::dupesearch_nzb::donor_now_ms()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_stop_all() {
    crate::dupesearch_nzb::donor_stop_all();
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_wait_all() {
    crate::dupesearch_nzb::donor_wait_all();
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_reset() {
    crate::dupesearch_nzb::donor_reset();
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_stopping() -> c_int {
    i32::from(crate::dupesearch_nzb::donor_stopping())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_body_gate_done(rust_gate: *mut c_void) -> c_int {
    if rust_gate.is_null() {
        return 0;
    }
    let gate = &*(rust_gate as *const crate::dupesearch_nzb::DonorBodyGate);
    i32::from(gate.done())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_body_once(
    rust_gate: *mut c_void,
    ctx: *mut c_void,
    download_cb: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
) -> c_int {
    if rust_gate.is_null() {
        return 4;
    }
    let gate = &*(rust_gate as *const crate::dupesearch_nzb::DonorBodyGate);
    match gate.body_once_fallible(|| {
        let code = download_cb.map_or(4, |cb| unsafe { cb(ctx) });
        if code < 0 {
            return Err(code);
        }
        Ok(match code {
            0 => crate::dupesearch_nzb::DonorAnswer::None,
            1 => crate::dupesearch_nzb::DonorAnswer::Present,
            2 => crate::dupesearch_nzb::DonorAnswer::Missing,
            3 => crate::dupesearch_nzb::DonorAnswer::BodyBad,
            _ => crate::dupesearch_nzb::DonorAnswer::Error,
        })
    }) {
        Ok(ans) => ans as c_int,
        Err(err_code) => err_code,
    }
}

struct CppDonorTransport {
    cpp_server: usize,
    exchange_cb: unsafe extern "C" fn(
        *mut c_void,
        usize,
        *const *const c_char,
        *const usize,
        *const *mut c_void,
        *const *const *const c_char,
        *const *const usize,
        *const usize,
        *mut c_int,
        *mut usize,
        *mut c_int,
    ) -> c_int,
}

impl crate::dupesearch_nzb::DonorServerExchange for CppDonorTransport {
    fn exchange(
        &self,
        batch: &[crate::dupesearch_nzb::DonorRequest],
    ) -> Result<Vec<crate::dupesearch_nzb::DonorAnswer>, crate::dupesearch_nzb::DonorExchangeError>
    {
        let count = batch.len();
        let mut msg_ptrs = Vec::with_capacity(count);
        let mut msg_lens = Vec::with_capacity(count);
        let mut gate_ptrs = Vec::with_capacity(count);
        let mut group_str_vecs: Vec<Vec<*const c_char>> = Vec::with_capacity(count);
        let mut group_len_vecs: Vec<Vec<usize>> = Vec::with_capacity(count);
        for req in batch {
            msg_ptrs.push(req.message_id.as_ptr().cast::<c_char>());
            msg_lens.push(req.message_id.len());
            gate_ptrs.push(
                req.gate
                    .as_ref()
                    .map_or(std::ptr::null_mut(), |g| std::sync::Arc::as_ptr(g) as *mut c_void),
            );
            let mut g_ptrs = Vec::with_capacity(req.groups.len());
            let mut g_lens = Vec::with_capacity(req.groups.len());
            for g in req.groups.iter() {
                g_ptrs.push(g.as_ptr().cast::<c_char>());
                g_lens.push(g.len());
            }
            group_str_vecs.push(g_ptrs);
            group_len_vecs.push(g_lens);
        }
        let mut group_ptrs_outer = Vec::with_capacity(count);
        let mut group_lens_outer = Vec::with_capacity(count);
        let mut group_counts = Vec::with_capacity(count);
        for i in 0..count {
            group_ptrs_outer.push(group_str_vecs[i].as_ptr());
            group_lens_outer.push(group_len_vecs[i].as_ptr());
            group_counts.push(group_str_vecs[i].len());
        }

        let mut out_answers = vec![0 as c_int; count];
        let mut out_count: usize = 0;
        let mut out_busy: c_int = 0;
        let threw = unsafe {
            (self.exchange_cb)(
                self.cpp_server as *mut c_void,
                count,
                msg_ptrs.as_ptr(),
                msg_lens.as_ptr(),
                gate_ptrs.as_ptr(),
                group_ptrs_outer.as_ptr(),
                group_lens_outer.as_ptr(),
                group_counts.as_ptr(),
                out_answers.as_mut_ptr(),
                &mut out_count,
                &mut out_busy,
            )
        };
        let to_answer = |v: c_int| match v {
            0 => crate::dupesearch_nzb::DonorAnswer::None,
            1 => crate::dupesearch_nzb::DonorAnswer::Present,
            2 => crate::dupesearch_nzb::DonorAnswer::Missing,
            3 => crate::dupesearch_nzb::DonorAnswer::BodyBad,
            _ => crate::dupesearch_nzb::DonorAnswer::Error,
        };
        let answers: Vec<crate::dupesearch_nzb::DonorAnswer> = out_answers
            .into_iter()
            .take(out_count.min(count))
            .map(to_answer)
            .collect();
        if threw != 0 {
            Err(crate::dupesearch_nzb::DonorExchangeError {
                answers,
                busy: out_busy != 0,
            })
        } else {
            Ok(answers)
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_server_new(
    cpp_server: *mut c_void,
    cap: c_int,
    exchange_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            usize,
            *const *const c_char,
            *const usize,
            *const *mut c_void,
            *const *const *const c_char,
            *const *const usize,
            *const usize,
            *mut c_int,
            *mut usize,
            *mut c_int,
        ) -> c_int,
    >,
) -> *mut c_void {
    let Some(cb) = exchange_cb else {
        return std::ptr::null_mut();
    };
    let srv = std::sync::Arc::new(crate::dupesearch_nzb::DonorServer::new(
        cap,
        CppDonorTransport {
            cpp_server: cpp_server as usize,
            exchange_cb: cb,
        },
    ));
    Box::into_raw(Box::new(srv)).cast::<c_void>()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_server_free(handle: *mut c_void) {
    if !handle.is_null() {
        drop(Box::from_raw(
            handle.cast::<std::sync::Arc<crate::dupesearch_nzb::DonorServer>>(),
        ));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_server_cap(handle: *const c_void) -> c_int {
    if handle.is_null() {
        return 1;
    }
    let srv = &*handle.cast::<std::sync::Arc<crate::dupesearch_nzb::DonorServer>>();
    srv.cap()
}

struct CppKeepAlive {
    ptr: usize,
    drop_fn: unsafe extern "C" fn(*mut c_void),
}

impl Drop for CppKeepAlive {
    fn drop(&mut self) {
        if self.ptr != 0 {
            unsafe { (self.drop_fn)(self.ptr as *mut c_void) };
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_check_posting(
    server_handles: *const *mut c_void,
    server_count: usize,
    keepalive_ptr: *mut c_void,
    keepalive_drop: Option<unsafe extern "C" fn(*mut c_void)>,
    ids_ptr: *const *const c_char,
    ids_lens: *const usize,
    ids_count: usize,
    group_ptrs: *const *const c_char,
    group_lens: *const usize,
    group_count: usize,
    percent: c_int,
    minimum: c_int,
    maximum: c_int,
    body_percent: c_int,
    max_body: c_int,
    seed: u32,
    probe: c_int,
    budget_ms: c_int,
    deadline_ms: i64,
    full: c_int,
    out_checked: *mut c_int,
    out_present: *mut c_int,
    out_missing: *mut c_int,
    out_error: *mut c_int,
    out_body_checked: *mut c_int,
    out_body_bad: *mut c_int,
) {
    let keepalive: Option<std::sync::Arc<dyn Send + Sync>> =
        keepalive_drop.map(|drop_fn| {
            std::sync::Arc::new(CppKeepAlive {
                ptr: keepalive_ptr as usize,
                drop_fn,
            }) as std::sync::Arc<dyn Send + Sync>
        });
    let mut servers = Vec::with_capacity(server_count);
    if !server_handles.is_null() {
        for i in 0..server_count {
            let h = *server_handles.add(i);
            if !h.is_null() {
                let srv = &*h.cast::<std::sync::Arc<crate::dupesearch_nzb::DonorServer>>();
                servers.push(srv.clone());
            }
        }
    }
    let mut ids = Vec::with_capacity(ids_count);
    if !ids_ptr.is_null() && !ids_lens.is_null() {
        for i in 0..ids_count {
            ids.push(String::from_utf8_lossy(bytes(*ids_ptr.add(i), *ids_lens.add(i))).into_owned());
        }
    }
    let mut groups = Vec::with_capacity(group_count);
    if !group_ptrs.is_null() && !group_lens.is_null() {
        for i in 0..group_count {
            groups.push(
                String::from_utf8_lossy(bytes(*group_ptrs.add(i), *group_lens.add(i))).into_owned(),
            );
        }
    }
    let options = crate::dupesearch_nzb::DonorOptions {
        sample: crate::dupesearch_nzb::DonorSampleOptions {
            percent,
            minimum,
            maximum,
            body_percent,
            max_body,
            seed,
        },
        probe,
        budget_ms,
        deadline_ms,
    };
    let res = crate::dupesearch_nzb::donor_check_posting_guarded(
        &servers,
        &ids,
        &std::sync::Arc::new(groups),
        &options,
        full != 0,
        keepalive,
    );
    if !out_checked.is_null() {
        *out_checked = res.checked;
    }
    if !out_present.is_null() {
        *out_present = res.present;
    }
    if !out_missing.is_null() {
        *out_missing = res.missing;
    }
    if !out_error.is_null() {
        *out_error = res.error;
    }
    if !out_body_checked.is_null() {
        *out_body_checked = res.body_checked;
    }
    if !out_body_bad.is_null() {
        *out_body_bad = res.body_bad;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_check_postings(
    server_handles: *const *mut c_void,
    server_count: usize,
    keepalive_ptr: *mut c_void,
    keepalive_drop: Option<unsafe extern "C" fn(*mut c_void)>,
    posting_count: usize,
    key_ptrs: *const *const c_char,
    key_lens: *const usize,
    ids_ptrs: *const *const *const c_char,
    ids_lens: *const *const usize,
    ids_counts: *const usize,
    group_ptrs: *const *const *const c_char,
    group_lens: *const *const usize,
    group_counts: *const usize,
    percent: c_int,
    minimum: c_int,
    maximum: c_int,
    body_percent: c_int,
    max_body: c_int,
    seed: u32,
    probe: c_int,
    budget_ms: c_int,
    deadline_ms: i64,
    full: c_int,
    concurrency: c_int,
    emit_ctx: *mut c_void,
    emit_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            usize,
            c_int,
            c_int,
            c_int,
            c_int,
            c_int,
            c_int,
        ),
    >,
) {
    let keepalive: Option<std::sync::Arc<dyn Send + Sync>> =
        keepalive_drop.map(|drop_fn| {
            std::sync::Arc::new(CppKeepAlive {
                ptr: keepalive_ptr as usize,
                drop_fn,
            }) as std::sync::Arc<dyn Send + Sync>
        });
    let mut servers = Vec::with_capacity(server_count);
    if !server_handles.is_null() {
        for i in 0..server_count {
            let h = *server_handles.add(i);
            if !h.is_null() {
                let srv = &*h.cast::<std::sync::Arc<crate::dupesearch_nzb::DonorServer>>();
                servers.push(srv.clone());
            }
        }
    }
    let mut postings = Vec::with_capacity(posting_count);
    for i in 0..posting_count {
        let key = if !key_ptrs.is_null() && !key_lens.is_null() {
            String::from_utf8_lossy(bytes(*key_ptrs.add(i), *key_lens.add(i))).into_owned()
        } else {
            String::new()
        };
        let mut ids = Vec::new();
        if !ids_ptrs.is_null() && !ids_lens.is_null() && !ids_counts.is_null() {
            let cnt = *ids_counts.add(i);
            let p = *ids_ptrs.add(i);
            let l = *ids_lens.add(i);
            if !p.is_null() && !l.is_null() {
                for j in 0..cnt {
                    ids.push(String::from_utf8_lossy(bytes(*p.add(j), *l.add(j))).into_owned());
                }
            }
        }
        let mut groups = Vec::new();
        if !group_ptrs.is_null() && !group_lens.is_null() && !group_counts.is_null() {
            let cnt = *group_counts.add(i);
            let p = *group_ptrs.add(i);
            let l = *group_lens.add(i);
            if !p.is_null() && !l.is_null() {
                for j in 0..cnt {
                    groups.push(String::from_utf8_lossy(bytes(*p.add(j), *l.add(j))).into_owned());
                }
            }
        }
        postings.push(crate::dupesearch_nzb::DonorPosting {
            key,
            ids,
            groups: std::sync::Arc::new(groups),
        });
    }
    let options = crate::dupesearch_nzb::DonorOptions {
        sample: crate::dupesearch_nzb::DonorSampleOptions {
            percent,
            minimum,
            maximum,
            body_percent,
            max_body,
            seed,
        },
        probe,
        budget_ms,
        deadline_ms,
    };
    let emit_ctx_addr = emit_ctx as usize;
    crate::dupesearch_nzb::donor_check_postings_guarded(
        &servers,
        &postings,
        &options,
        full != 0,
        concurrency,
        keepalive,
        |key, h| {
            if let Some(cb) = emit_cb {
                unsafe {
                    cb(
                        emit_ctx_addr as *mut c_void,
                        key.as_ptr().cast::<c_char>(),
                        key.len(),
                        h.checked,
                        h.present,
                        h.missing,
                        h.error,
                        h.body_checked,
                        h.body_bad,
                    );
                }
            }
        },
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_fetcher_new() -> *mut c_void {
    Box::into_raw(Box::new(crate::dupesearch_nzb::NzbFetcherCore::new())).cast::<c_void>()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_fetcher_free(handle: *mut c_void) {
    if !handle.is_null() {
        drop(Box::from_raw(
            handle.cast::<crate::dupesearch_nzb::NzbFetcherCore>(),
        ));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_fetcher_set_state_path(
    handle: *mut c_void,
    path: *const c_char,
    path_len: usize,
) {
    if !handle.is_null() {
        let core = &*handle.cast::<crate::dupesearch_nzb::NzbFetcherCore>();
        let s = String::from_utf8_lossy(bytes(path, path_len));
        core.set_state_path(&s);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_fetcher_load(handle: *mut c_void, now: i64) {
    if !handle.is_null() {
        let core = &*handle.cast::<crate::dupesearch_nzb::NzbFetcherCore>();
        core.load(now);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_fetcher_fetch(
    handle: *mut c_void,
    indexer: *const c_char,
    indexer_len: usize,
    title: *const c_char,
    title_len: usize,
    link: *const c_char,
    link_len: usize,
    size: i64,
    grabs: c_int,
    date: i64,
    deadline: i64,
    retries: c_int,
    budget_handle: *mut c_void,
    ctx: *mut c_void,
    now_cb: Option<unsafe extern "C" fn(*mut c_void) -> i64>,
    stopped_cb: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
    sleep_100ms_cb: Option<unsafe extern "C" fn(*mut c_void)>,
    http_fetch_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            usize,
            *const c_char,
            usize,
            usize,
            i64,
            *mut c_int,
            *mut *const c_char,
            *mut usize,
        ) -> c_int,
    >,
    log_info_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    log_warn_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    out_data: *mut RsBuf,
    out_scalars: *mut crate::dupesearch_nzb::RsNzbSummaryScalars,
    summary_ctx: *mut c_void,
    on_strings: Option<
        unsafe extern "C" fn(
            ctx: *mut c_void,
            poster_ptr: *const u8,
            poster_len: usize,
            main_name_ptr: *const u8,
            main_name_len: usize,
            par_name_ptr: *const u8,
            par_name_len: usize,
            par_id_ptr: *const u8,
            par_id_len: usize,
            par_bytes: i64,
        ),
    >,
    on_slice: Option<
        unsafe extern "C" fn(
            ctx: *mut c_void,
            kind: c_int,
            ptr1: *const u8,
            len1: usize,
            ptr2: *const u8,
            len2: usize,
            bytes: i64,
        ),
    >,
    on_recovery_bytes: Option<
        unsafe extern "C" fn(ctx: *mut c_void, ptr: *const u64, len: usize),
    >,
) -> c_int {
    if handle.is_null() {
        return 1;
    }
    let core = &*handle.cast::<crate::dupesearch_nzb::NzbFetcherCore>();
    let budget = if budget_handle.is_null() {
        None
    } else {
        Some(&*budget_handle.cast::<crate::dupewatch::FetchBudget>())
    };
    let listing = crate::dupesearch_nzb::NewznabResult {
        indexer: bytes(indexer, indexer_len).to_vec(),
        title: bytes(title, title_len).to_vec(),
        link: bytes(link, link_len).to_vec(),
        size,
        grabs,
        date,
        ..Default::default()
    };
    let fetched = core.fetch_with(
        &listing,
        deadline,
        retries,
        budget,
        || now_cb.map_or(0, |cb| unsafe { cb(ctx) }),
        crate::dupesearch_nzb::donor_now_ms,
        || stopped_cb.is_some_and(|cb| unsafe { cb(ctx) != 0 }),
        || {
            if let Some(cb) = sleep_100ms_cb {
                unsafe { cb(ctx) };
            }
        },
        |url, label, max_bytes, deadline_ms| {
            let Some(cb) = http_fetch_cb else {
                return (false, 0, Vec::new());
            };
            let mut status: c_int = 0;
            let mut body_ptr: *const c_char = std::ptr::null();
            let mut body_len: usize = 0;
            let ok = unsafe {
                cb(
                    ctx,
                    url.as_ptr().cast::<c_char>(),
                    url.len(),
                    label.as_ptr().cast::<c_char>(),
                    label.len(),
                    max_bytes,
                    deadline_ms,
                    &mut status,
                    &mut body_ptr,
                    &mut body_len,
                )
            };
            let body_vec = bytes(body_ptr, body_len).to_vec();
            (ok != 0, status, body_vec)
        },
        |msg| {
            if let Some(cb) = log_info_cb {
                if let Ok(c_msg) = std::ffi::CString::new(msg) {
                    unsafe { cb(ctx, c_msg.as_ptr()) };
                }
            }
        },
        |msg| {
            if let Some(cb) = log_warn_cb {
                if let Ok(c_msg) = std::ffi::CString::new(msg) {
                    unsafe { cb(ctx, c_msg.as_ptr()) };
                }
            }
        },
    );
    if fetched.reason == crate::dupesearch_nzb::NzbFetchReason::Ok {
        crate::dupesearch_nzb::emit_nzb_summary_to_ffi(
            &fetched.info,
            out_scalars,
            summary_ctx,
            on_strings,
            on_slice,
            on_recovery_bytes,
        );
        if !out_data.is_null() {
            *out_data = into_buf(fetched.data);
        }
    }
    fetched.reason as c_int
}


#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_identity_fetch_article(
    msg_id: *const c_char,
    msg_id_len: usize,
     deadline: i64,
    ctx: *mut c_void,
    now_cb: Option<unsafe extern "C" fn(*mut c_void) -> i64>,
    stopped_cb: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
    fetch_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            usize,
            *mut c_int,
            *mut i64,
            *mut i64,
        ) -> c_int,
    >,
    stop_fetch_cb: Option<unsafe extern "C" fn(*mut c_void)>,
) -> c_int {
    struct SendCtx(*mut c_void);
    unsafe impl Send for SendCtx {}
    unsafe impl Sync for SendCtx {}
    let send_ctx = SendCtx(ctx);
    let groups: [String; 0] = [];
    let res = crate::dupesearch_nzb::dupe_identity_fetch_article(
        bytes(msg_id, msg_id_len),
        &groups,
        deadline,
        || now_cb.map_or(0, |cb| unsafe { cb(send_ctx.0) }),
        || stopped_cb.is_some_and(|cb| unsafe { cb(send_ctx.0) != 0 }),
        |bracketed_id, _groups| {
            let sctx = &send_ctx;
            let Some(cb) = fetch_cb else {
                return Err(());
            };
            let mut success: c_int = 0;
            let mut offset: i64 = 0;
            let mut file_size: i64 = 0;
            let ok = unsafe {
                cb(
                    sctx.0,
                    bracketed_id.as_ptr().cast::<c_char>(),
                    bracketed_id.len(),
                    &mut success,
                    &mut offset,
                    &mut file_size,
                )
            };
            if ok == 0 {
                return Err(());
            }
            Ok(crate::dupesearch_nzb::FetchedArticleResult {
                success: success != 0,
                data: Vec::new(),
                offset,
                file_size,
            })
        },
        || {
            if let Some(cb) = stop_fetch_cb {
                unsafe { cb(send_ctx.0) };
            }
        },
    );
    i32::from(res.success)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_identity_read(
    data_bytes: i64,
    head_names: *const *const c_char,
    head_name_lens: *const usize,
    head_msg_ids: *const *const c_char,
    head_msg_id_lens: *const usize,
    head_bytes: *const i64,
    head_count: usize,
    deadline: i64,
    ctx: *mut c_void,
    now_cb: Option<unsafe extern "C" fn(*mut c_void) -> i64>,
    stopped_cb: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
    fetch_head_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            usize,
            i64,
            *mut c_int,
            *mut i64,
            *mut *const c_char,
            *mut usize,
        ),
    >,
    out_name: *mut RsBuf,
    out_ext: *mut RsBuf,
    out_bytes: *mut u64,
) -> c_int {
    let mut heads = Vec::with_capacity(head_count);
    for i in 0..head_count {
        let name = if head_names.is_null() || head_name_lens.is_null() {
            Vec::new()
        } else {
            bytes(*head_names.add(i), *head_name_lens.add(i)).to_vec()
        };
        let mid = if head_msg_ids.is_null() || head_msg_id_lens.is_null() {
            Vec::new()
        } else {
            bytes(*head_msg_ids.add(i), *head_msg_id_lens.add(i)).to_vec()
        };
        let b = if head_bytes.is_null() {
            0
        } else {
            *head_bytes.add(i)
        };
        heads.push((name, mid, b));
    }
    let groups: [String; 0] = [];
    let found = crate::dupesearch_nzb::dupe_identity_read(
        data_bytes,
        &heads,
        &groups,
        deadline,
        || now_cb.map_or(0, |cb| unsafe { cb(ctx) }),
        || stopped_cb.is_some_and(|cb| unsafe { cb(ctx) != 0 }),
        |msg_id, _groups, dl| {
            let Some(cb) = fetch_head_cb else {
                return crate::dupesearch_nzb::FetchedArticleResult::default();
            };
            let mut success: c_int = 0;
            let mut file_size: i64 = 0;
            let mut data_ptr: *const c_char = std::ptr::null();
            let mut data_len: usize = 0;
            unsafe {
                cb(
                    ctx,
                    msg_id.as_ptr().cast::<c_char>(),
                    msg_id.len(),
                    dl,
                    &mut success,
                    &mut file_size,
                    &mut data_ptr,
                    &mut data_len,
                );
            }
            let data = bytes(data_ptr, data_len).to_vec();
            crate::dupesearch_nzb::FetchedArticleResult {
                success: success != 0,
                data,
                offset: 0,
                file_size,
            }
        },
    );
    if let Some(ident) = found {
        if !out_name.is_null() {
            *out_name = into_buf(ident.name);
        }
        if !out_ext.is_null() {
            *out_ext = into_buf(ident.extension);
        }
        if !out_bytes.is_null() {
            *out_bytes = ident.bytes;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_benchmark_run(
    dir_ptr: *const c_char,
    dir_len: usize,
    block_size_bytes: usize,
    max_file_size_bytes: u64,
    timeout_secs: i64,
    space_ok: c_int,
    available_bytes: u64,
    space_err_ptr: *const c_char,
    space_err_len: usize,
    out_write_bytes: *mut u64,
    out_write_ms: *mut f64,
    out_read_bytes: *mut u64,
    out_read_ms: *mut f64,
    out_err: *mut RsBuf,
) -> c_int {
    let space_res = if space_ok != 0 {
        Ok(available_bytes)
    } else {
        Err(String::from_utf8_lossy(bytes(space_err_ptr, space_err_len)).into_owned())
    };
    let timeout = std::time::Duration::from_secs(timeout_secs.max(0) as u64);
    match crate::util::disk_benchmark_run_bytes(
        bytes(dir_ptr, dir_len),
        block_size_bytes,
        max_file_size_bytes,
        timeout,
        space_res,
    ) {
        Ok(res) => {
            if !out_write_bytes.is_null() {
                *out_write_bytes = res.write_bytes;
            }
            if !out_write_ms.is_null() {
                *out_write_ms = res.write_ms;
            }
            if !out_read_bytes.is_null() {
                *out_read_bytes = res.read_bytes;
            }
            if !out_read_ms.is_null() {
                *out_read_ms = res.read_ms;
            }
            0
        }
        Err(crate::util::DiskBenchmarkError::InvalidArgument(msg)) => {
            if !out_err.is_null() {
                *out_err = into_buf(msg.into_bytes());
            }
            1
        }
        Err(crate::util::DiskBenchmarkError::Runtime(msg)) => {
            if !out_err.is_null() {
                *out_err = into_buf(msg.into_bytes());
            }
            2
        }
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
    if rx.is_valid() { 1 } else { 0 }
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
    Box::into_raw(Box::new(crate::fsops::PosixDirBrowser::new(p, snapshot != 0))).cast()
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
        drop(Box::from_raw(handle.cast::<crate::fsops::PosixDirBrowser>()));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_article_batch_fetcher_new(
    ctx: *mut std::ffi::c_void,
    fetch_cb: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            usize,
            *const c_char,
            *const *const c_char,
            usize,
            *mut RsBuf,
            *mut i64,
            *mut i64,
            *mut c_int,
            *mut c_int,
            *mut c_int,
        ) -> c_int,
    >,
    stop_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
) -> *mut std::ffi::c_void {
    let ctx_addr = ctx as usize;
    let hook: Option<std::sync::Arc<dyn Fn() + Send + Sync>> = stop_cb.map(|cb| {
        std::sync::Arc::new(move || unsafe {
            cb(ctx_addr as *mut std::ffi::c_void);
        }) as std::sync::Arc<dyn Fn() + Send + Sync>
    });
    let batch = crate::nntp::ArticleBatchFetcher::new(
        move |worker_idx, req| {
            let Some(cb) = fetch_cb else {
                return Err("no fetch callback".to_string());
            };
            let mut mid_c = Vec::with_capacity(req.message_id.len() + 1);
            mid_c.extend_from_slice(&req.message_id);
            mid_c.push(0);

            let mut grp_c: Vec<Vec<u8>> = Vec::with_capacity(req.groups.len());
            for g in req.groups.iter() {
                let mut gc = Vec::with_capacity(g.len() + 1);
                gc.extend_from_slice(g);
                gc.push(0);
                grp_c.push(gc);
            }
            let grp_ptrs: Vec<*const c_char> =
                grp_c.iter().map(|g| g.as_ptr().cast::<c_char>()).collect();

            let mut data_buf = RsBuf {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            let mut offset: i64 = 0;
            let mut file_size: i64 = 0;
            let mut success: c_int = 0;
            let mut retry: c_int = 0;
            let mut transient: c_int = 0;
            let ok = unsafe {
                cb(
                    ctx_addr as *mut std::ffi::c_void,
                    worker_idx,
                    mid_c.as_ptr().cast::<c_char>(),
                    grp_ptrs.as_ptr(),
                    grp_ptrs.len(),
                    &mut data_buf,
                    &mut offset,
                    &mut file_size,
                    &mut success,
                    &mut retry,
                    &mut transient,
                )
            };
            let data = if !data_buf.data.is_null() {
                unsafe {
                    Vec::from_raw_parts(data_buf.data.cast::<u8>(), data_buf.len, data_buf.cap)
                }
            } else {
                Vec::new()
            };
            if ok == 0 {
                return Err("fetch exception".to_string());
            }
            Ok(crate::nntp::BatchFetchedArticle {
                data,
                offset,
                file_size,
                success: success != 0,
                retry: retry != 0,
                transient: transient != 0,
            })
        },
        hook,
    );
    Box::into_raw(Box::new(batch)).cast()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_article_batch_fetcher_free(handle: *mut std::ffi::c_void) {
    if !handle.is_null() {
        drop(Box::from_raw(handle.cast::<crate::nntp::ArticleBatchFetcher>()));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_article_batch_fetcher_set_worker_count(
    handle: *mut std::ffi::c_void,
    worker_count: c_int,
) {
    if !handle.is_null() {
        let batch = &*handle.cast::<crate::nntp::ArticleBatchFetcher>();
        batch.set_worker_count(worker_count);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_article_batch_fetcher_begin(
    handle: *mut std::ffi::c_void,
    req_count: usize,
    ctx: *mut std::ffi::c_void,
    get_req_cb: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            usize,
            *mut *const c_char,
            *mut usize,
        ) -> usize,
    >,
    get_group_cb: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            usize,
            usize,
            *mut *const c_char,
            *mut usize,
        ),
    >,
) {
    if handle.is_null() {
        return;
    }
    let batch = &*handle.cast::<crate::nntp::ArticleBatchFetcher>();
    let mut requests = Vec::with_capacity(req_count);
    for i in 0..req_count {
        let mut mid_ptr: *const c_char = std::ptr::null();
        let mut mid_len: usize = 0;
        let grp_count = get_req_cb.map_or(0, |cb| unsafe { cb(ctx, i, &mut mid_ptr, &mut mid_len) });
        let mut groups = Vec::with_capacity(grp_count);
        if let Some(gcb) = get_group_cb {
            for g in 0..grp_count {
                let mut g_ptr: *const c_char = std::ptr::null();
                let mut g_len: usize = 0;
                unsafe { gcb(ctx, i, g, &mut g_ptr, &mut g_len) };
                groups.push(bytes(g_ptr, g_len).to_vec());
            }
        }
        requests.push(crate::nntp::BatchRequest {
            message_id: bytes(mid_ptr, mid_len).to_vec(),
            groups: std::sync::Arc::new(groups),
        });
    }
    batch.begin(requests);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_article_batch_fetcher_next(
    handle: *mut std::ffi::c_void,
    out_data: *mut RsBuf,
    out_offset: *mut i64,
    out_file_size: *mut i64,
    out_success: *mut c_int,
    out_retry: *mut c_int,
    out_transient: *mut c_int,
) -> c_int {
    if handle.is_null() {
        return 0;
    }
    let batch = &*handle.cast::<crate::nntp::ArticleBatchFetcher>();
    match batch.next() {
        Some(res) => {
            if !out_data.is_null() {
                *out_data = into_buf(res.data);
            }
            if !out_offset.is_null() {
                *out_offset = res.offset;
            }
            if !out_file_size.is_null() {
                *out_file_size = res.file_size;
            }
            if !out_success.is_null() {
                *out_success = if res.success { 1 } else { 0 };
            }
            if !out_retry.is_null() {
                *out_retry = if res.retry { 1 } else { 0 };
            }
            if !out_transient.is_null() {
                *out_transient = if res.transient { 1 } else { 0 };
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_article_batch_fetcher_cancel_remaining(
    handle: *mut std::ffi::c_void,
) {
    if !handle.is_null() {
        let batch = &*handle.cast::<crate::nntp::ArticleBatchFetcher>();
        batch.cancel_remaining();
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_article_batch_fetcher_stop(handle: *mut std::ffi::c_void) {
    if !handle.is_null() {
        let batch = &*handle.cast::<crate::nntp::ArticleBatchFetcher>();
        batch.stop();
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
    pub add_task: Option<
        unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int, c_int, *const c_char),
    >,
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
    ) -> Option<(
        Option<std::ffi::CString>,
        Option<std::ffi::CString>,
        i32,
    )> {
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
        let ok = unsafe { cb(self.0.ctx, dir.as_ptr(), err_buf.as_mut_ptr(), err_buf.len()) };
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
    let Some((name, val)) =
        crate::options::split_and_convert_option_string(CStr::from_ptr(option))
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

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_execute_save_config(
    request: *mut *mut c_char,
    is_get: c_int,
    is_json: c_int,
    config_filename: *const c_char,
    out_fault: *mut c_int,
) -> RsBuf {
    if request.is_null() || (*request).is_null() {
        if !out_fault.is_null() {
            *out_fault = 1;
        }
        return into_buf(crate::xmlrpc_format::format_error_bytes(
            is_json != 0,
            2,
            b"Invalid parameter",
        ));
    }
    let raw = crate::rpcparams::Raw::new((*request).cast::<u8>());
    let fname = if config_filename.is_null() {
        None
    } else {
        Some(CStr::from_ptr(config_filename).to_bytes())
    };
    let (pos, fault, resp) = crate::xmlrpc_format::xmlrpc_execute_save_config_raw(
        raw,
        is_get != 0,
        is_json != 0,
        fname,
    );
    *request = (*request).add(pos);
    if !out_fault.is_null() {
        *out_fault = c_int::from(fault);
    }
    into_buf(resp)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_execute_load_config(
    is_json: c_int,
    user_access: c_int,
    ua_restricted: c_int,
    config_filename: *const c_char,
    out_fault: *mut c_int,
) -> RsBuf {
    let fname = if config_filename.is_null() {
        None
    } else {
        Some(CStr::from_ptr(config_filename).to_bytes())
    };
    let (fault, resp) = crate::xmlrpc_format::xmlrpc_execute_load_config(
        is_json != 0,
        user_access,
        ua_restricted,
        fname,
    );
    if !out_fault.is_null() {
        *out_fault = c_int::from(fault);
    }
    into_buf(resp)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_execute_tls_cipher(
    operation: c_int,
    user_access: c_int,
    ua_control: c_int,
    protocol: c_int,
    rp_json_rpc: c_int,
    http_method: c_int,
    hm_post: c_int,
    is_json: c_int,
    request: *const c_char,
    limits: crate::json::Limits,
    ctx: *mut c_void,
    backend_cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char) -> RsBuf>,
    out_fault: *mut c_int,
) -> RsBuf {
    let req_opt = if request.is_null() {
        None
    } else {
        Some(CStr::from_ptr(request).to_bytes())
    };
    let invoke = |op_kind: i32, job_id: Option<&[u8]>| -> Vec<u8> {
        let Some(cb) = backend_cb else {
            return Vec::new();
        };
        let c_id = job_id.and_then(|b| std::ffi::CString::new(b).ok());
        let id_ptr = c_id.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());
        let buf = cb(ctx, op_kind, id_ptr);
        if buf.data.is_null() {
            return Vec::new();
        }
        let slice = std::slice::from_raw_parts(buf.data.cast::<u8>(), buf.len);
        let out = slice.to_vec();
        nzbget_rs_free(buf);
        out
    };
    let (fault, resp) = crate::xmlrpc_format::xmlrpc_execute_tls_cipher(
        operation,
        user_access,
        ua_control,
        protocol,
        rp_json_rpc,
        http_method,
        hm_post,
        is_json != 0,
        req_opt,
        limits,
        invoke,
    );
    if !out_fault.is_null() {
        *out_fault = c_int::from(fault);
    }
    into_buf(resp)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_parse_edit_queue(
    request: *mut *mut c_char,
    is_get: c_int,
    is_json: c_int,
    ea_file_move_offset: c_int,
    ea_group_move_offset: c_int,
    out_action: *mut c_int,
    out_args_or_err: *mut RsBuf,
    out_err_code: *mut c_int,
    id_ctx: *mut c_void,
    add_id_cb: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
) -> c_int {
    if request.is_null() || (*request).is_null() {
        if !out_err_code.is_null() {
            *out_err_code = 2;
        }
        if !out_args_or_err.is_null() {
            *out_args_or_err = into_buf(b"Invalid parameter".to_vec());
        }
        return 0;
    }
    let raw = crate::rpcparams::Raw::new((*request).cast::<u8>());
    let res = crate::xmlrpc_format::xmlrpc_parse_edit_queue_raw(
        raw,
        is_get != 0,
        is_json != 0,
        ea_file_move_offset,
        ea_group_move_offset,
    );
    match res {
        Ok((pos, action, args, ids)) => {
            *request = (*request).add(pos);
            if !out_action.is_null() {
                *out_action = action;
            }
            if !out_args_or_err.is_null() {
                *out_args_or_err = into_buf(args);
            }
            if let Some(cb) = add_id_cb {
                for id in ids {
                    cb(id_ctx, id);
                }
            }
            1
        }
        Err((pos, code, msg)) => {
            *request = (*request).add(pos);
            if !out_err_code.is_null() {
                *out_err_code = code;
            }
            if !out_args_or_err.is_null() {
                *out_args_or_err = into_buf(msg.to_vec());
            }
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_parse_start_script(
    request: *mut *mut c_char,
    is_get: c_int,
    is_json: c_int,
    out_script: *mut RsBuf,
    out_command: *mut RsBuf,
    out_context: *mut RsBuf,
    entry_ctx: *mut c_void,
    add_entry_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
) -> c_int {
    if request.is_null() || (*request).is_null() {
        return 0;
    }
    let raw = crate::rpcparams::Raw::new((*request).cast::<u8>());
    let res = crate::xmlrpc_format::xmlrpc_parse_start_script_raw(
        raw,
        is_get != 0,
        is_json != 0,
    );
    let Some((pos, script, command, context, entries)) = res else {
        return 0;
    };
    *request = (*request).add(pos);
    if !out_script.is_null() {
        *out_script = into_buf(script);
    }
    if !out_command.is_null() {
        *out_command = into_buf(command);
    }
    if !out_context.is_null() {
        *out_context = into_buf(context);
    }
    if let Some(cb) = add_entry_cb {
        for (name, val) in entries {
            let n_buf = into_buf(name);
            let v_buf = into_buf(val);
            cb(entry_ctx, n_buf.data, v_buf.data);
            nzbget_rs_free(n_buf);
            nzbget_rs_free(v_buf);
        }
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_parse_test_disk_speed(
    request: *mut *mut c_char,
    is_get: c_int,
    is_json: c_int,
    out_dir_path: *mut RsBuf,
    out_buffer_bytes: *mut usize,
    out_max_file_bytes: *mut u64,
    out_timeout_sec: *mut c_int,
    out_err_msg: *mut *const c_char,
) -> c_int {
    if request.is_null() || (*request).is_null() {
        if !out_err_msg.is_null() {
            *out_err_msg = b"Invalid argument (Path)\0".as_ptr().cast::<c_char>();
        }
        return 0;
    }
    let raw = crate::rpcparams::Raw::new((*request).cast::<u8>());
    let res = crate::xmlrpc_format::xmlrpc_parse_test_disk_speed_raw(
        raw,
        is_get != 0,
        is_json != 0,
    );
    match res {
        Ok((pos, dir_path, buf_bytes, max_bytes, timeout_sec)) => {
            *request = (*request).add(pos);
            if !out_dir_path.is_null() {
                *out_dir_path = into_buf(dir_path);
            }
            if !out_buffer_bytes.is_null() {
                *out_buffer_bytes = buf_bytes;
            }
            if !out_max_file_bytes.is_null() {
                *out_max_file_bytes = max_bytes;
            }
            if !out_timeout_sec.is_null() {
                *out_timeout_sec = timeout_sec;
            }
            1
        }
        Err((pos, msg)) => {
            *request = (*request).add(pos);
            if !out_err_msg.is_null() {
                *out_err_msg = msg.as_ptr();
            }
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_parse_append_fleet(
    request: *mut *mut c_char,
    is_get: c_int,
    is_json: c_int,
    max_members: c_int,
    ctx: *mut c_void,
    header_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, c_int, c_int)>,
    member_cb: Option<
        unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *const c_char, usize),
    >,
    out_err: *mut RsBuf,
) -> c_int {
    if is_json == 0 {
        if !out_err.is_null() {
            *out_err = into_buf(b"appendfleet needs JSON-RPC".to_vec());
        }
        return 0;
    }
    if request.is_null() || (*request).is_null() {
        if !out_err.is_null() {
            *out_err = into_buf(b"Invalid parameter (DupeKey)".to_vec());
        }
        return 0;
    }
    let raw = crate::rpcparams::Raw::new((*request).cast::<u8>());
    let res = crate::xmlrpc_format::xmlrpc_parse_append_fleet_raw(
        raw,
        is_get != 0,
        is_json != 0,
        max_members,
    );
    match res {
        Ok((pos, parsed)) => {
            *request = (*request).add(pos);
            if let Some(hcb) = header_cb {
                let dk_buf = into_buf(parsed.dupe_key);
                let cat_buf = into_buf(parsed.category);
                hcb(
                    ctx,
                    dk_buf.data,
                    cat_buf.data,
                    parsed.priority,
                    parsed.timeout_sec,
                );
                nzbget_rs_free(dk_buf);
                nzbget_rs_free(cat_buf);
            }
            if let Some(mcb) = member_cb {
                for (name, is_url, content) in parsed.members {
                    let n_buf = into_buf(name);
                    let c_buf = into_buf(content);
                    mcb(ctx, n_buf.data, c_int::from(is_url), c_buf.data, c_buf.len);
                    nzbget_rs_free(n_buf);
                    nzbget_rs_free(c_buf);
                }
            }
            1
        }
        Err((pos, msg)) => {
            *request = (*request).add(pos);
            if !out_err.is_null() {
                *out_err = into_buf(msg);
            }
            0
        }
    }
}

#[repr(C)]
pub struct NzbgetRsParsedDownload {
    pub v13: c_int,
    pub filename: RsBuf,
    pub category: RsBuf,
    pub priority: c_int,
    pub add_top: c_int,
    pub is_url: c_int,
    pub content: RsBuf,
    pub add_paused: c_int,
    pub has_dupe_key: c_int,
    pub dupe_key: RsBuf,
    pub dupe_score: c_int,
    pub dupe_mode: c_int,
    pub auto_category: c_int,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_parse_download(
    request: *mut *mut c_char,
    is_get: c_int,
    is_json: c_int,
    dm_score: c_int,
    dm_all: c_int,
    dm_force: c_int,
    out_dl: *mut NzbgetRsParsedDownload,
    param_ctx: *mut c_void,
    add_param_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
    out_err_msg: *mut *const c_char,
) -> c_int {
    if request.is_null() || (*request).is_null() {
        if !out_err_msg.is_null() {
            *out_err_msg = b"Invalid parameter (Filename)\0".as_ptr().cast::<c_char>();
        }
        return 0;
    }
    let raw = crate::rpcparams::Raw::new((*request).cast::<u8>());
    let res = crate::xmlrpc_format::xmlrpc_parse_download_raw(
        raw,
        is_get != 0,
        is_json != 0,
        dm_score,
        dm_all,
        dm_force,
    );
    match res {
        Ok((pos, dl)) => {
            *request = (*request).add(pos);
            if !out_dl.is_null() {
                let (has_dk, dk_buf) = match dl.dupe_key {
                    Some(dk) => (1, into_buf(dk)),
                    None => (0, RsBuf { data: std::ptr::null_mut(), len: 0, cap: 0 }),
                };
                *out_dl = NzbgetRsParsedDownload {
                    v13: c_int::from(dl.v13),
                    filename: into_buf(dl.filename),
                    category: into_buf(dl.category),
                    priority: dl.priority,
                    add_top: c_int::from(dl.add_top),
                    is_url: c_int::from(dl.is_url),
                    content: into_buf(dl.content),
                    add_paused: c_int::from(dl.add_paused),
                    has_dupe_key: has_dk,
                    dupe_key: dk_buf,
                    dupe_score: dl.dupe_score,
                    dupe_mode: dl.dupe_mode,
                    auto_category: c_int::from(dl.auto_category),
                };
            }
            if let Some(cb) = add_param_cb {
                for (pname, pval) in dl.params {
                    let n_buf = into_buf(pname);
                    let v_buf = into_buf(pval);
                    cb(param_ctx, n_buf.data, v_buf.data);
                    nzbget_rs_free(n_buf);
                    nzbget_rs_free(v_buf);
                }
            }
            1
        }
        Err((pos, msg)) => {
            *request = (*request).add(pos);
            if !out_err_msg.is_null() {
                *out_err_msg = msg.as_ptr();
            }
            0
        }
    }
}

#[repr(C)]
pub struct NzbgetRsParsedViewFeed {
    pub id: c_int,
    pub include_non_matching: c_int,
    pub name: RsBuf,
    pub url: RsBuf,
    pub filter: RsBuf,
    pub backlog: c_int,
    pub pause_nzb: c_int,
    pub category: RsBuf,
    pub category_source: c_int,
    pub priority: c_int,
    pub interval: c_int,
    pub has_feed_filter: c_int,
    pub feed_filter: RsBuf,
    pub cache_time_sec: c_int,
    pub cache_id: RsBuf,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_parse_view_feed(
    request: *mut *mut c_char,
    is_get: c_int,
    is_json: c_int,
    preview: c_int,
    cs_nzb_file: c_int,
    cs_feed_file: c_int,
    cs_auto: c_int,
    out_vf: *mut NzbgetRsParsedViewFeed,
) -> c_int {
    if request.is_null() || (*request).is_null() {
        return 0;
    }
    let raw = crate::rpcparams::Raw::new((*request).cast::<u8>());
    let Some((pos, vf)) = crate::xmlrpc_format::xmlrpc_parse_view_feed_raw(
        raw,
        is_get != 0,
        is_json != 0,
        preview != 0,
        cs_nzb_file,
        cs_feed_file,
        cs_auto,
    ) else {
        return 0;
    };
    *request = (*request).add(pos);
    if !out_vf.is_null() {
        let (has_ff, ff_buf) = match vf.feed_filter {
            Some(ff) => (1, into_buf(ff)),
            None => (0, RsBuf { data: std::ptr::null_mut(), len: 0, cap: 0 }),
        };
        *out_vf = NzbgetRsParsedViewFeed {
            id: vf.id,
            include_non_matching: c_int::from(vf.include_non_matching),
            name: into_buf(vf.name),
            url: into_buf(vf.url),
            filter: into_buf(vf.filter),
            backlog: c_int::from(vf.backlog),
            pause_nzb: c_int::from(vf.pause_nzb),
            category: into_buf(vf.category),
            category_source: vf.category_source,
            priority: vf.priority,
            interval: vf.interval,
            has_feed_filter: has_ff,
            feed_filter: ff_buf,
            cache_time_sec: vf.cache_time_sec,
            cache_id: into_buf(vf.cache_id),
        };
    }
    1
}

#[repr(C)]
pub struct NzbgetRsParsedTestServer {
    pub host: RsBuf,
    pub username: RsBuf,
    pub password: RsBuf,
    pub cipher: RsBuf,
    pub port: c_int,
    pub timeout: c_int,
    pub cert_verif_level: c_int,
    pub encryption: c_int,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_parse_test_server(
    request_body: *const c_char,
    request_ptr: *mut *mut c_char,
    is_get: c_int,
    is_json: c_int,
    http_method: c_int,
    hm_post: c_int,
    cert_verif_count: c_int,
    limits: crate::json::Limits,
    out_ts: *mut NzbgetRsParsedTestServer,
    out_err_msg: *mut *const c_char,
) -> c_int {
    let body_opt = if request_body.is_null() {
        None
    } else {
        Some(CStr::from_ptr(request_body).to_bytes())
    };
    let raw_opt = if request_ptr.is_null() || (*request_ptr).is_null() {
        None
    } else {
        Some(crate::rpcparams::Raw::new((*request_ptr).cast::<u8>()))
    };
    let res = crate::xmlrpc_format::xmlrpc_parse_test_server_raw(
        body_opt,
        raw_opt,
        is_get != 0,
        is_json != 0,
        http_method,
        hm_post,
        cert_verif_count,
        limits,
    );
    match res {
        Ok((pos, ts)) => {
            if !request_ptr.is_null() && !(*request_ptr).is_null() {
                *request_ptr = (*request_ptr).add(pos);
            }
            if !out_ts.is_null() {
                *out_ts = NzbgetRsParsedTestServer {
                    host: into_buf(ts.host),
                    username: into_buf(ts.username),
                    password: into_buf(ts.password),
                    cipher: into_buf(ts.cipher),
                    port: ts.port,
                    timeout: ts.timeout,
                    cert_verif_level: ts.cert_verif_level,
                    encryption: c_int::from(ts.encryption),
                };
            }
            1
        }
        Err((pos, msg)) => {
            if !request_ptr.is_null() && !(*request_ptr).is_null() {
                *request_ptr = (*request_ptr).add(pos);
            }
            if !out_err_msg.is_null() {
                *out_err_msg = msg.as_ptr();
            }
            0
        }
    }
}

type DonorNextArticleFn = Option<
    unsafe extern "C" fn(
        *mut std::ffi::c_void,
        *mut c_int,
        *mut i64,
        *mut i64,
        *mut *const u8,
        *mut usize,
    ) -> c_int,
>;

type DonorBeginMemberBatchFn =
    Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const i32, usize)>;

struct FfiM1DonorEnv {
    ctx: *mut std::ffi::c_void,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    refresh_live_name_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, usize) -> *const c_char>,
    begin_member_batch_cb: DonorBeginMemberBatchFn,
    next_article_cb: DonorNextArticleFn,
    cancel_batch_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    add_progress_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64)>,
    add_recovered_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64)>,
    emit_message_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
}

impl crate::streamrepair::M1DonorEnv for FfiM1DonorEnv {
    fn is_stopped(&self) -> bool {
        self.is_stopped_cb
            .is_some_and(|cb| unsafe { cb(self.ctx) != 0 })
    }

    fn refresh_live_target_name(&mut self, target_index: usize) -> Option<Vec<u8>> {
        let cb = self.refresh_live_name_cb?;
        let ptr = unsafe { cb(self.ctx, target_index) };
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { input(ptr).to_vec() })
        }
    }

    fn begin_member_batch(&mut self, donor_file_index: i32, part_indices: &[i32]) {
        if let Some(cb) = self.begin_member_batch_cb {
            unsafe {
                cb(
                    self.ctx,
                    donor_file_index,
                    part_indices.as_ptr(),
                    part_indices.len(),
                );
            }
        }
    }

    fn next_article(&mut self) -> Option<crate::streamrepair::FetchedDonorArticle> {
        let cb = self.next_article_cb?;
        let mut success: c_int = 0;
        let mut offset: i64 = 0;
        let mut file_size: i64 = 0;
        let mut data_ptr: *const u8 = std::ptr::null();
        let mut data_len: usize = 0;
        let ok = unsafe {
            cb(
                self.ctx,
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

    fn cancel_remaining(&mut self) {
        if let Some(cb) = self.cancel_batch_cb {
            unsafe { cb(self.ctx) };
        }
    }

    fn add_progress_bytes(&mut self, bytes: i64) {
        if let Some(cb) = self.add_progress_cb {
            unsafe { cb(self.ctx, bytes) };
        }
    }

    fn add_recovered_bytes(&mut self, bytes: i64) {
        if let Some(cb) = self.add_recovered_cb {
            unsafe { cb(self.ctx, bytes) };
        }
    }

    fn emit_message_bytes(&mut self, kind: crate::streamrepair::TargetMessageKind, text: &[u8]) {
        let Some(cb) = self.emit_message_cb else {
            return;
        };
        let mut buf: Vec<u8> = text.iter().copied().filter(|&b| b != 0).collect();
        buf.push(0);
        unsafe { cb(self.ctx, kind as i32, buf.as_ptr().cast::<c_char>()) };
    }
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_repair_process_donor(
    dest_dir: *const c_char,
    live_mode: c_int,
    member_names: *const *const c_char,
    member_count: usize,
    target_filenames: *const *const c_char,
    target_encoded_sizes: *const i64,
    target_decoded_sizes: *const i64,
    target_article_counts: *const c_int,
    target_positional_ranks: *const c_int,
    target_positional_windows: *const c_int,
    target_steps_hashes: *const u64,
    target_patch_eligible: *const u8,
    target_hole_counts: *const usize,
    target_holes_flat: *const crate::dupefallback::NzbgetRsByteRange,
    target_count: usize,
    donor_info_name: *const c_char,
    donor_file_names: *const *const c_char,
    donor_file_sizes: *const i64,
    donor_file_is_par: *const u8,
    donor_file_steps_hashes: *const u64,
    donor_article_counts: *const usize,
    donor_article_sizes_flat: *const i32,
    donor_file_count: usize,
    ctx: *mut std::ffi::c_void,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    refresh_live_name_cb: Option<
        unsafe extern "C" fn(*mut std::ffi::c_void, usize) -> *const c_char,
    >,
    begin_member_batch_cb: DonorBeginMemberBatchFn,
    next_article_cb: DonorNextArticleFn,
    cancel_batch_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    add_progress_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64)>,
    add_recovered_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64)>,
    sync_target_cb: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            usize,
            *const c_char,
            i64,
            c_int,
            *const crate::dupefallback::NzbgetRsByteRange,
            usize,
        ),
    >,
    emit_message_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
) {
    let mut member_names_vec = Vec::with_capacity(member_count);
    if !member_names.is_null() && member_count > 0 {
        for &ptr in std::slice::from_raw_parts(member_names, member_count) {
            member_names_vec.push(input(ptr));
        }
    }
    let mut targets_vec = Vec::with_capacity(target_count);
    if target_count > 0
        && !target_filenames.is_null()
        && !target_encoded_sizes.is_null()
        && !target_decoded_sizes.is_null()
        && !target_article_counts.is_null()
        && !target_positional_ranks.is_null()
        && !target_positional_windows.is_null()
        && !target_steps_hashes.is_null()
        && !target_patch_eligible.is_null()
        && !target_hole_counts.is_null()
    {
        let fnames = std::slice::from_raw_parts(target_filenames, target_count);
        let esizes = std::slice::from_raw_parts(target_encoded_sizes, target_count);
        let dsizes = std::slice::from_raw_parts(target_decoded_sizes, target_count);
        let acounts = std::slice::from_raw_parts(target_article_counts, target_count);
        let pranks = std::slice::from_raw_parts(target_positional_ranks, target_count);
        let pwindows = std::slice::from_raw_parts(target_positional_windows, target_count);
        let shashes = std::slice::from_raw_parts(target_steps_hashes, target_count);
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
            targets_vec.push(crate::streamrepair::M1RepairTarget {
                filename: input(fnames[t]).to_vec(),
                decoded_file_size: dsizes[t],
                holes: flat[cursor..end].to_vec(),
                encoded_size: esizes[t],
                article_count: acounts[t],
                recreated: false,
                donor_set: Vec::new(),
                patch_eligible: peligible[t] != 0,
                positional_rank: pranks[t],
                positional_window: pwindows[t],
                steps_hash: shashes[t],
            });
            cursor = end;
        }
    }

    let mut donor_names_vec = Vec::with_capacity(donor_file_count);
    if !donor_file_names.is_null() && donor_file_count > 0 {
        for &ptr in std::slice::from_raw_parts(donor_file_names, donor_file_count) {
            donor_names_vec.push(input(ptr));
        }
    }
    let donor_sizes_slice = if donor_file_sizes.is_null() || donor_file_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(donor_file_sizes, donor_file_count)
    };
    let donor_is_par_slice = if donor_file_is_par.is_null() || donor_file_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(donor_file_is_par, donor_file_count)
    };
    let donor_hashes_slice = if donor_file_steps_hashes.is_null() || donor_file_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(donor_file_steps_hashes, donor_file_count)
    };
    let mut donor_articles_vec = Vec::with_capacity(donor_file_count);
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
            donor_articles_vec.push(flat[cursor..end].to_vec());
            cursor = end;
        }
    }

    let mut env = FfiM1DonorEnv {
        ctx,
        is_stopped_cb,
        refresh_live_name_cb,
        begin_member_batch_cb,
        next_article_cb,
        cancel_batch_cb,
        add_progress_cb,
        add_recovered_cb,
        emit_message_cb,
    };
    crate::streamrepair::process_m1_donor(
        input(dest_dir),
        live_mode != 0,
        &mut targets_vec,
        &member_names_vec,
        input(donor_info_name),
        &donor_names_vec,
        donor_sizes_slice,
        donor_is_par_slice,
        donor_hashes_slice,
        &donor_articles_vec,
        &mut env,
    );
    if let Some(cb) = sync_target_cb {
        for (idx, t) in targets_vec.iter().enumerate() {
            let mut fn_buf: Vec<u8> = t.filename.iter().copied().filter(|&b| b != 0).collect();
            fn_buf.push(0);
            cb(
                ctx,
                idx,
                fn_buf.as_ptr().cast::<c_char>(),
                t.decoded_file_size,
                c_int::from(t.recreated),
                t.holes.as_ptr(),
                t.holes.len(),
            );
        }
    }
}

pub struct NzbgetRsCrossPackSession(crate::streamrepair::CrossPackRepairSession);

type DonorFetchMemberPartFn = Option<
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

    fn sync_target_holes(
        &mut self,
        target_holes: &[Vec<crate::dupefallback::NzbgetRsByteRange>],
    ) {
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
            cb(
                self.ctx,
                counts.as_ptr(),
                flat.as_ptr(),
                counts.len(),
            );
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
        for mut line in crate::streamrepair::format_rar_volume_debug_lines(member_name, vol, files) {
            line.push(0);
            unsafe { cb(self.ctx, line.as_ptr() as *const c_char) };
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cross_pack_session_new(
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

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cross_pack_session_free(
    session: *mut NzbgetRsCrossPackSession,
) {
    if !session.is_null() {
        drop(Box::from_raw(session));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cross_pack_session_has_inner_holes(
    session: *const NzbgetRsCrossPackSession,
) -> c_int {
    let Some(s) = session.as_ref() else {
        return 0;
    };
    c_int::from(s.0.has_inner_holes())
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cross_pack_session_process_donor(
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
pub unsafe extern "C" fn nzbget_rs_maintenance_read_package_info_str(
    web_dir: *const c_char,
    sep: c_char,
    key: *const c_char,
    out_err_kind: *mut c_int,
    out_file_name: *mut RsBuf,
) -> RsBuf {
    match crate::extension::maintenance_read_package_info_str(
        input(web_dir),
        sep as u8,
        input(key),
    ) {
        Ok(val) => {
            if !out_err_kind.is_null() {
                *out_err_kind = 0;
            }
            if !out_file_name.is_null() {
                *out_file_name = into_buf(Vec::new());
            }
            into_buf(val)
        }
        Err((err_kind, file_name)) => {
            if !out_err_kind.is_null() {
                *out_err_kind = err_kind;
            }
            if !out_file_name.is_null() {
                *out_file_name = into_buf(file_name);
            }
            into_buf(Vec::new())
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_maintenance_prepare_signature_inputs(
    in_filename: *const c_char,
    sig_filename: *const c_char,
    pub_key_filename: *const c_char,
    out_in_hash: *mut u8,
    out_signature: *mut u8,
    out_pub_key: *mut RsBuf,
) -> c_int {
    match crate::extension::maintenance_prepare_signature_inputs(
        input(in_filename),
        input(sig_filename),
        input(pub_key_filename),
    ) {
        Some((hash, sig, pub_key)) => {
            if !out_in_hash.is_null() {
                std::ptr::copy_nonoverlapping(hash.as_ptr(), out_in_hash, 32);
            }
            if !out_signature.is_null() {
                std::ptr::copy_nonoverlapping(sig.as_ptr(), out_signature, 256);
            }
            if !out_pub_key.is_null() {
                *out_pub_key = into_buf(pub_key);
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ncurses_set_current_queue_entry(
    entry: c_int,
    queue_size: c_int,
    queue_win_client_height: c_int,
    inout_scroll_offset: *mut c_int,
    out_selected_entry: *mut c_int,
) {
    let scroll_in = if inout_scroll_offset.is_null() {
        0
    } else {
        *inout_scroll_offset
    };
    let (sel, scroll_out) = crate::ncurses_format::ncurses_set_current_queue_entry(
        entry,
        queue_size,
        scroll_in,
        queue_win_client_height,
    );
    if !inout_scroll_offset.is_null() {
        *inout_scroll_offset = scroll_out;
    }
    if !out_selected_entry.is_null() {
        *out_selected_entry = sel;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ncurses_handle_key(
    state: *mut crate::ncurses_format::NcursesKeyState,
    key: c_int,
    queue_size: c_int,
) {
    if !state.is_null() {
        crate::ncurses_format::ncurses_handle_key(&mut *state, key, queue_size);
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
        crate::downloadinfo::calc_health(size, par_size, current_failed_size, par_current_failed_size)
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
    c_int::from(crate::downloadinfo::completed_same_filename(input(a), input(b)))
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
pub unsafe extern "C" fn nzbget_rs_term_plot_line(
    text: *const c_char,
    row: c_int,
    pos: c_int,
    screen_width: c_int,
    use_color: c_int,
    color_pair: c_int,
) {
    crate::ncurses_format::term_plot_line(
        input(text),
        row,
        pos,
        screen_width,
        use_color != 0,
        color_pair,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ncurses_calc_window_sizes(
    queue_size: c_int,
    queue_window_percentage: c_int,
    inout_screen_height: *mut c_int,
    inout_screen_width: *mut c_int,
    out_queue_win_top: *mut c_int,
    out_queue_win_height: *mut c_int,
    out_queue_win_client_height: *mut c_int,
    out_messages_win_top: *mut c_int,
    out_messages_win_height: *mut c_int,
    out_messages_win_client_height: *mut c_int,
) {
    crate::ncurses_format::ncurses_calc_window_sizes(
        queue_size,
        queue_window_percentage,
        &mut *inout_screen_height,
        &mut *inout_screen_width,
        &mut *out_queue_win_top,
        &mut *out_queue_win_height,
        &mut *out_queue_win_client_height,
        &mut *out_messages_win_top,
        &mut *out_messages_win_height,
        &mut *out_messages_win_client_height,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ncurses_print_message(
    msg_kind: c_int,
    msg_time: i64,
    msg_text: *const c_char,
    row: c_int,
    max_lines: c_int,
    show_timestamp: c_int,
    time_correction: c_int,
    screen_width: c_int,
    use_color: c_int,
) -> c_int {
    crate::ncurses_format::ncurses_print_message(
        msg_kind,
        msg_time,
        input(msg_text),
        row,
        max_lines,
        show_timestamp != 0,
        time_correction,
        screen_width,
        use_color != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ncurses_print_key_input_bar(
    screen_height: c_int,
    screen_width: c_int,
    use_color: c_int,
    input_mode: c_int,
    group_files: c_int,
    selected_queue_entry: c_int,
    queue_size: c_int,
    input_value: c_int,
    input_number_index: c_int,
    hint: *const c_char,
    inout_start_hint: *mut i64,
    cur_time: i64,
) {
    crate::ncurses_format::ncurses_print_key_input_bar(
        screen_height,
        screen_width,
        use_color != 0,
        input_mode,
        group_files != 0,
        selected_queue_entry,
        queue_size,
        input_value,
        input_number_index,
        input(hint),
        &mut *inout_start_hint,
        cur_time,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ncurses_resolve_group_edit_action(
    action: c_int,
    has_group: c_int,
    remaining_size: i64,
    paused_size: i64,
    remaining_par_count: c_int,
    selected_entry: c_int,
    last_edit_entry: c_int,
    inout_last_pause_pars: *mut c_int,
) -> c_int {
    let mut pause_pars = if inout_last_pause_pars.is_null() {
        false
    } else {
        *inout_last_pause_pars != 0
    };
    let out = crate::ncurses_format::ncurses_resolve_group_edit_action(
        action,
        has_group != 0,
        remaining_size,
        paused_size,
        remaining_par_count,
        selected_entry,
        last_edit_entry,
        &mut pause_pars,
    );
    if !inout_last_pause_pars.is_null() {
        *inout_last_pause_pars = c_int::from(pause_pars);
    }
    out
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
                match crate::fsops::move_file_no_overwrite(
                    CStr::from_ptr(p1),
                    CStr::from_ptr(p2),
                ) {
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
pub unsafe extern "C" fn nzbget_rs_binrpc_init_message_base(
    out: *mut u8,
    request_type: c_int,
    struct_size: c_int,
    username: *const c_char,
    password: *const c_char,
) {
    if out.is_null() {
        return;
    }
    let buf: &mut [u8; crate::binrpc::BASE_SIZE] =
        std::slice::from_raw_parts_mut(out, crate::binrpc::BASE_SIZE)
            .try_into()
            .unwrap();
    crate::binrpc::init_message_base(buf, request_type, struct_size, input(username), input(password));
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_init_download_request(
    out: *mut u8,
    username: *const c_char,
    password: *const c_char,
    nzb_filename: *const c_char,
    nzb_content: *const c_char,
    is_url: c_int,
    category: *const c_char,
    add_first: c_int,
    add_paused: c_int,
    priority: c_int,
    dupe_score: c_int,
    dupe_mode: c_int,
    dupe_key: *const c_char,
    trailing_len: c_int,
    auto_category: c_int,
) {
    if out.is_null() {
        return;
    }
    let buf: &mut [u8; crate::binrpc::DOWNLOAD_REQUEST_SIZE] =
        std::slice::from_raw_parts_mut(out, crate::binrpc::DOWNLOAD_REQUEST_SIZE)
            .try_into()
            .unwrap();
    crate::binrpc::init_download_request(
        buf,
        input(username),
        input(password),
        input(nzb_filename),
        input(nzb_content),
        is_url != 0,
        input(category),
        add_first != 0,
        add_paused != 0,
        priority,
        dupe_score,
        dupe_mode,
        input(dupe_key),
        trailing_len,
        auto_category != 0,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_edit_parse(
    data: *const u8,
    data_len: u32,
    nr_ids: c_int,
    nr_names: c_int,
    names_len: c_int,
    text_len: c_int,
    out_ids: *mut i32,
) -> c_int {
    if !crate::binrpc::edit_sizes_ok(nr_ids, nr_names, names_len, text_len, data_len) {
        return 0;
    }
    if data.is_null() {
        return 1;
    }
    let slice = std::slice::from_raw_parts(data, data_len as usize);
    let ids_slice = if nr_ids <= 0 || out_ids.is_null() {
        &mut [][..]
    } else {
        std::slice::from_raw_parts_mut(out_ids.cast::<u32>(), nr_ids as usize)
    };
    c_int::from(crate::binrpc::edit_split(
        slice,
        text_len as usize,
        ids_slice,
        nr_names.max(0) as usize,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_calc_log_slice(
    msg_count: c_int,
    lines: c_int,
    id_from: u32,
    first_msg_id: u32,
    out_start: *mut c_int,
    out_nr_entries: *mut c_int,
) {
    let (start, nr_entries) =
        crate::binrpc::calc_log_slice(msg_count, lines, id_from, first_msg_id);
    if !out_start.is_null() {
        *out_start = start;
    }
    if !out_nr_entries.is_null() {
        *out_nr_entries = nr_entries;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_web_parse_request_or_mime(
    sig_or_filename: *const c_char,
    line: *mut c_char,
    out_method: *mut c_int,
    out_url_offset: *mut usize,
    out_mime: *mut *const c_char,
) -> c_int {
    if line.is_null() {
        if sig_or_filename.is_null() {
            if !out_mime.is_null() {
                *out_mime = std::ptr::null();
            }
            return 0;
        }
        if let Some(mime) = crate::binrpc::detect_content_type(input(sig_or_filename)) {
            if !out_mime.is_null() {
                *out_mime = mime.as_ptr();
            }
            1
        } else {
            if !out_mime.is_null() {
                *out_mime = std::ptr::null();
            }
            0
        }
    } else {
        if sig_or_filename.is_null() {
            return 0;
        }
        let sig = std::slice::from_raw_parts(sig_or_filename.cast::<u8>(), 4);
        let len = CStr::from_ptr(line).to_bytes_with_nul().len();
        let line_slice = std::slice::from_raw_parts_mut(line.cast::<u8>(), len);
        if let Some((method, offset)) = crate::binrpc::parse_web_request_line(sig, line_slice) {
            if !out_method.is_null() {
                *out_method = method;
            }
            if !out_url_offset.is_null() {
                *out_url_offset = offset;
            }
            1
        } else {
            0
        }
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
pub unsafe extern "C" fn nzbget_rs_stream_repair_compute_holes(
    covered: *const crate::dupefallback::NzbgetRsByteRange,
    count: usize,
    file_size: i64,
    out_holes: *mut crate::dupefallback::NzbgetRsByteRange,
    max_holes: usize,
) -> usize {
    let slice = if covered.is_null() || count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(covered, count)
    };
    let holes = crate::streamrepair::compute_holes(slice, file_size);
    let n = holes.len().min(max_holes);
    if !out_holes.is_null() && n > 0 {
        std::slice::from_raw_parts_mut(out_holes, n).copy_from_slice(&holes[..n]);
    }
    n
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_queue_coord_is_data_filename(filename: *const c_char) -> c_int {
    if filename.is_null() {
        return 0;
    }
    c_int::from(crate::queuecoord::is_data_filename(input(filename)))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_diskstate_parse_unpack_state(
    line1: *const c_char,
    line2: *const c_char,
    line3: *const c_char,
    ns_running: c_int,
    ns_none: c_int,
    ctx: *mut c_void,
    on_parsed_and_read_archives_cb: Option<
        unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int) -> *const c_char,
    >,
    add_archive_cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char, usize)>,
) -> c_int {
    let l1 = crate::diskstate_format::cstr_to_bytes(line1);
    let l2 = crate::diskstate_format::cstr_to_bytes(line2);
    let l3 = crate::diskstate_format::cstr_to_bytes(line3);
    let Some((direct, tried, repaired, last)) =
        crate::diskstate_format::parse_unpack_state_lines(l1, l2, l3, ns_running, ns_none)
    else {
        return 0;
    };
    let Some(read_cb) = on_parsed_and_read_archives_cb else {
        return 0;
    };
    let line4 = read_cb(
        ctx,
        direct,
        c_int::from(tried),
        c_int::from(repaired),
        last,
    );
    if line4.is_null() {
        return 0;
    }
    if let Some(cb) = add_archive_cb {
        let l4 = crate::diskstate_format::cstr_to_bytes(line4);
        let archives = l4.strip_prefix(b"ExtractedArchives=").unwrap_or(l4);
        let mut cursor = 0;
        while let Some((start, len, next)) =
            crate::diskstate_format::diskstate_next_comma_token(archives, cursor)
        {
            cb(ctx, archives.as_ptr().add(start) as *const c_char, len);
            cursor = next;
        }
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_is_par_file(
    par_file: c_int,
    first_content: c_int,
    fc_par2: c_int,
    filename_confirmed: c_int,
    filename: *const c_char,
) -> c_int {
    let bytes = if filename.is_null() {
        &[][..]
    } else {
        input(filename)
    };
    c_int::from(crate::dupefallback::is_par_file(
        par_file != 0,
        first_content,
        fc_par2,
        filename_confirmed != 0,
        bytes,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_par_cannot_cover(
    par_size: i64,
    par_failed_size: i64,
    current_failed_size: i64,
    current_success_size: i64,
    par_current_success_size: i64,
    total_size: i64,
    current_success_articles: c_int,
    current_failed_articles: c_int,
    with_margin: c_int,
) -> c_int {
    c_int::from(crate::dupefallback::par_cannot_cover(
        par_size,
        par_failed_size,
        current_failed_size,
        current_success_size,
        par_current_success_size,
        total_size,
        current_success_articles,
        current_failed_articles,
        with_margin != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_check_try_preconditions(
    first_content: c_int,
    fc_par2: c_int,
    fc_unknown: c_int,
    part_number: c_int,
    filename: *const c_char,
    file_deleted: c_int,
    nzb_deleting: c_int,
    nzb_parking: c_int,
    delete_status: c_int,
    ds_none: c_int,
    dupe_mode: c_int,
    dm_force: c_int,
) -> c_int {
    let bytes = if filename.is_null() {
        &[][..]
    } else {
        input(filename)
    };
    c_int::from(crate::dupefallback::check_try_preconditions(
        1,
        0,
        false,
        false,
        first_content,
        fc_par2,
        fc_unknown,
        part_number,
        bytes,
        file_deleted != 0,
        nzb_deleting != 0,
        nzb_parking != 0,
        delete_status,
        ds_none,
        dupe_mode,
        dm_force,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_compute_pin_lead_state(
    contributors: *const c_int,
    count: usize,
    cur_file_lead_donor_id: c_int,
    out_lead_snapshot: *mut c_int,
    out_next_lead: *mut c_int,
    out_donor_count: *mut c_int,
    out_new_file_lead: *mut c_int,
) {
    let slice = if contributors.is_null() || count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(contributors, count)
    };
    let (snap, next, dcount, new_lead) =
        crate::dupefallback::compute_pin_lead_state(slice, cur_file_lead_donor_id);
    if !out_lead_snapshot.is_null() {
        *out_lead_snapshot = snap;
    }
    if !out_next_lead.is_null() {
        *out_next_lead = next;
    }
    if !out_donor_count.is_null() {
        *out_donor_count = dcount;
    }
    if !out_new_file_lead.is_null() {
        *out_new_file_lead = new_lead;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_register_lead_failure(
    fallback_round: c_int,
    lead_snapshot: c_int,
    donor_count: c_int,
    next_lead: c_int,
    inout_lead_donor_id: *mut c_int,
    inout_lead_failures: *mut c_int,
    inout_lead_switches: *mut c_int,
) -> c_int {
    if inout_lead_donor_id.is_null()
        || inout_lead_failures.is_null()
        || inout_lead_switches.is_null()
    {
        return 0;
    }
    c_int::from(crate::dupefallback::register_lead_failure(
        fallback_round,
        lead_snapshot,
        donor_count,
        next_lead,
        &mut *inout_lead_donor_id,
        &mut *inout_lead_failures,
        &mut *inout_lead_switches,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_register_lead_success(
    fallback_round: c_int,
    lead_snapshot: c_int,
    lead_donor_id: c_int,
    expected_offset: i64,
    expected_end: i64,
    inout_lead_failures: *mut c_int,
    inout_lead_switches: *mut c_int,
) {
    if inout_lead_failures.is_null() || inout_lead_switches.is_null() {
        return;
    }
    crate::dupefallback::register_lead_success(
        fallback_round,
        lead_snapshot,
        lead_donor_id,
        expected_offset,
        expected_end,
        &mut *inout_lead_failures,
        &mut *inout_lead_switches,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_first_untiled_article(
    articles: *const crate::dupefallback::NzbgetRsArticleGeom,
    count: usize,
    decoded_size: i64,
) -> isize {
    let slice = if articles.is_null() || count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(articles, count)
    };
    crate::dupefallback::first_untiled_article(slice, decoded_size)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_merge_decoded_file_size(
    inout_decoded_file_size: *mut i64,
    article_file_size: i64,
) -> c_int {
    if inout_decoded_file_size.is_null() {
        return 0;
    }
    let (conflict, next_size) =
        crate::dupefallback::merge_decoded_file_size(*inout_decoded_file_size, article_file_size);
    *inout_decoded_file_size = next_size;
    c_int::from(conflict)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_classify_first_head(
    head: *const u8,
    len: usize,
) -> c_int {
    let slice = if head.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(head, len)
    };
    crate::dupefallback::classify_first_head(slice)
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

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_scanner_prepare_external_name(
    nzb_name: *const c_char,
    is_from_disk: c_int,
    buf_ptr: *const c_char,
    buf_size: c_int,
    inout_is_nzb: *mut c_int,
) -> RsBuf {
    let mut is_nzb = if !inout_is_nzb.is_null() {
        *inout_is_nzb != 0
    } else {
        false
    };
    if is_from_disk == 0 {
        let slice = if buf_ptr.is_null() || buf_size <= 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(buf_ptr as *const u8, buf_size as usize)
        };
        is_nzb = crate::scanner::is_nzb_xml_prefix(slice);
        if !inout_is_nzb.is_null() {
            *inout_is_nzb = c_int::from(is_nzb);
        }
    }
    let name_bytes = if nzb_name.is_null() {
        &[][..]
    } else {
        input(nzb_name)
    };
    let raw_for_valid = if crate::scanner::is_absolute_nzb_name(name_bytes) {
        &name_bytes[crate::paths::base_file_name(name_bytes)..]
    } else {
        name_bytes
    };
    let valid = crate::paths::make_valid_filename(raw_for_valid, false);
    into_buf(crate::scanner::normalize_external_nzb_name(
        name_bytes, &valid, is_nzb,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_summary_fingerprint(
    ids_ptr: *const *const c_char,
    ids_lens: *const usize,
    count: usize,
    out_hex16: *mut c_char,
) {
    let mut slices: Vec<&[u8]> = Vec::with_capacity(count);
    if !ids_ptr.is_null() && !ids_lens.is_null() {
        for i in 0..count {
            let p = *ids_ptr.add(i);
            let l = *ids_lens.add(i);
            if p.is_null() || l == 0 {
                slices.push(&[]);
            } else {
                slices.push(std::slice::from_raw_parts(p as *const u8, l));
            }
        }
    }
    let hex = crate::dupesearch_nzb::fnv1a_fingerprint(&slices);
    if !out_hex16.is_null() {
        std::ptr::copy_nonoverlapping(hex.as_ptr(), out_hex16 as *mut u8, 16);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_newznab_mask(text: *const c_char, len: usize) -> RsBuf {
    let slice = if text.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(text as *const u8, len)
    };
    into_buf(crate::dupesearch_nzb::mask_newznab_text(slice))
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NzbgetRsDonorScoreEntry {
    pub score: c_int,
    pub alive: f64,
    pub twin: c_int,
    pub bytes: i64,
    pub grabs: c_int,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_score_calc(
    op: c_int,
    pick_score: c_int,
    alive: f64,
    twin: c_int,
    used_ptr: *const c_int,
    used_len: usize,
) -> c_int {
    match op {
        0 => crate::donorscore::base(pick_score),
        1 => crate::donorscore::target(alive, twin != 0),
        2 => {
            let used = if used_ptr.is_null() || used_len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(used_ptr, used_len)
            };
            crate::donorscore::take_rank(alive, twin != 0, used)
        }
        _ => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donor_score_rerank(
    entries: *const NzbgetRsDonorScoreEntry,
    count: usize,
    pick_bytes: i64,
    out_scores: *mut c_int,
) {
    if entries.is_null() || out_scores.is_null() || count == 0 {
        return;
    }
    let slice = std::slice::from_raw_parts(entries, count);
    let rs_entries: Vec<crate::donorscore::Entry> = slice
        .iter()
        .map(|e| crate::donorscore::Entry {
            score: e.score,
            alive: e.alive,
            twin: e.twin != 0,
            bytes: e.bytes,
            grabs: e.grabs,
        })
        .collect();
    let scores = crate::donorscore::rerank(&rs_entries, pick_bytes);
    std::slice::from_raw_parts_mut(out_scores, count).copy_from_slice(&scores);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupesearch_parse_pending_meta(
    meta_ptr: *const c_char,
    meta_len: usize,
    out_title_off: *mut usize,
    out_title_len: *mut usize,
    out_indexer_off: *mut usize,
    out_indexer_len: *mut usize,
    out_grabs: *mut c_int,
    out_size: *mut i64,
    out_date: *mut i64,
) {
    let slice = if meta_ptr.is_null() || meta_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(meta_ptr as *const u8, meta_len)
    };
    let r = crate::dupesearch_nzb::parse_pending_meta(slice);
    if !out_title_off.is_null() {
        *out_title_off = r.title_offset;
    }
    if !out_title_len.is_null() {
        *out_title_len = r.title_len;
    }
    if !out_indexer_off.is_null() {
        *out_indexer_off = r.indexer_offset;
    }
    if !out_indexer_len.is_null() {
        *out_indexer_len = r.indexer_len;
    }
    if !out_grabs.is_null() {
        *out_grabs = r.grabs;
    }
    if !out_size.is_null() {
        *out_size = r.size;
    }
    if !out_date.is_null() {
        *out_date = r.date;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupesearch_parse_state_line(
    line_ptr: *const c_char,
    line_len: usize,
    now: i64,
    ttl_sec: i64,
    out_key_off: *mut usize,
    out_key_len: *mut usize,
    out_score: *mut c_int,
    out_time: *mut i64,
    out_title_off: *mut usize,
    out_title_len: *mut usize,
    out_retryable: *mut c_int,
) -> c_int {
    let slice = if line_ptr.is_null() || line_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(line_ptr as *const u8, line_len)
    };
    let r = crate::dupesearch_nzb::parse_dupesearch_state_line(slice, now, ttl_sec);
    if r.valid == 0 || r.within_ttl == 0 {
        return 0;
    }
    if !out_key_off.is_null() {
        *out_key_off = r.key_offset;
    }
    if !out_key_len.is_null() {
        *out_key_len = r.key_len;
    }
    if !out_score.is_null() {
        *out_score = r.score;
    }
    if !out_time.is_null() {
        *out_time = r.time;
    }
    if !out_title_off.is_null() {
        *out_title_off = r.title_offset;
    }
    if !out_title_len.is_null() {
        *out_title_len = r.title_len;
    }
    if !out_retryable.is_null() {
        *out_retryable = r.retryable;
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_url_decode_path_in_place(raw: *mut c_char) {
    if raw.is_null() {
        return;
    }
    let total_len = std::ffi::CStr::from_ptr(raw).to_bytes().len();
    let buf = std::slice::from_raw_parts_mut(raw as *mut u8, total_len + 1);
    let q = buf[..total_len]
        .iter()
        .position(|&b| b == b'?')
        .unwrap_or(total_len);
    let dec_len = crate::text::url_decode(&mut buf[..q]);
    let tail_len = total_len - q;
    if tail_len > 0 && dec_len != q {
        buf.copy_within(q..total_len, dec_len);
    }
    buf[dec_len + tail_len] = 0;
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_web_classify_url(op: c_int, s: *const c_char) -> c_int {
    let bytes = if s.is_null() { &[][..] } else { input(s) };
    match op {
        0 => crate::webserver::web_classify_get_dispatch(bytes),
        1 => c_int::from(crate::webserver::web_is_ignorable_404(bytes)),
        _ => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_web_format_response(
    kind: c_int,
    form_auth: c_int,
    keep_alive: c_int,
    unchanged: c_int,
    gzip: c_int,
    body_len: c_int,
    str_arg: *const c_char,
    origin: *const c_char,
    auth_token: *const c_char,
    content_type: *const c_char,
    etag: *const c_char,
    version: *const c_char,
    out_body: *mut RsBuf,
) -> RsBuf {
    let ver = if version.is_null() {
        &[][..]
    } else {
        input(version)
    };
    let orig = if origin.is_null() {
        &[][..]
    } else {
        input(origin)
    };
    let s_arg = if str_arg.is_null() {
        &[][..]
    } else {
        input(str_arg)
    };
    match kind {
        0 => into_buf(crate::webserver::format_auth_response(
            form_auth != 0,
            keep_alive != 0,
            ver,
        )),
        1 => into_buf(crate::webserver::format_options_response(
            keep_alive != 0,
            orig,
            ver,
        )),
        2 => {
            let (hdr, body) =
                crate::webserver::format_error_response(s_arg, keep_alive != 0, ver);
            if !out_body.is_null() {
                *out_body = into_buf(body);
            }
            into_buf(hdr)
        }
        3 => into_buf(crate::webserver::format_redirect_response(
            s_arg,
            keep_alive != 0,
            ver,
        )),
        4 => {
            let tok = if auth_token.is_null() {
                &[][..]
            } else {
                input(auth_token)
            };
            let ct = if content_type.is_null() {
                None
            } else {
                Some(input(content_type))
            };
            let et = if etag.is_null() {
                None
            } else {
                Some(input(etag))
            };
            into_buf(crate::webserver::format_body_response_header(
                unchanged != 0,
                keep_alive != 0,
                orig,
                form_auth != 0,
                tok,
                body_len,
                ct,
                gzip != 0,
                et,
                ver,
            ))
        }
        _ => RsBuf {
            data: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        },
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct RsServerMatchItem {
    pub name: *const c_char,
    pub host: *const c_char,
    pub port: c_int,
    pub user: *const c_char,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_classify_temp_or_legacy(
    op: c_int,
    filename: *const c_char,
    header: *const c_char,
    file_ver: c_int,
    legacy_ver: c_int,
    out_a: *mut c_int,
    out_b: *mut c_int,
) -> c_int {
    let fname = if filename.is_null() {
        &[][..]
    } else {
        input(filename)
    };
    match op {
        0 => {
            let (kind, id, part) = crate::diskstate_format::classify_temp_file(fname);
            if !out_a.is_null() {
                *out_a = id;
            }
            if !out_b.is_null() {
                *out_b = part;
            }
            kind
        }
        1 => match crate::diskstate_format::classify_queue_dir_id(fname) {
            Some((id, is_nzb_log)) => {
                if !out_a.is_null() {
                    *out_a = id;
                }
                if !out_b.is_null() {
                    *out_b = c_int::from(is_nzb_log);
                }
                1
            }
            None => 0,
        },
        2 => {
            let hdr = if header.is_null() {
                &[][..]
            } else {
                input(header)
            };
            match crate::diskstate_format::classify_legacy_file_state(
                fname,
                hdr,
                file_ver,
                legacy_ver,
            ) {
                Some((id, suffix_kind)) => {
                    if !out_a.is_null() {
                        *out_a = id;
                    }
                    if !out_b.is_null() {
                        *out_b = suffix_kind;
                    }
                    1
                }
                None => 0,
            }
        }
        3 => {
            let hdr = if header.is_null() {
                &[][..]
            } else {
                input(header)
            };
            crate::diskstate_format::parse_format_version(hdr)
        }
        4 => {
            let (err, is_legacy, need_inline_history) =
                crate::diskstate_format::check_queue_format_version(file_ver, legacy_ver);
            if !out_a.is_null() {
                *out_a = c_int::from(is_legacy);
            }
            if !out_b.is_null() {
                *out_b = c_int::from(need_inline_history);
            }
            err
        }
        5 => match crate::diskstate_format::diskstate_parse_queue_file_id_and_suffix(fname) {
            Some((id, suffix)) => {
                if !out_a.is_null() {
                    *out_a = id;
                }
                if !out_b.is_null() {
                    *out_b = c_int::from(suffix);
                }
                1
            }
            None => 0,
        },
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_parse_log_line(
    line: *const c_char,
    out_time: *mut c_int,
    out_kind: *mut c_int,
    out_text_off: *mut usize,
) -> c_int {
    if line.is_null() {
        return 0;
    }
    match crate::diskstate_format::parse_log_line(input(line)) {
        Some((time, kind, off)) => {
            if !out_time.is_null() {
                *out_time = time;
            }
            if !out_kind.is_null() {
                *out_kind = kind;
            }
            if !out_text_off.is_null() {
                *out_text_off = off;
            }
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_is_orphan_queue_file(
    filename: *const c_char,
    file_ids: *const c_int,
    file_len: usize,
    nzb_ids: *const c_int,
    nzb_len: usize,
) -> c_int {
    if filename.is_null() {
        return 0;
    }
    let f_slice = if file_ids.is_null() || file_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(file_ids, file_len)
    };
    let n_slice = if nzb_ids.is_null() || nzb_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(nzb_ids, nzb_len)
    };
    c_int::from(crate::diskstate_format::is_orphan_queue_file(
        input(filename),
        f_slice,
        n_slice,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_load_nzb_info(
    infile_ctx: *mut c_void,
    read_line_cb: crate::diskstate_format::ReadLineFn,
    format_version: c_int,
    pt_finished: c_int,
    target_ctx: *mut c_void,
    on_loaded_cb: unsafe extern "C" fn(
        ctx: *mut c_void,
        inp: *const crate::diskstate_format::NzbgetRsDiskNzbInfoInput,
        flags: c_int,
        completed: *const crate::diskstate_format::NzbgetRsDiskCompletedFile,
        completed_len: usize,
        params: *const crate::diskstate_format::NzbgetRsDiskParam,
        params_len: usize,
        scripts: *const crate::diskstate_format::NzbgetRsDiskScriptStatus,
        scripts_len: usize,
        stats: *const crate::diskstate_format::NzbgetRsDiskServerStat,
        stats_len: usize,
        files: *const crate::diskstate_format::NzbgetRsDiskFileStub,
        file_times: *const c_int,
        files_len: usize,
    ),
) -> c_int {
    c_int::from(crate::diskstate_format::load_nzb_info(
        infile_ctx,
        read_line_cb,
        format_version,
        pt_finished,
        target_ctx,
        on_loaded_cb,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_load_file_state(
    infile_ctx: *mut c_void,
    read_line_cb: crate::diskstate_format::ReadLineFn,
    format_version: c_int,
    completed: c_int,
    file_id: c_int,
    has_articles: c_int,
    articles_len: c_int,
    has_options: c_int,
    direct_write: c_int,
    force_direct_write: c_int,
    cur_output_initialized: c_int,
    sep: c_int,
    temp_dir: *const c_char,
    dest_dir: *const c_char,
    cur_filename: *const c_char,
    cur_output_filename: *const c_char,
    target_ctx: *mut c_void,
    article_cb: unsafe extern "C" fn(
        ctx: *mut c_void,
        op: c_int,
        idx: c_int,
        status: c_int,
        seg_offset: i64,
        seg_size: c_int,
        crc: u32,
        fallback_round: c_int,
        result_filename: *const c_char,
    ) -> c_int,
    on_summary_cb: unsafe extern "C" fn(
        ctx: *mut c_void,
        fields: *const crate::diskstate_format::NzbgetRsDiskFileStateInput,
        flags: c_int,
        completed_articles: c_int,
        new_output_filename: *const c_char,
        stats: *const crate::diskstate_format::NzbgetRsDiskServerStat,
        stats_len: usize,
    ),
) -> c_int {
    let tdir = if temp_dir.is_null() { &[][..] } else { input(temp_dir) };
    let ddir = if dest_dir.is_null() { &[][..] } else { input(dest_dir) };
    let cfn = if cur_filename.is_null() { &[][..] } else { input(cur_filename) };
    let cofn = if cur_output_filename.is_null() {
        &[][..]
    } else {
        input(cur_output_filename)
    };
    c_int::from(crate::diskstate_format::load_file_state(
        infile_ctx,
        read_line_cb,
        format_version,
        completed,
        file_id,
        has_articles,
        articles_len,
        has_options,
        direct_write,
        force_direct_write,
        cur_output_initialized,
        sep as u8,
        tdir,
        ddir,
        cfn,
        cofn,
        target_ctx,
        article_cb,
        on_summary_cb,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_match_servers(
    servers: *const RsServerMatchItem,
    server_count: usize,
    infile_ctx: *mut c_void,
    read_line_cb: crate::diskstate_format::ReadLineFn,
    out_state_ids: *mut c_int,
    out_perfect_match: *mut c_int,
) -> c_int {
    let srv_slice = if servers.is_null() || server_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(servers, server_count)
    };
    let mut srv_inputs = Vec::with_capacity(srv_slice.len());
    for s in srv_slice {
        srv_inputs.push(crate::diskstate_format::ServerMatchInput {
            name: if s.name.is_null() { &[][..] } else { input(s.name) },
            host: if s.host.is_null() { &[][..] } else { input(s.host) },
            port: s.port,
            user: if s.user.is_null() { &[][..] } else { input(s.user) },
        });
    }
    let mut state_ids = vec![0i32; srv_inputs.len()];
    let Some(perfect) = crate::diskstate_format::load_server_refs_and_match(
        &srv_inputs,
        infile_ctx,
        read_line_cb,
        &mut state_ids,
    ) else {
        return 0;
    };
    if !out_state_ids.is_null() && !state_ids.is_empty() {
        std::slice::from_raw_parts_mut(out_state_ids, state_ids.len())
            .copy_from_slice(&state_ids);
    }
    if !out_perfect_match.is_null() {
        *out_perfect_match = c_int::from(perfect);
    }
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_fs_op(
    op: c_int,
    queue_dir: *const c_char,
    sep: c_int,
    a1: c_int,
    a2: c_int,
    a3: c_int,
    a4: c_int,
    out1: *mut c_int,
    out2: *mut c_int,
) -> c_int {
    let qdir = if queue_dir.is_null() {
        &[][..]
    } else {
        input(queue_dir)
    };
    let mut o1 = 0;
    let mut o2 = 0;
    let res = crate::diskstate_format::diskstate_fs_op(
        op,
        qdir,
        sep as u8,
        a1,
        a2,
        a3,
        a4,
        &mut o1,
        &mut o2,
    );
    if !out1.is_null() {
        *out1 = o1;
    }
    if !out2.is_null() {
        *out2 = o2;
    }
    res
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_load_dupe_extras(
    infile_ctx: *mut c_void,
    read_line_cb: crate::diskstate_format::ReadLineFn,
    format_version: c_int,
    target_ctx: *mut c_void,
    on_header_cb: unsafe extern "C" fn(
        ctx: *mut c_void,
        rec_articles: c_int,
        rec_bytes: i64,
        rec_holes: c_int,
    ),
    on_job_cb: unsafe extern "C" fn(
        ctx: *mut c_void,
        job: *const crate::diskstate_format::NzbgetRsDiskRepairJob,
    ),
) -> c_int {
    c_int::from(crate::diskstate_format::load_dupe_extras(
        infile_ctx,
        read_line_cb,
        format_version,
        target_ctx,
        on_header_cb,
        on_job_cb,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_load_volume_stat(
    infile_ctx: *mut c_void,
    read_line_cb: crate::diskstate_format::ReadLineFn,
    format_version: c_int,
    target_ctx: *mut c_void,
    on_item_cb: unsafe extern "C" fn(
        ctx: *mut c_void,
        slot_idx: c_int,
        item: *const crate::diskstate_format::NzbgetRsDiskVolumeStatInput,
        has_custom_times: c_int,
    ),
) -> c_int {
    c_int::from(crate::diskstate_format::load_volume_stat(
        infile_ctx,
        read_line_cb,
        format_version,
        target_ctx,
        on_item_cb,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_load_file_info(
    infile_ctx: *mut c_void,
    read_line_cb: crate::diskstate_format::ReadLineFn,
    format_version: c_int,
    file_summary: c_int,
    articles: c_int,
    file_id: c_int,
    direct_write_opt: c_int,
    force_direct_write: c_int,
    sep: c_int,
    dest_dir: *const c_char,
    cur_filename: *const c_char,
    cur_output_filename: *const c_char,
    target_ctx: *mut c_void,
    on_summary_cb: unsafe extern "C" fn(
        ctx: *mut c_void,
        fields: *const crate::diskstate_format::NzbgetRsDiskFileInfoInput,
        flags: c_int,
    ),
    on_item_cb: unsafe extern "C" fn(
        ctx: *mut c_void,
        group_or_null: *const c_char,
        art_or_null: *const crate::diskstate_format::NzbgetRsDiskArticleInfo,
    ),
) -> c_int {
    let ddir = if dest_dir.is_null() { &[][..] } else { input(dest_dir) };
    let cfn = if cur_filename.is_null() { &[][..] } else { input(cur_filename) };
    let cofn = if cur_output_filename.is_null() {
        &[][..]
    } else {
        input(cur_output_filename)
    };
    c_int::from(crate::diskstate_format::load_file_info(
        infile_ctx,
        read_line_cb,
        format_version,
        file_summary,
        articles,
        file_id,
        direct_write_opt,
        force_direct_write,
        sep as u8,
        ddir,
        cfn,
        cofn,
        target_ctx,
        on_summary_cb,
        on_item_cb,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_diskstate_load_section(
    kind: c_int,
    infile_ctx: *mut c_void,
    read_line_cb: Option<crate::diskstate_format::ReadLineFn>,
    format_version: c_int,
    queue_dir: *const c_char,
    sep: c_int,
    now: i64,
    target_ctx: *mut c_void,
    on_entry_cb: unsafe extern "C" fn(
        ctx: *mut c_void,
        a1: c_int,
        a2: c_int,
        a3: c_int,
        u1: u32,
        u2: u32,
        i64_1: i64,
        s1: *const c_char,
        s2: *const c_char,
    ),
) -> c_int {
    let qdir = if queue_dir.is_null() {
        &[][..]
    } else {
        input(queue_dir)
    };
    c_int::from(crate::diskstate_format::load_section(
        kind,
        infile_ctx,
        read_line_cb,
        format_version,
        qdir,
        sep as u8,
        now,
        target_ctx,
        on_entry_cb,
    ))
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
                in_use,
                connected,
                free_times,
                a,
                b as c_int,
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
pub unsafe extern "C" fn nzbget_rs_article_fetch_limit_op(
    op: c_int,
    inout_bytes: *mut i64,
    bytes: i64,
    begin: i64,
    end: i64,
    file_size: i64,
) -> c_int {
    match op {
        0 => {
            if inout_bytes.is_null() {
                return 0;
            }
            c_int::from(crate::dupefallback::article_fetch_add_raw_bytes(
                &mut *inout_bytes,
                bytes,
            ))
        }
        1 => c_int::from(crate::dupefallback::article_fetch_header_within_limit(
            begin,
            bytes != 0,
        )),
        2 => {
            if inout_bytes.is_null() {
                return 0;
            }
            c_int::from(crate::dupefallback::article_fetch_add_decoded_bytes(
                &mut *inout_bytes,
                bytes,
                begin,
                end,
                file_size,
            ))
        }
        _ => 0,
    }
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
            let elapsed_slots =
                crate::statmeter::calc_month_slots_utc(loc_or_cur_time, b_i32);
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
            let (reached, action) = crate::statmeter::eval_quota_status(
                a_i32,
                b_i32,
                a_i64,
                b_i64,
                c_i32 != 0,
            );
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
            let (next_tier, transitioned) = crate::artwritercore::memory_monitor_step(
                tier,
                flag1,
                &mut *inout1,
                &mut *inout2,
            );
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
pub unsafe extern "C" fn nzbget_rs_dupe_probe_op(
    op: c_int,
    a: usize,
    b: c_int,
    c: c_int,
    d: c_int,
    str_arg: *const c_char,
    in_a: *const u8,
    in_b: *const c_int,
    in_c: *const c_int,
    out_ptr: *mut usize,
) -> c_int {
    match op {
        0 => {
            let idxs = crate::dupeprobe::sample_indexes(a, b);
            if !out_ptr.is_null() && !idxs.is_empty() {
                std::slice::from_raw_parts_mut(out_ptr, idxs.len()).copy_from_slice(&idxs);
            }
            idxs.len() as c_int
        }
        1 => {
            let resp = if str_arg.is_null() {
                None
            } else {
                Some(input(str_arg))
            };
            crate::dupeprobe::classify(resp)
        }
        2 => c_int::from(crate::dupeprobe::is_dead(b, c, d)),
        4 => {
            if in_a.is_null() || in_b.is_null() || in_c.is_null() || out_ptr.is_null() || a == 0 {
                return 0;
            }
            let active = std::slice::from_raw_parts(in_a, a);
            let groups = std::slice::from_raw_parts(in_b, a);
            let norm_levels = std::slice::from_raw_parts(in_c, a);
            let ids_inout = std::slice::from_raw_parts_mut(out_ptr as *mut c_int, a);
            let order =
                crate::dupeprobe::select_server_order(active, groups, norm_levels, ids_inout);
            ids_inout[..order.len()].copy_from_slice(&order);
            order.len() as c_int
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_dupe_probe_eval_outcome(
    op: c_int,
    existing: c_int,
    sampled: c_int,
    missing_servers: c_int,
    active_servers: c_int,
    unreachable_servers: c_int,
    reached_servers: c_int,
    finished: c_int,
    str_arg: *const c_char,
    out_outcome: *mut c_int,
) -> RsBuf {
    let s = if str_arg.is_null() {
        &[][..]
    } else {
        input(str_arg)
    };
    let (code, msg) = if op == 0 {
        crate::dupeprobe::eval_run_outcome(
            existing,
            sampled,
            missing_servers,
            active_servers,
            unreachable_servers,
            reached_servers,
            finished != 0,
            s,
        )
    } else {
        crate::dupeprobe::eval_recheck_outcome(existing, sampled, reached_servers, s)
    };
    if !out_outcome.is_null() {
        *out_outcome = code;
    }
    into_buf(msg)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_history_coord_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    d: c_int,
    e: c_int,
    f: i64,
    g: i64,
    str_arg: *const c_char,
) -> c_int {
    match op {
        0 => {
            // par_status: psNone=0, psSuccess=3; unpack_status: usNone=0, usSuccess=3; delete_status: dsNone=0
            let cleanup = a == 3
                || b == 3
                || (a == 0 && b == 0 && (f - g) == 0)
                || (c != 0 && d == 0)
                || e != 0;
            c_int::from(cleanup)
        }
        1 => c_int::from(crate::historycoord::should_park_completed_file(
            a,
            0, // CompletedFile::cfNone
            2, // CompletedFile::cfPartial
            b != 0,
        )),
        2 => {
            let txt = if str_arg.is_null() {
                &[][..]
            } else {
                input(str_arg)
            };
            // dafNone = 0, hcDupe = 4, nkNzb = 0
            c_int::from(crate::historycoord::should_recheck_failed_articles(
                a == 0,
                b == 4,
                c == 0,
                d,
                txt,
                e != 0,
            ))
        }
        3 => crate::historycoord::classify_dup_status(a, b, c != 0),
        4 => {
            let min_time = crate::historycoord::calc_min_time(f, b);
            crate::historycoord::classify_service_work_item(a, d, e, g, min_time, c != 0)
        }
        5 => c_int::from(crate::historycoord::should_delete_dest_dir(
            a == 0,
            b,
            0,
            c,
            2,
            d,
            2,
            5,
            e != 0,
        )),
        6 => c_int::from(crate::historycoord::eval_requeue_loaded_file(
            a,
            0,
            3,
            2,
            b != 0,
            c != 0,
            f,
        )),
        7 => {
            let txt = if str_arg.is_null() {
                &[][..]
            } else {
                input(str_arg)
            };
            c_int::from(crate::historycoord::is_temporary_output_name(txt))
        }
        8 => {
            let txt = if str_arg.is_null() {
                &[][..]
            } else {
                input(str_arg)
            };
            if txt.eq_ignore_ascii_case(b"YES")
                || txt.eq_ignore_ascii_case(b"TRUE")
                || txt == b"1"
            {
                3 // dsDupe
            } else {
                1 // dsManual
            }
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_history_reset_articles_batch(
    all_failed: c_int,
    reset_failed: c_int,
    is_par_file: c_int,
    partial_state: c_int,
    file_size: i64,
    file_missed_size: i64,
    file_total_articles: c_int,
    file_missed_articles: c_int,
    file_failed_size: *mut i64,
    file_failed_articles: *mut c_int,
    file_remaining_size: *mut i64,
    file_completed_articles: *mut c_int,
    file_success_size: *mut i64,
    file_success_articles: *mut c_int,
    nzb_failed_size: *mut i64,
    nzb_success_size: *mut i64,
    nzb_failed_articles: *mut c_int,
    nzb_success_articles: *mut c_int,
    nzb_par_failed_size: *mut i64,
    nzb_par_success_size: *mut i64,
    article_statuses: *mut c_int,
    article_sizes: *const c_int,
    out_reset_flags: *mut u8,
    article_count: usize,
) {
    let statuses = if article_statuses.is_null() || article_count == 0 {
        &mut [][..]
    } else {
        std::slice::from_raw_parts_mut(article_statuses, article_count)
    };
    let sizes = if article_sizes.is_null() || article_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(article_sizes, article_count)
    };
    let flags = if out_reset_flags.is_null() || article_count == 0 {
        &mut [][..]
    } else {
        std::slice::from_raw_parts_mut(out_reset_flags, article_count)
    };
    crate::historycoord::reset_articles_batch(
        all_failed != 0,
        reset_failed != 0,
        is_par_file != 0,
        partial_state,
        file_size,
        file_missed_size,
        file_total_articles,
        file_missed_articles,
        &mut *file_failed_size,
        &mut *file_failed_articles,
        &mut *file_remaining_size,
        &mut *file_completed_articles,
        &mut *file_success_size,
        &mut *file_success_articles,
        &mut *nzb_failed_size,
        &mut *nzb_success_size,
        &mut *nzb_failed_articles,
        &mut *nzb_success_articles,
        &mut *nzb_par_failed_size,
        &mut *nzb_par_success_size,
        statuses,
        sizes,
        flags,
    );
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
            crate::queueeditor::select_pars_to_pause(
                is_par,
                has_vol,
                sizes,
                a != 0,
                out_pause,
            );
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
            let (work_offset, new_last) =
                crate::queueeditor::clamp_move_offset(a, b, c, *inout1);
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
                    crate::queueeditor::eval_group_delete_flags(a, 19, 20, 21, b, c != 0, f1 as i32);
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
pub unsafe extern "C" fn nzbget_rs_dupe_identity_calc(
    op: c_int,
    present: c_int,
    missing: c_int,
    error: c_int,
    listed_parts: u64,
    declared_parts: u64,
    total_or_data_bytes: i64,
    article_bytes: u64,
    block_or_min: u64,
    alive_in: f64,
    rec_ptr: *const u64,
    rec_len: usize,
    out_f64: *mut f64,
) -> u64 {
    match op {
        0 => {
            if total_or_data_bytes <= 0 {
                0
            } else {
                crate::dupewatch::whole_bytes(
                    total_or_data_bytes as u64,
                    listed_parts,
                    declared_parts,
                )
            }
        }
        1 => {
            let v = crate::dupewatch::effective_alive(
                present,
                missing,
                error,
                listed_parts,
                declared_parts,
            );
            if !out_f64.is_null() {
                *out_f64 = v;
            }
            u64::from(v >= 0.0)
        }
        2 => u64::from(crate::dupewatch::dead_donor(
            present,
            missing,
            error,
            listed_parts,
            declared_parts,
            block_or_min as i32,
        )),
        3 => crate::dupewatch::unlisted_missing(present, listed_parts, declared_parts) as u64,
        4 => {
            let vols = if rec_ptr.is_null() || rec_len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(rec_ptr, rec_len)
            };
            u64::from(crate::dupewatch::par_doomed(
                total_or_data_bytes.max(0) as u64,
                article_bytes,
                vols,
                alive_in,
                block_or_min,
            ))
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_dupe_watch_item_flags(
    kind: c_int,
    in_history: c_int,
    dupe_success: c_int,
    deleting: c_int,
    delete_status: c_int,
    has_post_info: c_int,
    dupe_mode: c_int,
    is_donor: c_int,
    dupe_hint: c_int,
    has_dupe_fleet: c_int,
) -> u32 {
    crate::dupewatch::build_item_flags(
        kind,
        in_history != 0,
        dupe_success != 0,
        deleting != 0,
        delete_status,
        has_post_info != 0,
        dupe_mode,
        is_donor != 0,
        dupe_hint,
        has_dupe_fleet != 0,
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_dupe_watch_is_live_fleet_peer(
    peer_key: *const c_char,
    peer_len: usize,
    pick_key: *const c_char,
    pick_len: usize,
    peer_flags: u32,
) -> c_int {
    c_int::from(crate::dupewatch::is_live_fleet_peer(
        bytes(peer_key, peer_len),
        bytes(pick_key, pick_len),
        peer_flags,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_dupe_watch_format_state_line(
    now: i64,
    ttl_sec: i64,
    key: *const c_char,
    key_len: usize,
    score: c_int,
    time: i64,
    title: *const c_char,
    title_len: usize,
    retryable: c_int,
) -> RsBuf {
    into_buf(crate::dupewatch::format_searched_state_line(
        now,
        ttl_sec,
        bytes(key, key_len),
        score,
        time,
        bytes(title, title_len),
        retryable != 0,
    ))
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_feed_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    d: c_int,
    t1: i64,
    t2: i64,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    out_i32: *mut c_int,
) -> c_int {
    let b1 = if s1.is_null() { &[][..] } else { input(s1) };
    let b2 = if s2.is_null() { &[][..] } else { input(s2) };
    let b3 = if s3.is_null() { &[][..] } else { input(s3) };
    let b4 = if s4.is_null() { &[][..] } else { input(s4) };
    match op {
        0 => crate::feed::parse_prefixed_int(b1),
        1 => c_int::from(crate::feed::has_any_series_id(b4, b1, b2, b3, a, b, c)),
        2 => {
            let (start, force) = crate::feed::should_start_feed(a, t1, t2, b != 0, c, d);
            if !out_i32.is_null() {
                *out_i32 = c_int::from(force);
            }
            c_int::from(start)
        }
        3 => {
            let (interval, new_last) = crate::feed::calc_feed_next_interval(a != 0, b, c);
            if !out_i32.is_null() {
                *out_i32 = new_last;
            }
            interval
        }
        4 => {
            let (status, should_add) = crate::feed::classify_item_history(a != 0, b != 0, c != 0);
            if !out_i32.is_null() {
                *out_i32 = c_int::from(should_add);
            }
            status
        }
        5 => crate::feed::resolve_preview_item_status(a != 0, b, c != 0, d != 0),
        6 => c_int::from(crate::feed::is_feed_cache_expired(t1, a, t2)),
        7 => c_int::from(crate::feed::is_feed_download_hanging(t1, t2, a, b, c)),
        8 => c_int::from(crate::feed::is_feed_url_and_filter_updated(
            b1, b2, b3, b4, t1,
        )),
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_feed_str_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    d: c_int,
    e: c_int,
    f: c_int,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    s5: *const c_char,
    s6: *const c_char,
    out_buf2: *mut RsBuf,
    out_u32: *mut u32,
) -> RsBuf {
    let b1 = if s1.is_null() { &[][..] } else { input(s1) };
    let b2 = if s2.is_null() { &[][..] } else { input(s2) };
    let b3 = if s3.is_null() { &[][..] } else { input(s3) };
    let b4 = if s4.is_null() { &[][..] } else { input(s4) };
    let b5 = if s5.is_null() { &[][..] } else { input(s5) };
    let b6 = if s6.is_null() { &[][..] } else { input(s6) };
    match op {
        0 => {
            let (default_name, filter_hash) = crate::feed::feed_info_init(a, b1, b2);
            if !out_u32.is_null() {
                *out_u32 = filter_hash;
            }
            match default_name {
                Some(name) => into_buf(name),
                None => RsBuf {
                    data: std::ptr::null_mut(),
                    len: 0,
                    cap: 0,
                },
            }
        }
        1 => match crate::feed::parse_season_episode(b1) {
            Some((season, ep)) => {
                if !out_buf2.is_null() {
                    *out_buf2 = into_buf(ep);
                }
                into_buf(season)
            }
            None => RsBuf {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            },
        },
        2 => into_buf(crate::feed::build_feed_dupe_key(
            b1, b2, b3, b4, a, b, c, d, e, f, b5, b6,
        )),
        3 => into_buf(crate::feed::format_dupe_status_mask(a)),
        4 => into_buf(crate::feed::resolve_category(b1, a, b2).to_vec()),
        5 => {
            let norm = crate::feed::normalize_feed_nzb_name(b1);
            let filename = if norm.is_empty() {
                Vec::new()
            } else {
                crate::paths::make_valid_filename(&norm[..norm.len().min(1023)], false)
            };
            let cat = if !b2.is_empty() {
                b2.to_vec()
            } else {
                crate::feed::resolve_category(b3, a, b4).to_vec()
            };
            if !out_buf2.is_null() {
                *out_buf2 = into_buf(cat);
            }
            into_buf(filename)
        }
        6 => into_buf(crate::feed::format_temp_filename(
            b1, a as u8, b, c != 0, d, e, f,
        )),
        _ => RsBuf {
            data: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        },
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_twin_service_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    d: c_int,
    f1: i64,
    f2: i64,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    u8_1: *const u8,
    u8_2: *const u8,
    i32_1: *const c_int,
    i64_1: *const i64,
    len: usize,
    out_buf: *mut RsBuf,
) -> c_int {
    let b1 = if s1.is_null() { &[][..] } else { input(s1) };
    let b2 = if s2.is_null() { &[][..] } else { input(s2) };
    let b3 = if s3.is_null() { &[][..] } else { input(s3) };
    match op {
        0 => c_int::from(crate::twin::is_dupe_item(a != 0, f1, f2, b, c, d)),
        1 => c_int::from(crate::twin::is_print_usable(b1)),
        2 => {
            if u8_1.is_null() || u8_2.is_null() || i32_1.is_null() || len == 0 {
                return -1;
            }
            crate::twin::select_primary(
                std::slice::from_raw_parts(u8_1, len),
                std::slice::from_raw_parts(u8_2, len),
                std::slice::from_raw_parts(i32_1, len),
            )
        }
        3 => {
            let kind = crate::twin::resolve_kind(
                a != 0,
                b != 0,
                c != 0,
                b1,
                d != 0,
                f1 != 0,
                f2 != 0,
                f2 != 0,
                b2,
            );
            let non_empty = !kind.is_empty();
            if !out_buf.is_null() {
                *out_buf = into_buf(kind);
            }
            c_int::from(non_empty)
        }
        4 => {
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::twin::format_twin_kind_message(
                    b1, b2, b3, a != 0,
                ));
            }
            1
        }
        5 => crate::twin::parse_fingerprint_files(b1),
        6 => {
            if u8_1.is_null() || i64_1.is_null() || len == 0 {
                return -1;
            }
            crate::twin::select_min_par2_index(
                std::slice::from_raw_parts(u8_1, len),
                std::slice::from_raw_parts(i64_1, len),
                f1,
            )
        }
        7 => {
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::twin::format_identity(b1, b2));
            }
            1
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_xmlrpc_resp_op(
    op: c_int,
    is_json: c_int,
    int_val: c_int,
    u64_1: u64,
    u64_2: u64,
    f64_1: f64,
    f64_2: f64,
    out_buf: *mut RsBuf,
) -> c_int {
    match op {
        0 => c_int::from(crate::xmlrpc_format::is_safe_command(int_val)),
        1 => {
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::xmlrpc_format::format_bool_bytes(
                    is_json != 0,
                    int_val != 0,
                ));
            }
            1
        }
        2 => {
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::xmlrpc_format::format_int_bytes(
                    is_json != 0,
                    int_val,
                ));
            }
            1
        }
        3 => {
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::xmlrpc_format::format_disk_speed_bytes(
                    is_json != 0,
                    u64_1,
                    f64_1,
                    u64_2,
                    f64_2,
                ));
            }
            1
        }
        4 => {
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::xmlrpc_format::format_network_speed_bytes(
                    is_json != 0,
                    f64_1,
                ));
            }
            1
        }
        _ => 0,
    }
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_url_coord_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    t1: i64,
    t2: i64,
    s1: *const c_char,
    arr1: *const c_int,
    arr2: *const c_int,
    arr3: *const c_int,
    arr4: *const c_int,
    len: usize,
    out_i32_1: *mut c_int,
    out_i32_2: *mut c_int,
    out_i32_3: *mut c_int,
    out_buf: *mut RsBuf,
) -> isize {
    match op {
        0 => {
            let cur_url = if out_i32_2.is_null() { 0 } else { *out_i32_2 };
            let (new_url, new_del, retry) =
                crate::urlcoord::completed(a, b != 0, cur_url, c);
            if !out_i32_1.is_null() {
                *out_i32_1 = new_del;
            }
            if !out_i32_2.is_null() {
                *out_i32_2 = new_url;
            }
            if !out_i32_3.is_null() {
                *out_i32_3 = c_int::from(b != 0);
            }
            isize::from(retry)
        }
        1 => {
            let kinds = if arr1.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(arr1, len)
            };
            let url_st = if arr2.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(arr2, len)
            };
            let del_st = if arr3.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(arr3, len)
            };
            let prios = if arr4.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(arr4, len)
            };
            crate::urlcoord::select_next_url(kinds, url_st, del_st, prios, 1, 0, 0) as isize
        }
        2 => isize::from(crate::urlcoord::is_hanging_url_download(t1, t2, a, b, 1)),
        3 => {
            let temp_dir = if s1.is_null() { &[][..] } else { input(s1) };
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::urlcoord::format_temp_url_path(
                    temp_dir,
                    a as u8,
                    b,
                ));
            }
            1
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_scanner_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    d: c_int,
    t1: i64,
    t2: i64,
    t3: i64,
    t4: i64,
    s1: *const c_char,
    arr: *const c_int,
    len: usize,
    out_i32: *mut c_int,
    out_i64_1: *mut i64,
    out_i64_2: *mut i64,
) -> c_int {
    let b1 = if s1.is_null() { &[][..] } else { input(s1) };
    match op {
        0 => {
            let (next_int, next_pass) = crate::scanner::next_pass(a, b, c != 0, d);
            if !out_i32.is_null() {
                *out_i32 = next_pass;
            }
            next_int
        }
        1 => c_int::from(crate::scanner::can_process_extension(b1)),
        2 => {
            let (can_proc, upd_sz, upd_lc) =
                crate::scanner::eval_file_age(t1, t2, t3, t4, a);
            if !out_i64_1.is_null() {
                *out_i64_1 = upd_sz;
            }
            if !out_i64_2.is_null() {
                *out_i64_2 = upd_lc;
            }
            c_int::from(can_proc)
        }
        3 => c_int::from(crate::scanner::should_drop_scan_file(t1, t2, a, b)),
        4 => c_int::from(crate::scanner::should_skip_item(
            b1,
            b"_processed",
            b"_broken",
        )),
        5 => {
            let (ren_proc, act) =
                crate::scanner::classify_incoming_extension(a != 0, b != 0, b1);
            if !out_i32.is_null() {
                *out_i32 = c_int::from(ren_proc);
            }
            act
        }
        6 => crate::scanner::classify_missed_articles(a, b, c != 0),
        7 => {
            let statuses = if arr.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(arr, len)
            };
            crate::scanner::resolve_archive_add_status(a != 0, statuses, b, c)
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_scheduler_service_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    d: c_int,
    t1: i64,
    t2: i64,
    s1: *const c_char,
    s2: *const c_char,
    out_i32: *mut c_int,
) -> c_int {
    match op {
        0 => c_int::from(crate::scheduler::scheduler_task_less(a, b, c, d)),
        1 => c_int::from(crate::scheduler::scheduler_should_execute_process(
            a != 0,
            b,
            -1,
        )),
        2 => {
            let kb = if s1.is_null() { 0 } else { libc::atoi(s1) };
            crate::scheduler::scheduler_calc_speed_limit_bytes(kb)
        }
        3 => {
            let id = if s2.is_null() { 0 } else { libc::atoi(s2) };
            c_int::from(crate::scheduler::scheduler_matches_server_ref(id, a, s1, s2))
        }
        4 => {
            let id = if s2.is_null() { 0 } else { libc::atoi(s2) };
            let (matched, fetch_id) =
                crate::scheduler::scheduler_matches_feed_ref(id, a, s1, s2);
            if !out_i32.is_null() {
                *out_i32 = fetch_id;
            }
            c_int::from(matched)
        }
        5 => c_int::from(crate::scheduler::scheduler_should_autoresume(t1, t2)),
        6 => crate::scheduler::scheduler_bytes_to_kb(a),
        7 => crate::scheduler::scheduler_calc_next_work_interval(t1),
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_scheduler_service_str(op: c_int, val: c_int) -> *const c_char {
    let bytes = match op {
        0 => crate::scheduler::scheduler_pause_action_str(val != 0),
        1 => crate::scheduler::scheduler_server_action_str(val != 0),
        2 => crate::scheduler::scheduler_command_name(val),
        _ => b"\0",
    };
    bytes.as_ptr() as *const c_char
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_daemon_service_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    d: c_int,
    t1: i64,
    s1: *const c_char,
    s2: *const c_char,
    out1: *mut c_int,
    out2: *mut c_int,
    ctx: *mut c_void,
    cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
) -> c_int {
    match op {
        0 => {
            if a != 0 {
                crate::lifecycle::enable_core_dump();
            }
            0
        }
        1 => {
            let lock_file = if s1.is_null() {
                c""
            } else {
                CStr::from_ptr(s1)
            };
            let daemon_user = if s2.is_null() {
                c""
            } else {
                CStr::from_ptr(s2)
            };
            match crate::lifecycle::daemonize_setup(lock_file, daemon_user) {
                Ok((owned, trunc_failed)) => {
                    if !out1.is_null() {
                        *out1 = c_int::from(owned);
                    }
                    if !out2.is_null() {
                        *out2 = c_int::from(trunc_failed);
                    }
                    0
                }
                Err(code) => code,
            }
        }
        2 => {
            let dest_dir = if s1.is_null() {
                c""
            } else {
                CStr::from_ptr(s1)
            };
            let inter_dir = if s2.is_null() {
                None
            } else {
                Some(CStr::from_ptr(s2))
            };
            let (dest_low, inter_low) =
                crate::fsops::disk_service_check_disk_space(dest_dir, inter_dir, a);
            if !out1.is_null() {
                *out1 = c_int::from(dest_low);
            }
            if !out2.is_null() {
                *out2 = c_int::from(inter_low);
            }
            c_int::from(dest_low || inter_low)
        }
        3 => {
            let req_bytes = if s1.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(s1).to_bytes()
            };
            let (all_exist, next_reported) =
                crate::fsops::disk_service_check_required_dirs(req_bytes, a != 0, |dir| {
                    if let Some(f) = cb {
                        f(ctx, dir.as_ptr());
                    }
                });
            if !out1.is_null() {
                *out1 = c_int::from(next_reported);
            }
            c_int::from(all_exist)
        }
        4 => {
            let ip_bytes = if s1.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(s1).to_bytes()
            };
            c_int::from(crate::options::nzbget_should_start_secure_server(
                a != 0,
                cfg!(unix),
                ip_bytes,
            ))
        }
        5 => c_int::from(crate::options::nzbget_should_stop_standalone(
            a != 0,
            b != 0,
            c != 0,
            d != 0,
        )),
        6 => crate::options::roc_nzbget_reload_clamp_debounce_ms(t1, 100, 1500) as c_int,
        7 => {
            #[cfg(unix)]
            {
                let umask_val = a as u32;
                let res = if crate::options::nzbget_is_valid_umask(umask_val) {
                    let m = umask_val as libc::mode_t;
                    libc::umask(m);
                    m
                } else {
                    let m = libc::umask(0);
                    libc::umask(m);
                    m
                };
                res as c_int
            }
            #[cfg(not(unix))]
            {
                a
            }
        }
        _ => 0,
    }
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
            let (ready, upd_sec, is_comp, new_sec) = crate::prepost::eval_cleanup_job_state(
                a != 0,
                b != 0,
                c,
                12,
                d != 0,
                t1,
                t2,
                e,
            );
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_queue_coord_op(
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
    j: c_int,
    t1: i64,
    t2: i64,
    t3: i64,
    t4: i64,
    s1: *const c_char,
    s2: *const c_char,
    cstr_arr: *const *const c_char,
    i32_1: *const c_int,
    i32_2: *const c_int,
    u8_1: *const u8,
    i64_1: *const i64,
    len: usize,
    out1: *mut c_int,
    out2: *mut c_int,
    out3: *mut c_int,
    out_arr: *mut c_int,
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
        0 => {
            if cstr_arr.is_null() || i64_1.is_null() || out_arr.is_null() || len == 0 {
                return 0;
            }
            let ptrs = std::slice::from_raw_parts(cstr_arr, len);
            let sizes = std::slice::from_raw_parts(i64_1, len);
            let mut names: Vec<&[u8]> = Vec::with_capacity(len);
            for &p in ptrs {
                names.push(if p.is_null() {
                    &[][..]
                } else {
                    CStr::from_ptr(p).to_bytes()
                });
            }
            let mut out_idx = vec![0usize; len];
            let written = crate::queuecoord::find_dupe_file_indices(&names, sizes, &mut out_idx);
            let out_slice = std::slice::from_raw_parts_mut(out_arr, len);
            for k in 0..written {
                out_slice[k] = out_idx[k] as c_int;
            }
            written as c_int
        }
        1 => {
            if s1.is_null() {
                return 0;
            }
            c_int::from(crate::queuecoord::is_side_file(s1_bytes, s2_bytes))
        }
        2 => crate::queuecoord::validate_split_file_state(a, b, c != 0, d == 0),
        3 => {
            let levels = if i32_1.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(i32_1, len)
            };
            let actives = if u8_1.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(u8_1, len)
            };
            let conns = if i32_2.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(i32_2, len)
            };
            crate::queuecoord::calc_downloads_limit(levels, actives, conns)
        }
        4 => {
            let (posted_min, start_min) =
                crate::queuecoord::calc_propagation_wait_log_mins(t1, a, t2);
            if !out1.is_null() {
                *out1 = posted_min;
            }
            if !out2.is_null() {
                *out2 = start_min;
            }
            start_min
        }
        5 => {
            let donors = if i32_1.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(i32_1, len)
            };
            crate::queuecoord::select_dupe_donor_id(a, donors, s1_bytes, s2_bytes)
        }
        6 => {
            let (rec, next_file, next_nzb, trig_cutover) =
                crate::queuecoord::eval_dupe_recovery_and_cutover(
                    a,
                    b != 0,
                    s1_bytes,
                    s2_bytes,
                    c,
                    d,
                    e != 0,
                    f,
                );
            if !out1.is_null() {
                *out1 = next_file;
            }
            if !out2.is_null() {
                *out2 = next_nzb;
            }
            if !out3.is_null() {
                *out3 = c_int::from(trig_cutover);
            }
            c_int::from(rec)
        }
        7 => {
            let (fc, comp_parts) = crate::queuecoord::eval_file_completion_and_assembly(
                a != 0,
                b != 0,
                c,
                d != 0,
                e != 0,
                f != 0,
            );
            if !out1.is_null() {
                *out1 = c_int::from(fc);
            }
            if !out2.is_null() {
                *out2 = c_int::from(comp_parts);
            }
            c_int::from(comp_parts)
        }
        8 => c_int::from(crate::queuecoord::should_skip_health_check(
            a,
            3,
            2,
            b,
            3,
            c != 0,
            d,
            2,
            e,
            f,
            h,
            g,
        )),
        9 => {
            let other_sizes = if i64_1.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(i64_1, len)
            };
            c_int::from(crate::queuecoord::is_size_twin(
                a != 0,
                b != 0,
                t1,
                t3,
                t4,
                t2,
                other_sizes,
            ))
        }
        10 => c_int::from(crate::queuecoord::is_queued_dupe_active(
            a != 0,
            b,
            0,
            c,
            d != 0,
            e != 0,
            f,
            0,
            g != 0,
            h != 0,
            i,
            1,
        )),
        11 => c_int::from(crate::queuecoord::download_hopeless(a, b, c)),
        12 => {
            let (defer, upd_checked) = crate::queuecoord::should_defer_dupe_failover(
                a != 0,
                b,
                c,
                h,
                e,
                1,
                f != 0,
                d,
                g,
            );
            if !out1.is_null() {
                *out1 = c_int::from(upd_checked);
            }
            c_int::from(defer)
        }
        13 => {
            let eval = crate::queuecoord::eval_dead_download(
                a,
                b,
                c,
                d,
                e,
                f != 0,
                h,
                i,
                g,
                j,
            );
            if !out_arr.is_null() {
                let s = std::slice::from_raw_parts_mut(out_arr, 10);
                s[0] = eval.own;
                s[1] = eval.tried;
                s[2] = eval.dead_rate;
                s[3] = eval.dead_run;
                s[4] = eval.dead;
                s[5] = eval.projected;
                s[6] = eval.doomed;
                s[7] = eval.hopeless;
                s[8] = eval.far_below;
                s[9] = eval.below;
            }
            eval.dead
        }
        14 => c_int::from(crate::queuecoord::should_park_dead_without_backup(
            a != 0,
            b,
            1,
            c,
            d,
            e,
            f,
            g,
            h != 0,
            i != 0,
        )),
        15 => c_int::from(crate::queuecoord::should_wait_for_borrowing_on_par_damage(
            a != 0,
            b != 0,
            c,
            2,
            d != 0,
            e,
            1,
            f,
            h,
            g,
        )),
        16 => c_int::from(crate::queuecoord::is_hanging_download(t1, t2, a, b != 0)),
        17 => {
            let (disc_par, disc_infos) = crate::queuecoord::eval_discard_direct_rename_file(
                a != 0,
                b,
                c,
                d != 0,
                e != 0,
                f != 0,
                g,
            );
            if !out1.is_null() {
                *out1 = c_int::from(disc_par);
            }
            if !out2.is_null() {
                *out2 = c_int::from(disc_infos);
            }
            c_int::from(disc_par || disc_infos)
        }
        18 => crate::queuecoord::classify_completed_file_status(a, b, c, d),
        19 => {
            let (del_data, save_part) =
                crate::queuecoord::eval_delete_file_diskstate_flags(a, 1, 2, b, c, d);
            if !out1.is_null() {
                *out1 = c_int::from(del_data);
            }
            if !out2.is_null() {
                *out2 = c_int::from(save_part);
            }
            c_int::from(del_data)
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_queue_coord_str_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    t1: i64,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    i32_arr: *const c_int,
    len: usize,
    out_unique: *mut c_int,
    out_unique_len: *mut usize,
) -> RsBuf {
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
    match op {
        0 => {
            let ids = if i32_arr.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(i32_arr, len)
            };
            let (unique, formatted) = crate::queuecoord::format_donor_id_list(ids);
            if !out_unique.is_null() && !unique.is_empty() {
                let copy_len = unique.len().min(len);
                std::ptr::copy_nonoverlapping(unique.as_ptr(), out_unique, copy_len);
                if !out_unique_len.is_null() {
                    *out_unique_len = copy_len;
                }
            } else if !out_unique_len.is_null() {
                *out_unique_len = 0;
            }
            into_buf(formatted)
        }
        1 => into_buf(crate::queuecoord::format_par_damage_summary(
            a != 0,
            b,
            c,
            t1,
            len as i32,
        )),
        2 => into_buf(crate::queuecoord::resolve_completed_filename(
            a != 0,
            b,
            s1_bytes,
            s2_bytes,
            s3_bytes,
            crate::paths::PATH_SEPARATOR,
            c != 0,
        )),
        _ => into_buf(Vec::new()),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_queue_coord_counters_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    _d: c_int,
    t1: i64,
    t2: i64,
    s1: *const c_char,
    s2: *const c_char,
    ms1: *mut crate::queuecoord::NzbgetRsNzbMergeSplitCounters,
    ms2: *mut crate::queuecoord::NzbgetRsNzbMergeSplitCounters,
    sf: *const crate::queuecoord::NzbgetRsSplitFileCounters,
    i32_inout: *mut c_int,
    i64_inout: *mut i64,
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
        0 => {
            if !ms1.is_null() && !ms2.is_null() {
                crate::queuecoord::merge_counters(&mut *ms1, &*ms2);
            }
            0
        }
        1 => {
            if !ms1.is_null() && !ms2.is_null() && !sf.is_null() {
                crate::queuecoord::split_transfer_file(&mut *ms1, &mut *ms2, &*sf);
            }
            0
        }
        2 => {
            if !i32_inout.is_null() && !i64_inout.is_null() {
                let i32s = std::slice::from_raw_parts_mut(i32_inout, 2);
                let i64s = std::slice::from_raw_parts_mut(i64_inout, 3);
                let (fs, ns, ps, fa, na) = crate::queuecoord::apply_success_article_counters(
                    t1,
                    a != 0,
                    i64s[0],
                    i64s[1],
                    i64s[2],
                    i32s[0],
                    i32s[1],
                );
                i64s[0] = fs;
                i64s[1] = ns;
                i64s[2] = ps;
                i32s[0] = fa;
                i32s[1] = na;
            }
            0
        }
        3 => {
            if !i32_inout.is_null() && !i64_inout.is_null() {
                let i32s = std::slice::from_raw_parts_mut(i32_inout, 3);
                let i64s = std::slice::from_raw_parts_mut(i64_inout, 3);
                let (dfr, ff_sz, nf_sz, npf_sz, ff_art, nf_art) =
                    crate::queuecoord::apply_failed_article_counters(
                        b,
                        c,
                        a,
                        i32s[0],
                        i64s[0],
                        i64s[1],
                        i64s[2],
                        i32s[1],
                        i32s[2],
                    );
                i32s[0] = dfr;
                i32s[1] = ff_art;
                i32s[2] = nf_art;
                i64s[0] = ff_sz;
                i64s[1] = nf_sz;
                i64s[2] = npf_sz;
            }
            0
        }
        4 => {
            if !i32_inout.is_null() && !i64_inout.is_null() {
                let i32s = std::slice::from_raw_parts_mut(i32_inout, 1);
                let i64s = std::slice::from_raw_parts_mut(i64_inout, 3);
                let (rf, rn, pz, comp, fc) =
                    crate::queuecoord::advance_article_completion_counters(
                        i64s[0],
                        i64s[1],
                        i64s[2],
                        a,
                        b != 0,
                        i32s[0],
                        c,
                    );
                i64s[0] = rf;
                i64s[1] = rn;
                i64s[2] = pz;
                i32s[0] = comp;
                return c_int::from(fc);
            }
            0
        }
        5 => {
            if !i32_inout.is_null() && !i64_inout.is_null() {
                let i32s = std::slice::from_raw_parts_mut(i32_inout, 3);
                let i64s = std::slice::from_raw_parts_mut(i64_inout, 5);
                let (mut f_fa, mut n_fa, mut n_sa) = (i32s[0], i32s[1], i32s[2]);
                let (mut f_fs, mut n_fs, mut n_ss, mut np_fs, mut np_ss) =
                    (i64s[0], i64s[1], i64s[2], i64s[3], i64s[4]);
                crate::queuecoord::apply_assembly_failure_counters(
                    a,
                    t1,
                    b != 0,
                    &mut f_fa,
                    &mut f_fs,
                    &mut n_fa,
                    &mut n_sa,
                    &mut n_fs,
                    &mut n_ss,
                    &mut np_fs,
                    &mut np_ss,
                );
                i32s[0] = f_fa;
                i32s[1] = n_fa;
                i32s[2] = n_sa;
                i64s[0] = f_fs;
                i64s[1] = n_fs;
                i64s[2] = n_ss;
                i64s[3] = np_fs;
                i64s[4] = np_ss;
            }
            0
        }
        6 => {
            if !i32_inout.is_null() && !i64_inout.is_null() {
                let i32s = std::slice::from_raw_parts_mut(i32_inout, 6);
                let i64s = std::slice::from_raw_parts_mut(i64_inout, 6);
                let (mut f_sa, mut f_fa, mut n_sa, mut n_fa, mut f_dr, mut n_dr) =
                    (i32s[0], i32s[1], i32s[2], i32s[3], i32s[4], i32s[5]);
                let (mut f_ss, mut f_fs, mut n_ss, mut n_fs, mut np_ss, mut np_fs) =
                    (i64s[0], i64s[1], i64s[2], i64s[3], i64s[4], i64s[5]);
                crate::queuecoord::demote_article_counters(
                    a,
                    b != 0,
                    c != 0,
                    s1_bytes,
                    s2_bytes,
                    &mut f_ss,
                    &mut f_fs,
                    &mut f_sa,
                    &mut f_fa,
                    &mut n_ss,
                    &mut n_fs,
                    &mut np_ss,
                    &mut np_fs,
                    &mut n_sa,
                    &mut n_fa,
                    &mut f_dr,
                    &mut n_dr,
                );
                i32s[0] = f_sa;
                i32s[1] = f_fa;
                i32s[2] = n_sa;
                i32s[3] = n_fa;
                i32s[4] = f_dr;
                i32s[5] = n_dr;
                i64s[0] = f_ss;
                i64s[1] = f_fs;
                i64s[2] = n_ss;
                i64s[3] = n_fs;
                i64s[4] = np_ss;
                i64s[5] = np_fs;
            }
            0
        }
        7 => {
            if !i32_inout.is_null() && !i64_inout.is_null() {
                let i32s = std::slice::from_raw_parts_mut(i32_inout, 2);
                let i64s = std::slice::from_raw_parts_mut(i64_inout, 9);
                let (nr, npz, nsa, nss, npss, nfa, nfs, npfs, fr) =
                    crate::queuecoord::discard_downloaded_articles_counters(
                        i64s[0],
                        i64s[1],
                        i32s[0],
                        i64s[2],
                        i64s[3],
                        i32s[1],
                        i64s[4],
                        i64s[5],
                        a,
                        b,
                        t1,
                        c,
                        t2,
                        i64s[6],
                        i64s[7],
                    );
                i64s[0] = nr;
                i64s[1] = npz;
                i32s[0] = nsa;
                i64s[2] = nss;
                i64s[3] = npss;
                i32s[1] = nfa;
                i64s[4] = nfs;
                i64s[5] = npfs;
                i64s[8] = fr;
            }
            0
        }
        _ => 0,
    }
}

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
    t2: i64,
    t3: i64,
    t4: i64,
    s1: *const c_char,
    s2: *const c_char,
    i32_arr: *const c_int,
    u8_arr: *const u8,
    i64_arr: *const i64,
    len: usize,
    out1: *mut c_int,
    out2: *mut c_int,
    out3: *mut c_int,
    out4: *mut c_int,
    out_arr: *mut c_int,
    out_i64_arr: *mut i64,
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
        0 => c_int::from(crate::repair::should_use_par_quick(
            a != 0,
            b != 0,
            c != 0,
        )),
        1 => {
            let (new_status, set_rep, rep_val, wait_time, par_sec) =
                crate::repair::complete_par_check(a, b, 2, 3, 1, 1, 3, 4, 2, c, d, t1, t2);
            if !out1.is_null() {
                *out1 = c_int::from(set_rep);
            }
            if !out2.is_null() {
                *out2 = c_int::from(rep_val);
            }
            if !out3.is_null() {
                *out3 = wait_time;
            }
            if !out4.is_null() {
                *out4 = par_sec;
            }
            new_status
        }
        2 => {
            let counts = if i32_arr.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(i32_arr, len)
            };
            let (selected, rem_needed) = crate::repair::select_more_pars(counts, a);
            if !out1.is_null() {
                *out1 = rem_needed;
            }
            let copy_len = selected.len().min(len);
            if !out_arr.is_null() && copy_len > 0 {
                for (i, &idx) in selected.iter().take(copy_len).enumerate() {
                    *out_arr.add(i) = idx as c_int;
                }
            }
            copy_len as c_int
        }
        3 => {
            let (new_stage, changed, set_rt, new_rsec, cancel, est_sec, est_min) =
                crate::repair::eval_par_progress_update(
                    a,
                    b,
                    1,
                    2,
                    3,
                    4,
                    t1,
                    t2,
                    t3,
                    c,
                    d != 0,
                    e,
                    f != 0,
                    t4 as i32,
                );
            if !out1.is_null() {
                *out1 = new_stage;
            }
            if !out2.is_null() {
                *out2 = c_int::from(changed)
                    | (c_int::from(set_rt) << 1)
                    | (c_int::from(cancel) << 2);
            }
            if !out3.is_null() {
                *out3 = new_rsec;
            }
            if !out4.is_null() {
                *out4 = est_sec;
            }
            est_min
        }
        4 => {
            if !out_i64_arr.is_null() {
                let ts = std::slice::from_raw_parts_mut(out_i64_arr, 4);
                let (s_stage, s_start, s_par, s_rep) =
                    crate::repair::calc_paused_timestamps(t1, t2, ts[0], ts[1], ts[2], ts[3]);
                ts[0] = s_stage;
                ts[1] = s_start;
                ts[2] = s_par;
                ts[3] = s_rep;
            }
            0
        }
        5 => c_int::from(crate::repair::should_load_partial_segments(
            a,
            2,
            b,
            c != 0,
        )),
        6 => crate::repair::classify_file_crc_status(a, b != 0, c != 0, 1, 3, 2, 1, 3, 2, 0),
        7 => c_int::from(crate::repair::should_consider_twin_candidate(
            a != 0,
            b,
            c,
            d != 0,
            s1_bytes,
            e != 0,
            f != 0,
        )),
        8 => {
            let is_par2 = if u8_arr.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(u8_arr, len)
            };
            let sizes = if i64_arr.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(i64_arr, len)
            };
            crate::repair::select_best_twin_par2_index(is_par2, sizes, t1)
                .map_or(-1, |idx| idx as c_int)
        }
        9 => c_int::from(crate::repair::should_skip_twin_volume_entry(
            a != 0,
            s1_bytes,
            s2_bytes,
        )),
        10 => crate::repair::estimate_twin_vol_blocks(t1, t2 as u64),
        11 => {
            let blocks = if i32_arr.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(i32_arr, len)
            };
            let (avail, covers) = crate::repair::sum_and_check_twin_blocks(blocks, a);
            if !out1.is_null() {
                *out1 = avail;
            }
            c_int::from(covers)
        }
        12 => {
            let blocks = if i32_arr.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(i32_arr, len)
            };
            crate::repair::pick_twin_volume(blocks, a).map_or(-1, |idx| idx as c_int)
        }
        13 => {
            let (usable, eff_blocks, strict_ok) = crate::repair::eval_find_pars_candidate(
                a != 0,
                b,
                t1,
                t2,
                c != 0,
                d != 0,
                s1_bytes,
                s2_bytes,
            );
            if !out1.is_null() {
                *out1 = eff_blocks;
            }
            if !out2.is_null() {
                *out2 = c_int::from(strict_ok);
            }
            c_int::from(usable)
        }
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
        27 => c_int::from(crate::extension::should_skip_queue_event_interval(
            a, b, c, t1, t2,
        )),
        28 => c_int::from(crate::extension::should_ignore_queued_script_event(
            a != 0,
            b,
            0,
            c,
            1,
            d,
            6,
            2,
        )),
        29 => {
            if a == 0 {
                return 0;
            }
            let event_name =
                if b >= 0 && (b as usize) < crate::extension::QUEUE_EVENT_NAMES.len() {
                    crate::extension::QUEUE_EVENT_NAMES[b as usize]
                } else {
                    b"UNKNOWN"
                };
            if !s1_bytes.is_empty()
                && !s1_bytes.windows(event_name.len()).any(|w| w == event_name)
            {
                return 0;
            }
            if s2_bytes.eq_ignore_ascii_case(b"yes")
                || s2_bytes.eq_ignore_ascii_case(b"on")
                || s2_bytes == b"1"
            {
                return 1;
            }
            if b == 1 {
                2
            } else {
                0
            }
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_stream_repair_ctrl_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    d: c_int,
    e: c_int,
    t1: i64,
    t2: i64,
    t3: i64,
    f1: f64,
    s1: *const c_char,
    cstr_arr: *const *const c_char,
    i64_arr: *const i64,
    len: usize,
    out1: *mut c_int,
    out2: *mut c_int,
    i64_inout: *mut i64,
    out_buf: *mut RsBuf,
) -> c_int {
    let s1_bytes = if s1.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(s1).to_bytes()
    };
    match op {
        0 => c_int::from(crate::streamrepair::is_par2_job_or_file(a != 0, s1_bytes)),
        1 => {
            let mut names = Vec::with_capacity(len);
            if !cstr_arr.is_null() {
                for &p in std::slice::from_raw_parts(cstr_arr, len) {
                    names.push(if p.is_null() {
                        &[][..]
                    } else {
                        CStr::from_ptr(p).to_bytes()
                    });
                }
            }
            let sizes = if i64_arr.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(i64_arr, len)
            };
            let (rank, win) =
                crate::streamrepair::compute_positional_rank(&names, sizes, s1_bytes, t1);
            if !out1.is_null() {
                *out1 = rank;
            }
            if !out2.is_null() {
                *out2 = win;
            }
            rank
        }
        2 => {
            if !out1.is_null() && !i64_inout.is_null() {
                let i64s = std::slice::from_raw_parts_mut(i64_inout, 2);
                let (outcome, new_art, new_fsz, new_pfsz) =
                    crate::streamrepair::classify_remaining_hole_target(
                        a != 0,
                        t1,
                        b != 0,
                        c != 0,
                        d,
                        t2,
                        t3,
                        *out1,
                        i64s[0],
                        i64s[1],
                    );
                *out1 = new_art;
                i64s[0] = new_fsz;
                i64s[1] = new_pfsz;
                return outcome;
            }
            0
        }
        3 => c_int::from(crate::streamrepair::is_incomplete_nzb_hole(t1, t2)),
        4 => {
            if !i64_inout.is_null() {
                let s = std::slice::from_raw_parts_mut(i64_inout, 8);
                let (nf, ns, ncf, ncs, npf, nps, npcf, npcs) =
                    crate::streamrepair::credit_repaired_sizes(
                        t1, t2, s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
                    );
                s[0] = nf;
                s[1] = ns;
                s[2] = ncf;
                s[3] = ncs;
                s[4] = npf;
                s[5] = nps;
                s[6] = npcf;
                s[7] = npcs;
            }
            0
        }
        5 => {
            let (reset_pr, set_ps, new_ps, set_fpf, clr_rpc, chk_mp) =
                crate::streamrepair::resolve_completion_par_action(
                    t1,
                    a != 0,
                    b,
                    0,
                    c,
                    1,
                    d,
                    e,
                    5,
                    0,
                );
            if !out1.is_null() {
                *out1 = c_int::from(reset_pr)
                    | (c_int::from(set_ps) << 1)
                    | (c_int::from(set_fpf) << 2)
                    | (c_int::from(clr_rpc) << 3)
                    | (c_int::from(chk_mp) << 4);
            }
            new_ps
        }
        6 => {
            if !i64_inout.is_null() && !i64_arr.is_null() {
                let ins = std::slice::from_raw_parts(i64_arr, 6);
                let mut new_prog = 0i64;
                let mut reset_last = false;
                let reason = crate::streamrepair::watchdog_eval_tick(
                    t1,
                    t2,
                    a != 0,
                    b,
                    ins[0],
                    ins[1],
                    ins[2],
                    f1,
                    c,
                    d,
                    e,
                    len as i32,
                    ins[3],
                    ins[4],
                    ins[5],
                    &mut new_prog,
                    &mut reset_last,
                );
                *i64_inout = new_prog;
                if !out1.is_null() {
                    *out1 = c_int::from(reset_last);
                }
                if let Some(msg) = reason {
                    if !out_buf.is_null() {
                        *out_buf = into_buf(msg);
                    }
                    return 1;
                }
            }
            0
        }
        7 => {
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::streamrepair::format_whole_files_suffix(a));
            }
            0
        }
        8 => {
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::repair::build_twin_filename(s1_bytes, len));
            }
            0
        }
        9 => {
            if !out_buf.is_null() {
                let reason = crate::repair::twin_par2_reject_reason(a);
                let end = reason.iter().position(|&b| b == 0).unwrap_or(reason.len());
                *out_buf = into_buf(reason[..end].to_vec());
            }
            0
        }
        10 => {
            let norm = crate::extension::normalize_special_env_var_name(s1_bytes);
            if norm.as_slice() != s1_bytes {
                if !out_buf.is_null() {
                    *out_buf = into_buf(norm);
                }
                return 1;
            }
            0
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nzbget_rs_dupesearch_op(
    op: c_int,
    a: c_int,
    b: c_int,
    c: c_int,
    d: c_int,
    e: c_int,
    f: c_int,
    g: c_int,
    t1: i64,
    t2: i64,
    f1: f64,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    cstr1: *const *const c_char,
    cstr2: *const *const c_char,
    u8_1: *const u8,
    u8_2: *const u8,
    i32_1: *const c_int,
    i32_2: *const c_int,
    i64_1: *const i64,
    i64_2: *const i64,
    len: usize,
    out1: *mut c_int,
    out2: *mut c_int,
    out_arr: *mut c_int,
    out_i64: *mut i64,
    out_buf: *mut RsBuf,
) -> c_int {
    let b_or_empty = |p: *const c_char| -> &[u8] {
        if p.is_null() {
            &[][..]
        } else {
            CStr::from_ptr(p).to_bytes()
        }
    };
    let s1_bytes = b_or_empty(s1);
    let s2_bytes = b_or_empty(s2);
    let s3_bytes = b_or_empty(s3);
    let s4_bytes = b_or_empty(s4);
    match op {
        0 => {
            let mut keys = Vec::with_capacity(len);
            let mut vals = Vec::with_capacity(len);
            if !cstr1.is_null() && !cstr2.is_null() && len > 0 {
                let ks = std::slice::from_raw_parts(cstr1, len);
                let vs = std::slice::from_raw_parts(cstr2, len);
                for i in 0..len {
                    keys.push(b_or_empty(ks[i]));
                    vals.push(b_or_empty(vs[i]));
                }
            }
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::dupesearch_nzb::build_newznab_url_percent(
                    s1_bytes, &keys, &vals, s2_bytes,
                ));
            }
            0
        }
        1 => c_int::from(crate::dupesearch_nzb::saved_history_backup(
            a != 0,
            b as u32,
            c,
            d as u32,
            3,
            6,
        )),
        2 => {
            let qf_exists = !s1_bytes.is_empty()
                && crate::fsops::file_exists(s1_bytes, &crate::fsops::posix_stat);
            let has_marked_alive = !s2.is_null();
            let marked_alive_val = if has_marked_alive {
                crate::dupesearch_nzb::parse_atoi_bytes(s2_bytes)
            } else {
                0
            };
            c_int::from(crate::dupesearch_nzb::placement_available(
                (a & 1) != 0,
                (a & 2) != 0,
                (a & 4) != 0,
                b,
                0,
                1,
                3,
                6,
                c,
                1,
                d,
                2,
                (a & 8) != 0,
                (a & 16) != 0,
                (a & 32) != 0,
                (a & 64) != 0,
                (a & 128) != 0,
                e as u32,
                f as u32,
                qf_exists,
                g,
                t1 as i32,
                (a & 256) != 0,
                has_marked_alive,
                marked_alive_val,
            ))
        }
        3 => {
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::dupesearch_nzb::lower_key(s1_bytes));
            }
            0
        }
        4 => {
            let qf = if !cstr1.is_null() {
                b_or_empty(*cstr1)
            } else {
                &[][..]
            };
            let item_low = crate::dupesearch_nzb::lower_key(s1_bytes);
            let same_key = item_low.as_slice() == s2_bytes;
            let same_rel = crate::releasename::same_release(s3_bytes, s4_bytes);
            let add_known = d == 0 && !qf.is_empty() && f != 1 && (same_key || same_rel);
            let item_score = (t1 >> 32) as i32;
            let pick_score = (t1 as u32) as i32;
            let item_hash = (t2 >> 32) as u32;
            let pick_hash = t2 as u32;
            let add_member = if a == 0 {
                crate::dupesearch_nzb::dupesearch_is_rank_only_queued_member(
                    b != 0,
                    c != 0,
                    d,
                    0,
                    e,
                    0,
                    g != 0,
                    f,
                    0,
                    3,
                    item_score,
                    pick_score,
                ) && same_key
                    && same_rel
                    && !qf.is_empty()
                    && !qf.contains(&b'|')
            } else {
                c == 0
                    && crate::dupesearch_nzb::saved_history_backup(
                        true, pick_hash, f, item_hash, 3, 6,
                    )
                    && same_key
                    && same_rel
                    && !qf.is_empty()
                    && !qf.contains(&b'|')
            };
            c_int::from(add_known) | (c_int::from(add_member) << 1)
        }
        5 => crate::dupesearch_nzb::dupesearch_calc_expand_budget_ms(a, t1, t2),
        6 => {
            let reason = crate::dupesearch_nzb::dupesearch_classify_search_retry_reason(
                a,
                b,
                c != 0,
                d != 0,
                e,
                f,
                g,
            );
            if reason != 0 && !out_buf.is_null() {
                let msg: &[u8] = match reason {
                    1 => b"no indexer answered",
                    2 => b"an indexer query failed and nothing came back",
                    _ => b"no posting could be fetched",
                };
                *out_buf = into_buf(msg.to_vec());
            }
            reason
        }
        7 => {
            if !i64_1.is_null() && !i32_1.is_null() && !i64_2.is_null() && !out_arr.is_null() && len > 0 {
                let tb = std::slice::from_raw_parts(i64_1, len);
                let gr = std::slice::from_raw_parts(i32_1, len);
                let dt = std::slice::from_raw_parts(i64_2, len);
                let mut order = vec![0usize; len];
                crate::dupesearch_nzb::sort_verified_postings(tb, gr, dt, t1, &mut order);
                let out_s = std::slice::from_raw_parts_mut(out_arr, len);
                for i in 0..len {
                    out_s[i] = order[i] as c_int;
                }
            }
            0
        }
        8 => {
            let (score, pct, add_zero) = crate::dupesearch_nzb::eval_member_rerank(
                a,
                b,
                c,
                d,
                f1,
                e != 0,
            );
            if !out1.is_null() {
                *out1 = pct;
            }
            if !out2.is_null() {
                *out2 = c_int::from(add_zero);
            }
            score
        }
        9 => {
            let mut keys = Vec::with_capacity(len);
            let counts = if i32_1.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(i32_1, len)
            };
            if !cstr1.is_null() && len > 0 {
                for &p in std::slice::from_raw_parts(cstr1, len) {
                    keys.push(b_or_empty(p));
                }
            }
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::dupesearch_nzb::dupesearch_format_rejected_outcome(
                    &keys, counts,
                ));
            }
            0
        }
        10 => {
            let (_, retry_disc, rank_inc) = crate::dupesearch_nzb::dupesearch_eval_post_place_retry(
                a != 0,
                b != 0,
                c != 0,
                d as usize,
            );
            if !out1.is_null() {
                *out1 = c_int::from(retry_disc);
            }
            if !out2.is_null() {
                *out2 = c_int::from(rank_inc);
            }
            0
        }
        11 => {
            let pick_norm = crate::releasename::normalize(s3_bytes);
            let job_norm = crate::releasename::normalize(s4_bytes);
            let code = crate::dupesearch_nzb::eval_pick_gone(
                a != 0,
                b != 0,
                c,
                1,
                s1_bytes,
                s2_bytes,
                &pick_norm,
                &job_norm,
            );
            if code != 0 && !out_buf.is_null() {
                let msg: &[u8] = match code {
                    1 => b"the pick was deleted",
                    2 => b"the duplicate key of the pick changed",
                    _ => b"the name of the pick changed",
                };
                *out_buf = into_buf(msg.to_vec());
            }
            code
        }
        12 => c_int::from(crate::dupesearch_nzb::mark_searching_pending(
            s1_bytes,
            s2_bytes,
            s3_bytes,
            a != 0,
        )),
        13 => c_int::from(crate::dupesearch_nzb::save_pending_header(
            s1_bytes, s2_bytes, s3_bytes, s4_bytes,
        )),
        14 => {
            let data = if u8_1.is_null() || len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(u8_1, len)
            };
            c_int::from(crate::dupesearch_nzb::save_pending_item(
                s1_bytes,
                a as usize,
                s2_bytes,
                s3_bytes,
                b,
                t1,
                t2,
                data,
                c != 0,
            ))
        }
        15 => c_int::from(crate::dupesearch_nzb::save_pending_finish(s1_bytes)),
        16 => crate::dupesearch_nzb::dupesearch_parse_pending_dir_id(s1_bytes),
        17 => c_int::from(crate::dupesearch_nzb::check_pending_identity(
            s1_bytes, s2_bytes, s3_bytes, s4_bytes,
        )),
        18 => {
            let (alive, dead, comp) = crate::dupesearch_nzb::eval_fleet_member_health(
                a != 0,
                b != 0,
                c,
                d,
                e,
                f,
                g,
            );
            if !out1.is_null() {
                *out1 = alive;
            }
            if !out2.is_null() {
                *out2 = c_int::from(dead);
            }
            c_int::from(comp)
        }
        19 => {
            if !u8_1.is_null()
                && !i32_1.is_null()
                && !u8_2.is_null()
                && !i32_2.is_null()
                && !i64_1.is_null()
                && !out_arr.is_null()
                && len > 0
            {
                let has_err = std::slice::from_raw_parts(u8_1, len);
                let same_as = std::slice::from_raw_parts(i32_1, len);
                let dead = std::slice::from_raw_parts(u8_2, len);
                let alive = std::slice::from_raw_parts(i32_2, len);
                let tb = std::slice::from_raw_parts(i64_1, len);
                let mut ranked = vec![0usize; len];
                let n = crate::dupesearch_nzb::rank_fleet_candidates(
                    has_err,
                    same_as,
                    dead,
                    alive,
                    tb,
                    &mut ranked,
                );
                let out_s = std::slice::from_raw_parts_mut(out_arr, n);
                for i in 0..n {
                    out_s[i] = ranked[i] as c_int;
                }
                return n as c_int;
            }
            0
        }
        20 => {
            if a != 0 || !s1_bytes.eq_ignore_ascii_case(s2_bytes) {
                return 0;
            }
            if !out1.is_null() && !out2.is_null() && !out_arr.is_null() && !out_i64.is_null() {
                let (nt, nl, nrid, nrsc) = crate::dupesearch_nzb::fleet_update_key_score_bounds(
                    *out_i64,
                    *out1,
                    g != 0,
                    b != 0,
                    c != 0,
                    d,
                    0,
                    *out2,
                    *out_arr,
                    e,
                    f,
                );
                *out_i64 = nt;
                *out1 = nl;
                *out2 = nrid;
                *out_arr = nrsc;
            }
            1
        }
        21 => crate::dupesearch_nzb::fleet_calc_member_add_score(a, b, t1, c),
        22 => {
            let mut reason_bytes = s2_bytes.to_vec();
            let mut updated_reason = 0;
            if a == 0 && reason_bytes.is_empty() {
                let code = crate::dupesearch_nzb::eval_fleet_outcome_reason(
                    c != 0,
                    d != 0,
                    e != 0,
                    f != 0,
                );
                let r: &[u8] = match code {
                    0 => b"NO_USABLE_MEMBERS",
                    1 => b"INCOMPLETE",
                    2 => b"ALL_DEAD",
                    3 => b"ALREADY_DOWNLOADED",
                    _ => b"NOT_QUEUED",
                };
                reason_bytes = r.to_vec();
                updated_reason = 1;
                if !out1.is_null() {
                    *out1 = code;
                }
            }
            if !out_buf.is_null() {
                *out_buf = into_buf(crate::dupesearch_nzb::fleet_format_summary_log(
                    g,
                    s1_bytes,
                    a,
                    &reason_bytes,
                    b != 0,
                ));
            }
            updated_reason
        }
        23 => c_int::from(crate::dupesearch_nzb::dupesearch_is_better_parent_candidate(
            a != 0,
            b,
            0,
            c != 0,
            d,
            0,
            e != 0,
            crate::dupesearch_nzb::lower_key(s1_bytes).as_slice() == s2_bytes,
            f,
            g,
            t1 != 0,
            t2 as i32,
        )),
        _ => 0,
    }
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
    pub send_cb:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, c_int) -> c_int>,
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
    let mut tls_error = matches!(op, 0 | 1 | 3 | 7 | 8 | 9 | 11)
        && !e.tls_error.is_null()
        && *e.tls_error != 0;

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
    pub read_line_cb:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut c_char, c_int, *mut c_int) -> c_int>,
    pub read_body_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut c_char, c_int) -> c_int>,
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
