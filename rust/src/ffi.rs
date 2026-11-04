//! C ABI. Strings come in as NUL-terminated C strings; results go out as a
//! buffer the caller copies and then frees with nzbget_rs_free.

use std::ffi::{c_char, c_int, CStr};

#[path = "tlscipher/native.rs"]
mod tls_cipher_native;

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
/// # Safety
/// argv contains argc valid NUL-terminated strings from main; argc must be 5.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_tls_cipher_resolver(argc: c_int, argv: *const *const c_char) -> c_int {
    if argc != 5 || argv.is_null() { return 2; }
    let pointers = unsafe { std::slice::from_raw_parts(argv, argc as usize) };
    let mut args = Vec::new();
    for &pointer in &pointers[1..] {
        if pointer.is_null() { return 2; }
        let Ok(value) = unsafe { CStr::from_ptr(pointer) }.to_str() else { return 2; };
        args.push(value.to_owned());
    }
    tls_cipher_native::resolver(&args)
}

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

fn into_buf(mut v: Vec<u8>) -> RsBuf {
    v.push(0);
    let len = v.len() - 1;
    let mut v = std::mem::ManuallyDrop::new(v);
    RsBuf { data: v.as_mut_ptr() as *mut c_char, len, cap: v.capacity() }
}

unsafe fn input<'a>(raw: *const c_char) -> &'a [u8] {
    if raw.is_null() { &[] } else { CStr::from_ptr(raw).to_bytes() }
}

/// # Safety
/// `raw` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_json_encode(raw: *const c_char) -> RsBuf {
    let mut v = Vec::new();
    crate::escape::json_encode(input(raw), &mut v);
    into_buf(v)
}

type EncoderSpans<'a> = (&'a [u8], &'a mut [std::mem::MaybeUninit<u8>]);

/// Validate all representability/endpoints/overlap before creating borrows.
///
/// # Safety
/// For accepted ranges, raw is readable through its first NUL at input_len,
/// and non-null out is writable for cap bytes. Empty/null ranges are handled
/// without constructing null slices.
unsafe fn known_encoder_spans<'a, const EXPANSION: usize>(raw: *const c_char, input_len: usize, out: *mut c_char, cap: usize) -> Option<EncoderSpans<'a>> {
    if input_len > (isize::MAX as usize - 1) / EXPANSION || raw.is_null() && input_len != 0 {
        return None;
    }
    let source_range = if raw.is_null() { None } else {
        Some((raw as usize, (raw as usize).checked_add(input_len + 1)?))
    };
    if !out.is_null() {
        if cap > isize::MAX as usize { return None; }
        let begin = out as usize;
        let end = begin.checked_add(cap)?;
        if cap != 0 && source_range.is_some_and(|(s, e)| begin < e && s < end) { return None; }
    }
    let source = if input_len == 0 { &[] } else { std::slice::from_raw_parts(raw.cast::<u8>(), input_len) };
    let output = if out.is_null() || cap == 0 { &mut [] } else {
        std::slice::from_raw_parts_mut(out.cast::<std::mem::MaybeUninit<u8>>(), cap)
    };
    Some((source, output))
}


/// # Safety
/// `raw` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xml_encode(raw: *const c_char) -> RsBuf {
    let mut v = Vec::new();
    crate::escape::xml_encode(input(raw), &mut v);
    into_buf(v)
}

// ---- twins and alts (twin.rs); lists cross the ABI as text lines ----

fn sigs_text(sigs: &[crate::twin::FileSig]) -> Vec<u8> {
    let mut v = Vec::with_capacity(sigs.iter().map(|s| s.name.len() + s.md5.len() + 24).sum());
    for s in sigs {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        let mut rest = s.length;
        loop {
            at -= 1;
            digits[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        v.extend_from_slice(&digits[at..]);
        v.push(b'\t');
        v.extend_from_slice(s.md5.as_bytes());
        v.push(b'\t');
        // the line's own separators in a name are made "_" (in UTF-8 text, byte by byte)
        v.extend(s.name.bytes().map(|b| if matches!(b, b'\n' | b'\t' | b'\r') { b'_' } else { b }));
        v.push(b'\n');
    }
    v
}

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

/// The par2 data's file list as "length\tmd5\tname" lines, and nothing stored.
///
/// # Safety
/// `data` points to `len` readable bytes (or is null with `len` 0).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_twin_par2_sigs(data: *const u8, len: usize) -> RsBuf {
    into_buf(sigs_text(&crate::twin::par2_sigs(bytes_arg(data, len))))
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

/// The <file> entries of the nzb-file at `path`, as lines: "F\tsize\tpar2\tfilename\tsubject",
/// then its "G\tgroup" and "S\tbytes\tmessage-id" lines.
///
/// # Safety
/// `path` is null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_entries(path: *const c_char) -> RsBuf {
    let xml = std::fs::read(path_arg(path)).unwrap_or_default();
    let clean = |s: &str| s.replace(['\n', '\t', '\r'], "_");
    let mut v = Vec::new();
    for e in crate::twin::read_nzb(&xml) {
        v.extend_from_slice(format!("F\t{}\t{}\t{}\t{}\n", e.size(), u8::from(e.is_par2()), clean(&e.filename), clean(&e.subject)).as_bytes());
        for g in &e.groups {
            v.extend_from_slice(format!("G\t{}\n", clean(g)).as_bytes());
        }
        for (bytes, id) in &e.segments {
            v.extend_from_slice(format!("S\t{bytes}\t{}\n", clean(id)).as_bytes());
        }
    }
    into_buf(v)
}

/// # Safety
/// `buf` came from an nzbget_rs_* function and wasn't freed yet.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_free(buf: RsBuf) {
    if !buf.data.is_null() {
        drop(Vec::from_raw_parts(buf.data as *mut u8, buf.len + 1, buf.cap));
    }
}

/// Both fields are meaningful on failure, too: the legacy matcher retains
/// partial captures. Count can exceed capacity after backtracking.
#[repr(C)]
pub struct WildResult {
    pub matched: c_int,
    pub count: usize,
}

/// Match, folding case with glibc's tolower table `table` (indexed -128..=255)
/// when it isn't null, else with the caller's `fold` (tolower of a byte in the
/// current locale). Writes up to capacity
/// pairs and returns the total count; retry with that capacity if needed.
/// NULL positions disables capture collection, irrespective of capacity.
///
/// # Safety
/// `pattern` and `text` are null (empty) or valid NUL-terminated strings.
/// `table` is null or valid for indexes -128..=255 (`char_signed`: whether the
/// caller's char is signed, which picks the index of bytes from 0x80); `fold`,
/// when supplied, takes a byte value 0..=255 and must not unwind. A null `fold`
/// is ignored when a table is supplied; with neither, the result is (0, 0).
/// `positions` is null or points to
/// `capacity` writable pairs of C ints, disjoint from all the input storage.
/// All buffers remain caller-owned. Panics abort rather than crossing the ABI.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_wild_match(
    pattern: *const c_char,
    text: *const c_char,
    positions: *mut [c_int; 2],
    capacity: usize,
    table: *const c_int,
    char_signed: c_int,
    fold: Option<extern "C" fn(c_int) -> c_int>,
) -> WildResult {
    if table.is_null() && fold.is_none() {
        return WildResult { matched: 0, count: 0 };
    }
    let call = |b: u8| fold.expect("callback checked above")(b as c_int);
    let lower = if table.is_null() {
        crate::wildmask::Lower::Fold(&call)
    } else {
        // SAFETY: the caller supplies entries -128..=255, with `table`
        // pointing at entry zero. Keep raw-pointer handling at the FFI edge.
        crate::wildmask::Lower::Table(&*table.sub(128).cast::<[c_int; 384]>(), char_signed != 0)
    };
    let mut v = Vec::new();
    let matched = crate::wildmask::wild_match(
        &lower, input(pattern), input(text),
        if positions.is_null() { None } else { Some(&mut v) },
    );
    let count = v.len();
    // Avoid even constructing a slice from NULL for zero-length output.
    if !positions.is_null() && capacity != 0 && count != 0 {
        let out = std::slice::from_raw_parts_mut(positions, count.min(capacity));
        for (slot, (start, len)) in out.iter_mut().zip(v) {
            *slot = [start, len];
        }
    }
    WildResult { matched: matched.into(), count }
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

/// The next JSON value in `text` (WebUtil::JsonNextValue): its start, with
/// its length in `value_length`, or null.
///
/// # Safety
/// `text` is null or a NUL-terminated string; `value_length` is null or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_json_next_value(text: *const c_char, value_length: *mut c_int) -> *const c_char {
    if text.is_null() || value_length.is_null() {
        return std::ptr::null();
    }
    match crate::decode::json_next_value(CStr::from_ptr(text).to_bytes()) {
        Some((start, len)) => {
            *value_length = len as c_int;
            text.add(start)
        }
        None => std::ptr::null(),
    }
}

/// Crc32::Combine: the CRC of A then B from CRC(A), CRC(B) and B's length.
#[no_mangle]
pub extern "C" fn nzbget_rs_crc32_combine(crc1: u32, crc2: u32, len2: u32) -> u32 {
    crate::crc::combine(crc1, crc2, len2)
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
/// `raw` is null or a writable NUL-terminated string. `lower`, if supplied,
/// takes an ASCII hex letter and returns the caller's tolower result; it must
/// not unwind or access `raw`. A null callback leaves the buffer unchanged.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xml_decode(raw: *mut c_char, lower: Option<extern "C" fn(c_int) -> c_int>) {
    let Some(lower) = lower else { return };
    in_place(raw, |b| crate::text::xml_decode(b, &|c| lower(c as c_int)))
}

/// WebUtil::XmlStripTags, in place.
///
/// # Safety
/// `raw` is null or a writable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xml_strip_tags(raw: *mut c_char) {
    in_place(raw, |b| {
        crate::text::xml_strip_tags(b);
        b.len()
    })
}

/// WebUtil::XmlRemoveEntities, in place; `is_alpha` takes a byte in 0..=255
/// and classifies it in the caller's locale and char signedness.
/// A null callback leaves the buffer unchanged.
///
/// # Safety
/// `raw` is null or a writable NUL-terminated string; `is_alpha`, if supplied,
/// doesn't unwind or access `raw`.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_xml_remove_entities(raw: *mut c_char, is_alpha: Option<extern "C" fn(c_int) -> c_int>) {
    let Some(is_alpha) = is_alpha else { return };
    in_place(raw, |b| crate::text::xml_remove_entities(b, &|c| is_alpha(c as c_int) != 0))
}

/// WebUtil::HttpUnquote, in place.
///
/// # Safety
/// `raw` is null or a writable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_http_unquote(raw: *mut c_char) {
    in_place(raw, crate::text::http_unquote)
}

/// WebUtil::UrlDecode, in place.
///
/// # Safety
/// `raw` is null or a writable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_url_decode(raw: *mut c_char) {
    in_place(raw, crate::text::url_decode)
}

/// WebUtil::UrlEncode; free the result with nzbget_rs_free.
///
/// # Safety
/// `raw` is null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_url_encode(raw: *const c_char) -> RsBuf {
    let mut v = Vec::new();
    crate::text::url_encode(input(raw), &mut v);
    into_buf(v)
}

/// WebUtil::Latin1ToUtf8; free the result with nzbget_rs_free.
///
/// # Safety
/// `raw` is null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_latin1_to_utf8(raw: *const c_char) -> RsBuf {
    let mut v = Vec::new();
    crate::text::latin1_to_utf8(input(raw), &mut v);
    into_buf(v)
}

/// Latin1 into caller-owned potentially uninitialized storage. Returns required
/// bytes excluding NUL, or SIZE_MAX for rejected bounds or overlap. If n < cap,
/// result and NUL are initialized; otherwise retry with n + 1. Null output
/// measures with cap ignored. Rust never allocates or frees caller storage.
///
/// # Safety
/// Source is null only at input_len zero, otherwise readable through its first
/// NUL exactly at input_len. Non-null output is writable for cap bytes.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_latin1_to_utf8_n(raw: *const c_char, input_len: usize, out: *mut c_char, cap: usize) -> usize {
    let Some((source, output)) = known_encoder_spans::<2>(raw, input_len, out, cap) else { return usize::MAX };
    let capacity = output.len();
    let dst = if capacity == 0 { std::ptr::null_mut() } else { output.as_mut_ptr().cast() };
    crate::text::latin1_known_into(source, raw.cast(), dst, capacity)
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

/// WebUtil::JsonFindField: a pointer into `text` and the value length, or
/// null (`value_length` untouched).
///
/// # Safety
/// `text` and `field` are null or NUL-terminated; `value_length` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_json_find_field(text: *const c_char, field: *const c_char, value_length: *mut c_int) -> *const c_char {
    if text.is_null() || field.is_null() || value_length.is_null() {
        return std::ptr::null();
    }
    match crate::webutil::json_find_field(input(text), input(field)) {
        Some((start, len)) => {
            *value_length = len as c_int;
            text.add(start)
        }
        None => std::ptr::null(),
    }
}

/// WebUtil::ParseContentDispositionFilename; `data` is null for no file name
/// (the C++ null CString). Case folds as strncasecmp: glibc's tolower `table`
/// (entries -128..=255, pointing at entry zero) indexed by unsigned byte, or
/// `fold` (tolower of a byte 0..=255) when `table` is null.
///
/// # Safety
/// `cd` is null or NUL-terminated; `table` is null or as described; `fold`
/// doesn't unwind.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_content_disposition_filename(
    cd: *const c_char,
    table: *const c_int,
    fold: Option<extern "C" fn(c_int) -> c_int>,
) -> RsBuf {
    let none = RsBuf { data: std::ptr::null_mut(), len: 0, cap: 0 };
    if table.is_null() && fold.is_none() {
        return none;
    }
    let call = |b: u8| fold.expect("callback checked above")(b as c_int);
    let lower = if table.is_null() {
        crate::wildmask::Lower::Fold(&call)
    } else {
        crate::wildmask::Lower::Table(&*table.sub(128).cast::<[c_int; 384]>(), false)
    };
    match crate::webutil::content_disposition_filename(input(cd), &lower) {
        Some(v) => into_buf(v),
        None => none,
    }
}

/// Util::FormatSize; free the result with nzbget_rs_free.
#[no_mangle]
pub extern "C" fn nzbget_rs_format_size(size: i64) -> RsBuf {
    into_buf(crate::util::format_size(size))
}

/// Util::FormatSpeed; free the result with nzbget_rs_free.
#[no_mangle]
pub extern "C" fn nzbget_rs_format_speed(bytes_per_second: i64) -> RsBuf {
    into_buf(crate::util::format_speed(bytes_per_second))
}

/// Util::AlphaNum.
///
/// # Safety
/// `s` is null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_alpha_num(s: *const c_char) -> c_int {
    crate::util::alpha_num(input(s)) as c_int
}

/// Util::HashBJ96 over `len` bytes (the C++ uint32 of an int length).
///
/// # Safety
/// `buf` is null or readable for `len` bytes (as uint32), at most isize::MAX.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hash_bj96(buf: *const c_char, len: c_int, init: u32) -> u32 {
    let len = len as u32 as usize;
    if buf.is_null() || len == 0 {
        return crate::util::hash_bj96(&[], init);
    }
    crate::util::hash_bj96(std::slice::from_raw_parts(buf.cast(), len), init)
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

/// Util::MatchFileExt. Case folding: glibc's tolower `table` (entries
/// -128..=255, pointing at entry zero) when not null, indexed by unsigned
/// byte for the extension compare (strcasecmp) and as the C++ char
/// (`char_signed`) for wildcard extensions (WildMask); else the callbacks
/// `case_fold` (a byte 0..=255) and `mask_fold` (as WildMask's).
///
/// # Safety
/// The strings are null or NUL-terminated; `table` is null or as described;
/// the callbacks don't unwind or mutate inputs. Callbacks may be null when
/// a table is supplied.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_match_file_ext(
    filename: *const c_char,
    list: *const c_char,
    separators: *const c_char,
    table: *const c_int,
    char_signed: c_int,
    case_fold: Option<extern "C" fn(c_int) -> c_int>,
    mask_fold: Option<extern "C" fn(c_int) -> c_int>,
) -> c_int {
    use crate::wildmask::Lower;
    if table.is_null() && (case_fold.is_none() || mask_fold.is_none()) {
        return 0;
    }
    let case_call = |b: u8| case_fold.expect("callback checked above")(b as c_int);
    let mask_call = |b: u8| mask_fold.expect("callback checked above")(b as c_int);
    let (case_lower, mask_lower) = if table.is_null() {
        (Lower::Fold(&case_call), Lower::Fold(&mask_call))
    } else {
        let t = &*table.sub(128).cast::<[c_int; 384]>();
        (Lower::Table(t, false), Lower::Table(t, char_signed != 0))
    };
    crate::util::match_file_ext(input(filename), input(list), input(separators), &case_lower, &mask_lower) as c_int
}

/// URL::ParseUrl's result: each part as a start and length in the address,
/// start -1 for a part not set.
#[repr(C)]
pub struct UrlParts {
    pub valid: c_int,
    pub port: c_int,
    /// protocol, user, password, host, resource: [start, length]
    pub parts: [[isize; 2]; 5],
}

/// URL::ParseUrl (rust/src/url.rs). A valid URL without a resource has
/// resource start -1: the caller uses "/".
///
/// # Safety
/// `address` is null or NUL-terminated; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_parse_url(address: *const c_char, out: *mut UrlParts) {
    if out.is_null() {
        return;
    }
    let mut r = UrlParts { valid: 0, port: 0, parts: [[-1, 0]; 5] };
    if !address.is_null() {
        let u = crate::url::parse_url(CStr::from_ptr(address));
        r.valid = u.valid as c_int;
        r.port = u.port;
        for (k, p) in [u.protocol, u.user, u.password, u.host, u.resource].into_iter().enumerate() {
            if let Some((start, len)) = p {
                r.parts[k] = [start as isize, len as isize];
            }
        }
    }
    *out = r;
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
    /// RegEx(pattern, bufSize): a handle
    pub regex_new: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, c_int) -> usize>,
    /// RegEx::Match: -1 when it doesn't match, else GetMatchCount, with up to
    /// `capacity` (start, length) pairs written
    pub regex_match: Option<unsafe extern "C" fn(*mut std::ffi::c_void, usize, *const c_char, *mut [c_int; 2], c_int) -> c_int>,
    pub apply: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const FeedOptions)>,
    pub set_match: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, c_int)>,
    /// case folding for WildMask: glibc's tolower table (entries -128..=255,
    /// pointing at entry zero) or null for `fold`
    pub lower_table: *const c_int,
    pub char_signed: c_int,
    pub fold: Option<extern "C" fn(c_int) -> c_int>,
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
    lower: crate::wildmask::Lower<'c>,
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

    fn regex_new(&mut self, pattern: &[u8], buf_size: i32) -> usize {
        let p = std::ffi::CString::new(pattern).unwrap_or_default();
        unsafe { (self.cb.regex_new.expect("callbacks validated"))(self.cb.user, p.as_ptr(), buf_size) }
    }

    fn regex_match(&mut self, handle: usize, text: &[u8]) -> Option<Vec<(i32, i32)>> {
        let t = std::ffi::CString::new(text).unwrap_or_default();
        let mut groups = [[0 as c_int; 2]; 100];
        let n = unsafe { (self.cb.regex_match.expect("callbacks validated"))(self.cb.user, handle, t.as_ptr(), groups.as_mut_ptr(), 100) };
        (n >= 0).then(|| groups[..(n as usize).min(100)].iter().map(|g| (g[0], g[1])).collect())
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

    fn lower(&self) -> &crate::wildmask::Lower<'_> {
        &self.lower
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
/// A nonnull lower_table spans entries -128..255 as described above.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_filter_match(filter: *mut crate::feedfilter::FeedFilter, item: *const FeedItemCallbacks) {
    if filter.is_null() || item.is_null() {
        return;
    }
    let cb = &*item;
    if cb.field.is_none() || cb.season_episode.is_none() || cb.regex_new.is_none()
        || cb.regex_match.is_none() || cb.apply.is_none() || cb.set_match.is_none()
        || (cb.lower_table.is_null() && cb.fold.is_none())
    {
        return;
    }
    let fold = cb.fold;
    let call = move |b: u8| fold.expect("callbacks validated")(b as c_int);
    let lower = if cb.lower_table.is_null() {
        crate::wildmask::Lower::Fold(&call)
    } else {
        crate::wildmask::Lower::Table(&*cb.lower_table.sub(128).cast::<[c_int; 384]>(), cb.char_signed != 0)
    };
    let mut c_item = CItem { cb, lower };
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

/// Deobfuscation::Deobfuscate; free the result with nzbget_rs_free.
///
/// # Safety
/// `s` is null or readable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_deobfuscate(s: *const c_char, len: usize) -> RsBuf {
    into_buf(crate::deobfuscation::deobfuscate(bytes(s, len)))
}

/// FileTypes' name checks (rust/src/filetypes.rs), by number (see nzbget_rs.h).
///
/// # Safety
/// `s` is null or readable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_file_type(which: c_int, s: *const c_char, len: usize) -> c_int {
    use crate::filetypes::*;
    const CHECKS: [fn(&[u8]) -> bool; 25] = [
        is_seven_zip_ext, is_rar_ext, is_rar_volume_ext, is_numeric_volume_ext, is_all_digits_ext, is_archive_ext,
        is_disc_structure_ext, is_disc_structure_dir, is_disc_descriptor_ext, is_disc_image_ext,
        is_generic_disc_image_ext, is_clutter_dir, is_clutter_file, is_parity_ext, is_video_ext, is_audio_ext,
        is_subtitle_ext, is_nfo_ext, is_book_ext, is_image_ext, is_sample_stem, is_seven_zip_file, is_rar_file,
        is_archive_file, is_sample_file,
    ];
    match usize::try_from(which).ok().and_then(|w| CHECKS.get(w)) {
        Some(check) => check(bytes(s, len)) as c_int,
        None => 0,
    }
}

fn static_str(e: &'static CStr, out_len: *mut usize) -> *const c_char {
    if !out_len.is_null() {
        unsafe { *out_len = e.to_bytes().len() };
    }
    e.as_ptr()
}

/// FileTypes::SniffExtension of a header: a static extension ("" for none),
/// its length in `out_len`.
///
/// # Safety
/// `header` is null or readable for `len` bytes; `out_len` null or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sniff_extension(header: *const u8, len: usize, out_len: *mut usize) -> *const c_char {
    let h = if header.is_null() || len == 0 { &[][..] } else { std::slice::from_raw_parts(header, len) };
    static_str(crate::filetypes::sniff_extension(h), out_len)
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

/// FileSystem::ReservedChar.
#[no_mangle]
pub extern "C" fn nzbget_rs_reserved_char(c: c_char) -> c_int {
    crate::paths::reserved_char(c as u8) as c_int
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

/// A CollectionAnalyzer::FileEntry (rust/src/collection.rs).
#[repr(C)]
pub struct FileEntryC {
    pub path: *const c_char,
    pub path_len: usize,
    pub rename_prefix: *const c_char,
    pub rename_prefix_len: usize,
    pub filename: *const c_char,
    pub filename_len: usize,
    pub stem: *const c_char,
    pub stem_len: usize,
    pub ext: *const c_char,
    pub ext_len: usize,
    pub size: u64,
}

unsafe fn entries(files: *const FileEntryC, count: usize) -> Vec<crate::collection::Entry> {
    if files.is_null() || count == 0 {
        return Vec::new();
    }
    std::slice::from_raw_parts(files, count)
        .iter()
        .map(|f| crate::collection::Entry {
            path: bytes(f.path, f.path_len).to_vec(),
            rename_prefix: bytes(f.rename_prefix, f.rename_prefix_len).to_vec(),
            filename: bytes(f.filename, f.filename_len).to_vec(),
            stem: bytes(f.stem, f.stem_len).to_vec(),
            ext: bytes(f.ext, f.ext_len).to_vec(),
            size: f.size,
        })
        .collect()
}

/// AnalysisResult by index into the files (-1: an empty FileEntry); the
/// index arrays have room for `count` each.
#[repr(C)]
pub struct AnalysisC {
    pub main_video: isize,
    pub sample_video: isize,
    pub main_book: isize,
    pub subtitles: *mut usize,
    pub subtitle_count: usize,
    pub nfos: *mut usize,
    pub nfo_count: usize,
    pub other_files: *mut usize,
    pub other_count: usize,
    pub ambiguous: c_int,
    pub disc_structure: c_int,
    pub has_audio: c_int,
}

/// CollectionAnalyzer::Analyze.
///
/// # Safety
/// `files` is null (empty) or holds `count` valid entries. Entry strings
/// are null (empty) or readable for their lengths. `out` is null or writable;
/// its arrays are null (counts only) or have room for `count` indices and
/// are disjoint from `out`. All storage remains caller-owned.
/// Panics abort rather than crossing the ABI.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_collection_analyze(files: *const FileEntryC, count: usize, out: *mut AnalysisC) {
    if out.is_null() {
        return;
    }
    let a = crate::collection::analyze(&entries(files, count));
    let o = &mut *out;
    let idx = |i: Option<usize>| i.map_or(-1, |k| k as isize);
    o.main_video = idx(a.main_video);
    o.sample_video = idx(a.sample_video);
    o.main_book = idx(a.main_book);
    let fill = |dst: *mut usize, src: &[usize]| {
        if !dst.is_null() {
            for (k, &v) in src.iter().take(count).enumerate() {
                *dst.add(k) = v;
            }
        }
        src.len().min(count)
    };
    o.subtitle_count = fill(o.subtitles, &a.subtitles);
    o.nfo_count = fill(o.nfos, &a.nfos);
    o.other_count = fill(o.other_files, &a.other_files);
    o.ambiguous = a.ambiguous as c_int;
    o.disc_structure = a.disc_structure as c_int;
    o.has_audio = a.has_audio as c_int;
}

/// What CollectionAnalyzer::BuildPlan asks of the C++ side, and where it
/// puts the plan.
#[repr(C)]
pub struct PlanCallbacks {
    pub user: *mut std::ffi::c_void,
    pub exists: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, usize) -> c_int>,
    pub ignored: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, usize) -> c_int>,
    /// a rename: the file's index, the new path, the new file name
    pub action: Option<unsafe extern "C" fn(*mut std::ffi::c_void, usize, *const c_char, usize, *const c_char, usize)>,
    /// the stem of a path as the platform's path has it, written to `out`
    /// (room for `cap` bytes); returns its length (more than `cap`: called
    /// again with that room); None: the stem of the bytes as they are
    pub stem: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, usize, *mut c_char, usize) -> usize>,
}

/// RenamePlan's flags; the effective base name is returned.
#[repr(C)]
#[derive(Default)]
pub struct PlanFlagsC {
    pub ambiguous: c_int,
    pub disc_structure: c_int,
    pub can_rename: c_int,
    pub target_name_obfuscated: c_int,
}

struct CDisk<'c>(&'c PlanCallbacks);

impl crate::collection::Disk for CDisk<'_> {
    fn stem(&mut self, path: &[u8]) -> Vec<u8> {
        let Some(f) = self.0.stem else {
            let name = path.iter().rposition(|&b| b == b'/' || b == b'\\').map_or(path, |k| &path[k + 1..]);
            return crate::collection::stem_ext(name).0.to_vec();
        };
        let mut buf = vec![0u8; path.len() * 3 + 16];
        loop {
            let n = unsafe { f(self.0.user, path.as_ptr().cast(), path.len(), buf.as_mut_ptr().cast(), buf.len()) };
            if n <= buf.len() {
                buf.truncate(n);
                return buf;
            }
            buf.resize(n, 0);
        }
    }

    fn exists(&mut self, path: &[u8]) -> bool {
        self.0.exists.is_some_and(|f| unsafe { f(self.0.user, path.as_ptr().cast(), path.len()) != 0 })
    }
    fn ignored(&mut self, path: &[u8]) -> bool {
        self.0.ignored.is_some_and(|f| unsafe { f(self.0.user, path.as_ptr().cast(), path.len()) != 0 })
    }
}

/// CollectionAnalyzer::BuildPlan for the walked files; free the returned
/// effective base name with nzbget_rs_free.
///
/// # Safety
/// `files` is null (empty) or holds `count` valid entries, with strings null
/// (empty) or readable for their lengths. `target` is null (empty) or readable
/// for `target_len`. `callbacks` is null or a valid table; its functions must
/// not unwind or invalidate the table/flags. Callback strings are borrowed
/// only for the call. `flags` is null or writable and disjoint from the table.
/// Null table/flags returns an empty buffer, clearing non-null flags. Null
/// exists/ignored means false; null action discards actions. Panics abort.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_collection_plan(
    files: *const FileEntryC,
    count: usize,
    disc_dir: c_int,
    target: *const c_char,
    target_len: usize,
    callbacks: *const PlanCallbacks,
    flags: *mut PlanFlagsC,
) -> RsBuf {
    if !flags.is_null() {
        *flags = PlanFlagsC::default();
    }
    if callbacks.is_null() || flags.is_null() {
        return into_buf(Vec::new());
    }
    let cb = &*callbacks;
    let plan = crate::collection::build_plan(&entries(files, count), disc_dir != 0, bytes(target, target_len), &mut CDisk(cb));
    for a in &plan.actions {
        if let Some(action) = cb.action {
            action(cb.user, a.src, a.dst_path.as_ptr().cast(), a.dst_path.len(), a.new_filename.as_ptr().cast(), a.new_filename.len());
        }
    }
    *flags = PlanFlagsC {
        ambiguous: plan.ambiguous as c_int,
        disc_structure: plan.disc_structure as c_int,
        can_rename: plan.can_rename as c_int,
        target_name_obfuscated: plan.target_name_obfuscated as c_int,
    };
    into_buf(plan.effective_base_name)
}

/// CollectionAnalyzer's names: 0 ResolveTargetName(a: meta, b: nzb),
/// 1 ResolveSubtitleName(a: base, b: stem, c: ext), 2 ResolveSampleName(a:
/// base, b: ext); free the result with nzbget_rs_free.
///
/// # Safety
/// Each text is null or readable for its length.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_collection_name(
    op: c_int,
    a: *const c_char,
    a_len: usize,
    b: *const c_char,
    b_len: usize,
    c: *const c_char,
    c_len: usize,
) -> RsBuf {
    let (a, b, c) = (bytes(a, a_len), bytes(b, b_len), bytes(c, c_len));
    into_buf(match op {
        0 => crate::collection::resolve_target_name(a, b),
        1 => crate::collection::resolve_subtitle_name(a, b, c),
        2 => crate::collection::resolve_sample_name(a, b),
        _ => Vec::new(),
    })
}

unsafe fn lower_of<'f>(table: *const c_int, char_signed: c_int, call: &'f dyn Fn(u8) -> c_int) -> crate::wildmask::Lower<'f> {
    if table.is_null() {
        crate::wildmask::Lower::Fold(call)
    } else {
        crate::wildmask::Lower::Table(&*table.sub(128).cast::<[c_int; 384]>(), char_signed != 0)
    }
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
    pub lower_table: *const c_int,
    pub char_signed: c_int,
    pub fold: Option<extern "C" fn(c_int) -> c_int>,
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
/// `input` is null or readable; its strings are null or NUL-terminated;
/// `lower_table` as for WildMask; `fold` doesn't unwind. `out` is null or
/// writable and disjoint from all input storage. Null input denies access.
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
    let fold = i.fold;
    let call = move |b: u8| fold.map_or(b as c_int, |f| f(b as c_int));
    if i.lower_table.is_null() && fold.is_none() {
        return;
    }
    let lower = lower_of(i.lower_table, i.char_signed, &call);
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

/// WebProcessor::IsAuthorizedIp.
///
/// # Safety
/// The strings are null or NUL-terminated; `table` as for WildMask; `fold`
/// doesn't unwind.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_web_authorized_ip(
    option: *const c_char,
    remote: *const c_char,
    table: *const c_int,
    char_signed: c_int,
    fold: Option<extern "C" fn(c_int) -> c_int>,
) -> c_int {
    if table.is_null() && fold.is_none() {
        return 0;
    }
    let call = move |b: u8| fold.map_or(b as c_int, |f| f(b as c_int));
    let lower = lower_of(table, char_signed, &call);
    crate::webserver::is_authorized_ip(input(option), input(remote), &lower) as c_int
}

/// Decoder (rust/src/decoder.rs) with rapidyenc's decoder and CRC.
#[no_mangle]
pub extern "C" fn nzbget_rs_decoder_new(decode: Option<crate::decoder::DecodeFn>, crc: Option<crate::decoder::CrcFn>) -> *mut crate::decoder::Decoder {
    let (Some(decode), Some(crc)) = (decode, crc) else { return std::ptr::null_mut() };
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

/// Util::TrimLeft/TrimRight/Trim(std::string&) (`left`, `right`) and
/// Util::SanitizeLine (`sanitize`, which also blanks control characters in
/// place): the kept range [*start, return value) of the `len` bytes.
///
/// # Safety
/// `data` is null or writable for `len` bytes; `start` is null or writable
/// after the buffer access. `right_space`, when supplied, classifies a byte
/// as the caller's C++ char and must not unwind or access `data`. If absent,
/// no trailing bytes are classified as whitespace. NULL `start` skips output.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_trim_string(data: *mut c_char, len: usize, left: c_int, right: c_int, sanitize: c_int, start: *mut usize, right_space: Option<extern "C" fn(c_int) -> c_int>) -> usize {
    let space = |b: u8| right_space.is_some_and(|f| f(b as c_int) != 0);
    let (s, e) = if data.is_null() || len == 0 {
        (0, 0)
    } else {
        let buf = std::slice::from_raw_parts_mut(data.cast::<u8>(), len);
        if sanitize != 0 { crate::util::sanitize_line(buf, space) } else { crate::util::trim_string(buf, left != 0, right != 0, space) }
    };
    if !start.is_null() { *start = s; }
    e
}

/// Util::EndsWith over byte ranges (std::string_view).
///
/// # Safety
/// `s` and `suffix` are null or readable for their lengths.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ends_with(s: *const c_char, len: usize, suffix: *const c_char, suffix_len: usize, case_sensitive: c_int) -> c_int {
    crate::util::ends_with(span(s, len), span(suffix, suffix_len), case_sensitive != 0) as c_int
}

/// Util::FormatBuffer; free with nzbget_rs_free.
///
/// # Safety
/// `buf` is null or readable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_format_buffer(buf: *const c_char, len: c_int) -> RsBuf {
    into_buf(crate::util::format_buffer(span(buf, len.max(0) as usize)))
}

/// WebUtil::ParseRfc822DateTime (0 for null).
///
/// # Safety
/// `s` is null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_parse_rfc822_date_time(s: *const c_char) -> i64 {
    if s.is_null() {
        return 0;
    }
    crate::util::parse_rfc822_date_time(CStr::from_ptr(s))
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

/// Options::ParseTime (0 for null input); the outputs are written as the
/// C++ wrote them, also on failure.
///
/// # Safety
/// `time` is null or NUL-terminated; nonnull outputs are writable and disjoint
/// from `time`, but may alias each other. Null arguments return 0 without writes.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_parse_time(time: *const c_char, hours: *mut c_int, minutes: *mut c_int) -> c_int {
    if time.is_null() || hours.is_null() || minutes.is_null() {
        return 0;
    }
    crate::options::parse_time_with(CStr::from_ptr(time), |h| hours.write(h), |m| minutes.write(m)) as c_int
}

/// Options::ParseWeekDays; `*bits` is written as the C++ wrote it.
///
/// # Safety
/// `week_days` is null or NUL-terminated; nonnull `bits` is writable and disjoint
/// from the input. Null input means empty; a null output returns 0.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_parse_week_days(week_days: *const c_char, bits: *mut c_int) -> c_int {
    if bits.is_null() {
        return 0;
    }
    let mut result = 0;
    let valid = crate::options::parse_week_days(input(week_days), &mut result);
    bits.write(result);
    valid as c_int
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

/// Options::ConvertOldOption: the current name and value; free both with
/// nzbget_rs_free. Null inputs read as empty. Null outputs discard that result;
/// identical output pointers retain only the new value.
///
/// # Safety
/// `option` and `value` are null or NUL-terminated; nonnull outputs are writable
/// and disjoint from the inputs and either equal or disjoint from each other.
/// Their previous contents are not freed.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_convert_old_option(option: *const c_char, value: *const c_char, new_option: *mut RsBuf, new_value: *mut RsBuf) {
    let as_c = |p: *const c_char| if p.is_null() { c"" } else { CStr::from_ptr(p) };
    let (o, v) = crate::options::convert_old_option(as_c(option), as_c(value));
    if !new_option.is_null() && new_option != new_value {
        new_option.write(into_buf(o.into_bytes()));
    }
    if !new_value.is_null() {
        new_value.write(into_buf(v.into_bytes()));
    }
}

/// Options::HasScript.
///
/// # Safety
/// `list` and `name` are null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_has_script(list: *const c_char, name: *const c_char) -> c_int {
    let name = if name.is_null() { c"" } else { CStr::from_ptr(name) };
    crate::options::has_script(input(list), name) as c_int
}

/// Options::ParseCategorySource: 0 Auto, 1 NZBFile, 2 FeedFile.
///
/// # Safety
/// `value` is null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_parse_category_source(value: *const c_char) -> c_int {
    crate::options::parse_category_source((!value.is_null()).then(|| CStr::from_ptr(value)))
}

/// Borrowed bytes: NzbgetRsStr.
#[repr(C)]
pub struct RsStr {
    pub data: *const c_char,
    pub len: usize,
}

fn rs_str(v: &[u8]) -> RsStr {
    RsStr { data: v.as_ptr().cast(), len: v.len() }
}

const NO_STR: RsStr = RsStr { data: std::ptr::null(), len: 0 };

/// ExtensionLoader::V1's parsing of a script file's `len` bytes. Owns the
/// result independently of the input. Null input is empty regardless of len;
/// a missing classifier or unrepresentable length returns null.
///
/// # Safety
/// Nonnull `data` is readable for `len` bytes (at most isize::MAX).
/// `right_space` classifies a byte as C++ Util::TrimRight does and must not
/// unwind. Rust panics abort rather than crossing the ABI.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_v1_parse(
    data: *const c_char, len: usize, right_space: Option<extern "C" fn(c_int) -> c_int>,
) -> *mut crate::extload::Script {
    let Some(space) = right_space else { return std::ptr::null_mut() };
    if len > isize::MAX as usize { return std::ptr::null_mut() }
    ext_handle(crate::extload::parse(span(data, len), &|b| space(b as c_int) != 0))
}

fn ext_handle(script: Option<crate::extload::Script>) -> *mut crate::extload::Script {
    script.map_or(std::ptr::null_mut(), |s| Box::into_raw(Box::new(s)))
}

/// Streaming variant: next returns 1 for a line, 0 for EOF, -1 for failure.
/// A failure discards the partial parse. It is never called after the header
/// terminator. Null callbacks return null.
///
/// # Safety
/// `next` must not unwind. Its output is null (empty), or readable for len
/// bytes (<= isize::MAX) until the next call. It may mutate only its context
/// and the supplied output, not Rust-owned memory. `context` is passed through
/// untouched. The classifier has the same contract as ext_v1_parse.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_v1_read(
    next: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut RsStr) -> c_int>,
    context: *mut std::ffi::c_void,
    right_space: Option<extern "C" fn(c_int) -> c_int>,
) -> *mut crate::extload::Script {
    let (Some(next), Some(space)) = (next, right_space) else { return std::ptr::null_mut() };
    let mut failed = false;
    let mut lines = std::iter::from_fn(|| {
        let mut line = NO_STR;
        match next(context, &mut line) {
            0 => None,
            1 if line.len <= isize::MAX as usize => Some(span(line.data, line.len).to_vec()),
            _ => { failed = true; None }
        }
    });
    let script = crate::extload::parse_lines(&mut lines, &|b| space(b as c_int) != 0);
    if failed { std::ptr::null_mut() } else { ext_handle(script) }
}

/// Frees a handle from nzbget_rs_ext_v1_parse.
///
/// # Safety
/// `h` is null or a handle not yet freed.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_v1_free(h: *mut crate::extload::Script) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

/// The script kind bits (1 post-processing, 2 scan, 4 queue, 8 scheduler, 16 feed).
///
/// # Safety
/// `h` is null (0) or a live handle.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_v1_kind(h: *const crate::extload::Script) -> c_int {
    h.as_ref().map_or(0, |s| s.kind)
}

/// Script texts: 0 about (untrimmed), 1 queue events, 2 task time; script
/// lists: 3 description line `i`, 4 requirement `i`. Borrowed from the handle.
///
/// # Safety
/// `h` is null (empty) or a live handle.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_v1_text(h: *const crate::extload::Script, which: c_int, i: usize) -> RsStr {
    let Some(s) = h.as_ref() else { return NO_STR };
    match which {
        0 => rs_str(&s.about),
        1 => rs_str(&s.queue_events),
        2 => rs_str(&s.task_time),
        3 => s.description.get(i).map_or(NO_STR, |v| rs_str(v)),
        4 => s.requirements.get(i).map_or(NO_STR, |v| rs_str(v)),
        _ => NO_STR,
    }
}

/// Counts: 0 description lines, 1 requirements, 2 options, 3 commands.
///
/// # Safety
/// `h` is null (0) or a live handle.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_v1_count(h: *const crate::extload::Script, which: c_int) -> usize {
    let Some(s) = h.as_ref() else { return 0 };
    match which {
        0 => s.description.len(),
        1 => s.requirements.len(),
        2 => s.options.len(),
        3 => s.commands.len(),
        _ => 0,
    }
}

fn ext_item(s: &crate::extload::Script, command: c_int, i: usize) -> Option<&crate::extload::Item> {
    if command != 0 { s.commands.get(i) } else { s.options.get(i) }
}

/// Option (`command` 0) or command (1) `i`: its texts - 0 section name, 1
/// section prefix, 2 name, 3 action (commands), 4 description line `j` -
/// borrowed from the handle.
///
/// # Safety
/// `h` is null (empty) or a live handle.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_v1_item_text(h: *const crate::extload::Script, command: c_int, i: usize, field: c_int, j: usize) -> RsStr {
    let Some(item) = h.as_ref().and_then(|s| ext_item(s, command, i)) else { return NO_STR };
    match field {
        0 => rs_str(&item.section.name),
        1 => rs_str(&item.section.prefix),
        2 => rs_str(&item.name),
        3 => rs_str(&item.action),
        4 => item.description.get(j).map_or(NO_STR, |v| rs_str(v)),
        _ => NO_STR,
    }
}

/// Option or command `i`: 0 whether its section is multi, 1 its description
/// line count, 2 its select value count (options).
///
/// # Safety
/// `h` is null (0) or a live handle.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_v1_item_count(h: *const crate::extload::Script, command: c_int, i: usize, which: c_int) -> usize {
    let Some(item) = h.as_ref().and_then(|s| ext_item(s, command, i)) else { return 0 };
    match which {
        0 => item.section.multi as usize,
        1 => item.description.len(),
        2 => item.select.len(),
        _ => 0,
    }
}

/// Option `i`'s value (`j` == SIZE_MAX) or select value `j`: 1 with the
/// number in `*num`, 0 with the text in `*text` (borrowed), -1 if none.
///
/// # Safety
/// `h` is null or a live handle; `num` and `text` are null or writable,
/// aligned, and disjoint from the handle and each other. A null required
/// output returns -1; other outputs are left untouched. Borrowed text remains
/// valid until the handle is freed and must not be mutated or freed.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_v1_select(h: *const crate::extload::Script, i: usize, j: usize, num: *mut f64, text: *mut RsStr) -> c_int {
    use crate::extload::Select;
    let Some(item) = h.as_ref().and_then(|s| s.options.get(i)) else { return -1 };
    let value = if j == usize::MAX { Some(&item.value) } else { item.select.get(j) };
    match value {
        Some(Select::Num(n)) if !num.is_null() => {
            *num = *n;
            1
        }
        Some(Select::Str(t)) if !text.is_null() => {
            *text = rs_str(t);
            0
        }
        _ => -1,
    }
}

/// NzbgetRsHttpResponse: WebDownloader::CheckResponse's decision.
#[repr(C)]
pub struct HttpResponse {
    pub result: c_int,
    pub set_status: c_int,
    pub http_status: c_int,
    pub warn: c_int,
    pub status_offset: usize,
}

/// WebDownloader::CheckResponse for the first response line (null: closed).
///
/// # Safety
/// `response` is null or NUL-terminated; `out` is null (a no-op) or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_http_check_response(response: *const c_char, out: *mut HttpResponse) {
    if out.is_null() {
        return;
    }
    let r = crate::webdownload::check_response((!response.is_null()).then(|| CStr::from_ptr(response)));
    // C++ supplies uninitialized output storage. Read the input before
    // writing, so callers may also reuse its storage for the result.
    out.write(HttpResponse {
        result: r.result,
        set_status: r.set_status as c_int,
        http_status: r.http_status,
        warn: r.warn,
        status_offset: r.status_offset,
    });
}

/// WebDownloader::ProcessHeader: 0 nothing, 1 Content-Length (`*value`),
/// 2 gzip, 3 Content-Disposition, 4 Location (the address at `line + *value`).
///
/// # Safety
/// `line` is null (nothing) or NUL-terminated; `value` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_http_header(line: *const c_char, redirecting: c_int, value: *mut c_int) -> c_int {
    if line.is_null() {
        return 0;
    }
    let (action, v) = crate::webdownload::process_header(CStr::from_ptr(line), redirecting != 0);
    if !value.is_null() {
        value.write(v);
    }
    action
}

/// WebDownloader::ParseRedirect: where `location` leads from `old_url`; free
/// with nzbget_rs_free. Null inputs read as empty.
///
/// # Safety
/// `old_url` and `location` are null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_http_redirect(old_url: *const c_char, location: *const c_char) -> RsBuf {
    let as_c = |p: *const c_char| if p.is_null() { c"" } else { CStr::from_ptr(p) };
    into_buf(crate::webdownload::redirect(as_c(old_url), as_c(location)))
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
        // Validated before parsing; never reference an unavailable libc symbol.
        (self.0.getopt_long.unwrap())(argc, argv)
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
        || (use_long != 0 && sink.getopt_long.is_none())
    {
        return -1;
    }
    match crate::cmdline::parse(argc, argv, use_long != 0, &mut FfiCmdline(sink)) {
        Ok(()) => 0,
        Err(_) => -1,
    }
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

    // One test owns getopt globals throughout; no parallel parser tests.
    type KnownEncoder = unsafe extern "C" fn(*const c_char, usize, *mut c_char, usize) -> usize;

    #[test]
    fn known_encoders_capacity_nul_and_retry() {
        let inputs = [b"".as_slice(), b"plain", b" a\"b\\c/d\n\t ", b"\x80\xbf\xc0\xff",
            b"\xc0\x80", b"\xf7\xbf\xbf\xbf", b"a\xe2\x82", "😀é".as_bytes()];
        for input in inputs {
            let source = [input, &[0]].concat();
            for (encode, expansion) in [(nzbget_rs_latin1_to_utf8_n as KnownEncoder, 2)] {
                let mut expected = Vec::new();
                crate::text::latin1_to_utf8(input, &mut expected);
                assert!(expected.len() <= expansion * input.len());
                for cap in 0..=expansion * input.len() + 2 {
                    let mut storage = vec![std::mem::MaybeUninit::<u8>::uninit(); cap + 2];
                    storage[0].write(0x55); storage[cap + 1].write(0xaa);
                    let n = unsafe { encode(source.as_ptr().cast(), input.len(), storage.as_mut_ptr().add(1).cast(), cap) };
                    assert_eq!(n, expected.len());
                    assert_eq!(unsafe { storage[0].assume_init() }, 0x55);
                    assert_eq!(unsafe { storage[cap + 1].assume_init() }, 0xaa);
                    if n < cap {
                        let result = unsafe { std::slice::from_raw_parts(storage.as_ptr().add(1).cast::<u8>(), n + 1) };
                        assert_eq!(&result[..n], expected); assert_eq!(result[n], 0);
                    }
                    assert_eq!(&source[..input.len()], input);
                }
                let n = unsafe { encode(source.as_ptr().cast(), input.len(), std::ptr::null_mut(), usize::MAX) };
                assert_eq!(n, expected.len());
                let mut exact = vec![0x55u8; n + 1];
                assert_eq!(unsafe { encode(source.as_ptr().cast(), input.len(), exact.as_mut_ptr().cast(), exact.len()) }, n);
                assert_eq!(&exact[..n], expected); assert_eq!(exact[n], 0);
                let mut empty = [0x55u8];
                assert_eq!(unsafe { encode(std::ptr::null(), 0, empty.as_mut_ptr().cast(), 1) }, 0);
                assert_eq!(empty, [0]);
                assert_eq!(unsafe { encode(std::ptr::null(), 0, std::ptr::null_mut(), 0) }, 0);
            }
        }

    }

    #[test]
    fn known_encoders_reject_overlap_and_arithmetic_before_writes() {
        for (encode, expansion) in [(nzbget_rs_latin1_to_utf8_n as KnownEncoder, 2)] {
            let mut data = [b'#'; 48]; data[8..12].copy_from_slice(b"a b\0");
            let original = data;
            for (start, cap) in [(8, 8), (0, 10), (11, 8)] {
                assert_eq!(unsafe { encode(data.as_ptr().add(8).cast(), 3, data.as_mut_ptr().add(start).cast(), cap) }, usize::MAX);
                assert_eq!(data, original);
            }
            // Empty output ranges may measure even at the source address.
            assert_ne!(unsafe { encode(data.as_ptr().add(8).cast(), 3, data.as_mut_ptr().add(8).cast(), 0) }, usize::MAX);
            let output = data.as_mut_ptr().cast();
            for input_len in [usize::MAX, (isize::MAX as usize - 1) / expansion + 1] {
                assert_eq!(unsafe { encode(c"x".as_ptr(), input_len, output, 1) }, usize::MAX);
            }
            assert_eq!(unsafe { encode(std::ptr::null(), 1, output, 1) }, usize::MAX);
            assert_eq!(unsafe { encode(c"x".as_ptr(), 1, output, usize::MAX) }, usize::MAX);
            assert_eq!(unsafe { encode(usize::MAX as *const c_char, 0, output, 1) }, usize::MAX);
            assert_eq!(unsafe { encode(c"x".as_ptr(), 1, usize::MAX as *mut c_char, 1) }, usize::MAX);
            assert_eq!(data, original);
        }
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
            assert_eq!(nzbget_rs_cmdline_parse(1, argv.as_mut_ptr(), 1, &sink), -1);
            sink.error = None;
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

/// Borrowed native stat callback; see nzbget_rs.h. It must not unwind.
#[cfg(unix)]
pub type FsStat = unsafe extern "C" fn(*const c_char) -> c_int;

#[cfg(unix)]
fn fs_stat(stat: Option<FsStat>, path: &[u8]) -> i32 {
    let Some(stat) = stat else { return -22 }; // EINVAL
    let mut path = path.to_vec();
    path.push(0);
    unsafe { stat(path.as_ptr().cast()) }
}

/// FileSystem::FileExists (0 for null).
///
/// # Safety
/// `path` is null or NUL-terminated. `stat`, when present, follows the
/// FsStat contract and must not unwind.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_file_exists(path: *const c_char, stat: Option<FsStat>) -> c_int {
    (!path.is_null() && crate::fsops::file_exists(input(path), &|_| stat.map_or(-22, |f| f(path)))) as c_int
}

/// FileSystem::DirectoryExists (0 for null).
///
/// # Safety
/// `path` is null or NUL-terminated. `stat`, when present, follows the
/// FsStat contract and must not unwind.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_directory_exists(path: *const c_char, stat: Option<FsStat>) -> c_int {
    (!path.is_null() && crate::fsops::directory_exists(input(path), &|_| stat.map_or(-22, |f| f(path)))) as c_int
}

/// FileSystem::ForceDirectories: 1 created or there, else 0 with the message
/// in `*errmsg` (free with nzbget_rs_free; empty on success).
///
/// # Safety
/// `path` is null or NUL-terminated; `errmsg` is null or writable and disjoint
/// from the input. Every non-null output is initialized, including on failure.
/// `stat`, when present, follows the FsStat contract and must not unwind.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_force_directories(path: *const c_char, errmsg: *mut RsBuf, stat: Option<FsStat>) -> c_int {
    if errmsg.is_null() {
        return 0;
    }
    if path.is_null() || stat.is_none() {
        errmsg.write(into_buf(Vec::new()));
        return 0;
    }
    let mut msg = Vec::new();
    let ok = crate::fsops::force_directories(input(path), &mut msg, &|p| fs_stat(stat, p));
    errmsg.write(into_buf(msg));
    ok as c_int
}

/// FileSystem::MakeUniqueFilename; free with nzbget_rs_free.
///
/// # Safety
/// `dir` and `basename` are null (empty) or NUL-terminated. `stat`, when
/// present, follows the FsStat contract and must not unwind.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_make_unique_filename(dir: *const c_char, basename: *const c_char, stat: Option<FsStat>) -> RsBuf {
    let as_c = |p: *const c_char| if p.is_null() { c"" } else { CStr::from_ptr(p) };
    into_buf(crate::fsops::make_unique_filename(as_c(dir), as_c(basename), &|p| fs_stat(stat, p)))
}

/// FileSystem::ExpandHomePath with the home directory the C++ looked up
/// (null: none); free with nzbget_rs_free.
///
/// # Safety
/// `filename` and `home` are null or NUL-terminated.
#[cfg(unix)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_expand_home_path(filename: *const c_char, home: *const c_char) -> RsBuf {
    let filename = (!filename.is_null()).then(|| CStr::from_ptr(filename));
    let home = (!home.is_null()).then(|| input(home));
    into_buf(crate::fsops::expand_home_path(filename, home))
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
/// caller-owned. `fold` takes a byte (0..=255), returns tolower of the caller's
/// char, and must not unwind. A missing fold fails; panics cannot cross the ABI.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_parse_par_filename(name: *const c_char, confirmed: c_int, base_len: *mut c_int, blocks: *mut c_int,
    table: *const c_int, char_signed: c_int, fold: Option<extern "C" fn(c_int) -> c_int>) -> c_int {
    if name.is_null() {
        return 0;
    }
    let Some(fold) = fold else { return 0 };
    let name = par_filename_input(name);
    let lower = par_fold(table, char_signed, fold);
    match crate::parparser::parse_par_filename(&name, confirmed != 0, &lower) {
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
/// Each name is null or readable through its first NUL or 1023 bytes. `fold`
/// has the same contract as nzbget_rs_parse_par_filename. Inputs stay borrowed.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_same_par_collection(name1: *const c_char, name2: *const c_char, confirmed: c_int,
    table: *const c_int, char_signed: c_int, fold: Option<extern "C" fn(c_int) -> c_int>) -> c_int {
    if name1.is_null() || name2.is_null() {
        return 0;
    }
    let Some(fold) = fold else { return 0 };
    let name1 = par_filename_input(name1);
    let name2 = par_filename_input(name2);
    let lower = par_fold(table, char_signed, fold);
    // Each local copy has a trailing NUL, even when the input did not.
    crate::parparser::same_par_collection(CStr::from_ptr(name1.as_ptr().cast()), CStr::from_ptr(name2.as_ptr().cast()), confirmed != 0,
        &lower) as c_int
}

/// ParParser::FilenameFold: glibc's tolower table read in place for the C++
/// char's signedness (what tolower((char)byte) reads), else the callback.
///
/// # Safety
/// `table` is null or glibc's tolower table (valid for -128..=255) for the
/// calling thread's locale.
unsafe fn par_fold<'t>(table: *const c_int, char_signed: c_int, fold: extern "C" fn(c_int) -> c_int) -> impl Fn(u8) -> u8 + 't {
    let table = (!table.is_null()).then(|| &*table.sub(128).cast::<[c_int; 384]>());
    let signed = char_signed != 0;
    move |b: u8| match table {
        Some(t) => t[(if signed { b as i8 as i32 } else { b as i32 } + 128) as usize] as u8,
        None => fold(b as c_int) as u8,
    }
}

/// NzbgetRsRarIo: RarVolume's content source and header cipher.
#[repr(C)]
pub struct RarIo {
    pub ctx: *mut std::ffi::c_void,
    pub size: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> i64>,
    pub read: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64, *mut u8, i64) -> c_int>,
    pub rar3_prepare: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const u8) -> c_int>,
    pub rar5_prepare: Option<unsafe extern "C" fn(*mut std::ffi::c_void, u8, *const u8) -> c_int>,
    pub init: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const u8) -> c_int>,
    pub decrypt: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const u8, *mut u8) -> c_int>,
    pub free: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    pub add_file: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const RarFileOut) -> c_int>,
    pub set_data_offset: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64)>,
}

/// NzbgetRsRarVolume: RarVolume's state, read and written back.
#[repr(C)]
pub struct RarVolumeState {
    pub version: c_int,
    pub volume_no: u32,
    pub new_naming: c_int,
    pub has_next_volume: c_int,
    pub multi_volume: c_int,
    pub encrypted: c_int,
    pub has_password: c_int,
    pub decrypt_buf: [u8; 16],
    pub decrypt_pos: u8,
}

/// NzbgetRsRarFile: one file of the volume (borrowed for the add_file call).
#[repr(C)]
pub struct RarFileOut {
    pub filename: *const c_char,
    pub filename_len: usize,
    pub time: u32,
    pub attr: u32,
    pub size: i64,
    pub split_before: c_int,
    pub split_after: c_int,
    pub method: u32,
    pub stored: c_int,
    pub packed_size: i64,
    pub data_offset: i64,
    pub encrypted_data: c_int,
    pub has_salt: c_int,
    pub salt: [u8; 8],
    pub has_crypt: c_int,
    pub crypt_version: u64,
    pub crypt_flags: u64,
    pub crypt_kdf_count: u8,
    pub crypt_salt: [u8; 16],
    pub crypt_iv: [u8; 16],
    pub crypt_check_value: [u8; 12],
    pub crypt_has_check: c_int,
}

struct FfiRar<'a>(&'a RarIo);

impl crate::rarreader::Source for FfiRar<'_> {
    fn size(&mut self) -> i64 {
        self.0.size.map_or(-1, |f| unsafe { f(self.0.ctx) })
    }
    fn read(&mut self, offset: i64, buf: &mut [u8]) -> bool {
        self.0.read.is_some_and(|f| unsafe { f(self.0.ctx, offset, buf.as_mut_ptr(), buf.len() as i64) } != 0)
    }
}

impl crate::rarreader::Cipher for FfiRar<'_> {
    fn rar3_prepare(&mut self, salt: &[u8; 8]) -> bool {
        self.0.rar3_prepare.is_some_and(|f| unsafe { f(self.0.ctx, salt.as_ptr()) } != 0)
    }
    fn rar5_prepare(&mut self, kdf_count: u8, salt: &[u8; 16]) -> bool {
        self.0.rar5_prepare.is_some_and(|f| unsafe { f(self.0.ctx, kdf_count, salt.as_ptr()) } != 0)
    }
    fn init(&mut self, key_bits: i32, iv: Option<&[u8; 16]>) -> bool {
        let iv = iv.map_or(std::ptr::null(), |v| v.as_ptr());
        self.0.init.is_some_and(|f| unsafe { f(self.0.ctx, key_bits, iv) } != 0)
    }
    fn decrypt(&mut self, input: &[u8; 16], output: &mut [u8; 16]) -> bool {
        self.0.decrypt.is_some_and(|f| unsafe { f(self.0.ctx, input.as_ptr(), output.as_mut_ptr()) } != 0)
    }
    fn free(&mut self) {
        if let Some(f) = self.0.free {
            unsafe { f(self.0.ctx) }
        }
    }
}

impl crate::rarreader::Files for FfiRar<'_> {
    fn add(&mut self, f: &crate::rarreader::RarFile) -> bool {
        let Some(add) = self.0.add_file else { return true };
        let c = f.crypt.unwrap_or_default();
        let out = RarFileOut {
            filename: if f.filename.is_empty() { c"".as_ptr() } else { f.filename.as_ptr().cast() },
            filename_len: f.filename.len(),
            time: f.time,
            attr: f.attr,
            size: f.size,
            split_before: f.split_before as c_int,
            split_after: f.split_after as c_int,
            method: f.method,
            stored: f.stored as c_int,
            packed_size: f.packed_size,
            data_offset: f.data_offset,
            encrypted_data: f.encrypted_data as c_int,
            has_salt: f.has_salt as c_int,
            salt: f.salt,
            has_crypt: f.crypt.is_some() as c_int,
            crypt_version: c.version,
            crypt_flags: c.flags,
            crypt_kdf_count: c.kdf_count,
            crypt_salt: c.salt,
            crypt_iv: c.iv,
            crypt_check_value: c.check_value,
            crypt_has_check: c.has_check as c_int,
        };
        unsafe { add(self.0.ctx, &out) != 0 }
    }
    fn set_data_offset(&mut self, position: i64) {
        if self.0.add_file.is_none() { return; }
        if let Some(set) = self.0.set_data_offset {
            unsafe { set(self.0.ctx, position) }
        }
    }
}

/// RarVolume::ReadFrom: parses the source into `state` (updated) and hands the
/// files read to add_file, in order (also when the parse fails). 1 when the
/// volume was read to its end, else 0.
///
/// # Safety
/// Non-NULL `io` and `state` must be aligned, initialized objects. `io` stays
/// immutable and `state` is exclusively borrowed for the call, including
/// callbacks. Callbacks must not unwind or retain borrowed pointers; read
/// writes at most `len` bytes, decrypt at most 16 bytes. ctx is caller-owned.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rar_read(io: *const RarIo, state: *mut RarVolumeState) -> c_int {
    let (Some(io), Some(st)) = (io.as_ref(), state.as_mut()) else { return 0 };
    let mut v = crate::rarreader::Volume {
        version: st.version,
        volume_no: st.volume_no,
        new_naming: st.new_naming != 0,
        has_next_volume: st.has_next_volume != 0,
        multi_volume: st.multi_volume != 0,
        encrypted: st.encrypted != 0,
        has_password: st.has_password != 0,
        decrypt_buf: st.decrypt_buf,
        decrypt_pos: st.decrypt_pos,
    };
    let mut source = FfiRar(io);
    let mut cipher = FfiRar(io);
    let mut files = FfiRar(io);
    let ok = crate::rarreader::read_from(&mut v, &mut source, &mut cipher, &mut files);
    st.version = v.version;
    st.volume_no = v.volume_no;
    st.new_naming = v.new_naming as c_int;
    st.has_next_volume = v.has_next_volume as c_int;
    st.multi_volume = v.multi_volume as c_int;
    st.encrypted = v.encrypted as c_int;
    st.decrypt_buf = v.decrypt_buf;
    st.decrypt_pos = v.decrypt_pos;
    ok as c_int
}

/// NzbgetRsRarRenFile
#[repr(C)]
pub struct RarRenFile {
    pub name: *const c_char,
    pub name_len: usize,
    pub size: i64,
    pub split_before: c_int,
    pub split_after: c_int,
}

/// NzbgetRsRarRenVolume
#[repr(C)]
pub struct RarRenVolume {
    pub filename: *const c_char,
    pub filename_len: usize,
    pub version: c_int,
    pub volume_no: u32,
    pub multi_volume: c_int,
    pub has_next_volume: c_int,
    pub new_naming: c_int,
    pub files: *const RarRenFile,
    pub file_count: usize,
}

/// NzbgetRsRarRenEnv
#[repr(C)]
pub struct RarRenEnv {
    pub ctx: *mut std::ffi::c_void,
    pub file_exists: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub match_part: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *mut usize, *mut usize) -> c_int>,
    pub rename: Option<unsafe extern "C" fn(*mut std::ffi::c_void, usize, *const c_char)>,
}

struct FfiRarRen<'a>(&'a RarRenEnv);

fn c_string(b: &[u8]) -> std::ffi::CString {
    // the names come from C strings (no NUL inside)
    std::ffi::CString::new(b).unwrap_or_default()
}

impl crate::rarrenamer::Env for FfiRarRen<'_> {
    fn file_exists(&mut self, path: &[u8]) -> bool {
        let p = c_string(path);
        self.0.file_exists.is_some_and(|f| unsafe { f(self.0.ctx, p.as_ptr()) } != 0)
    }
    fn match_part(&mut self, name: &[u8]) -> Option<(usize, usize)> {
        let n = c_string(name);
        let f = self.0.match_part?;
        let (mut start, mut len) = (0usize, 0usize);
        (unsafe { f(self.0.ctx, n.as_ptr(), &mut start, &mut len) } != 0).then_some((start, len))
    }
    fn rename(&mut self, index: usize, dest: &[u8]) {
        let d = c_string(dest);
        if let Some(f) = self.0.rename {
            unsafe { f(self.0.ctx, index, d.as_ptr()) }
        }
    }
}

/// RarRenamer::RenameFiles for the volumes read from `dir`: renames the sets
/// not properly named through the callbacks.
///
/// # Safety
/// `dir` is NUL-terminated; `volumes` points to `count` volumes whose strings
/// (readable for their stated byte lengths) and file arrays are valid for the
/// call; `env` is valid and its callbacks don't unwind. Null pointers are
/// accepted as documented in nzbget_rs.h. Callback strings are borrowed only
/// for that callback and must not be freed. Rust panics abort at this ABI
/// boundary rather than unwinding into C++.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rar_rename_files(dir: *const c_char, volumes: *const RarRenVolume, count: usize, env: *const RarRenEnv) {
    let Some(env) = env.as_ref() else { return };
    if dir.is_null() || (volumes.is_null() && count > 0) {
        return;
    }
    let raw: &[RarRenVolume] = if count == 0 { &[] } else { std::slice::from_raw_parts(volumes, count) };
    let text = |p: *const c_char, len: usize| if p.is_null() || len == 0 { Vec::new() } else { std::slice::from_raw_parts(p.cast::<u8>(), len).to_vec() };
    let vols: Vec<crate::rarrenamer::Volume> = raw
        .iter()
        .map(|v| crate::rarrenamer::Volume {
            filename: text(v.filename, v.filename_len),
            version: v.version,
            volume_no: v.volume_no,
            multi_volume: v.multi_volume != 0,
            has_next_volume: v.has_next_volume != 0,
            new_naming: v.new_naming != 0,
            files: if v.files.is_null() || v.file_count == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(v.files, v.file_count)
                    .iter()
                    .map(|f| crate::rarrenamer::File {
                        name: text(f.name, f.name_len),
                        size: f.size,
                        split_before: f.split_before != 0,
                        split_after: f.split_after != 0,
                    })
                    .collect()
            },
        })
        .collect();
    crate::rarrenamer::rename_files(input(dir), &vols, &mut FfiRarRen(env));
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
/// from 1, not written back) or a valid int. `exists` (null: nothing exists)
/// gets a borrowed NUL-terminated full path and must not unwind.
/// `format` is null (classic formatting) or writes a borrowed candidate
/// buffer and length, valid until the next callback, and returns 1; 0 stops
/// the search. Neither callback may unwind or access `vol` during the call.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_direct_par_name(old_name: *const c_char, dest_dir: *const c_char, set_id: *const c_char,
    separator: c_char, vol: *mut c_int, exists: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    format: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *mut *const c_char, *mut usize) -> c_int>,
    ctx: *mut std::ffi::c_void) -> RsBuf {
    let mut local = 1;
    let vol = if vol.is_null() { &mut local } else { &mut *vol };
    let mut exists = |path: &[u8]| {
        let Some(exists) = exists else { return false };
        let mut path = path.to_vec();
        path.push(0);
        exists(ctx, path.as_ptr().cast()) != 0
    };
    let name = if let Some(format) = format {
        crate::directrenamer::par_name_formatted(input(old_name), input(dest_dir), input(set_id), separator as u8, vol, &mut exists, &mut |v| {
            let (mut data, mut len) = (std::ptr::null(), 0);
            if format(ctx, v, &mut data, &mut len) == 0 { return None; }
            Some(bytes_arg(data.cast(), len).to_vec())
        })
    } else {
        crate::directrenamer::par_name(input(old_name), input(dest_dir), input(set_id), separator as u8, vol, &mut exists)
    };
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
/// NUL-terminated. `fold` is ParParser's and must not unwind; null returns 0.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_direct_need_rename_pars(pars: *const ParName, count: usize, fold: Option<extern "C" fn(c_int) -> c_int>) -> c_int {
    let Some(fold) = fold else { return 0 };
    if pars.is_null() {
        return 0;
    }
    let pars: Vec<_> = std::slice::from_raw_parts(pars, count).iter().map(|p| {
        ((!p.filename.is_null()).then(|| CStr::from_ptr(p.filename)), if p.set_id.is_null() { c"" } else { CStr::from_ptr(p.set_id) })
    }).collect();
    crate::directrenamer::need_rename_pars(&pars, &|b| fold(b as c_int) as u8) as c_int
}

/// NzbgetRsManifestSection.
#[repr(C)]
pub struct ManifestSection {
    pub multi: c_int,
    pub name: RsStr,
    pub prefix: RsStr,
}

/// NzbgetRsManifestSelect: a number, or a string when is_string.
#[repr(C)]
pub struct ManifestSelect {
    pub is_string: c_int,
    pub number: f64,
    pub string: RsStr,
}

/// NzbgetRsManifestOption.
#[repr(C)]
pub struct ManifestOptionOut {
    pub section: ManifestSection,
    pub name: RsStr,
    pub display_name: RsStr,
    pub description: *const RsStr,
    pub description_count: usize,
    pub value: ManifestSelect,
    pub select: *const ManifestSelect,
    pub select_count: usize,
}

/// NzbgetRsManifestCommand.
#[repr(C)]
pub struct ManifestCommandOut {
    pub section: ManifestSection,
    pub name: RsStr,
    pub display_name: RsStr,
    pub action: RsStr,
    pub description: *const RsStr,
    pub description_count: usize,
}

/// NzbgetRsManifest: the string fields in the order of manifest::FIELDS
/// (`has` says which are set), the texts, options and commands.
#[repr(C)]
pub struct ManifestOut {
    pub fields: [RsStr; 12],
    pub has: [c_int; 12],
    pub description: *const RsStr,
    pub description_count: usize,
    pub requirements: *const RsStr,
    pub requirements_count: usize,
    pub options: *const ManifestOptionOut,
    pub option_count: usize,
    pub commands: *const ManifestCommandOut,
    pub command_count: usize,
    pub sections: *const ManifestSection,
    pub section_count: usize,
}

fn rs_strs(v: &[Vec<u8>]) -> Vec<RsStr> {
    v.iter().map(|s| rs_str(s)).collect()
}

fn manifest_section(s: &crate::manifest::Section) -> ManifestSection {
    ManifestSection { multi: s.multi as c_int, name: rs_str(&s.name), prefix: rs_str(&s.prefix) }
}

fn manifest_select(s: &crate::manifest::Select) -> ManifestSelect {
    match s {
        crate::manifest::Select::Number(n) => ManifestSelect { is_string: 0, number: *n, string: rs_str(b"") },
        crate::manifest::Select::String(s) => ManifestSelect { is_string: 1, number: 0.0, string: rs_str(s) },
    }
}

/// ManifestFile::Load on the text the C++ read: 1 loaded (`loaded` was
/// called), 0 refused, -1 a syntax error, -2 not a JSON object, -3 an option,
/// command or section that isn't an object. On 0 and -3 the callback receives
/// the partial writes preceding the failure, as in the original C++.
///
/// # Safety
/// `text` is null (empty) or `len` readable bytes (at most isize::MAX).
/// `loaded` (null: not called) gets a manifest borrowed only for the call
/// and must not unwind. Rust panics abort rather than crossing the ABI.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_manifest_load(text: *const c_char, len: usize, limits: crate::json::Limits, ctx: *mut std::ffi::c_void,
    loaded: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const ManifestOut)>) -> c_int {
    use crate::manifest::Outcome;
    let (manifest, status) = match crate::manifest::load(bytes_arg(text.cast(), len), limits) {
        Outcome::Loaded(m) => (m, 1),
        Outcome::Refused(m) => (m, 0),
        Outcome::Syntax => return -1,
        Outcome::NotObject => return -2,
        Outcome::ElementNotObject(m) => (m, -3),
    };
    let Some(loaded) = loaded else { return status };
    let description = rs_strs(&manifest.description);
    let requirements = rs_strs(&manifest.requirements);
    let option_texts: Vec<(Vec<RsStr>, Vec<ManifestSelect>)> = manifest.options.iter()
        .map(|o| (rs_strs(&o.description), o.select.iter().map(manifest_select).collect())).collect();
    let options: Vec<ManifestOptionOut> = manifest.options.iter().zip(&option_texts).map(|(o, (d, s))| ManifestOptionOut {
        section: manifest_section(&o.section), name: rs_str(&o.name), display_name: rs_str(&o.display_name),
        description: d.as_ptr(), description_count: d.len(), value: manifest_select(&o.value),
        select: s.as_ptr(), select_count: s.len(),
    }).collect();
    let command_texts: Vec<Vec<RsStr>> = manifest.commands.iter().map(|c| rs_strs(&c.description)).collect();
    let commands: Vec<ManifestCommandOut> = manifest.commands.iter().zip(&command_texts).map(|(c, d)| ManifestCommandOut {
        section: manifest_section(&c.section), name: rs_str(&c.name), display_name: rs_str(&c.display_name),
        action: rs_str(&c.action), description: d.as_ptr(), description_count: d.len(),
    }).collect();
    let sections: Vec<ManifestSection> = manifest.sections.iter().map(manifest_section).collect();
    let out = ManifestOut {
        fields: std::array::from_fn(|i| rs_str(manifest.fields[i].as_deref().unwrap_or(b""))),
        has: std::array::from_fn(|i| manifest.fields[i].is_some() as c_int),
        description: description.as_ptr(), description_count: description.len(),
        requirements: requirements.as_ptr(), requirements_count: requirements.len(),
        options: options.as_ptr(), option_count: options.len(),
        commands: commands.as_ptr(), command_count: commands.len(),
        sections: sections.as_ptr(), section_count: sections.len(),
    };
    loaded(ctx, &out);
    status
}

/// NzbgetRsTestServerParams.
#[repr(C)]
pub struct TestServerParams {
    pub host: RsStr,
    pub username: RsStr,
    pub password: RsStr,
    pub cipher: RsStr,
    pub port: c_int,
    pub timeout: c_int,
    pub cert_verif_level: c_int,
    pub encryption: c_int,
}

/// TestServerXmlCommand's JSON body: 1 parsed (`parsed` was called), 0 not
/// JSON, -1 invalid parameters.
///
/// # Safety
/// `request` is null (empty) or NUL-terminated. `parsed` (null: not called)
/// gets parameters borrowed only for the call and must not unwind.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_test_server_params(request: *const c_char, limits: crate::json::Limits, ctx: *mut std::ffi::c_void,
    parsed: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const TestServerParams)>) -> c_int {
    use crate::rpcparams::ServerParamsError;
    match crate::rpcparams::test_server_params(input(request), limits) {
        Ok(p) => {
            if let Some(parsed) = parsed {
                let out = TestServerParams { host: rs_str(&p.host), username: rs_str(&p.username), password: rs_str(&p.password),
                    cipher: rs_str(&p.cipher), port: p.port, timeout: p.timeout, cert_verif_level: p.cert_verif_level,
                    encryption: p.encryption as c_int };
                parsed(ctx, &out);
            }
            1
        }
        Err(ServerParamsError::Json) => 0,
        Err(ServerParamsError::Params) => -1,
    }
}

/// NzbgetRsParRenamerEnv: ParRenamer's callbacks. Strings are borrowed for
/// the call and NUL-terminated; an int result of -1 (an int64 of -1 for
/// read) reports a C++ exception, which ends the run.
#[repr(C)]
#[derive(Default)]
pub struct ParRenamerEnv {
    pub ctx: *mut std::ffi::c_void,
    pub stopped: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    pub progress: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, c_int) -> c_int>,
    pub message: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char) -> c_int>,
    pub debug: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub list_dir: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *mut std::ffi::c_void,
        unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)) -> c_int>,
    pub dir_exists: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub file_exists: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub file_size: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *mut i64) -> c_int>,
    pub read: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *mut u8, usize) -> i64>,
    pub load_par: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *mut usize) -> c_int>,
    pub par_entry: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void,
        unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char, *const c_char)) -> c_int>,
    pub close_par: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    pub find_main_pars: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *mut std::ffi::c_void,
        unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)) -> c_int>,
    pub register_parred: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub rename: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *const c_char) -> c_int>,
    pub ignored_ext: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub fold: Option<extern "C" fn(c_int) -> c_int>,
    pub state_changed: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut ParRenamerState)>,
    pub previous_dirs: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *mut std::ffi::c_void,
        unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)) -> c_int>,
    pub remember_dir: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub destination: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *mut std::ffi::c_void,
        unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)) -> c_int>,
}

/// NzbgetRsParRenamerState: ParRenamer's members, read and written back.
#[repr(C)]
#[derive(Default, Debug, PartialEq)]
pub struct ParRenamerState {
    pub file_count: c_int,
    pub cur_file: c_int,
    pub renamed_count: c_int,
    pub has_missed_files: c_int,
    pub has_damaged_par_files: c_int,
    pub detect_missing: c_int,
}

struct ParRenamerFfi<'a>(&'a ParRenamerEnv);

fn nul(s: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = s.iter().take_while(|&&b| b != 0).copied().collect();
    v.push(0);
    v
}

unsafe extern "C" fn par_renamer_add_name(sink: *mut std::ffi::c_void, name: *const c_char) {
    (*sink.cast::<Vec<Vec<u8>>>()).push(input(name).to_vec());
}

unsafe extern "C" fn par_renamer_add_entry(sink: *mut std::ffi::c_void, damaged: c_int, name: *const c_char, hash: *const c_char) {
    use crate::parrenamer::ParEntry;
    let entry = if damaged != 0 { ParEntry::Damaged } else { ParEntry::File { name: input(name).to_vec(), hash: input(hash).to_vec() } };
    (*sink.cast::<Vec<crate::parrenamer::ParEntry>>()).push(entry);
}

impl ParRenamerFfi<'_> {
    fn int(r: c_int) -> crate::parrenamer::R<c_int> {
        if r < 0 { Err(crate::parrenamer::Abort) } else { Ok(r) }
    }

    fn names(&self, f: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *mut std::ffi::c_void,
        unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)) -> c_int>, dir: &[u8]) -> crate::parrenamer::R<Vec<Vec<u8>>> {
        let mut names: Vec<Vec<u8>> = Vec::new();
        if let Some(f) = f {
            let dir = nul(dir);
            Self::int(unsafe { f(self.0.ctx, dir.as_ptr().cast(), (&mut names as *mut Vec<Vec<u8>>).cast(), par_renamer_add_name) })?;
        }
        Ok(names)
    }

    fn path_int(&self, f: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>, path: &[u8]) -> crate::parrenamer::R<c_int> {
        let Some(f) = f else { return Ok(0) };
        let path = nul(path);
        Self::int(unsafe { f(self.0.ctx, path.as_ptr().cast()) })
    }
}

impl crate::parrenamer::Env for ParRenamerFfi<'_> {
    fn destination(&mut self, initial: &[u8]) -> crate::parrenamer::R<Vec<u8>> {
        if self.0.destination.is_none() { return Ok(initial.to_vec()) }
        self.names(self.0.destination, b"")?.pop().ok_or(crate::parrenamer::Abort)
    }
    fn state_changed(&mut self, state: &mut crate::parrenamer::State) {
        if let Some(f) = self.0.state_changed {
            let mut st = ParRenamerState {
                file_count: state.file_count, cur_file: state.cur_file, renamed_count: state.renamed_count,
                has_missed_files: state.has_missed_files as c_int,
                has_damaged_par_files: state.has_damaged_par_files as c_int,
                detect_missing: state.detect_missing as c_int,
            };
            unsafe { f(self.0.ctx, &mut st) };
            state.detect_missing = st.detect_missing != 0;
        }
    }
    fn previous_dirs(&mut self) -> crate::parrenamer::R<Vec<Vec<u8>>> {
        self.names(self.0.previous_dirs, b"")
    }
    fn remember_dir(&mut self, dir: &[u8]) -> crate::parrenamer::R<()> {
        self.path_int(self.0.remember_dir, dir).map(drop)
    }
    fn stopped(&mut self) -> crate::parrenamer::R<bool> {
        let Some(f) = self.0.stopped else { return Ok(false) };
        Ok(Self::int(unsafe { f(self.0.ctx) })? != 0)
    }
    fn progress(&mut self, label: &[u8], stage: i32) -> crate::parrenamer::R<()> {
        let Some(f) = self.0.progress else { return Ok(()) };
        let label = nul(label);
        Self::int(unsafe { f(self.0.ctx, label.as_ptr().cast(), stage) }).map(drop)
    }
    fn message(&mut self, kind: crate::parrenamer::Kind, text: &[u8]) -> crate::parrenamer::R<()> {
        let Some(f) = self.0.message else { return Ok(()) };
        let text = nul(text);
        Self::int(unsafe { f(self.0.ctx, kind as c_int, text.as_ptr().cast()) }).map(drop)
    }
    fn debug(&mut self, text: &[u8]) -> crate::parrenamer::R<()> {
        self.path_int(self.0.debug, text).map(drop)
    }
    fn list_dir(&mut self, dir: &[u8]) -> crate::parrenamer::R<Vec<Vec<u8>>> {
        self.names(self.0.list_dir, dir)
    }
    fn dir_exists(&mut self, path: &[u8]) -> crate::parrenamer::R<bool> {
        Ok(self.path_int(self.0.dir_exists, path)? != 0)
    }
    fn file_exists(&mut self, path: &[u8]) -> crate::parrenamer::R<bool> {
        Ok(self.path_int(self.0.file_exists, path)? != 0)
    }
    fn file_size(&mut self, path: &[u8]) -> crate::parrenamer::R<i64> {
        let Some(f) = self.0.file_size else { return Ok(0) };
        let path = nul(path);
        let mut size = 0;
        Self::int(unsafe { f(self.0.ctx, path.as_ptr().cast(), &mut size) })?;
        Ok(size)
    }
    fn read(&mut self, path: &[u8], buf: &mut [u8]) -> crate::parrenamer::R<crate::parrenamer::Read> {
        use crate::parrenamer::Read;
        let Some(f) = self.0.read else { return Ok(Read::OpenFailed) };
        let path = nul(path);
        match unsafe { f(self.0.ctx, path.as_ptr().cast(), buf.as_mut_ptr(), buf.len()) } {
            -2 => Ok(Read::OpenFailed),
            -3 => Ok(Read::ReadFailed),
            n if n >= 0 => Ok(Read::Bytes(n as usize)),
            _ => Err(crate::parrenamer::Abort),
        }
    }
    fn load_par(&mut self, path: &[u8]) -> crate::parrenamer::R<Option<usize>> {
        let Some(f) = self.0.load_par else { return Ok(None) };
        let path = nul(path);
        let mut count = 0;
        let loaded = Self::int(unsafe { f(self.0.ctx, path.as_ptr().cast(), &mut count) })?;
        Ok((loaded != 0).then_some(count))
    }
    fn par_entry(&mut self) -> crate::parrenamer::R<crate::parrenamer::ParEntry> {
        let Some(f) = self.0.par_entry else { return Err(crate::parrenamer::Abort) };
        let mut entries: Vec<crate::parrenamer::ParEntry> = Vec::new();
        Self::int(unsafe { f(self.0.ctx, (&mut entries as *mut Vec<crate::parrenamer::ParEntry>).cast(),
            par_renamer_add_entry) })?;
        entries.pop().ok_or(crate::parrenamer::Abort)
    }
    fn close_par(&mut self) {
        if let Some(f) = self.0.close_par { unsafe { f(self.0.ctx) }; }
    }
    fn find_main_pars(&mut self, dir: &[u8]) -> crate::parrenamer::R<Vec<Vec<u8>>> {
        self.names(self.0.find_main_pars, dir)
    }
    fn register_parred(&mut self, name: &[u8]) -> crate::parrenamer::R<()> {
        self.path_int(self.0.register_parred, name).map(drop)
    }
    fn rename(&mut self, src: &[u8], dst: &[u8]) -> crate::parrenamer::R<bool> {
        let Some(f) = self.0.rename else { return Ok(false) };
        let (src, dst) = (nul(src), nul(dst));
        Ok(Self::int(unsafe { f(self.0.ctx, src.as_ptr().cast(), dst.as_ptr().cast()) })? != 0)
    }
    fn ignored_ext(&mut self, name: &[u8]) -> crate::parrenamer::R<bool> {
        Ok(self.path_int(self.0.ignored_ext, name)? != 0)
    }
    fn fold(&self, byte: u8) -> u8 {
        match self.0.fold {
            Some(f) => f(byte as c_int) as u8,
            None => byte,
        }
    }
}

/// ParRenamer::Execute: 1 when the run ended, 0 when a callback reported an
/// exception (the state then holds what was done until then).
///
/// # Safety
/// `env` and `state` are null (returns 1 without doing anything) or valid;
/// `state` is exclusively borrowed for the call. `info_name` and `dest_dir`
/// are null (empty) or NUL-terminated. The callbacks follow
/// NzbgetRsParRenamerEnv's contract and must not unwind.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_renamer_execute(env: *const ParRenamerEnv, state: *mut ParRenamerState,
    info_name: *const c_char, dest_dir: *const c_char, separator: c_char) -> c_int {
    let (Some(env), Some(st)) = (env.as_ref(), state.as_mut()) else { return 1 };
    let mut rs = crate::parrenamer::State {
        file_count: st.file_count, cur_file: st.cur_file, renamed_count: st.renamed_count,
        has_missed_files: st.has_missed_files != 0, has_damaged_par_files: st.has_damaged_par_files != 0,
        detect_missing: st.detect_missing != 0,
    };
    // A callback may replace the caller's CString members. Do not keep Rust
    // references to their allocations alive while calling back into C++.
    let info_name = input(info_name).to_vec();
    let dest_dir = input(dest_dir).to_vec();
    let result = crate::parrenamer::execute(&mut ParRenamerFfi(env), &mut rs, &info_name, &dest_dir, separator as u8);
    st.file_count = rs.file_count;
    st.cur_file = rs.cur_file;
    st.renamed_count = rs.renamed_count;
    st.has_missed_files = rs.has_missed_files as c_int;
    st.has_damaged_par_files = rs.has_damaged_par_files as c_int;
    st.detect_missing = rs.detect_missing as c_int;
    result.is_ok() as c_int
}

/// NzbgetRsFsOps: FileSystem's calls for the deletes and RenameBak. Strings
/// are borrowed NUL-terminated paths; -1 reports a C++ exception.
#[repr(C)]
pub struct FsOps {
    pub ctx: *mut std::ffi::c_void,
    pub path_type: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub remove_dir: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub delete_file: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub delete_link: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, c_int) -> c_int>,
    pub list: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *mut std::ffi::c_void,
        unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)) -> c_int>,
    pub file_exists: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub dir_exists: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>,
    pub move_file: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, *const c_char) -> c_int>,
    pub last_error: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void,
        unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)) -> c_int>,
    pub too_long: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
}

struct FsOpsFfi<'a>(&'a FsOps);

unsafe extern "C" fn fs_ops_set(sink: *mut std::ffi::c_void, text: *const c_char) {
    *sink.cast::<Vec<u8>>() = input(text).to_vec();
}

impl FsOpsFfi<'_> {
    fn int(r: c_int) -> crate::fsdelete::R<c_int> {
        if r < 0 { Err(crate::fsdelete::Abort) } else { Ok(r) }
    }
    fn path(&self, f: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) -> c_int>, path: &[u8]) -> crate::fsdelete::R<c_int> {
        let Some(f) = f else { return Ok(0) };
        let path = nul(path);
        Self::int(unsafe { f(self.0.ctx, path.as_ptr().cast()) })
    }
}

fn path_type_from(t: c_int) -> crate::fsdelete::PathType {
    use crate::fsdelete::PathType::*;
    match t { 1 => File, 2 => Directory, 3 => Link, 4 => DirectoryLink, _ => Error }
}

impl crate::fsdelete::Ops for FsOpsFfi<'_> {
    fn path_type(&mut self, path: &[u8]) -> crate::fsdelete::R<crate::fsdelete::PathType> {
        Ok(path_type_from(self.path(self.0.path_type, path)?))
    }
    fn remove_dir(&mut self, path: &[u8]) -> crate::fsdelete::R<bool> {
        Ok(self.path(self.0.remove_dir, path)? != 0)
    }
    fn delete_file(&mut self, path: &[u8]) -> crate::fsdelete::R<bool> {
        Ok(self.path(self.0.delete_file, path)? != 0)
    }
    fn delete_link(&mut self, path: &[u8], kind: crate::fsdelete::PathType) -> crate::fsdelete::R<bool> {
        let Some(f) = self.0.delete_link else { return Ok(false) };
        let path = nul(path);
        let kind = match kind { crate::fsdelete::PathType::DirectoryLink => 4, _ => 3 };
        Ok(Self::int(unsafe { f(self.0.ctx, path.as_ptr().cast(), kind) })? != 0)
    }
    fn list(&mut self, dir: &[u8]) -> crate::fsdelete::R<Vec<Vec<u8>>> {
        let mut names: Vec<Vec<u8>> = Vec::new();
        if let Some(f) = self.0.list {
            let dir = nul(dir);
            Self::int(unsafe { f(self.0.ctx, dir.as_ptr().cast(), (&mut names as *mut Vec<Vec<u8>>).cast(), par_renamer_add_name) })?;
        }
        Ok(names)
    }
    fn file_exists(&mut self, path: &[u8]) -> crate::fsdelete::R<bool> {
        Ok(self.path(self.0.file_exists, path)? != 0)
    }
    fn dir_exists(&mut self, path: &[u8]) -> crate::fsdelete::R<bool> {
        Ok(self.path(self.0.dir_exists, path)? != 0)
    }
    fn move_file(&mut self, from: &[u8], to: &[u8]) -> crate::fsdelete::R<bool> {
        let Some(f) = self.0.move_file else { return Ok(false) };
        let (from, to) = (nul(from), nul(to));
        Ok(Self::int(unsafe { f(self.0.ctx, from.as_ptr().cast(), to.as_ptr().cast()) })? != 0)
    }
    fn last_error(&mut self) -> crate::fsdelete::R<Vec<u8>> {
        let mut text = Vec::new();
        if let Some(f) = self.0.last_error {
            Self::int(unsafe { f(self.0.ctx, (&mut text as *mut Vec<u8>).cast(), fs_ops_set) })?;
        }
        Ok(text)
    }
    fn too_long(&mut self) -> crate::fsdelete::R<()> {
        let Some(f) = self.0.too_long else { return Ok(()) };
        Self::int(unsafe { f(self.0.ctx) }).map(drop)
    }
}

/// FileSystem::DeleteDirectory: 1 removed, 0 not, -1 a callback's exception.
///
/// # Safety
/// `ops` is null (returns 0) or valid with callbacks that don't unwind;
/// `dir` is null (empty) or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_delete_directory(ops: *const FsOps, dir: *const c_char, separator: c_char) -> c_int {
    let Some(ops) = ops.as_ref() else { return 0 };
    match crate::fsdelete::delete_directory(&mut FsOpsFfi(ops), input(dir), separator as u8) {
        Ok(done) => done as c_int,
        Err(_) => -1,
    }
}

/// FileSystem::DeleteDirectoryWithContent: 1 all deleted, 0 not, -1 a
/// callback's exception. `errmsg` (null: not written) gets the error, empty
/// when cleared; free it with nzbget_rs_free.
///
/// # Safety
/// As nzbget_rs_delete_directory; `errmsg` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_delete_directory_with_content(ops: *const FsOps, dir: *const c_char, separator: c_char,
    errmsg: *mut RsBuf) -> c_int {
    let mut text = Vec::new();
    let result = match ops.as_ref() {
        Some(ops) => crate::fsdelete::delete_directory_with_content(&mut FsOpsFfi(ops), input(dir), separator as u8, &mut text),
        None => Ok(false),
    };
    if !errmsg.is_null() {
        errmsg.write(into_buf(text));
    }
    match result {
        Ok(done) => done as c_int,
        Err(_) => -1,
    }
}

/// FileSystem::RenameBak: 1 moved, 0 not, -1 a callback's exception.
/// `new_name` (null: not written) gets the name tried last; free it with
/// nzbget_rs_free.
///
/// # Safety
/// As nzbget_rs_delete_directory; `filename`/`bak_part` are null (empty) or
/// NUL-terminated; `new_name` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rename_bak(ops: *const FsOps, filename: *const c_char, bak_part: *const c_char,
    remove_old_extension: c_int, new_name: *mut RsBuf) -> c_int {
    let mut name = Vec::new();
    let result = match ops.as_ref() {
        Some(ops) => crate::fsdelete::rename_bak(&mut FsOpsFfi(ops), input(filename), input(bak_part), remove_old_extension != 0, &mut name),
        None => Ok(false),
    };
    if !new_name.is_null() {
        new_name.write(into_buf(name));
    }
    match result {
        Ok(done) => done as c_int,
        Err(_) => -1,
    }
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

/// ReceiveTrailing's length check: 1 when 0 <= len <= max.
#[no_mangle]
pub extern "C" fn nzbget_rs_binrpc_trailing_ok(len: i64, max: i64) -> c_int {
    crate::binrpc::trailing_ok(len, max) as c_int
}

/// EditQueue's size check: 1 when the client's counts and lengths fit
/// each other and the data length.
#[no_mangle]
pub extern "C" fn nzbget_rs_binrpc_edit_sizes(ids: i32, names: i32, names_len: i32, text_len: i32, data_len: u32) -> c_int {
    crate::binrpc::edit_sizes_ok(ids, names, names_len, text_len, data_len) as c_int
}

/// EditQueue's trailing data: checks that `nr_ids` ids after the text and
/// `nr_names` names after them fit, each name ending within the data, and
/// writes the ids into `ids` (host order); 1 when all fit, 0 (with `ids`
/// untouched) otherwise.
///
/// # Safety
/// `data` is readable for `len` bytes (or null with `len` 0); `ids` is a
/// writable, aligned array of `nr_ids` (or null for 0).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_edit_split(data: *const c_char, len: usize, text_len: usize, nr_ids: usize,
    nr_names: usize, ids: *mut u32) -> c_int {
    if nr_ids > 0 && ids.is_null() {
        return 0;
    }
    let data = bytes_arg(data.cast(), len);
    let ids: &mut [u32] = if nr_ids > 0 { std::slice::from_raw_parts_mut(ids, nr_ids) } else { &mut [] };
    crate::binrpc::edit_split(data, text_len, ids, nr_names) as c_int
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

/// UrlCoordinator::UrlCompleted's statuses: reads and writes `url_status`
/// and `delete_status`, writes `retry`.
///
/// # Safety
/// The pointers are null (not read or written) or valid.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_url_completed(downloader: c_int, deleting: c_int, url_status: *mut c_int,
    delete_status: *mut c_int, retry: *mut c_int) {
    let read = |p: *mut c_int| if p.is_null() { 0 } else { *p };
    let (url, delete, again) = crate::urlcoord::completed(downloader, deleting != 0, read(url_status), read(delete_status));
    if !url_status.is_null() {
        *url_status = url;
    }
    if !delete_status.is_null() {
        *delete_status = delete;
    }
    if !retry.is_null() {
        *retry = again as c_int;
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

/// The trailing data of a List response (binrpc::encode of the NZBs, then
/// the parameters, then the files, in one allocation; free with
/// nzbget_rs_free). Empty (null data, length 0) with no entries or when
/// the data would be more than INT_MAX bytes. NULL arrays are empty.
///
/// # Safety
/// The arrays are null or readable for their counts, the entries' strings
/// readable for their lengths.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_encode_list(nzbs: *const crate::binrpc::NzbEntry, nzb_count: usize,
    params: *const crate::binrpc::ParamEntry, param_count: usize, files: *const crate::binrpc::FileEntry,
    file_count: usize) -> RsBuf {
    use crate::binrpc::{encode, encoded_len};
    let (nzbs, params, files) = (batch(nzbs, nzb_count), batch(params, param_count), batch(files, file_count));
    let total = encoded_len(nzbs).saturating_add(encoded_len(params)).saturating_add(encoded_len(files));
    response(total, |out| {
        let mut at = encode(nzbs, out);
        at += encode(params, &mut out[at..]);
        at + encode(files, &mut out[at..])
    })
}

/// The trailing data of a Log response (as nzbget_rs_binrpc_encode_list).
///
/// # Safety
/// As nzbget_rs_binrpc_encode_list.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_encode_log(log: *const crate::binrpc::LogEntry, count: usize) -> RsBuf {
    let log = batch(log, count);
    response(crate::binrpc::encoded_len(log), |out| crate::binrpc::encode(log, out))
}

/// The trailing data of a PostQueue response (as nzbget_rs_binrpc_encode_list).
///
/// # Safety
/// As nzbget_rs_binrpc_encode_list.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_encode_post_queue(jobs: *const crate::binrpc::PostEntry, count: usize) -> RsBuf {
    let jobs = batch(jobs, count);
    response(crate::binrpc::encoded_len(jobs), |out| crate::binrpc::encode(jobs, out))
}

/// The trailing data of a History response (as nzbget_rs_binrpc_encode_list).
///
/// # Safety
/// As nzbget_rs_binrpc_encode_list.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_encode_history(history: *const crate::binrpc::HistoryEntry, count: usize)
    -> RsBuf {
    let history = batch(history, count);
    response(crate::binrpc::encoded_len(history), |out| crate::binrpc::encode(history, out))
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

/// The remote client's reading of up to `max` list-response entries from
/// `*at` of the `len` bytes of data into `out`: the number read, with `*at`
/// past them. Fewer than `max` when an entry or one of its strings (up to
/// its NUL) doesn't fit in `len`: the entries before it are read. The
/// strings point into `data` (and end at their NULs).
///
/// # Safety
/// `data` is readable for `len` (or null with 0); `at` is valid; `out` is
/// writable for `max` entries (or null for 0) and may be uninitialized.
unsafe fn read_entries<E: crate::binrpc::Entry>(data: *const c_char, len: usize, at: *mut usize, out: *mut E,
    max: usize) -> usize {
    if at.is_null() || (max > 0 && out.is_null()) {
        return 0;
    }
    crate::binrpc::read_many(bytes_arg(data.cast(), len), &mut *at, out, max)
}

/// The NZB entries of a List response (see read_entries above).
///
/// # Safety
/// As read_entries above.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_read_nzbs(data: *const c_char, len: usize, at: *mut usize,
    out: *mut crate::binrpc::NzbEntry, max: usize) -> usize {
    read_entries(data, len, at, out, max)
}

/// The parameter entries of a List response (see read_entries above).
///
/// # Safety
/// As read_entries above.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_read_params(data: *const c_char, len: usize, at: *mut usize,
    out: *mut crate::binrpc::ParamEntry, max: usize) -> usize {
    read_entries(data, len, at, out, max)
}

/// The file entries of a List response (see read_entries above).
///
/// # Safety
/// As read_entries above.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_binrpc_read_files(data: *const c_char, len: usize, at: *mut usize,
    out: *mut crate::binrpc::FileEntry, max: usize) -> usize {
    read_entries(data, len, at, out, max)
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

/// NzbFile::AddFileInfo's totals for one file.
#[repr(C)]
pub struct RsFileTotals {
    kept: usize,
    total_articles: c_int,
    missed_articles: c_int,
    size: i64,
    remaining_size: i64,
    missed_size: i64,
    cut_short: c_int,
}

/// A file's segments (`parts` and `sizes`, `count` of them in the nzb's
/// order) as NzbFile::AddFileInfo makes them: 1 with the kept segments'
/// indexes in part order in `order[0..totals.kept]` and `totals` set; 0
/// (outputs untouched) when none is left or an argument is missing.
///
/// # Safety
/// `subject` is null (empty) or NUL-terminated; `parts`, `sizes` and
/// `order` are arrays of `count` (null for 0); `ct` and `totals` valid.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_file_totals(subject: *const c_char, parts: *const c_int, sizes: *const i64,
    count: usize, ct: *const RsCtype, order: *mut usize, totals: *mut RsFileTotals) -> c_int {
    let Some(ct) = rs_ctype(ct) else { return 0 };
    if totals.is_null() || (count > 0 && (parts.is_null() || sizes.is_null() || order.is_null())) {
        return 0;
    }
    let (parts, sizes) = if count > 0 {
        (std::slice::from_raw_parts(parts, count), std::slice::from_raw_parts(sizes, count))
    } else {
        (&[][..], &[][..])
    };
    let Some(t) = crate::nzbfile::file_totals(input(subject), parts, sizes, &ct) else { return 0 };
    std::ptr::copy_nonoverlapping(t.order.as_ptr(), order, t.order.len());
    *totals = RsFileTotals {
        kept: t.order.len(),
        total_articles: t.total_articles,
        missed_articles: t.missed_articles,
        size: t.size,
        remaining_size: t.remaining_size,
        missed_size: t.missed_size,
        cut_short: t.cut_short as c_int,
    };
    1
}

/// NzbFile::DeclaredParts: the "(1/N)" part count ending the subject, 0
/// for none (or a missing ctype).
///
/// # Safety
/// `subject` is null (empty) or NUL-terminated; `ct` valid.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_declared_parts(subject: *const c_char, ct: *const RsCtype) -> c_int {
    rs_ctype(ct).map_or(0, |ct| crate::nzbfile::declared_parts(input(subject), &ct))
}

/// NzbFile::HasDuplicateFilenames over `count` files' names and subjects.
///
/// # Safety
/// `names` and `subjects` are arrays of `count` NUL-terminated strings (null
/// entries are empty), or null for 0.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nzb_duplicate_filenames(names: *const *const c_char,
    subjects: *const *const c_char, count: usize) -> c_int {
    if count > 0 && (names.is_null() || subjects.is_null()) {
        return 0;
    }
    let files: Vec<(&[u8], &[u8])> = (0..count).map(|i| (input(*names.add(i)), input(*subjects.add(i)))).collect();
    crate::nzbfile::has_duplicate_filenames(&files) as c_int
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
            let none = nzbget_rs_binrpc_encode_log(null(), 5);
            assert_eq!((none.data, none.len), (null_mut(), 0));
            nzbget_rs_free(none);
            let none = nzbget_rs_binrpc_encode_list(null(), 0, null(), 0, null(), 0);
            assert_eq!(none.len, 0);
            nzbget_rs_free(none);

            let entries = [log(1, 2, b"hello"), log(2, 4, b""), log(3, 0, b"abcd")];
            let buf = nzbget_rs_binrpc_encode_log(entries.as_ptr(), entries.len());
            assert_eq!(buf.len, 24 + 20 + 24);
            // the text is a C string past the data, too
            assert_eq!(*buf.data.add(buf.len), 0);
            let mut out = [std::mem::MaybeUninit::<LogEntry>::uninit(); 4];
            let mut at = 0;
            let n = nzbget_rs_binrpc_read_entries_for_test(buf.data, buf.len, &mut at, out.as_mut_ptr().cast());
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
            let buf = nzbget_rs_binrpc_encode_list(nzbs.as_ptr(), 1, params.as_ptr(), 1, files.as_ptr(), 1);
            assert_eq!(buf.len, (68 + 8) + (12 + 4) + (48 + 4));
            let data = bytes(&buf);
            assert_eq!(&data[0..4], [0, 0, 0, 1]);
            assert_eq!(data[68..70], *b"f\0");
            assert_eq!(&data[76..80], [0, 0, 0, 1]);
            let (mut at, mut nz, mut pa, mut fi) = (0usize, MaybeUninitNzb::new(), MaybeUninitPar::new(), MaybeUninitFile::new());
            assert_eq!(nzbget_rs_binrpc_read_nzbs(buf.data, buf.len, &mut at, nz.0.as_mut_ptr().cast(), 1), 1);
            assert_eq!(nzbget_rs_binrpc_read_params(buf.data, buf.len, &mut at, pa.0.as_mut_ptr().cast(), 1), 1);
            assert_eq!(nzbget_rs_binrpc_read_files(buf.data, buf.len, &mut at, fi.0.as_mut_ptr().cast(), 1), 1);
            assert_eq!(at, buf.len);
            assert_eq!(nz.0[0].assume_init().matched, 1);
            assert_eq!(pa.0[0].assume_init().nzb_index, 1);
            assert_eq!(fi.0[0].assume_init().id, 9);
            nzbget_rs_free(buf);

            // the other two kinds
            let jobs = [PostEntry { id: 1, stage: 7, stage_progress: 100, file_progress: 0, start_time: 0, stage_time: 0,
                texts: [Text::EMPTY, Text::new(b"Name"), Text::EMPTY, Text::EMPTY] }];
            let buf = nzbget_rs_binrpc_encode_post_queue(jobs.as_ptr(), 1);
            assert_eq!(buf.len, 40 + 1 + 5 + 1 + 1);
            let listing = nzbget_rs_binrpc_post_queue_listing(buf.data, buf.len, 1);
            assert_eq!(bytes(&listing), b"[1] Name, Unpacking, 10%\n");
            nzbget_rs_free(listing);
            nzbget_rs_free(buf);
            let history = [HistoryEntry { id: 1, kind: 1, time: 0, size_lo: 2048, size_hi: 0, file_count: 2, par_status: 3,
                script_status: 3, url_status: 0, texts: [Text::new(b"Nzb")] }];
            let buf = nzbget_rs_binrpc_encode_history(history.as_ptr(), 1);
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

    /// Reads up to 3 log entries (a test stand-in for a kind's reader).
    unsafe fn nzbget_rs_binrpc_read_entries_for_test(data: *const c_char, len: usize, at: *mut usize, out: *mut LogEntry) -> usize {
        read_entries(data, len, at, out, 3)
    }

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
            let owned = nzbget_rs_binrpc_encode_log(entries.as_ptr(), entries.len());
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
    fn readers_check_their_arguments() {
        unsafe {
            let entries = [log(1, 2, b"hello")];
            let buf = nzbget_rs_binrpc_encode_log(entries.as_ptr(), 1);
            let mut out = [std::mem::MaybeUninit::<LogEntry>::uninit(); 1];
            let mut at = 0;
            // no position, no output
            assert_eq!(nzbget_rs_binrpc_read_nzbs(buf.data, buf.len, null_mut(), null_mut(), 0), 0);
            assert_eq!(nzbget_rs_binrpc_read_nzbs(buf.data, buf.len, &mut at, null_mut(), 1), 0);
            assert_eq!(at, 0);
            // zero entries, a null output, and no data
            assert_eq!(nzbget_rs_binrpc_read_nzbs(buf.data, buf.len, &mut at, null_mut(), 0), 0);
            assert_eq!(read_entries(null(), 0, &mut at, out.as_mut_ptr().cast::<LogEntry>(), 1), 0);
            assert_eq!(read_entries(buf.data, buf.len - 1, &mut at, out.as_mut_ptr().cast::<LogEntry>(), 1), 0);
            assert_eq!(at, 0);
            assert_eq!(read_entries(buf.data, buf.len, &mut at, out.as_mut_ptr().cast::<LogEntry>(), 1), 1);
            assert_eq!(at, buf.len);
            nzbget_rs_free(buf);
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
            let buf = nzbget_rs_binrpc_encode_log(entries.as_ptr(), 1);
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
    fn fsdelete_null_ops_initialize_outputs() {
        use std::ptr::{null, null_mut};
        // Poison rather than zero the output: a caller need not initialize
        // an out parameter, even when the operation fails.
        let poison = || RsBuf { data: null_mut(), len: usize::MAX, cap: usize::MAX };
        unsafe {
            let mut error = poison();
            assert_eq!(nzbget_rs_delete_directory_with_content(null(), null(), b'/' as c_char, &mut error), 0);
            assert_eq!(error.len, 0);
            assert_eq!(input(error.data), b"");
            nzbget_rs_free(error);
            let mut name = poison();
            assert_eq!(nzbget_rs_rename_bak(null(), null(), null(), 0, &mut name), 0);
            assert_eq!(name.len, 0);
            assert_eq!(input(name.data), b"");
            nzbget_rs_free(name);
            assert_eq!(nzbget_rs_delete_directory(null(), null(), b'/' as c_char), 0);
            assert_eq!(nzbget_rs_delete_directory_with_content(null(), null(), 0, null_mut()), 0);
            assert_eq!(nzbget_rs_rename_bak(null(), null(), null(), 0, null_mut()), 0);
            assert_eq!(nzbget_rs_file_extension(null(), 0), -1);
            assert_eq!(nzbget_rs_file_extension(b"a.\0b.c".as_ptr().cast(), 6), 4);
        }
    }

    #[test]
    fn fsdelete_long_callback_and_abort() {
        unsafe extern "C" fn too_long(ctx: *mut std::ffi::c_void) -> c_int {
            let calls = &mut *ctx.cast::<i32>();
            *calls += 1;
            if *calls < 0 { -1 } else { 0 }
        }
        let mut calls = 0;
        let ops = FsOps {
            ctx: (&mut calls as *mut i32).cast(), path_type: None, remove_dir: None,
            delete_file: None, delete_link: None, list: None, file_exists: None,
            dir_exists: None, move_file: None, last_error: None, too_long: Some(too_long),
        };
        let path = nul(&vec![b'a'; 1024]);
        unsafe {
            let mut error = std::mem::MaybeUninit::uninit();
            assert_eq!(nzbget_rs_delete_directory_with_content(&ops, path.as_ptr().cast(), b'/' as c_char, error.as_mut_ptr()), 0);
            let error = error.assume_init();
            assert_eq!(input(error.data), [&b"could not delete "[..], &path[..1024], b": "].concat());
            assert!(calls > 0);
            nzbget_rs_free(error);
            calls = -2;
            assert_eq!(nzbget_rs_delete_directory(&ops, path.as_ptr().cast(), b'/' as c_char), -1);
            assert_eq!(calls, -1);
            calls = -2;
            let mut aborted = std::mem::MaybeUninit::uninit();
            assert_eq!(nzbget_rs_delete_directory_with_content(&ops, path.as_ptr().cast(), b'/' as c_char, aborted.as_mut_ptr()), -1);
            let aborted = aborted.assume_init();
            assert_eq!(calls, -1);
            assert_eq!(aborted.len, 0);
            nzbget_rs_free(aborted);
        }
    }

    #[test]
    fn fsdelete_borrowed_buffers_and_callback_abort() {
        use std::ptr::{null, null_mut};
        struct State { calls: Vec<Vec<u8>>, abort: bool }
        unsafe extern "C" fn kind(ctx: *mut std::ffi::c_void, path: *const c_char) -> c_int {
            let st = &mut *ctx.cast::<State>();
            st.calls.push(input(path).to_vec());
            if input(path) == b"root" { 2 } else if st.abort { -1 } else { 1 }
        }
        unsafe extern "C" fn list(_: *mut std::ffi::c_void, _: *const c_char,
            sink: *mut std::ffi::c_void, add: unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)) -> c_int {
            let mut name = b"file\0".to_vec();
            add(sink, name.as_ptr().cast());
            name.fill(b'x'); // the callback's buffer may immediately be reused
            0
        }
        unsafe extern "C" fn last_error(_: *mut std::ffi::c_void, sink: *mut std::ffi::c_void,
            set: unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)) -> c_int {
            let mut text = b"failure\0".to_vec();
            set(sink, text.as_ptr().cast());
            text.fill(b'x');
            0
        }
        unsafe extern "C" fn fail(ctx: *mut std::ffi::c_void, path: *const c_char) -> c_int {
            (*ctx.cast::<State>()).calls.push(input(path).to_vec());
            0
        }
        let mut st = State { calls: Vec::new(), abort: false };
        let mut ops = FsOps {
            ctx: (&mut st as *mut State).cast(), path_type: Some(kind), remove_dir: Some(fail),
            delete_file: Some(fail), delete_link: None, list: Some(list),
            file_exists: None, dir_exists: None, move_file: None, last_error: Some(last_error), too_long: None,
        };
        unsafe {
            let mut error = std::mem::MaybeUninit::uninit();
            assert_eq!(nzbget_rs_delete_directory_with_content(&ops, c"root".as_ptr(), b'/' as c_char, error.as_mut_ptr()), 0);
            let error = error.assume_init();
            assert_eq!(input(error.data), b"could not delete root/file: failure");
            assert_eq!(st.calls, [b"root".to_vec(), b"root/file".to_vec(), b"root/file".to_vec(), b"root".to_vec()]);
            st.calls.clear();
            st.abort = true;
            let mut aborted = std::mem::MaybeUninit::uninit();
            assert_eq!(nzbget_rs_delete_directory_with_content(&ops, c"root".as_ptr(), b'/' as c_char, aborted.as_mut_ptr()), -1);
            let aborted = aborted.assume_init();
            assert_eq!(aborted.len, 0);
            assert_eq!(st.calls, [b"root".to_vec(), b"root/file".to_vec()]);
            // An independent call cannot invalidate a previously returned buffer.
            assert_eq!(input(error.data), b"could not delete root/file: failure");
            nzbget_rs_free(error);
            nzbget_rs_free(aborted);
            ops.path_type = None;
            assert_eq!(nzbget_rs_delete_directory_with_content(&ops, null(), 0, null_mut()), 0);
            let mut name = std::mem::MaybeUninit::uninit();
            assert_eq!(nzbget_rs_rename_bak(&ops, null(), null(), 0, name.as_mut_ptr()), 0);
            let name = name.assume_init();
            assert_eq!(input(name.data), b".");
            nzbget_rs_free(name);
        }
    }

    #[test]
    fn parrenamer_nulls_borrowed_names_and_exception_state() {
        use std::ptr::{null, null_mut};
        unsafe extern "C" fn list(_: *mut std::ffi::c_void, _: *const c_char,
            sink: *mut std::ffi::c_void, add: unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)) -> c_int {
            let name = std::ffi::CString::new("file").unwrap();
            add(sink, name.as_ptr());
            // The name is destroyed before Rust consumes the directory snapshot.
            0
        }
        unsafe extern "C" fn fail(_: *mut std::ffi::c_void, _: c_int, _: *const c_char) -> c_int { -1 }
        unsafe extern "C" fn changed(ctx: *mut std::ffi::c_void, st: *mut ParRenamerState) {
            let counts = &mut *ctx.cast::<Vec<(i32, i32)>>();
            counts.push(((*st).file_count, (*st).cur_file));
        }
        unsafe {
            let mut st = ParRenamerState::default();
            let mut env = ParRenamerEnv::default();
            assert_eq!(nzbget_rs_par_renamer_execute(null(), &mut st, null(), null(), b'/' as c_char), 1);
            assert_eq!(nzbget_rs_par_renamer_execute(&env, null_mut(), null(), null(), b'/' as c_char), 1);
            assert_eq!(nzbget_rs_par_renamer_execute(&env, &mut st, null(), null(), b'/' as c_char), 1);
            assert_eq!(st, ParRenamerState::default());
            let mut counts = Vec::<(i32, i32)>::new();
            env.ctx = (&mut counts as *mut Vec<(i32, i32)>).cast();
            env.list_dir = Some(list);
            env.message = Some(fail);
            env.state_changed = Some(changed);
            assert_eq!(nzbget_rs_par_renamer_execute(&env, &mut st, null(), null(), b'/' as c_char), 0);
            assert_eq!((st.file_count, st.cur_file, st.renamed_count), (1, 1, 0));
            assert_eq!(counts.last(), Some(&(1, 1)));
        }
    }

    #[test]
    fn manifest_nulls_and_partial_borrowed_view() {
        use std::ptr::{null, null_mut};
        unsafe extern "C" fn copy_author(ctx: *mut std::ffi::c_void, view: *const ManifestOut) {
            let view = &*view;
            let author = &view.fields[0];
            *ctx.cast::<Vec<u8>>() = std::slice::from_raw_parts(author.data.cast(), author.len).to_vec();
        }
        unsafe {
            assert_eq!(nzbget_rs_manifest_load(null(), usize::MAX, crate::json::TEST_LIMITS, null_mut(), None), -1);
            let text = br#"{"author":"copied\u0000discarded"}"#;
            let mut author = Vec::<u8>::new();
            assert_eq!(nzbget_rs_manifest_load(text.as_ptr().cast(), text.len(), crate::json::TEST_LIMITS,
                (&mut author as *mut Vec<u8>).cast(), Some(copy_author)), 0);
            // The callback's copy is still owned after all borrowed storage is freed.
            assert_eq!(author, b"copied");
            assert_eq!(nzbget_rs_manifest_load(text.as_ptr().cast(), text.len(), crate::json::TEST_LIMITS, null_mut(), None), 0);
        }
    }

    #[test]
    fn rarrenamer_nulls_and_borrowed_callback_strings() {
        use std::ptr::{null, null_mut};
        unsafe extern "C" fn rename(ctx: *mut std::ffi::c_void, index: usize, dest: *const c_char) {
            let out = &mut *ctx.cast::<Vec<(usize, Vec<u8>)>>();
            // Copy while borrowed, and retain the copy after the call returns.
            out.push((index, CStr::from_ptr(dest).to_bytes().to_vec()));
        }
        let mut out: Vec<(usize, Vec<u8>)> = Vec::new();
        let mut env = RarRenEnv { ctx: (&mut out as *mut Vec<(usize, Vec<u8>)>).cast(),
            file_exists: None, match_part: None, rename: Some(rename) };
        let file = RarRenFile { name: c"movie.mkv".as_ptr(), name_len: 9, size: 1,
            split_before: 0, split_after: 0 };
        let mut volume = RarRenVolume { filename: c"obfuscated".as_ptr(), filename_len: 10,
            version: 5, volume_no: 0, multi_volume: 0, has_next_volume: 0,
            new_naming: 1, files: &file, file_count: 1 };
        unsafe {
            nzbget_rs_rar_rename_files(null(), null(), 0, null());
            nzbget_rs_rar_rename_files(null(), &volume, 1, &env);
            nzbget_rs_rar_rename_files(c"dir".as_ptr(), null(), 1, &env);
            nzbget_rs_rar_rename_files(c"dir".as_ptr(), null(), 0, &env);
            assert!(out.is_empty());
            nzbget_rs_rar_rename_files(c"dir".as_ptr(), &volume, 1, &env);
            assert_eq!(out, vec![(0, [b"dir".as_slice(), &[crate::paths::PATH_SEPARATOR], b"movie.part0001.rar"].concat())]);
            volume.files = null();
            volume.filename = null();
            nzbget_rs_rar_rename_files(c"dir".as_ptr(), &volume, 1, &env);
            assert_eq!(out.len(), 1);
            env.ctx = null_mut();
            env.rename = None;
            volume.files = &file;
            nzbget_rs_rar_rename_files(c"dir".as_ptr(), &volume, 1, &env);
        }
    }

    #[test]
    fn rarreader_nulls_borrowed_files_and_failed_append() {
        use std::ptr::{null, null_mut};
        struct Context {
            data: Vec<u8>,
            names: Vec<Vec<u8>>,
            positions: Vec<i64>,
            reject: bool,
            freed: usize,
        }
        unsafe extern "C" fn size(ctx: *mut std::ffi::c_void) -> i64 {
            (*(ctx as *mut Context)).data.len() as i64
        }
        unsafe extern "C" fn read(ctx: *mut std::ffi::c_void, offset: i64, buf: *mut u8, len: i64) -> c_int {
            let ctx = &*(ctx as *mut Context);
            let Some(data) = ctx.data.get(offset as usize..offset as usize + len as usize) else { return 0 };
            std::ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len());
            1
        }
        unsafe extern "C" fn add(ctx: *mut std::ffi::c_void, file: *const RarFileOut) -> c_int {
            let ctx = &mut *(ctx as *mut Context);
            if ctx.reject { return 0; }
            let file = &*file;
            assert_eq!(file.data_offset, -1);
            ctx.names.push(std::slice::from_raw_parts(file.filename.cast(), file.filename_len).to_vec());
            1
        }
        unsafe extern "C" fn offset(ctx: *mut std::ffi::c_void, position: i64) {
            (*(ctx as *mut Context)).positions.push(position);
        }
        unsafe extern "C" fn free(ctx: *mut std::ffi::c_void) {
            (*(ctx as *mut Context)).freed += 1;
        }
        let mut state = RarVolumeState {
            version: 0, volume_no: 0, new_naming: 0, has_next_volume: 0,
            multi_volume: 0, encrypted: 0, has_password: 0,
            decrypt_buf: [0; 16], decrypt_pos: 16,
        };
        let mut io = RarIo {
            ctx: null_mut(), size: None, read: None, rar3_prepare: None,
            rar5_prepare: None, init: None, decrypt: None, free: None,
            add_file: None, set_data_offset: None,
        };
        unsafe {
            assert_eq!(nzbget_rs_rar_read(null(), null_mut()), 0);
            assert_eq!(nzbget_rs_rar_read(null(), &mut state), 0);
            assert_eq!(nzbget_rs_rar_read(&io, null_mut()), 0);
            assert_eq!(nzbget_rs_rar_read(&io, &mut state), 0);
            let mut ctx = Context {
                // Two headers (one empty name), followed by an invalid block.
                data: b"Rar!\x1a\x07\x01\0".to_vec(),
                names: Vec::new(), positions: Vec::new(), reject: false, freed: 0,
            };
            ctx.data.extend([0,0,0,0,9,2,0,0,0,0,0,0,1,b'a']);
            ctx.data.extend([0,0,0,0,8,2,0,0,0,0,0,0,0]);
            io.ctx = (&mut ctx as *mut Context).cast();
            io.size = Some(size);
            io.read = Some(read);
            io.add_file = Some(add);
            io.set_data_offset = Some(offset);
            io.free = Some(free);
            assert_eq!(nzbget_rs_rar_read(&io, &mut state), 0);
            assert_eq!(ctx.names, [b"a".to_vec(), Vec::new()]);
            assert_eq!(ctx.positions, [22, 35]);
            assert_eq!(ctx.freed, 1);
            ctx.data[21] = b'z';
            assert_eq!(ctx.names[0], b"a"); // callback owns its copy
            ctx.reject = true;
            assert_eq!(nzbget_rs_rar_read(&io, &mut state), 0);
            assert_eq!(ctx.names.len(), 2);
            assert_eq!(ctx.positions, [22, 35]); // no seek after rejected append
            assert_eq!(ctx.freed, 2);
            // All byte values are valid FFI state. The main header switches
            // encryption on before any decrypt normalizes an oversized pos.
            ctx.data = b"Rar!\x1a\x07\0".to_vec();
            ctx.data.extend([0, 0, 0x73, 0x80, 0, 13, 0]);
            ctx.data.extend([0; 6]);
            state.has_password = 1;
            for pos in 0..=u8::MAX {
                state.encrypted = 0;
                state.decrypt_pos = pos;
                assert_eq!(nzbget_rs_rar_read(&io, &mut state), 0);
            }
        }
    }

    #[test]
    fn parparser_bounded_input_and_outputs() {
        use std::ptr::{null, null_mut};
        extern "C" fn fold(b: c_int) -> c_int { (b as u8).to_ascii_lowercase() as c_int }
        unsafe {
            let mut output = 77;
            let p = &mut output as *mut c_int;
            for name in [null(), c"bad".as_ptr()] {
                assert_eq!(nzbget_rs_parse_par_filename(name, 1, p, p, std::ptr::null(), 0, Some(fold)), 0);
                assert_eq!(output, 77);
            }
            assert_eq!(nzbget_rs_parse_par_filename(c"a.par2".as_ptr(), 1, p, p, std::ptr::null(), 0, None), 0);
            assert_eq!(output, 77);
            assert_eq!(nzbget_rs_parse_par_filename(c"a.vol+2.par2".as_ptr(), -1, p, p, std::ptr::null(), 0, Some(fold)), 1);
            assert_eq!(output, -1); // blocks wins when outputs alias
            assert_eq!(nzbget_rs_parse_par_filename(c"a.par2".as_ptr(), 1, null_mut(), null_mut(), std::ptr::null(), 0, Some(fold)), 1);
            assert_eq!(nzbget_rs_same_par_collection(null(), c"a.par2".as_ptr(), 1, std::ptr::null(), 0, Some(fold)), 0);
            assert_eq!(nzbget_rs_same_par_collection(c"a.par2".as_ptr(), null(), 1, std::ptr::null(), 0, Some(fold)), 0);
            assert_eq!(nzbget_rs_same_par_collection(c"a.par2".as_ptr(), c"a.par2".as_ptr(), 1, std::ptr::null(), 0, None), 0);

            let mut name = [b'a'; crate::parparser::BSTRING_MAX];
            name[1018..].copy_from_slice(b".par2");
            assert_eq!(nzbget_rs_parse_par_filename(name.as_ptr().cast(), 1, p, null_mut(), std::ptr::null(), 0, Some(fold)), 1);
            assert_eq!(output, 1018);
            assert_eq!(nzbget_rs_same_par_collection(name.as_ptr().cast(), name.as_ptr().cast(), 1, std::ptr::null(), 0, Some(fold)), 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn fsops_null_outputs_and_ownership() {
        unsafe extern "C" fn directory(_: *const c_char) -> c_int { 2 }
        unsafe {
            let mut msg = std::mem::MaybeUninit::uninit();
            assert_eq!(nzbget_rs_force_directories(std::ptr::null(), msg.as_mut_ptr(), Some(directory)), 0);
            let msg = msg.assume_init();
            assert_eq!(msg.len, 0);
            assert_eq!(CStr::from_ptr(msg.data), c"");
            nzbget_rs_free(msg);
            assert_eq!(nzbget_rs_force_directories(c".".as_ptr(), std::ptr::null_mut(), Some(directory)), 0);
            assert_eq!(nzbget_rs_file_exists(std::ptr::null(), Some(directory)), 0);
            assert_eq!(nzbget_rs_directory_exists(c".".as_ptr(), None), 0);
            let mut source = b"~/x\0".to_vec();
            let expanded = nzbget_rs_expand_home_path(source.as_ptr().cast(), c"".as_ptr());
            source[2] = b'y';
            assert_eq!(CStr::from_ptr(expanded.data), c"/x");
            nzbget_rs_free(expanded);
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
    fn webdownload_nulls_offsets_and_owned_redirects() {
        unsafe {
            nzbget_rs_http_check_response(std::ptr::null(), std::ptr::null_mut());
            let mut response = std::mem::MaybeUninit::<HttpResponse>::uninit();
            nzbget_rs_http_check_response(std::ptr::null(), response.as_mut_ptr());
            let response = response.assume_init();
            assert_eq!((response.result, response.set_status, response.warn), (2, 0, 1));

            let line = c"HTTP/1.1 4040 Missing";
            let mut response = std::mem::MaybeUninit::<HttpResponse>::uninit();
            nzbget_rs_http_check_response(line.as_ptr(), response.as_mut_ptr());
            let response = response.assume_init();
            assert_eq!((response.result, response.http_status, response.warn), (3, 4040, 3));
            assert_eq!(CStr::from_ptr(line.as_ptr().add(response.status_offset)), c"4040 Missing");

            // Output pointers require writable storage, not initialized Rust
            // values; they may overlap an input whose bytes are read first.
            let mut slot = std::mem::MaybeUninit::<HttpResponse>::uninit();
            let input = slot.as_mut_ptr().cast::<c_char>();
            std::ptr::copy_nonoverlapping(c"HTTP 404".as_ptr(), input, 9);
            nzbget_rs_http_check_response(input, slot.as_mut_ptr());
            assert_eq!(slot.assume_init().http_status, 404);

            let mut uninit_value = std::mem::MaybeUninit::<c_int>::uninit();
            assert_eq!(nzbget_rs_http_header(c"Content-Length: 17".as_ptr(), 0, uninit_value.as_mut_ptr()), 1);
            assert_eq!(uninit_value.assume_init(), 17);

            let mut value = 123;
            assert_eq!(nzbget_rs_http_header(std::ptr::null(), 1, &mut value), 0);
            assert_eq!(value, 123);
            assert_eq!(nzbget_rs_http_header(c"Content-Length: -7".as_ptr(), 0, std::ptr::null_mut()), 1);
            let line = c"Location: /next";
            assert_eq!(nzbget_rs_http_header(line.as_ptr(), -1, &mut value), 4);
            assert_eq!(CStr::from_ptr(line.as_ptr().add(value as usize)), c"/next");

            let empty = nzbget_rs_http_redirect(std::ptr::null(), std::ptr::null());
            assert_eq!(CStr::from_ptr(empty.data), c"(null)://(null)");
            nzbget_rs_free(empty);
            let base = std::ffi::CString::new("http://h:81/a/file?q").unwrap();
            let location = std::ffi::CString::new("next").unwrap();
            let buf = nzbget_rs_http_redirect(base.as_ptr(), location.as_ptr());
            drop(base);
            drop(location);
            assert_eq!(std::slice::from_raw_parts(buf.data.cast::<u8>(), buf.len), b"http://h:81/a/next");
            assert_eq!(*buf.data.add(buf.len), 0);
            nzbget_rs_free(buf);
        }
    }

    extern "C" fn ext_space(b: c_int) -> c_int {
        i32::from((b as u8).is_ascii_whitespace())
    }

    #[test]
    fn extload_owned_handle_borrowed_bytes_and_nulls() {
        unsafe {
            let null = std::ptr::null();
            assert!(nzbget_rs_ext_v1_parse(null, usize::MAX, Some(ext_space)).is_null());
            assert!(nzbget_rs_ext_v1_parse(null, 0, None).is_null());
            nzbget_rs_ext_v1_free(std::ptr::null_mut());
            let null = std::ptr::null();
            assert_eq!(nzbget_rs_ext_v1_kind(null), 0);
            assert_eq!(nzbget_rs_ext_v1_count(null, 2), 0);
            assert_eq!(nzbget_rs_ext_v1_text(null, 0, 0).len, 0);
            assert_eq!(nzbget_rs_ext_v1_item_text(null, 0, 0, 0, 0).len, 0);
            assert_eq!(nzbget_rs_ext_v1_item_count(null, 0, 0, 0), 0);
            assert_eq!(nzbget_rs_ext_v1_select(null, 0, 0, std::ptr::null_mut(), std::ptr::null_mut()), -1);
            let input = b"### NZBGET SCAN SCRIPT\n### OPTIONS\n# Range (1-2).\n#N=1.5\n#S=a\0b\n#Cmd@go\n".to_vec();
            let h = nzbget_rs_ext_v1_parse(input.as_ptr().cast(), input.len(), Some(ext_space));
            drop(input);
            assert!(!h.is_null());
            assert_eq!(nzbget_rs_ext_v1_kind(h), 2);
            assert_eq!(nzbget_rs_ext_v1_count(h, 2), 2);
            assert_eq!(nzbget_rs_ext_v1_count(h, -1), 0);
            assert_eq!(nzbget_rs_ext_v1_item_text(h, 0, usize::MAX, 0, 0).len, 0);
            assert_eq!(nzbget_rs_ext_v1_item_text(h, 0, 0, 4, usize::MAX).len, 0);
            assert_eq!(nzbget_rs_ext_v1_item_count(h, 1, usize::MAX, 0), 0);
            let mut number = 77.0;
            let mut text = NO_STR;
            assert_eq!(nzbget_rs_ext_v1_select(h, 0, usize::MAX, &mut number, &mut text), 1);
            assert_eq!(number, 1.5);
            assert!(text.data.is_null());
            assert_eq!(nzbget_rs_ext_v1_select(h, 0, usize::MAX, std::ptr::null_mut(), &mut text), -1);
            assert_eq!(nzbget_rs_ext_v1_select(h, 1, usize::MAX, &mut number, &mut text), 0);
            assert_eq!(span(text.data, text.len), b"a\0b");
            assert_eq!(number, 1.5);
            assert_eq!(nzbget_rs_ext_v1_select(h, 1, usize::MAX, &mut number, std::ptr::null_mut()), -1);
            assert_eq!(nzbget_rs_ext_v1_select(h, usize::MAX, 0, &mut number, &mut text), -1);
            assert_eq!(span(text.data, text.len), b"a\0b");
            assert_eq!(nzbget_rs_ext_v1_select(h, 0, 99, &mut number, &mut text), -1);
            nzbget_rs_ext_v1_free(h);
        }
    }

    #[test]
    fn extload_reader_stops_and_discards_failed_parse() {
        struct Reader { index: usize, fail_at: usize }
        unsafe extern "C" fn next(context: *mut std::ffi::c_void, out: *mut RsStr) -> c_int {
            let r = &mut *context.cast::<Reader>();
            if r.index == r.fail_at { return -1 }
            let lines: [&[u8]; 4] = [b"### NZBGET SCAN SCRIPT", b"### OPTIONS", b"#N=x", b" SCRIPT"];
            let Some(line) = lines.get(r.index) else { return -1 };
            r.index += 1;
            *out = rs_str(line);
            1
        }
        unsafe {
            assert!(nzbget_rs_ext_v1_read(None, std::ptr::null_mut(), Some(ext_space)).is_null());
            assert!(nzbget_rs_ext_v1_read(Some(next), std::ptr::null_mut(), None).is_null());
            for fail_at in 0..=4 {
                let mut r = Reader { index: 0, fail_at };
                let h = nzbget_rs_ext_v1_read(Some(next), (&mut r as *mut Reader).cast(), Some(ext_space));
                assert_eq!(r.index, fail_at);
                assert_eq!(h.is_null(), fail_at < 4);
                nzbget_rs_ext_v1_free(h);
            }
        }
    }

    #[test]
    fn options_ffi_outputs() {
        use std::{mem::MaybeUninit, ptr::null_mut};
        unsafe {
            let mut out = 77;
            let p = &mut out as *mut c_int;
            assert_eq!(nzbget_rs_parse_time(c"23:59".as_ptr(), p, p), 1);
            assert_eq!(out, 59);
            assert_eq!(nzbget_rs_parse_time(c"24:00".as_ptr(), p, p), 0);
            assert_eq!(out, 24);
            assert_eq!(nzbget_rs_parse_time(c"*".as_ptr(), p, p), 1);
            assert_eq!(out, 0);
            assert_eq!(nzbget_rs_parse_time(c"1:*".as_ptr(), p, p), 0);
            assert_eq!(out, 1);
            assert_eq!(nzbget_rs_parse_time(c"bad".as_ptr(), p, p), 0);
            assert_eq!(out, 1);
            assert_eq!(nzbget_rs_parse_time(std::ptr::null(), p, p), 0);
            assert_eq!(nzbget_rs_parse_time(c"*".as_ptr(), p, null_mut()), 0);
            assert_eq!(nzbget_rs_parse_time(c"*".as_ptr(), null_mut(), p), 0);
            assert_eq!(out, 1);
            let (mut h, mut m) = (MaybeUninit::uninit(), MaybeUninit::uninit());
            assert_eq!(nzbget_rs_parse_time(c"*:12".as_ptr(), h.as_mut_ptr(), m.as_mut_ptr()), 1);
            assert_eq!((h.assume_init(), m.assume_init()), (-2, 12));
            assert_eq!(nzbget_rs_parse_week_days(std::ptr::null(), p), 1);
            assert_eq!(out, 0);
            assert_eq!(nzbget_rs_parse_week_days(c"1".as_ptr(), null_mut()), 0);

            let (mut o, mut v) = (MaybeUninit::<RsBuf>::uninit(), MaybeUninit::<RsBuf>::uninit());
            let option = std::ffi::CString::new("WriteBufferSize").unwrap();
            let value = std::ffi::CString::new("-1").unwrap();
            nzbget_rs_convert_old_option(option.as_ptr(), value.as_ptr(), o.as_mut_ptr(), v.as_mut_ptr());
            drop((option, value));
            let (o, v) = (o.assume_init(), v.assume_init());
            assert_eq!(CStr::from_ptr(o.data), c"WriteBuffer");
            assert_eq!(CStr::from_ptr(v.data), c"1024");
            nzbget_rs_free(o);
            nzbget_rs_free(v);
            for which in 0..3 {
                let mut out = MaybeUninit::<RsBuf>::uninit();
                let p = out.as_mut_ptr();
                nzbget_rs_convert_old_option(std::ptr::null(), std::ptr::null(),
                    if which == 0 { null_mut() } else { p },
                    if which == 1 { null_mut() } else { p });
                let out = out.assume_init();
                assert_eq!(CStr::from_ptr(out.data), c"");
                nzbget_rs_free(out);
            }
            nzbget_rs_convert_old_option(std::ptr::null(), std::ptr::null(), null_mut(), null_mut());
            assert_eq!(nzbget_rs_validate_option_name(std::ptr::null(), None, null_mut()), 0);
            assert_eq!(nzbget_rs_validate_option_name(c"Server1.Host".as_ptr(), None, null_mut()), 1);
            assert_eq!(nzbget_rs_has_script(std::ptr::null(), std::ptr::null()), 0);
            assert_eq!(nzbget_rs_has_script(c"a.py".as_ptr(), std::ptr::null()), 0);
            assert_eq!(nzbget_rs_parse_category_source(std::ptr::null()), 1);
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
        extern "C" fn space(byte: c_int) -> c_int {
            // An unsigned-char caller in a locale with a high-byte space.
            (byte == 255 || byte == 32) as c_int
        }
        unsafe {
            let null = std::ptr::null_mut();
            assert!(nzbget_rs_trim_line(null, 0).is_null());
            assert_eq!(nzbget_rs_trim_string(null, 10, 1, 1, 1, null.cast(), None), 0);
            assert_eq!(nzbget_rs_parse_rfc822_date_time(null), 0);
            assert_eq!(nzbget_rs_ends_with(null, 0, null, 0, 0), 1);
            assert_eq!(nzbget_rs_ends_with(null, 0, c"x".as_ptr(), 1, 0), 0);
            for buf in [nzbget_rs_split_command_line(null), nzbget_rs_format_buffer(null, 10),
                nzbget_rs_format_buffer(c"x".as_ptr(), -1)]
            {
                assert_eq!(buf.len, 0);
                assert_eq!(*buf.data, 0);
                nzbget_rs_free(buf);
            }
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
            let mut buf = *b"!\t\0x\xff!";
            let mut start = usize::MAX;
            let end = nzbget_rs_trim_string(buf.as_mut_ptr().add(1).cast(), 4, 1, 1, 1,
                &mut start, Some(space));
            assert_eq!((start, end), (2, 3));
            assert_eq!(&buf, b"!  x\xff!");
            let mut all = *b" \t\r\n";
            assert_eq!(nzbget_rs_trim_string(all.as_mut_ptr().cast(), all.len(), 1, 1, 1,
                &mut start, Some(space)), 4);
            assert_eq!(start, 4); // empty kept range, valid for resize then erase
            let mut source = b"a 'b c'\0".to_vec();
            let words = nzbget_rs_split_command_line(source.as_ptr().cast());
            let hex = nzbget_rs_format_buffer(source.as_ptr().cast(), 2);
            source.fill(b'x');
            assert_eq!(span(words.data, words.len), b"a\0b c\0");
            assert_eq!(span(hex.data, hex.len), b"61 20 ");
            nzbget_rs_free(words);
            assert_eq!(input(hex.data), b"61 20 ");
            nzbget_rs_free(hex);
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
            assert_eq!(nzbget_rs_web_authorized_ip(std::ptr::null(), std::ptr::null(), std::ptr::null(), 1, None), 0);
        }
    }

    #[test]
    fn collection_null_inputs_and_callbacks() {
        unsafe {
            let mut out = AnalysisC {
                main_video: 0, sample_video: 0, main_book: 0,
                subtitles: std::ptr::null_mut(), subtitle_count: 99,
                nfos: std::ptr::null_mut(), nfo_count: 99,
                other_files: std::ptr::null_mut(), other_count: 99,
                ambiguous: 1, disc_structure: 1, has_audio: 1,
            };
            nzbget_rs_collection_analyze(std::ptr::null(), usize::MAX, &mut out);
            assert_eq!((out.main_video, out.sample_video, out.main_book), (-1, -1, -1));
            assert_eq!((out.subtitle_count, out.nfo_count, out.other_count), (0, 0, 0));
            assert_eq!((out.ambiguous, out.disc_structure, out.has_audio), (0, 0, 0));
            nzbget_rs_collection_analyze(std::ptr::null(), 0, std::ptr::null_mut());

            let cb = PlanCallbacks { user: std::ptr::null_mut(), exists: None, ignored: None, action: None, stem: None };
            let mut flags = PlanFlagsC { can_rename: 1, ..PlanFlagsC::default() };
            let empty = nzbget_rs_collection_plan(std::ptr::null(), 99, 0, std::ptr::null(), 99, std::ptr::null(), &mut flags);
            assert_eq!(empty.len, 0);
            assert_eq!(flags.can_rename, 0);
            nzbget_rs_free(empty);
            let empty = nzbget_rs_collection_plan(std::ptr::null(), 99, 1, std::ptr::null(), 99, &cb, &mut flags);
            assert_eq!(flags.disc_structure, 1);
            assert_eq!(flags.can_rename, 0);
            nzbget_rs_free(empty);
            let empty = nzbget_rs_collection_plan(std::ptr::null(), 0, 0, std::ptr::null(), 0, &cb, std::ptr::null_mut());
            nzbget_rs_free(empty);

            let file = FileEntryC {
                path: c"/d/abc.mkv".as_ptr(), path_len: 10,
                rename_prefix: c"/d/".as_ptr(), rename_prefix_len: 3,
                filename: c"abc.mkv".as_ptr(), filename_len: 7,
                stem: c"abc".as_ptr(), stem_len: 3,
                ext: c".mkv".as_ptr(), ext_len: 4, size: 1,
            };
            let base = nzbget_rs_collection_plan(&file, 1, 0, c"Movie.2026".as_ptr(), 10, &cb, &mut flags);
            assert_eq!(flags.can_rename, 1);
            assert_eq!(std::slice::from_raw_parts(base.data.cast::<u8>(), base.len), b"Movie.2026");
            // Another result must neither overwrite nor free the first one.
            for op in 0..=2 {
                let name = nzbget_rs_collection_name(op, std::ptr::null(), 99, std::ptr::null(), 99, std::ptr::null(), 99);
                let expected: &[u8] = if op == 2 { b"-sample" } else { b"" };
                assert_eq!(std::slice::from_raw_parts(name.data.cast::<u8>(), name.len), expected);
                nzbget_rs_free(name);
            }
            assert_eq!(std::slice::from_raw_parts(base.data.cast::<u8>(), base.len), b"Movie.2026");
            nzbget_rs_free(base);
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
                for which in -1..=25 {
                    assert_eq!(nzbget_rs_file_type(which, std::ptr::null(), len), 0);
                }
                let mut out_len = usize::MAX;
                let ext = nzbget_rs_sniff_extension(std::ptr::null(), len, &mut out_len);
                assert!(!ext.is_null());
                assert_eq!(out_len, 0);
                assert_eq!(CStr::from_ptr(ext), c"");
            }
            let name = b".rar\0.mkv";
            assert_eq!(nzbget_rs_file_type(1, name.as_ptr().cast(), 4), 1);
            assert_eq!(nzbget_rs_file_type(1, name.as_ptr().cast(), name.len()), 0);
            let header = b"%PDF-".to_vec();
            let mut out_len = 0;
            let ext = nzbget_rs_sniff_extension(header.as_ptr(), header.len(), &mut out_len);
            assert_eq!(out_len, 4);
            drop(header);
            assert_eq!(CStr::from_ptr(ext), c".pdf");
            assert_eq!(CStr::from_ptr(nzbget_rs_sniff_extension(b"ID3".as_ptr(), 3, std::ptr::null_mut())), c".mp3");
            // Later calls and destruction of the input do not invalidate results.
            assert_eq!(CStr::from_ptr(ext), c".pdf");
        }
    }

    #[test]
    fn deobfuscation_nulls_lengths_and_owned_buffers() {
        unsafe {
            for len in [0, 10, usize::MAX] {
                assert_eq!(nzbget_rs_is_excessively_obfuscated(std::ptr::null(), len), 0);
                let result = nzbget_rs_deobfuscate(std::ptr::null(), len);
                assert_eq!(result.len, 0);
                assert_eq!(*result.data, 0);
                nzbget_rs_free(result);
            }
            let mut input = b"prefix \"a\0b\" suffix".to_vec();
            let first = nzbget_rs_deobfuscate(input.as_ptr().cast(), input.len());
            let second = nzbget_rs_deobfuscate(input.as_ptr().cast(), 6);
            input.fill(b'x');
            drop(input);
            assert_eq!(std::slice::from_raw_parts(first.data.cast::<u8>(), first.len + 1), b"a\0b\0");
            assert_eq!(std::slice::from_raw_parts(second.data.cast::<u8>(), second.len + 1), b"prefix\0");
            nzbget_rs_free(second);
            nzbget_rs_free(first);
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
        unsafe extern "C" fn regex_new(_: *mut c_void, _: *const c_char, _: c_int) -> usize { 0 }
        unsafe extern "C" fn regex_match(_: *mut c_void, _: usize, _: *const c_char, _: *mut [c_int; 2], _: c_int) -> c_int { -1 }
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
        extern "C" fn fold(c: c_int) -> c_int { c }
        let mut p = Probe::default();
        let cb = FeedItemCallbacks {
            user: (&mut p as *mut Probe).cast(), field: Some(field),
            season_episode: Some(season), regex_new: Some(regex_new), regex_match: Some(regex_match),
            apply: Some(apply), set_match: Some(set_match), lower_table: std::ptr::null(),
            char_signed: 1, fold: Some(fold),
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
            for missing in 0..7 {
                let mut bad = FeedItemCallbacks { ..cb };
                match missing {
                    0 => bad.field = None,
                    1 => bad.season_episode = None,
                    2 => bad.regex_new = None,
                    3 => bad.regex_match = None,
                    4 => bad.apply = None,
                    5 => bad.set_match = None,
                    _ => bad.fold = None,
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
            assert_eq!(nzbget_rs_alpha_num(std::ptr::null()), 1);
            assert_eq!(nzbget_rs_hash_bj96(std::ptr::null(), 7, 42), crate::util::hash_bj96(&[], 42));
            nzbget_rs_reduce_str(std::ptr::null_mut(), std::ptr::null(), std::ptr::null());
            let mut raw = *b"abc\0";
            nzbget_rs_reduce_str(raw.as_mut_ptr().cast(), std::ptr::null(), c"".as_ptr());
            assert_eq!(&raw, b"abc\0");
            for (format, expected) in [
                (nzbget_rs_format_size as extern "C" fn(i64) -> RsBuf, &b"512 B\0"[..]),
                (nzbget_rs_format_speed, &b"0 KB/s\0"[..]),
            ] {
                let first = format(512);
                let second = format(12345);
                nzbget_rs_free(second);
                assert_eq!(std::slice::from_raw_parts(first.data.cast::<u8>(), first.len + 1), expected);
                nzbget_rs_free(first);
            }
            let mut table = [0; 384];
            for (i, entry) in table.iter_mut().enumerate() {
                *entry = if i >= 128 { ((i - 128) as u8).to_ascii_lowercase() as c_int } else { i as c_int - 128 };
            }
            for list in [c".NZB", c"*.NZ?"] {
                assert_eq!(nzbget_rs_match_file_ext(c"a.nzb".as_ptr(), list.as_ptr(), c",".as_ptr(),
                    table.as_ptr().add(128), 1, None, None), 1);
            }
            assert_eq!(nzbget_rs_match_file_ext(std::ptr::null(), std::ptr::null(), std::ptr::null(),
                std::ptr::null(), 1, None, None), 0);
        }
    }

    extern "C" fn entity_alpha(byte: c_int) -> c_int {
        assert!((0..=255).contains(&byte));
        (byte == b'@' as c_int || byte == 0xe9 || (byte as u8).is_ascii_alphabetic()) as c_int
    }

    extern "C" fn digit_lower(byte: c_int) -> c_int {
        (byte as u8).to_ascii_lowercase() as c_int
    }

    #[test]
    fn webutil_nulls_borrowing_and_ownership() {
        unsafe {
            for find in [nzbget_rs_xml_find_tag, nzbget_rs_json_find_field] {
                let mut length = -7;
                for (text, name) in [(std::ptr::null(), c"x".as_ptr()),
                                     (c"".as_ptr(), std::ptr::null()),
                                     (c"".as_ptr(), c"x".as_ptr())] {
                    assert!(find(text, name, &mut length).is_null());
                    assert_eq!(length, -7);
                }
                assert!(find(c"".as_ptr(), c"x".as_ptr(), std::ptr::null_mut()).is_null());
            }
            let xml = c"<x>abc</x>";
            let json = c"\"x\": 123";
            let mut length = -7;
            assert_eq!(nzbget_rs_xml_find_tag(xml.as_ptr(), c"x".as_ptr(), &mut length), xml.as_ptr().add(3));
            assert_eq!(length, 3);
            assert_eq!(nzbget_rs_json_find_field(json.as_ptr(), c"x".as_ptr(), &mut length), json.as_ptr().add(5));
            assert_eq!(length, 3);

            for fold in [None, Some(digit_lower as extern "C" fn(c_int) -> c_int)] {
                let result = nzbget_rs_content_disposition_filename(std::ptr::null(), std::ptr::null(), fold);
                assert!(result.data.is_null());
                nzbget_rs_free(result);
            }
            for (mut raw, expected) in [(b"filename=\"\"\0".to_vec(), &b"\0"[..]),
                                         (b"FILENAME=abc\0".to_vec(), &b"abc\0"[..])] {
                let result = nzbget_rs_content_disposition_filename(raw.as_ptr().cast(), std::ptr::null(), Some(digit_lower));
                raw.fill(b'!');
                assert!(!result.data.is_null());
                assert_eq!(result.len, expected.len() - 1);
                assert_eq!(std::slice::from_raw_parts(result.data.cast::<u8>(), result.len + 1), expected);
                nzbget_rs_free(result);
            }
        }
    }

    #[test]
    fn text_null_inputs_and_callback() {
        unsafe {
            for f in [nzbget_rs_xml_strip_tags,
                      nzbget_rs_http_unquote, nzbget_rs_url_decode] {
                f(std::ptr::null_mut());
            }
            nzbget_rs_xml_decode(std::ptr::null_mut(), Some(digit_lower));
            nzbget_rs_xml_decode(std::ptr::null_mut(), None);
            let mut xml = *b"&#xA;\0tail";
            nzbget_rs_xml_decode(xml.as_mut_ptr().cast(), None);
            assert_eq!(&xml, b"&#xA;\0tail");
            nzbget_rs_xml_remove_entities(std::ptr::null_mut(), Some(entity_alpha));
            nzbget_rs_xml_remove_entities(std::ptr::null_mut(), None);
            let mut raw = *b"&amp;\0tail";
            nzbget_rs_xml_remove_entities(raw.as_mut_ptr().cast(), None);
            assert_eq!(&raw, b"&amp;\0tail");
            for f in [nzbget_rs_url_encode, nzbget_rs_latin1_to_utf8] {
                let result = f(std::ptr::null());
                assert_eq!(result.len, 0);
                assert!(!result.data.is_null());
                assert_eq!(*result.data, 0);
                nzbget_rs_free(result);
            }
        }
    }

    #[test]
    fn text_locale_callback_and_embedded_nuls() {
        unsafe {
            let mut raw = *b"&@;&\xe9;&#12;\0tail";
            nzbget_rs_xml_remove_entities(raw.as_mut_ptr().cast(), Some(entity_alpha));
            assert_eq!(&raw[..4], b"   \0");
            assert_eq!(&raw[11..], b"\0tail");
            let mut url = *b"%00a%41\0tail";
            nzbget_rs_url_decode(url.as_mut_ptr().cast());
            assert_eq!(&url, b"\0aA\0%41\0tail");
        }
    }

    #[test]
    fn text_encoders_return_independently_owned_buffers() {
        unsafe {
            for (f, expected) in [
                (nzbget_rs_url_encode as unsafe extern "C" fn(*const c_char) -> RsBuf, &b"\xe9%20x\0"[..]),
                (nzbget_rs_latin1_to_utf8, &b"\xc3\xa9 x\0"[..]),
            ] {
                let mut raw = *b"\xe9 x\0ignored";
                let result = f(raw.as_ptr().cast());
                raw.fill(b'!');
                assert_eq!(result.len, expected.len() - 1);
                assert_eq!(std::slice::from_raw_parts(result.data.cast::<u8>(), result.len + 1), expected);
                nzbget_rs_free(result);
            }
        }
    }

    #[test]
    fn decoder_null_arguments_and_failed_value_preserve_length() {
        unsafe {
            assert_eq!(nzbget_rs_decode_base64(std::ptr::null(), 4, std::ptr::null_mut()), 0);
            nzbget_rs_json_decode(std::ptr::null_mut());
            let mut length = -7;
            for text in [std::ptr::null(), b"\0".as_ptr().cast(), b"\"x\\a\0".as_ptr().cast()] {
                assert!(nzbget_rs_json_next_value(text, &mut length).is_null());
                assert_eq!(length, -7);
            }
            assert!(nzbget_rs_json_next_value(b"123\0".as_ptr().cast(), std::ptr::null_mut()).is_null());
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

            let text = b" ,\"x\\\"y\", rest\0";
            let mut length = -7;
            assert_eq!(nzbget_rs_json_next_value(text.as_ptr().cast(), &mut length), text.as_ptr().add(2).cast());
            assert_eq!(length, 6);
        }
    }

    extern "C" fn lower(b: c_int) -> c_int {
        (b as u8).to_ascii_lowercase() as c_int
    }

    #[test]
    fn wildcard_null_callback() {
        let mut table = [0; 384];
        for (i, slot) in table.iter_mut().enumerate() {
            *slot = lower((i as c_int - 128) as u8 as c_int);
        }
        let mut positions = [[-99; 2]; 2];
        unsafe {
            let result = nzbget_rs_wild_match(
                b"A?\0".as_ptr().cast(), b"ab\0".as_ptr().cast(),
                positions.as_mut_ptr(), positions.len(), table.as_ptr().add(128), 1, None,
            );
            assert_eq!((result.matched, result.count), (1, 1));
            assert_eq!(positions, [[1, 1], [-99; 2]]);
            let result = nzbget_rs_wild_match(
                b"A?\0".as_ptr().cast(), b"ab\0".as_ptr().cast(),
                positions.as_mut_ptr(), positions.len(), std::ptr::null(), 1, None,
            );
            assert_eq!((result.matched, result.count), (0, 0));
            assert_eq!(positions, [[1, 1], [-99; 2]]);
        }
    }

    #[test]
    fn wildcard_null_inputs_and_output() {
        unsafe {
            let result = nzbget_rs_wild_match(
                std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), usize::MAX, std::ptr::null(), 1, Some(lower),
            );
            assert_eq!((result.matched, result.count), (1, 0));
            let result = nzbget_rs_wild_match(
                b"?\0".as_ptr().cast(), std::ptr::null(), std::ptr::null_mut(), 0, std::ptr::null(), 1, Some(lower),
            );
            assert_eq!((result.matched, result.count), (0, 0));
        }
    }

    #[test]
    fn wildcard_failure_preserves_partial_positions() {
        let mut positions = [[-99; 2]; 3];
        unsafe {
            let result = nzbget_rs_wild_match(
                b"?x\0".as_ptr().cast(), b"ay\0".as_ptr().cast(),
                positions.as_mut_ptr(), positions.len(), std::ptr::null(), 1, Some(lower),
            );
            assert_eq!((result.matched, result.count), (0, 1));
            assert_eq!(positions, [[0, 1], [-99; 2], [-99; 2]]);
        }
    }

    #[test]
    fn wildcard_reports_untruncated_count_without_overwriting_capacity() {
        let mut positions = [[-99; 2]; 2];
        unsafe {
            let result = nzbget_rs_wild_match(
                b"*?ab\0".as_ptr().cast(), b"aaaaaaaaab\0".as_ptr().cast(),
                positions.as_mut_ptr(), 1, std::ptr::null(), 1, Some(lower),
            );
            assert_eq!(result.matched, 1);
            assert!(result.count > 5);
            assert_eq!(positions[1], [-99; 2]);
            let mut full = vec![[0; 2]; result.count];
            let retried = nzbget_rs_wild_match(
                b"*?ab\0".as_ptr().cast(), b"aaaaaaaaab\0".as_ptr().cast(),
                full.as_mut_ptr(), full.len(), std::ptr::null(), 1, Some(lower),
            );
            assert_eq!((retried.matched, retried.count), (1, result.count));
            assert_eq!(full[0], positions[0]);
            let zero = nzbget_rs_wild_match(
                b"*?ab\0".as_ptr().cast(), b"aaaaaaaaab\0".as_ptr().cast(),
                positions.as_mut_ptr(), 0, std::ptr::null(), 1, Some(lower),
            );
            assert_eq!(zero.count, result.count);
            assert_eq!(positions[1], [-99; 2]);
        }
    }

    type Encoder = unsafe extern "C" fn(*const c_char) -> RsBuf;

    fn check(encode: Encoder, input: *const c_char, expected: &[u8]) {
        unsafe {
            let buf = encode(input);
            assert!(!buf.data.is_null());
            assert_eq!(buf.len, expected.len());
            assert!(buf.cap > buf.len);
            assert_eq!(CStr::from_ptr(buf.data).to_bytes(), expected);
            nzbget_rs_free(buf);
        }
    }

    #[test]
    fn null_and_empty_input_return_owned_empty_strings() {
        for encode in [nzbget_rs_json_encode as Encoder, nzbget_rs_xml_encode] {
            check(encode, std::ptr::null(), b"");
            check(encode, b"\0".as_ptr().cast(), b"");
        }
    }

    #[test]
    fn input_stops_at_first_nul_including_inside_utf8() {
        for encode in [nzbget_rs_json_encode as Encoder, nzbget_rs_xml_encode] {
            check(encode, b"abc\0ignored\0".as_ptr().cast(), b"abc");
            check(encode, b"a\xe2\x82\0ignored\0".as_ptr().cast(), b"a");
            check(encode, b"a\xf0\x9f\x98\0ignored\0".as_ptr().cast(), b"a");
        }
    }

    #[test]
    fn preserves_legacy_malformed_utf8() {
        check(nzbget_rs_json_encode, b"\xc0\x80\0".as_ptr().cast(), b"\\u0000");
        check(nzbget_rs_xml_encode, b"\xc0\x80\0".as_ptr().cast(), b".");
        // Only the first continuation byte is validated by the old C++.
        check(nzbget_rs_json_encode, b"\xe2\x82A\0".as_ptr().cast(), b"\\u2081");
        check(nzbget_rs_xml_encode, b"\xe2\x82A\0".as_ptr().cast(), b"&#x002081;");
        check(nzbget_rs_json_encode, b"\xf7\xbf\xbf\xbf\0".as_ptr().cast(), b"\\udfbf\\udfff");
        check(nzbget_rs_xml_encode, b"\xf7\xbf\xbf\xbf\0".as_ptr().cast(), b".");
    }

    #[test]
    fn result_owns_storage_independently_of_input() {
        for encode in [nzbget_rs_json_encode as Encoder, nzbget_rs_xml_encode] {
            let mut input = b"plain\0".to_vec();
            unsafe {
                let first = encode(input.as_ptr().cast());
                input[0] = b'P';
                let second = encode(input.as_ptr().cast());
                drop(input);
                assert_eq!(CStr::from_ptr(first.data).to_bytes(), b"plain");
                nzbget_rs_free(first);
                assert_eq!(CStr::from_ptr(second.data).to_bytes(), b"Plain");
                nzbget_rs_free(second);
            }
        }
    }

    #[test]
    fn null_buffer_can_be_freed() {
        unsafe { nzbget_rs_free(RsBuf { data: std::ptr::null_mut(), len: 0, cap: 0 }) };
    }
}

// ---- dupesearch (daemon/dupesearch/) ----

/// DonorScore::Base.
#[no_mangle]
pub extern "C" fn nzbget_rs_donorscore_base(pick_score: c_int) -> c_int {
    crate::donorscore::base(pick_score)
}

/// DonorScore::Target.
#[no_mangle]
pub extern "C" fn nzbget_rs_donorscore_target(alive: f64, twin: c_int) -> c_int {
    crate::donorscore::target(alive, twin != 0)
}

/// One DonorScore::Entry across the C ABI.
#[repr(C)]
pub struct NzbgetRsDonorEntry {
    pub score: c_int,
    pub alive: f64,
    pub twin: c_int,
    pub bytes: i64,
    pub grabs: c_int,
}

/// DonorScore::Rerank: `out` (writable for `n`) gets the scores parallel to
/// `entries` (readable for `n`; either may be NULL when `n` is 0).
///
/// # Safety
/// `entries` and `out` must be valid for `n` elements.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_donorscore_rerank(entries: *const NzbgetRsDonorEntry, n: usize,
                                                     pick_bytes: i64, out: *mut c_int) {
    if n == 0 {
        return;
    }
    let raw = std::slice::from_raw_parts(entries, n);
    let entries: Vec<crate::donorscore::Entry> = raw
        .iter()
        .map(|e| crate::donorscore::Entry { score: e.score, alive: e.alive, twin: e.twin != 0, bytes: e.bytes,
                                            grabs: e.grabs })
        .collect();
    let scores = crate::donorscore::rerank(&entries, pick_bytes);
    std::ptr::copy_nonoverlapping(scores.as_ptr(), out, n);
}

/// The bytes of a (pointer, length) argument; NULL is empty.
unsafe fn arg<'a>(data: *const c_char, len: usize) -> &'a [u8] {
    if data.is_null() || len == 0 { &[] } else { std::slice::from_raw_parts(data.cast(), len) }
}

/// ReleaseName::Clean.
///
/// # Safety
/// `name` must be readable for `len` bytes (or NULL).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_releasename_clean(name: *const c_char, len: usize) -> RsBuf {
    into_buf(crate::releasename::clean(arg(name, len)))
}

/// ReleaseName::Normalize.
///
/// # Safety
/// `name` must be readable for `len` bytes (or NULL).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_releasename_normalize(name: *const c_char, len: usize) -> RsBuf {
    into_buf(crate::releasename::normalize(arg(name, len)))
}

/// ReleaseName::ShortQuery.
///
/// # Safety
/// `title` must be readable for `len` bytes (or NULL).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_releasename_short_query(title: *const c_char, len: usize) -> RsBuf {
    into_buf(crate::releasename::short_query(arg(title, len)))
}

/// ReleaseName::Parse, encoded as releasename::encode describes.
///
/// # Safety
/// `name` must be readable for `len` bytes (or NULL).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_releasename_parse(name: *const c_char, len: usize) -> RsBuf {
    into_buf(crate::releasename::encode(&crate::releasename::parse(arg(name, len))))
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

/// Posting::GroupListings (see write_groups).
///
/// # Safety
/// `listings` readable for `n`; `indices` and `group_sizes` writable for `n`; `groups` writable.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_posting_group(raw: *const NzbgetRsListing, n: usize, indices: *mut usize,
                                                 group_sizes: *mut usize, groups: *mut usize) -> usize {
    write_groups(crate::posting::group_listings(&listings(raw, n)), indices, group_sizes, groups)
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

/// A bounded decoded first article. Result: u64 size LE, u32 extension length LE,
/// extension bytes, name bytes. Empty means unknown; release with nzbget_rs_free.
/// # Safety
/// Inputs are readable for their lengths, or NULL (empty).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_inner(data: *const c_char, len: usize, name: *const c_char,
    name_len: usize, total: u64, minimum: u64) -> RsBuf {
    let Some(found) = crate::dupeidentity::inner_entry(arg(data, len), arg(name, name_len), total, minimum) else {
        return into_buf(Vec::new());
    };
    let mut out = found.bytes.to_le_bytes().to_vec();
    out.extend_from_slice(&(found.extension.len() as u32).to_le_bytes());
    out.extend_from_slice(&found.extension);
    out.extend_from_slice(&found.name);
    into_buf(out)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_same_data(a_bytes: u64, a_files: u32, b_bytes: u64, b_files: u32) -> c_int {
    c_int::from(crate::dupeidentity::same_data(a_bytes, a_files, b_bytes, b_files))
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

/// Returns unique query strings: u32 count LE, then u32 length LE and bytes.
/// # Safety
/// Name readable for its length, or NULL (empty). Release result with nzbget_rs_free.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_queries(name: *const c_char, len: usize) -> RsBuf {
    let queries = crate::dupewatch::lenient_queries(arg(name, len));
    let mut out = (queries.len() as u32).to_le_bytes().to_vec();
    for query in queries { out.extend_from_slice(&(query.len() as u32).to_le_bytes()); out.extend_from_slice(&query); }
    into_buf(out)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_should_expand(alive: f64, missing: i32, failed: c_int, whole: c_int) -> c_int {
    c_int::from(crate::dupewatch::should_expand(alive, missing, failed != 0, whole != 0))
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
pub unsafe extern "C" fn nzbget_rs_dupe_budget_take(budget: *const crate::dupewatch::FetchBudget) -> c_int {
    if budget.is_null() { return 0; }
    c_int::from((*budget).take())
}

/// # Safety
/// Budget is live; concurrent calls are allowed, but free must not run concurrently.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_budget_spent(budget: *const crate::dupewatch::FetchBudget) -> c_int {
    if budget.is_null() { return 1; }
    c_int::from((*budget).spent())
}

#[repr(C)]
pub struct DupePart { pub number: i64, pub bytes: i64 }
/// # Safety
/// Parts readable for count; output counters writable. NULL parts/count zero are empty.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_part_counts(parts: *const DupePart, count: usize, declared: u32,
    parity: c_int, listed: *mut u64, total: *mut u64) {
    if listed.is_null() || total.is_null() { return; }
    let parts = if count == 0 || parts.is_null() { &[][..] } else { std::slice::from_raw_parts(parts, count) };
    let values: Vec<_> = parts.iter().map(|p| (p.number,p.bytes)).collect();
    let result = crate::dupewatch::part_counts(&values, declared, parity != 0);
    *listed = result.0;
    *total = result.1;
}
#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_whole_bytes(bytes: u64, listed: u64, total: u64) -> u64 {
    crate::dupewatch::whole_bytes(bytes, listed, total)
}
#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_unlisted_missing(answered: i32, listed: u64, total: u64) -> i32 {
    crate::dupewatch::unlisted_missing(answered, listed, total)
}
#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_effective_alive(present: i32, missing: i32, error: i32, listed: u64, total: u64) -> f64 {
    crate::dupewatch::effective_alive(present, missing, error, listed, total)
}
#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_is_dead(present: i32, missing: i32, error: i32, listed: u64, total: u64, minimum: i32) -> c_int {
    c_int::from(crate::dupewatch::dead_donor(present, missing, error, listed, total, minimum))
}
/// # Safety
/// The decoded body is readable for len, or NULL (empty).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_par_block(data: *const c_char, len: usize) -> u64 {
    crate::twin::block_size(arg(data,len))
}
/// # Safety
/// Recovery sizes readable for count, or count zero/NULL (empty).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_par_doomed(data_bytes: u64, article_bytes: u64, volumes: *const u64,
    count: usize, alive: f64, block: u64) -> c_int {
    let volumes = if count == 0 || volumes.is_null() { &[][..] } else { std::slice::from_raw_parts(volumes,count) };
    c_int::from(crate::dupewatch::par_doomed(data_bytes,article_bytes,volumes,alive,block))
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

#[no_mangle]
pub extern "C" fn nzbget_rs_memory_pressure_effective_downloads_limit(configured_limit: u32) -> u32 {
    let tier = crate::memory_pressure::get_last_tier();
    crate::memory_pressure::calculate_effective_downloads_limit(configured_limit, tier)
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
pub extern "C" fn nzbget_rs_memory_pressure_set_override(tier: i32) {
    crate::memory_pressure::set_override_tier(tier);
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
pub extern "C" fn nzbget_rs_lifecycle_get_phase() -> u32 {
    crate::lifecycle::LifecycleManager::global().get_phase() as u32
}

#[no_mangle]
pub extern "C" fn nzbget_rs_lifecycle_set_phase(phase: u32) {
    crate::lifecycle::LifecycleManager::global().set_phase(phase.into());
}

#[no_mangle]
pub extern "C" fn nzbget_rs_is_reloading() -> bool {
    crate::lifecycle::LifecycleManager::global().is_reloading()
}

#[no_mangle]
pub extern "C" fn nzbget_rs_is_stopping() -> bool {
    crate::lifecycle::LifecycleManager::global().is_stopping()
}

#[no_mangle]
pub extern "C" fn nzbget_rs_reload() {
    crate::lifecycle::LifecycleManager::global().trigger_reload();
}

#[no_mangle]
pub extern "C" fn nzbget_rs_shutdown() {
    crate::lifecycle::LifecycleManager::global().trigger_shutdown();
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
        nzbget_rs_lifecycle_set_phase(1);
        assert_eq!(nzbget_rs_lifecycle_get_phase(), 1);
        nzbget_rs_lifecycle_set_phase(0);
        assert_eq!(nzbget_rs_lifecycle_get_phase(), 0);

        assert!(!nzbget_rs_is_reloading());
        assert!(!nzbget_rs_is_stopping());

        nzbget_rs_signal_notify(0); // no-op for signum 0
    }
}

// ============================================================================
// DupeArticleFallback, ParDamage, DupeProbe, ServerPool, ArticleFetcher &
// ArticleDownloader Hot-Path C ABI (rust/src/dupefallback.rs)
// ============================================================================

#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_fallback_par_cannot_cover(
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
    crate::dupefallback::par_cannot_cover(
        par_size,
        par_failed_size,
        current_failed_size,
        current_success_size,
        par_current_success_size,
        total_size,
        current_success_articles,
        current_failed_articles,
        with_margin != 0,
    ) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_has_extension(filename: *const c_char) -> c_int {
    if filename.is_null() {
        return 0;
    }
    let bytes = CStr::from_ptr(filename).to_bytes();
    crate::dupefallback::has_extension(bytes) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_fallback_sizes_match(
    size1: i64,
    size2: i64,
    div: c_int,
) -> c_int {
    crate::dupefallback::sizes_match(size1, size2, div) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_declared_parts(subject: *const c_char) -> c_int {
    if subject.is_null() {
        return 0;
    }
    let bytes = CStr::from_ptr(subject).to_bytes();
    crate::dupefallback::declared_parts(bytes)
}

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
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_steps_match(
    target_sizes: *const c_int,
    target_count: usize,
    donor_sizes: *const c_int,
    donor_count: usize,
) -> c_int {
    if target_sizes.is_null() || donor_sizes.is_null() || target_count == 0 || donor_count == 0 {
        return 0;
    }
    let target = std::slice::from_raw_parts(target_sizes, target_count);
    let donor = std::slice::from_raw_parts(donor_sizes, donor_count);
    crate::dupefallback::article_size_steps_match(target, donor) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_structure_matches(
    target_subject: *const c_char,
    target_total_articles: c_int,
    target_size: i64,
    target_parts: *const c_int,
    target_sizes: *const c_int,
    target_count: usize,
    donor_subject: *const c_char,
    donor_total_articles: c_int,
    donor_size: i64,
    donor_parts: *const c_int,
    donor_sizes: *const c_int,
    donor_count: usize,
) -> c_int {
    if target_count == 0
        || target_parts.is_null()
        || target_sizes.is_null()
        || (donor_count > 0 && (donor_parts.is_null() || donor_sizes.is_null()))
    {
        return 0;
    }
    let t_subj = if target_subject.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(target_subject).to_bytes()
    };
    let d_subj = if donor_subject.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(donor_subject).to_bytes()
    };
    let t_parts = std::slice::from_raw_parts(target_parts, target_count);
    let t_sizes = std::slice::from_raw_parts(target_sizes, target_count);
    let d_parts = if donor_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(donor_parts, donor_count)
    };
    let d_sizes = if donor_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(donor_sizes, donor_count)
    };
    crate::dupefallback::structure_matches(
        t_subj,
        target_total_articles,
        target_size,
        t_parts,
        t_sizes,
        d_subj,
        donor_total_articles,
        donor_size,
        d_parts,
        d_sizes,
    ) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_fallback_expected_offset(
    index: isize,
    part_number: c_int,
    prev_part_number: c_int,
    prev_status: c_int,
    prev_offset: i64,
    prev_size: c_int,
) -> i64 {
    crate::dupefallback::expected_segment_offset(
        index,
        part_number,
        prev_part_number,
        prev_status,
        prev_offset,
        prev_size,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_fallback_expected_end(
    index: isize,
    count: usize,
    part_number: c_int,
    decoded_file_size: i64,
    next_part_number: c_int,
    next_status: c_int,
    next_offset: i64,
    next_size: c_int,
) -> i64 {
    crate::dupefallback::expected_segment_end(
        index,
        count,
        part_number,
        decoded_file_size,
        next_part_number,
        next_status,
        next_offset,
        next_size,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_fallback_segment_aligned(
    segment_offset: i64,
    segment_size: c_int,
    expected_offset: i64,
    expected_end: i64,
) -> c_int {
    crate::dupefallback::segment_aligned(segment_offset, segment_size, expected_offset, expected_end)
        as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_first_untiled(
    articles: *const crate::dupefallback::NzbgetRsArticleGeom,
    count: usize,
    decoded_size: i64,
) -> isize {
    if decoded_size <= 0 {
        return -1;
    }
    let slice = if count == 0 || articles.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(articles, count)
    };
    crate::dupefallback::first_untiled_article(slice, decoded_size)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_merge_decoded_size(
    decoded_file_size: *mut i64,
    article_file_size: i64,
) -> c_int {
    if decoded_file_size.is_null() {
        return 0;
    }
    let (conflict, new_val) =
        crate::dupefallback::merge_decoded_file_size(*decoded_file_size, article_file_size);
    *decoded_file_size = new_val;
    conflict as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_register_lead_failure(
    fallback_round: c_int,
    lead_snapshot: c_int,
    donor_count: c_int,
    next_lead: c_int,
    lead_donor_id: *mut c_int,
    lead_failures: *mut c_int,
    lead_switches: *mut c_int,
) -> c_int {
    if lead_donor_id.is_null() || lead_failures.is_null() || lead_switches.is_null() {
        return 0;
    }
    crate::dupefallback::register_lead_failure(
        fallback_round,
        lead_snapshot,
        donor_count,
        next_lead,
        &mut *lead_donor_id,
        &mut *lead_failures,
        &mut *lead_switches,
    ) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_fallback_register_lead_success(
    fallback_round: c_int,
    lead_snapshot: c_int,
    lead_donor_id: c_int,
    expected_offset: i64,
    expected_end: i64,
    lead_failures: *mut c_int,
    lead_switches: *mut c_int,
) {
    if lead_failures.is_null() || lead_switches.is_null() {
        return;
    }
    crate::dupefallback::register_lead_success(
        fallback_round,
        lead_snapshot,
        lead_donor_id,
        expected_offset,
        expected_end,
        &mut *lead_failures,
        &mut *lead_switches,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par2_range_covered(
    ranges: *const crate::dupefallback::NzbgetRsByteRange,
    count: usize,
    from: i64,
    to: i64,
) -> c_int {
    let slice = if count == 0 || ranges.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(ranges, count)
    };
    crate::dupefallback::range_covered(slice, from, to) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par2_scan_file(
    path: *const c_char,
    ctx: *mut std::ffi::c_void,
    on_main: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, u64)>,
    on_desc: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const c_char,
            *const c_char,
            *const c_char,
            u64,
            *const c_char,
            usize,
        ),
    >,
    on_ifsc: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const c_char,
            *const c_char,
            *const u32,
            usize,
        ),
    >,
) {
    if path.is_null() {
        return;
    }
    #[cfg(unix)]
    let path_ref = {
        use std::os::unix::ffi::OsStrExt;
        std::path::Path::new(std::ffi::OsStr::from_bytes(CStr::from_ptr(path).to_bytes()))
    };
    #[cfg(not(unix))]
    let path_buf = std::path::PathBuf::from(CStr::from_ptr(path).to_string_lossy().into_owned());
    #[cfg(not(unix))]
    let path_ref = path_buf.as_path();

    crate::dupefallback::scan_par2_file(
        path_ref,
        |set_id, block_size| {
            if let Some(cb) = on_main {
                let mut set_c = [0u8; 33];
                set_c[..32].copy_from_slice(set_id);
                cb(ctx, set_c.as_ptr().cast(), block_size);
            }
        },
        |set_id, file_id, hash16k, length, name| {
            if let Some(cb) = on_desc {
                let mut set_c = [0u8; 33];
                let mut file_c = [0u8; 33];
                let mut hash_c = [0u8; 33];
                set_c[..32].copy_from_slice(set_id);
                file_c[..32].copy_from_slice(file_id);
                hash_c[..32].copy_from_slice(hash16k);
                cb(
                    ctx,
                    set_c.as_ptr().cast(),
                    file_c.as_ptr().cast(),
                    hash_c.as_ptr().cast(),
                    length,
                    name.as_ptr().cast(),
                    name.len(),
                );
            }
        },
        |set_id, file_id, crcs| {
            if let Some(cb) = on_ifsc {
                let mut set_c = [0u8; 33];
                let mut file_c = [0u8; 33];
                set_c[..32].copy_from_slice(set_id);
                file_c[..32].copy_from_slice(file_id);
                cb(
                    ctx,
                    set_c.as_ptr().cast(),
                    file_c.as_ptr().cast(),
                    crcs.as_ptr(),
                    crcs.len(),
                );
            }
        },
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_damage_blocks_touched(
    holes: *const crate::dupefallback::NzbgetRsByteRange,
    count: usize,
    block_size: u64,
) -> c_int {
    let slice = if count == 0 || holes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(holes, count)
    };
    crate::dupefallback::blocks_touched(slice, block_size)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_par_damage_project(
    known: c_int,
    damaged_blocks: c_int,
    recovery_blocks: c_int,
    done_bytes: i64,
    total_bytes: i64,
    out_projected_blocks: *mut c_int,
    out_certain: *mut c_int,
    out_projected: *mut c_int,
) {
    let (proj, certain, projected) = crate::dupefallback::project_par_damage(
        known != 0,
        damaged_blocks,
        recovery_blocks,
        done_bytes,
        total_bytes,
    );
    if !out_projected_blocks.is_null() {
        *out_projected_blocks = proj;
    }
    if !out_certain.is_null() {
        *out_certain = certain as c_int;
    }
    if !out_projected.is_null() {
        *out_projected = projected as c_int;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_probe_sample_indexes(
    total: usize,
    count: c_int,
    out_indexes: *mut usize,
    cap: usize,
) -> usize {
    let indexes = crate::dupefallback::dupe_probe_sample_indexes(total, count);
    if !out_indexes.is_null() && cap > 0 {
        let write_len = indexes.len().min(cap);
        std::ptr::copy_nonoverlapping(indexes.as_ptr(), out_indexes, write_len);
    }
    indexes.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupe_probe_classify(response: *const c_char) -> c_int {
    let opt = if response.is_null() {
        None
    } else {
        Some(CStr::from_ptr(response).to_bytes())
    };
    crate::dupefallback::dupe_probe_classify(opt)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupe_probe_is_dead(
    existing: c_int,
    definitive_servers: c_int,
    active_servers: c_int,
) -> c_int {
    crate::dupefallback::dupe_probe_is_dead(existing, definitive_servers, active_servers) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_server_pool_normalize_levels(
    servers: *const crate::dupefallback::NzbgetRsServerNormInput,
    count: usize,
    out_sorted_indices: *mut usize,
    out_norm_levels: *mut c_int,
) -> c_int {
    if count == 0 || servers.is_null() || out_sorted_indices.is_null() || out_norm_levels.is_null() {
        return 0;
    }
    let srv_slice = std::slice::from_raw_parts(servers, count);
    let sorted_slice = std::slice::from_raw_parts_mut(out_sorted_indices, count);
    let norm_slice = std::slice::from_raw_parts_mut(out_norm_levels, count);
    crate::dupefallback::server_pool_normalize_levels(srv_slice, sorted_slice, norm_slice)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_server_pool_is_blocked(
    block_time: i64,
    cur_time: i64,
    retry_interval: c_int,
) -> c_int {
    crate::dupefallback::server_pool_is_blocked(block_time, cur_time, retry_interval) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_server_pool_level_should_close(
    in_use: *const u8,
    connected: *const u8,
    free_times: *const i64,
    count: usize,
    cur_time: i64,
    hold_seconds: c_int,
) -> c_int {
    if count == 0 || in_use.is_null() || connected.is_null() || free_times.is_null() {
        return 0;
    }
    let u = std::slice::from_raw_parts(in_use, count);
    let c = std::slice::from_raw_parts(connected, count);
    let f = std::slice::from_raw_parts(free_times, count);
    crate::dupefallback::server_pool_level_should_close(u, c, f, cur_time, hold_seconds) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_article_fetch_add_raw_bytes(
    raw_bytes: *mut i64,
    bytes: i64,
) -> c_int {
    if raw_bytes.is_null() {
        return 0;
    }
    crate::dupefallback::article_fetch_add_raw_bytes(&mut *raw_bytes, bytes) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_article_fetch_header_within_limit(
    raw_bytes: i64,
    declared_range_known: c_int,
) -> c_int {
    crate::dupefallback::article_fetch_header_within_limit(raw_bytes, declared_range_known != 0)
        as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_article_fetch_add_decoded_bytes(
    decoded_bytes: *mut i64,
    bytes: i64,
    begin: i64,
    end: i64,
    file_size: i64,
) -> c_int {
    if decoded_bytes.is_null() {
        return 0;
    }
    crate::dupefallback::article_fetch_add_decoded_bytes(
        &mut *decoded_bytes,
        bytes,
        begin,
        end,
        file_size,
    ) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_article_downloader_check_response(
    response: *const c_char,
    auth_error: c_int,
) -> c_int {
    let opt = if response.is_null() {
        None
    } else {
        Some(CStr::from_ptr(response).to_bytes())
    };
    crate::dupefallback::article_downloader_check_response(opt, auth_error != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_article_downloader_validate_yenc(
    begin_pos: i64,
    end_pos: i64,
    article_file_size: i64,
    expected_file_size: i64,
    fallback_round: c_int,
    dupe_expected_offset: i64,
    dupe_expected_end: i64,
    out_offset: *mut i64,
    out_size: *mut c_int,
) -> c_int {
    match crate::dupefallback::article_downloader_validate_yenc(
        begin_pos,
        end_pos,
        article_file_size,
        expected_file_size,
        fallback_round,
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
        Err(code) => code,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_is_eligible(filename: *const c_char) -> c_int {
    if filename.is_null() {
        return 0;
    }
    crate::streamrepair::is_stream_eligible(CStr::from_ptr(filename).to_bytes()) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_compute_holes(
    covered: *const crate::dupefallback::NzbgetRsByteRange,
    count: usize,
    file_size: i64,
    out_holes: *mut crate::dupefallback::NzbgetRsByteRange,
    cap: usize,
) -> usize {
    let slice = if count == 0 || covered.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(covered, count)
    };
    let holes = crate::streamrepair::compute_holes(slice, file_size);
    if !out_holes.is_null() && cap > 0 {
        let n = holes.len().min(cap);
        std::ptr::copy_nonoverlapping(holes.as_ptr(), out_holes, n);
    }
    holes.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_total_size(
    ranges: *const crate::dupefallback::NzbgetRsByteRange,
    count: usize,
) -> i64 {
    let slice = if count == 0 || ranges.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(ranges, count)
    };
    crate::streamrepair::total_size(slice)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_intersects_any(
    range: crate::dupefallback::NzbgetRsByteRange,
    ranges: *const crate::dupefallback::NzbgetRsByteRange,
    count: usize,
) -> c_int {
    let slice = if count == 0 || ranges.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(ranges, count)
    };
    crate::streamrepair::intersects_any(range, slice) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_estimate_donor_ranges(
    article_sizes: *const c_int,
    count: usize,
    target_file_size: i64,
    out_ranges: *mut crate::dupefallback::NzbgetRsByteRange,
) -> usize {
    if count == 0 || article_sizes.is_null() {
        return 0;
    }
    let sizes = std::slice::from_raw_parts(article_sizes, count);
    let ranges = crate::streamrepair::estimate_donor_ranges(sizes, target_file_size);
    if !out_ranges.is_null() && !ranges.is_empty() {
        std::ptr::copy_nonoverlapping(ranges.as_ptr(), out_ranges, ranges.len());
    }
    ranges.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_select_probe_parts(
    donor_ranges: *const crate::dupefallback::NzbgetRsByteRange,
    donor_count: usize,
    holes: *const crate::dupefallback::NzbgetRsByteRange,
    hole_count: usize,
    count: c_int,
    out_parts: *mut c_int,
    cap: usize,
) -> usize {
    let donors = if donor_count == 0 || donor_ranges.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(donor_ranges, donor_count)
    };
    let hole_slice = if hole_count == 0 || holes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(holes, hole_count)
    };
    let parts = crate::streamrepair::select_probe_parts(donors, hole_slice, count);
    if !out_parts.is_null() && cap > 0 {
        let n = parts.len().min(cap);
        std::ptr::copy_nonoverlapping(parts.as_ptr(), out_parts, n);
    }
    parts.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_subtract_covered(
    holes: *const crate::dupefallback::NzbgetRsByteRange,
    hole_count: usize,
    covered: crate::dupefallback::NzbgetRsByteRange,
    out_holes: *mut crate::dupefallback::NzbgetRsByteRange,
    cap: usize,
) -> usize {
    let hole_slice = if hole_count == 0 || holes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(holes, hole_count)
    };
    let rem = crate::streamrepair::subtract_covered(hole_slice, covered);
    if !out_holes.is_null() && cap > 0 {
        let n = rem.len().min(cap);
        std::ptr::copy_nonoverlapping(rem.as_ptr(), out_holes, n);
    }
    rem.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_volume_set_key(filename: *const c_char) -> RsBuf {
    let bytes = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    into_buf(crate::streamrepair::volume_set_key(bytes))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stream_decoded_size_plausible(
    decoded_size: i64,
    encoded_size: i64,
) -> c_int {
    crate::streamrepair::decoded_size_plausible(decoded_size, encoded_size) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_suffix_key(filename: *const c_char) -> RsBuf {
    let bytes = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    into_buf(crate::streamrepair::suffix_key(bytes))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_volume_key(filename: *const c_char) -> RsBuf {
    let bytes = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    into_buf(crate::streamrepair::volume_key(bytes))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stream_exceeds_decompress_cap(
    total_bytes: i64,
    next_chunk: i64,
    total_extent: i64,
    member_declared_size: i64,
) -> c_int {
    crate::streamrepair::exceeds_decompress_cap(
        total_bytes,
        next_chunk,
        total_extent,
        member_declared_size,
    ) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_composite_to_members(
    member_indexes: *const i32,
    disk_bases: *const i64,
    member_sizes: *const i64,
    member_count: usize,
    range: crate::dupefallback::NzbgetRsByteRange,
    out_pieces: *mut crate::streamrepair::NzbgetRsMemberRange,
    cap: usize,
) -> usize {
    if member_count == 0
        || member_indexes.is_null()
        || disk_bases.is_null()
        || member_sizes.is_null()
    {
        return 0;
    }
    let idx = std::slice::from_raw_parts(member_indexes, member_count);
    let bases = std::slice::from_raw_parts(disk_bases, member_count);
    let sizes = std::slice::from_raw_parts(member_sizes, member_count);
    let pieces = crate::streamrepair::composite_to_members(idx, bases, sizes, range);
    if !out_pieces.is_null() && cap > 0 {
        let n = pieces.len().min(cap);
        std::ptr::copy_nonoverlapping(pieces.as_ptr(), out_pieces, n);
    }
    pieces.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_holed_source_intersects(
    holes: *const crate::dupefallback::NzbgetRsByteRange,
    hole_count: usize,
    offset: i64,
    size: i64,
) -> c_int {
    let hole_slice = if hole_count == 0 || holes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(holes, hole_count)
    };
    crate::streamrepair::holed_source_intersects(hole_slice, offset, size) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_content_map_to_inner(
    runs: *const crate::streamrepair::NzbgetRsContentRun,
    run_count: usize,
    member_index: i32,
    member_range: crate::dupefallback::NzbgetRsByteRange,
    out_ranges: *mut crate::dupefallback::NzbgetRsByteRange,
    cap: usize,
) -> usize {
    let run_slice = if run_count == 0 || runs.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(runs, run_count)
    };
    let res = crate::streamrepair::content_map_to_inner(run_slice, member_index, member_range);
    if !out_ranges.is_null() && cap > 0 {
        let n = res.len().min(cap);
        std::ptr::copy_nonoverlapping(res.as_ptr(), out_ranges, n);
    }
    res.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_content_map_from_inner(
    runs: *const crate::streamrepair::NzbgetRsContentRun,
    run_count: usize,
    inner_range: crate::dupefallback::NzbgetRsByteRange,
    out_pieces: *mut crate::streamrepair::NzbgetRsMemberRange,
    cap: usize,
) -> usize {
    let run_slice = if run_count == 0 || runs.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(runs, run_count)
    };
    let res = crate::streamrepair::content_map_from_inner(run_slice, inner_range);
    if !out_pieces.is_null() && cap > 0 {
        let n = res.len().min(cap);
        std::ptr::copy_nonoverlapping(res.as_ptr(), out_pieces, n);
    }
    res.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_content_map_cipher_range(
    runs: *const crate::streamrepair::NzbgetRsContentRun,
    cipher_data_offsets: *const i64,
    cipher_sizes: *const i64,
    has_crypto: *const u8,
    run_count: usize,
    cipher_range: crate::dupefallback::NzbgetRsByteRange,
    out_pieces: *mut crate::streamrepair::NzbgetRsMemberRange,
    cap: usize,
) -> usize {
    if run_count == 0
        || runs.is_null()
        || cipher_data_offsets.is_null()
        || cipher_sizes.is_null()
        || has_crypto.is_null()
    {
        return 0;
    }
    let run_slice = std::slice::from_raw_parts(runs, run_count);
    let offsets_slice = std::slice::from_raw_parts(cipher_data_offsets, run_count);
    let sizes_slice = std::slice::from_raw_parts(cipher_sizes, run_count);
    let crypto_slice = std::slice::from_raw_parts(has_crypto, run_count);
    let res = crate::streamrepair::content_map_cipher_range(
        run_slice,
        offsets_slice,
        sizes_slice,
        crypto_slice,
        cipher_range,
    );
    if !out_pieces.is_null() && cap > 0 {
        let n = res.len().min(cap);
        std::ptr::copy_nonoverlapping(res.as_ptr(), out_pieces, n);
    }
    res.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_content_mapper_group_sets(
    member_names: *const *const c_char,
    count: usize,
    ctx: *mut std::ffi::c_void,
    on_set: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const i32, usize)>,
) {
    let Some(cb) = on_set else {
        return;
    };
    let mut slices: Vec<&[u8]> = Vec::with_capacity(count);
    if !member_names.is_null() {
        let ptrs = std::slice::from_raw_parts(member_names, count);
        for &p in ptrs {
            slices.push(if p.is_null() {
                b""
            } else {
                CStr::from_ptr(p).to_bytes()
            });
        }
    }
    let sets = crate::streamrepair::content_mapper_group_sets(&slices);
    for (fmt, members) in sets {
        cb(ctx, fmt, members.as_ptr(), members.len());
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_content_mapper_coalesce_ranges(
    ranges: *const crate::dupefallback::NzbgetRsByteRange,
    count: usize,
    out_ranges: *mut crate::dupefallback::NzbgetRsByteRange,
    cap: usize,
) -> usize {
    let slice = if count == 0 || ranges.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(ranges, count)
    };
    let merged = crate::streamrepair::content_mapper_coalesce_ranges(slice);
    if !out_ranges.is_null() && cap > 0 {
        let n = merged.len().min(cap);
        std::ptr::copy_nonoverlapping(merged.as_ptr(), out_ranges, n);
    }
    merged.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_content_mapper_build_zip_map(
    member_indexes: *const i32,
    member_sizes: *const i64,
    count: usize,
    ctx: *mut std::ffi::c_void,
    read_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, i64, *mut u8, i64) -> c_int>,
    on_success: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const c_char,
            i64,
            *const crate::streamrepair::NzbgetRsContentRun,
            usize,
        ),
    >,
    on_error: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) -> c_int {
    let Some(read_fn) = read_cb else {
        return 0;
    };
    let idx = if count == 0 || member_indexes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(member_indexes, count)
    };
    let sizes = if count == 0 || member_sizes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(member_sizes, count)
    };
    match crate::streamrepair::content_mapper_build_zip_map(idx, sizes, |m_idx, offset, buf| {
        read_fn(ctx, m_idx, offset, buf.as_mut_ptr(), buf.len() as i64) != 0
    }) {
        Ok((mut inner_name, inner_size, runs)) => {
            inner_name.push(0);
            if let Some(cb) = on_success {
                cb(
                    ctx,
                    inner_name.as_ptr().cast(),
                    inner_size,
                    runs.as_ptr(),
                    runs.len(),
                );
            }
            1
        }
        Err(reason) => {
            if let Some(cb) = on_error {
                let mut reason_c = reason.as_bytes().to_vec();
                reason_c.push(0);
                cb(ctx, reason_c.as_ptr().cast());
            }
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_content_mapper_build_seven_zip_map(
    member_indexes: *const i32,
    member_sizes: *const i64,
    count: usize,
    ctx: *mut std::ffi::c_void,
    read_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, i64, *mut u8, i64) -> c_int>,
    on_success: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const c_char,
            i64,
            *const crate::streamrepair::NzbgetRsContentRun,
            usize,
        ),
    >,
    on_error: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) -> c_int {
    let Some(read_fn) = read_cb else {
        return 0;
    };
    let idx = if count == 0 || member_indexes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(member_indexes, count)
    };
    let sizes = if count == 0 || member_sizes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(member_sizes, count)
    };
    match crate::streamrepair::content_mapper_build_seven_zip_map(
        idx,
        sizes,
        |m_idx, offset, buf| read_fn(ctx, m_idx, offset, buf.as_mut_ptr(), buf.len() as i64) != 0,
    ) {
        Ok((mut inner_name, inner_size, runs)) => {
            inner_name.push(0);
            if let Some(cb) = on_success {
                cb(
                    ctx,
                    inner_name.as_ptr().cast(),
                    inner_size,
                    runs.as_ptr(),
                    runs.len(),
                );
            }
            1
        }
        Err(reason) => {
            if let Some(cb) = on_error {
                let mut reason_c = reason.as_bytes().to_vec();
                reason_c.push(0);
                cb(ctx, reason_c.as_ptr().cast());
            }
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_build_probe_windows(
    inner_size: i64,
    holes: *const crate::dupefallback::NzbgetRsByteRange,
    hole_count: usize,
    original_holes: *const crate::dupefallback::NzbgetRsByteRange,
    orig_count: usize,
    probe_count: c_int,
    min_probe_compare_bytes: i64,
    out_windows: *mut crate::dupefallback::NzbgetRsByteRange,
    cap: usize,
) -> usize {
    let h = if hole_count == 0 || holes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(holes, hole_count)
    };
    let orig = if orig_count == 0 || original_holes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(original_holes, orig_count)
    };
    let windows = crate::streamrepair::build_probe_windows(
        inner_size,
        h,
        orig,
        probe_count,
        min_probe_compare_bytes,
    );
    if !out_windows.is_null() && cap > 0 {
        let n = windows.len().min(cap);
        std::ptr::copy_nonoverlapping(windows.as_ptr(), out_windows, n);
    }
    windows.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_plain_compare_floor(
    runs: *const crate::streamrepair::NzbgetRsContentRun,
    run_count: usize,
    original_inner_holes: *const crate::dupefallback::NzbgetRsByteRange,
    orig_count: usize,
    windows: *const crate::dupefallback::NzbgetRsByteRange,
    window_count: usize,
) -> i64 {
    let r = if run_count == 0 || runs.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(runs, run_count)
    };
    let orig = if orig_count == 0 || original_inner_holes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(original_inner_holes, orig_count)
    };
    let w = if window_count == 0 || windows.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(windows, window_count)
    };
    crate::streamrepair::plain_compare_floor(r, orig, w)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_encrypted_chain_start(
    window_offset: i64,
    window_end: i64,
    block_size: i64,
    original_holes: *const crate::dupefallback::NzbgetRsByteRange,
    orig_count: usize,
) -> i64 {
    let orig = if orig_count == 0 || original_holes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(original_holes, orig_count)
    };
    crate::streamrepair::encrypted_chain_start(window_offset, window_end, block_size, orig)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_buffer_has_varied_data(
    data: *const u8,
    len: usize,
) -> c_int {
    if len < 2 || data.is_null() {
        return 0;
    }
    crate::streamrepair::buffer_has_varied_data(std::slice::from_raw_parts(data, len)) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stream_compute_positional_rank(
    member_names: *const *const c_char,
    member_sizes: *const i64,
    member_count: usize,
    target_filename: *const c_char,
    target_decoded_size: i64,
    out_rank: *mut c_int,
    out_window: *mut c_int,
) {
    let mut names: Vec<&[u8]> = Vec::with_capacity(member_count);
    if !member_names.is_null() {
        let ptrs = std::slice::from_raw_parts(member_names, member_count);
        for &p in ptrs {
            names.push(if p.is_null() {
                b""
            } else {
                CStr::from_ptr(p).to_bytes()
            });
        }
    }
    let sizes = if member_count == 0 || member_sizes.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(member_sizes, member_count)
    };
    let target = if target_filename.is_null() {
        b""
    } else {
        CStr::from_ptr(target_filename).to_bytes()
    };
    let (rank, window) =
        crate::streamrepair::compute_positional_rank(&names, sizes, target, target_decoded_size);
    if !out_rank.is_null() {
        *out_rank = rank;
    }
    if !out_window.is_null() {
        *out_window = window;
    }
}

// ============================================================================
// Post-Repair & DupeCoordinator Hot-Path FFI (`rust/src/postrepair.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupecoord_same_name_or_key(
    name1: *const c_char,
    dupe_key1: *const c_char,
    name2: *const c_char,
    dupe_key2: *const c_char,
) -> c_int {
    let n1 = if name1.is_null() { b"" } else { CStr::from_ptr(name1).to_bytes() };
    let k1 = if dupe_key1.is_null() { b"" } else { CStr::from_ptr(dupe_key1).to_bytes() };
    let n2 = if name2.is_null() { b"" } else { CStr::from_ptr(name2).to_bytes() };
    let k2 = if dupe_key2.is_null() { b"" } else { CStr::from_ptr(dupe_key2).to_bytes() };
    crate::postrepair::same_name_or_key(n1, k1, n2, k2) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupecoord_failover_warranted(
    item_score: c_int,
    health: c_int,
    backup_score: c_int,
) -> c_int {
    crate::postrepair::failover_warranted(item_score, health, backup_score) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupecoord_parse_alive_permille(
    value: *const c_char,
) -> c_int {
    crate::postrepair::parse_alive_permille_cstr(value)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupecoord_fails_over(
    dupe_check: c_int,
    dupe_mode: c_int,
    health_check: c_int,
) -> c_int {
    crate::postrepair::fails_over(dupe_check != 0, dupe_mode, health_check) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupecoord_classify_no_backup_item(
    delete_status: c_int,
    dupe_mode: c_int,
    file_exists: c_int,
    health: c_int,
    critical_health: c_int,
    mark_status: c_int,
    alive: c_int,
) -> c_int {
    crate::postrepair::classify_no_backup_item(
        delete_status,
        dupe_mode,
        file_exists != 0,
        health,
        critical_health,
        mark_status,
        alive,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupecoord_format_no_backup_reason(
    total: c_int,
    not_backup: c_int,
    forced: c_int,
    no_file: c_int,
    unhealthy: c_int,
    bad: c_int,
    dead: c_int,
) -> RsBuf {
    into_buf(crate::postrepair::format_no_backup_reason(
        total, not_backup, forced, no_file, unhealthy, bad, dead,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupecoord_status_from_text(
    text_status: *const c_char,
) -> c_int {
    let s = if text_status.is_null() {
        b""
    } else {
        CStr::from_ptr(text_status).to_bytes()
    };
    crate::postrepair::status_from_text(s)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupecoord_select_dupe_backup(
    has_self_nzb: c_int,
    self_full_content_hash: u32,
    dupe_health: c_int,
    history: *const crate::postrepair::NzbgetRsDupeHistoryCandidate,
    history_count: usize,
    queue: *const crate::postrepair::NzbgetRsDupeQueueCandidate,
    queue_count: usize,
    ctx: *mut std::ffi::c_void,
    file_exists_cb: Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void, history_index: usize) -> c_int>,
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
    crate::postrepair::select_dupe_backup(
        has_self_nzb != 0,
        self_full_content_hash,
        dupe_health != 0,
        h,
        q,
        |idx| {
            if let Some(cb) = file_exists_cb {
                cb(ctx, idx) != 0
            } else {
                false
            }
        },
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dupematcher_size_diff_ok(
    size1: i64,
    size2: i64,
    max_diff_percent: c_int,
) -> c_int {
    crate::postrepair::size_diff_ok(size1, size2, max_diff_percent) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupematcher_parse_rar_lister_line(
    text: *const c_char,
    expected_size: i64,
    max_size: *mut i64,
    compressed: *mut c_int,
    last_size_max: *mut c_int,
) -> c_int {
    crate::postrepair::parse_rar_lister_line_cstr(
        text,
        expected_size,
        max_size,
        compressed,
        last_size_max,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_streamcrypto_rar3_swizzle_key(
    digest20: *const u8,
    key16_out: *mut u8,
) {
    if digest20.is_null() || key16_out.is_null() {
        return;
    }
    let d: &[u8; 20] = &*(digest20 as *const [u8; 20]);
    let k: &mut [u8; 16] = &mut *(key16_out as *mut [u8; 16]);
    crate::postrepair::rar3_swizzle_key(d, k);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_streamcrypto_rar5_fold_psw_check(
    value32: *const u8,
    check8_out: *mut u8,
) {
    if value32.is_null() || check8_out.is_null() {
        return;
    }
    let v: &[u8; 32] = &*(value32 as *const [u8; 32]);
    let c: &mut [u8; 8] = &mut *(check8_out as *mut [u8; 8]);
    crate::postrepair::rar5_fold_psw_check(v, c);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_repair_select_more_pars(
    sorted_block_counts: *const c_int,
    count: usize,
    block_needed: c_int,
    out_indices: *mut usize,
    cap: usize,
    out_remaining_needed: *mut c_int,
) -> usize {
    let counts = if count == 0 || sorted_block_counts.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(sorted_block_counts, count)
    };
    let (indices, rem) = crate::postrepair::select_more_pars(counts, block_needed);
    if !out_remaining_needed.is_null() {
        *out_remaining_needed = rem;
    }
    if !out_indices.is_null() && cap > 0 {
        let n = indices.len().min(cap);
        std::ptr::copy_nonoverlapping(indices.as_ptr(), out_indices, n);
    }
    indices.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_repair_pick_twin_volume(
    sorted_volume_blocks: *const c_int,
    count: usize,
    left: c_int,
) -> isize {
    let vols = if count == 0 || sorted_volume_blocks.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(sorted_volume_blocks, count)
    };
    crate::postrepair::pick_twin_volume(vols, left)
        .map(|i| i as isize)
        .unwrap_or(-1)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_repair_check_par_time_limit(
    par_time_limit_min: c_int,
    is_repairing_stage: c_int,
    elapsed_sec: i64,
    stage_progress: c_int,
    out_estimated_sec: *mut c_int,
) -> c_int {
    if let Some(est) = crate::postrepair::check_par_time_limit(
        par_time_limit_min,
        is_repairing_stage != 0,
        elapsed_sec,
        stage_progress,
    ) {
        if !out_estimated_sec.is_null() {
            *out_estimated_sec = est;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_parchecker_maybe_splitted_fragment(
    filename1: *const c_char,
    filename2: *const c_char,
) -> c_int {
    if filename1.is_null() || filename2.is_null() {
        return 0;
    }
    let f1 = CStr::from_ptr(filename1).to_bytes();
    let f2 = CStr::from_ptr(filename2).to_bytes();
    crate::postrepair::maybe_splitted_fragment(f1, f2) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_parchecker_calc_stage_progress(
    stage: c_int,
    file_progress: c_int,
    processed_count: c_int,
    files_to_repair: c_int,
    source_files_count: c_int,
    extra_files: c_int,
    quick_files: c_int,
) -> c_int {
    crate::postrepair::calc_stage_progress(
        stage,
        file_progress,
        processed_count,
        files_to_repair,
        source_files_count,
        extra_files,
        quick_files,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_parchecker_compute_valid_blocks(
    block_count: usize,
    block_size: i64,
    file_size: i64,
    segments: *const crate::postrepair::NzbgetRsParSegment,
    segment_count: usize,
    out_valid: *mut u8,
) {
    if block_count == 0 || out_valid.is_null() {
        return;
    }
    let segs = if segment_count == 0 || segments.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(segments, segment_count)
    };
    let valid_slice = std::slice::from_raw_parts_mut(out_valid, block_count);
    crate::postrepair::compute_valid_blocks(block_count, block_size, file_size, segs, valid_slice);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_parchecker_smart_calc_range_crc(
    start: i64,
    end: i64,
    segments: *const crate::postrepair::NzbgetRsParSegment,
    segment_count: usize,
    ctx: *mut std::ffi::c_void,
    dumb_calc_cb: Option<
        unsafe extern "C" fn(ctx: *mut std::ffi::c_void, start: i64, end: i64, out_crc: *mut u32) -> c_int,
    >,
    out_crc: *mut u32,
) -> c_int {
    let segs = if segment_count == 0 || segments.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(segments, segment_count)
    };
    let res = crate::postrepair::smart_calc_range_crc(start, end, segs, |s, e| {
        let cb = dumb_calc_cb?;
        let mut c = 0u32;
        if cb(ctx, s, e, &mut c) != 0 {
            Some(c)
        } else {
            None
        }
    });
    if let Some(crc) = res {
        if !out_crc.is_null() {
            *out_crc = crc;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_parchecker_classify_streambuf_line(
    msg: *const c_char,
    out_offset: *mut usize,
) -> c_int {
    if msg.is_null() {
        if !out_offset.is_null() {
            *out_offset = 0;
        }
        return 1;
    }
    let bytes = CStr::from_ptr(msg).to_bytes();
    let (extra_debug, offset) = crate::postrepair::classify_streambuf_line(bytes);
    if !out_offset.is_null() {
        *out_offset = offset;
    }
    extra_debug as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_detect_rar_signature(
    sig_bytes: *const u8,
    sig_len: usize,
) -> c_int {
    if sig_bytes.is_null() || sig_len != 8 {
        return 0;
    }
    crate::postrepair::detect_rar_signature(std::slice::from_raw_parts(sig_bytes, sig_len))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_unpack_check_joinable_fragments(
    count: c_int,
    min_seg: c_int,
    max_seg: c_int,
    dif_size_min: c_int,
    par_success: c_int,
) -> c_int {
    crate::postrepair::check_joinable_fragments(
        count,
        min_seg,
        max_seg,
        dif_size_min,
        par_success != 0,
    ) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_parse_message(
    text: *const c_char,
    unpack_extended_dir: *const c_char,
    is_seven_zip: c_int,
    out_info: *mut crate::postrepair::NzbgetRsUnpackMessageInfo,
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
    *out_info = crate::postrepair::parse_unpack_message(t, d, is_seven_zip != 0);
}

// ============================================================================
// QueueCoordinator, ArticleWriter, HistoryCoordinator, QueueEditor &
// DownloadInfo Hot-Path FFI (`rust/src/queuehotpath.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_script_calc_total_status(
    statuses: *const i32,
    len: usize,
) -> c_int {
    let slice = if len == 0 || statuses.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(statuses, len)
    };
    crate::queuehotpath::script_calc_total_status(slice)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_dir_name_of(
    name: *const c_char,
    reserve: usize,
) -> RsBuf {
    let bytes = crate::queuehotpath::cstr_to_bytes(name);
    let mut out = crate::queuehotpath::dir_name_of(bytes, reserve);
    out.push(0);
    into_buf(out)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_calc_health(
    size: i64,
    par_size: i64,
    current_failed_size: i64,
    par_current_failed_size: i64,
) -> c_int {
    crate::queuehotpath::calc_health(
        size,
        par_size,
        current_failed_size,
        par_current_failed_size,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_calc_critical_health(
    size: i64,
    par_size: i64,
    par_current_failed_size: i64,
    allow_estimation: c_int,
) -> c_int {
    crate::queuehotpath::calc_critical_health(
        size,
        par_size,
        par_current_failed_size,
        allow_estimation != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_update_min_max_time(
    times: *const i64,
    len: usize,
    out_min: *mut i64,
    out_max: *mut i64,
) {
    let slice = if len == 0 || times.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(times, len)
    };
    let (mn, mx) = crate::queuehotpath::calc_min_max_time(slice);
    if !out_min.is_null() {
        *out_min = mn;
    }
    if !out_max.is_null() {
        *out_max = mx;
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_is_dupe_success(
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
    crate::queuehotpath::is_dupe_success(
        mark_status,
        delete_status,
        par_status,
        unpack_status,
        url_status,
        health,
        critical_health_est,
        critical_health_exact,
        health_check_dupe != 0,
    ) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_make_text_status(
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
    crate::queuehotpath::make_text_status_cstr(
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
    .cast()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_completed_same_filename(
    a: *const c_char,
    b: *const c_char,
) -> c_int {
    if a.is_null() || b.is_null() {
        return 0;
    }
    crate::queuehotpath::completed_same_filename(
        CStr::from_ptr(a).to_bytes(),
        CStr::from_ptr(b).to_bytes(),
    ) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_calc_write_buffer_size(
    write_buffer_kb: c_int,
    rec_size: c_int,
) -> c_int {
    crate::queuehotpath::calc_write_buffer_size(write_buffer_kb, rec_size)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_classify_completed_file_status(
    total_articles: c_int,
    success_articles: c_int,
    missed_articles: c_int,
    failed_articles: c_int,
) -> c_int {
    crate::queuehotpath::classify_completed_file_status(
        total_articles,
        success_articles,
        missed_articles,
        failed_articles,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_resolve_completed_filename(
    completed: c_int,
    file_status: c_int,
    output_filename: *const c_char,
    file_filename: *const c_char,
    dest_dir: *const c_char,
    path_sep: c_char,
    output_file_exists: c_int,
) -> RsBuf {
    let mut out = crate::queuehotpath::resolve_completed_filename(
        completed != 0,
        file_status,
        crate::queuehotpath::cstr_to_bytes(output_filename),
        crate::queuehotpath::cstr_to_bytes(file_filename),
        crate::queuehotpath::cstr_to_bytes(dest_dir),
        path_sep as u8,
        output_file_exists != 0,
    );
    out.push(0);
    into_buf(out)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_article_cache_should_flush(
    allocated: usize,
    effective_limit: usize,
    tier: c_int,
    just_flushed: c_int,
    reset_counter: c_int,
    is_stopped: c_int,
    direct_write: c_int,
    out_flush_everything: *mut c_int,
    out_need_emergency_flush: *mut c_int,
) -> c_int {
    let (should, flush_all, emerg) = crate::queuehotpath::article_cache_should_flush(
        allocated,
        effective_limit,
        tier,
        just_flushed != 0,
        reset_counter,
        is_stopped != 0,
        direct_write != 0,
    );
    if !out_flush_everything.is_null() {
        *out_flush_everything = flush_all as c_int;
    }
    if !out_need_emergency_flush.is_null() {
        *out_need_emergency_flush = emerg as c_int;
    }
    should as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_memory_monitor_step(
    raw_sample: c_int,
    current_tier: c_int,
    candidate_tier: *mut c_int,
    consecutive_samples: *mut c_int,
    out_next_tier: *mut c_int,
) -> c_int {
    let mut cand = if candidate_tier.is_null() {
        0
    } else {
        *candidate_tier
    };
    let mut consec = if consecutive_samples.is_null() {
        0
    } else {
        *consecutive_samples
    };
    let (next, transitioned) =
        crate::queuehotpath::memory_monitor_step(raw_sample, current_tier, &mut cand, &mut consec);
    if !candidate_tier.is_null() {
        *candidate_tier = cand;
    }
    if !consecutive_samples.is_null() {
        *consecutive_samples = consec;
    }
    if !out_next_tier.is_null() {
        *out_next_tier = next;
    }
    transitioned as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_adjust_downloads_limit(
    norm_levels: *const i32,
    actives: *const u8,
    max_conns: *const i32,
    count: usize,
) -> c_int {
    if count == 0 || norm_levels.is_null() || actives.is_null() || max_conns.is_null() {
        return 2;
    }
    crate::queuehotpath::calc_downloads_limit(
        std::slice::from_raw_parts(norm_levels, count),
        std::slice::from_raw_parts(actives, count),
        std::slice::from_raw_parts(max_conns, count),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_is_propagation_wait(
    post_time: i64,
    propagation_delay: c_int,
    cur_date: i64,
) -> c_int {
    crate::queuehotpath::is_propagation_wait(post_time, propagation_delay, cur_date) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_select_next_file(
    files: *const crate::queuehotpath::RsCandidateFile,
    count: usize,
    propagation_delay: c_int,
    cur_date: i64,
    out_best_idx: *mut c_int,
    out_prop_wait_idx: *mut c_int,
) {
    let slice = if count == 0 || files.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(files, count)
    };
    let (best, prop) =
        crate::queuehotpath::select_next_file(slice, propagation_delay, cur_date);
    if !out_best_idx.is_null() {
        *out_best_idx = best;
    }
    if !out_prop_wait_idx.is_null() {
        *out_prop_wait_idx = prop;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_is_data_filename(filename: *const c_char) -> c_int {
    crate::queuehotpath::is_data_filename(crate::queuehotpath::cstr_to_bytes(filename)) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_size_near(size1: i64, size2: i64) -> c_int {
    crate::queuehotpath::size_near(size1, size2) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_download_hopeless(
    total: c_int,
    current_success: c_int,
    current_failed: c_int,
) -> c_int {
    crate::queuehotpath::download_hopeless(total, current_success, current_failed) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_eval_dead_download(
    current_success: c_int,
    dupe_recovered: c_int,
    current_failed: c_int,
    file_count: c_int,
    dupe_failed_run: c_int,
    par_cannot_cover_exact: c_int,
    critical: c_int,
    par_edge_margin: c_int,
    files_tried: c_int,
    probing_sec: c_int,
    out_eval: *mut crate::queuehotpath::RsDeadDownloadEval,
) {
    if out_eval.is_null() {
        return;
    }
    *out_eval = crate::queuehotpath::eval_dead_download(
        current_success,
        dupe_recovered,
        current_failed,
        file_count,
        dupe_failed_run,
        par_cannot_cover_exact != 0,
        critical,
        par_edge_margin,
        files_tried,
        probing_sec,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_is_side_file(
    filename: *const c_char,
    ext_cleanup_disk: *const c_char,
) -> c_int {
    crate::queuehotpath::is_side_file(
        crate::queuehotpath::cstr_to_bytes(filename),
        crate::queuehotpath::cstr_to_bytes(ext_cleanup_disk),
    ) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_is_hanging_download(
    now: i64,
    last_update: i64,
    article_timeout: c_int,
    is_running: c_int,
) -> c_int {
    crate::queuehotpath::is_hanging_download(now, last_update, article_timeout, is_running != 0)
        as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_should_cleanup_parked_files(
    par_status: c_int,
    unpack_status: c_int,
    failed_size: i64,
    par_failed_size: i64,
    delete_status: c_int,
    parking: c_int,
    unpack_cleaned_up_disk: c_int,
) -> c_int {
    crate::queuehotpath::should_cleanup_parked_files(
        par_status,
        unpack_status,
        failed_size,
        par_failed_size,
        delete_status,
        parking != 0,
        unpack_cleaned_up_disk != 0,
    ) as c_int
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_classify_dup_status(
    mark_status: c_int,
    delete_status: c_int,
    is_dupe_success: c_int,
) -> c_int {
    crate::queuehotpath::classify_dup_status(mark_status, delete_status, is_dupe_success != 0)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qhp_should_reset_article(
    article_status: c_int,
    partial_state: c_int,
    all_failed: c_int,
    reset_failed: c_int,
) -> c_int {
    crate::queuehotpath::should_reset_article(
        article_status,
        partial_state,
        all_failed != 0,
        reset_failed != 0,
    ) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_parse_dupe_mode(
    text: *const c_char,
    out_mode: *mut c_int,
) -> c_int {
    if let Some(m) = crate::queuehotpath::parse_dupe_mode(crate::queuehotpath::cstr_to_bytes(text)) {
        if !out_mode.is_null() {
            *out_mode = m;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_parse_sort_spec(
    sort: *const c_char,
    out_criteria: *mut c_int,
    out_order: *mut c_int,
) -> c_int {
    if let Some((crit, ord)) =
        crate::queuehotpath::parse_sort_spec(crate::queuehotpath::cstr_to_bytes(sort))
    {
        if !out_criteria.is_null() {
            *out_criteria = crit;
        }
        if !out_order.is_null() {
            *out_order = ord;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_clamp_move_offset(
    index: c_int,
    nr_entries: c_int,
    offset: c_int,
    last_dest_pos: c_int,
    out_work_offset: *mut c_int,
    out_last_dest_pos: *mut c_int,
) {
    let (w, last) = crate::queuehotpath::clamp_move_offset(index, nr_entries, offset, last_dest_pos);
    if !out_work_offset.is_null() {
        *out_work_offset = w;
    }
    if !out_last_dest_pos.is_null() {
        *out_last_dest_pos = last;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_select_pars_to_pause(
    is_par: *const u8,
    has_vol: *const u8,
    sizes: *const i64,
    len: usize,
    extra_pars_only: c_int,
    out_pause: *mut u8,
) {
    if len == 0 || is_par.is_null() || has_vol.is_null() || sizes.is_null() || out_pause.is_null() {
        return;
    }
    crate::queuehotpath::select_pars_to_pause(
        std::slice::from_raw_parts(is_par, len),
        std::slice::from_raw_parts(has_vol, len),
        std::slice::from_raw_parts(sizes, len),
        extra_pars_only != 0,
        std::slice::from_raw_parts_mut(out_pause, len),
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qhp_compare_inner_files(
    name1: *const c_char,
    par1: c_int,
    size1: i64,
    name2: *const c_char,
    par2: c_int,
    size2: i64,
    same_par_collection: c_int,
) -> c_int {
    crate::queuehotpath::compare_inner_files(
        crate::queuehotpath::cstr_to_bytes(name1),
        par1 != 0,
        size1,
        crate::queuehotpath::cstr_to_bytes(name2),
        par2 != 0,
        size2,
        same_par_collection != 0,
    ) as c_int
}

// ============================================================================
// DiskState, PrePostProcessor, Scanner & TwinCheck Hot-Path FFI (`rust/src/diskstatepp.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_parse_format_version(
    format_signature: *const c_char,
) -> c_int {
    crate::diskstatepp::parse_format_version(crate::diskstatepp::cstr_to_bytes(format_signature))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_sanitize_line_breaks(buf: *mut c_char, len: usize) {
    if buf.is_null() || len == 0 {
        return;
    }
    let slice = std::slice::from_raw_parts_mut(buf as *mut u8, len);
    crate::diskstatepp::sanitize_line_breaks_in_place(slice);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_parse_ids_file(
    text: *const c_char,
    out_nzb_id: *mut c_int,
    out_file_id: *mut c_int,
) -> c_int {
    if let Some((nzb_id, file_id)) =
        crate::diskstatepp::parse_ids_file(crate::diskstatepp::cstr_to_bytes(text))
    {
        if !out_nzb_id.is_null() {
            *out_nzb_id = nzb_id;
        }
        if !out_file_id.is_null() {
            *out_file_id = file_id;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_classify_queue_dir_id(
    filename: *const c_char,
    out_is_nzb_log: *mut c_int,
) -> c_int {
    if let Some((id, is_nzb_log)) =
        crate::diskstatepp::classify_queue_dir_id(crate::diskstatepp::cstr_to_bytes(filename))
    {
        if !out_is_nzb_log.is_null() {
            *out_is_nzb_log = if is_nzb_log { 1 } else { 0 };
        }
        id
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_classify_legacy_file_state(
    filename: *const c_char,
    header_line: *const c_char,
    file_version: c_int,
    legacy_version: c_int,
    out_id: *mut c_int,
    out_suffix_kind: *mut c_int,
) -> c_int {
    if let Some((id, suffix_kind)) = crate::diskstatepp::classify_legacy_file_state(
        crate::diskstatepp::cstr_to_bytes(filename),
        crate::diskstatepp::cstr_to_bytes(header_line),
        file_version,
        legacy_version,
    ) {
        if !out_id.is_null() {
            *out_id = id;
        }
        if !out_suffix_kind.is_null() {
            *out_suffix_kind = suffix_kind;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_validate_repair_hole(
    offset_hi: u32,
    offset_lo: u32,
    size_hi: u32,
    size_lo: u32,
    decoded_size: i64,
    out_offset: *mut i64,
    out_size: *mut i64,
) -> c_int {
    if let Some((offset, size)) = crate::diskstatepp::validate_repair_hole(
        offset_hi,
        offset_lo,
        size_hi,
        size_lo,
        decoded_size,
    ) {
        if !out_offset.is_null() {
            *out_offset = offset;
        }
        if !out_size.is_null() {
            *out_size = size;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_has_dupe_extras(
    recovered_articles: c_int,
    recovered_bytes: i64,
    recovered_holes: c_int,
    repair_jobs_count: usize,
) -> c_int {
    if crate::diskstatepp::has_dupe_extras(
        recovered_articles,
        recovered_bytes,
        recovered_holes,
        repair_jobs_count,
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_pack_unpack_tried_flags(
    unpack_tried: c_int,
    par_repaired: c_int,
) -> c_int {
    crate::diskstatepp::pack_unpack_tried_flags(unpack_tried != 0, par_repaired != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_parse_unpack_state_lines(
    line1: *const c_char,
    line2: *const c_char,
    line3: *const c_char,
    ns_running: c_int,
    ns_none: c_int,
    out_direct_status: *mut c_int,
    out_unpack_tried: *mut c_int,
    out_par_repaired: *mut c_int,
    out_last_unpack_status: *mut c_int,
) -> c_int {
    if let Some((ds, ut, pr, lus)) = crate::diskstatepp::parse_unpack_state_lines(
        crate::diskstatepp::cstr_to_bytes(line1),
        crate::diskstatepp::cstr_to_bytes(line2),
        crate::diskstatepp::cstr_to_bytes(line3),
        ns_running,
        ns_none,
    ) {
        if !out_direct_status.is_null() {
            *out_direct_status = ds;
        }
        if !out_unpack_tried.is_null() {
            *out_unpack_tried = if ut { 1 } else { 0 };
        }
        if !out_par_repaired.is_null() {
            *out_par_repaired = if pr { 1 } else { 0 };
        }
        if !out_last_unpack_status.is_null() {
            *out_last_unpack_status = lus;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_normalize_post_stage(
    post_stage: c_int,
    format_version: c_int,
    pt_finished: c_int,
) -> c_int {
    crate::diskstatepp::normalize_post_stage(post_stage, format_version, pt_finished)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_eval_article_load_state(
    format_version: c_int,
    completed: c_int,
    fields: c_int,
    status_in: c_int,
    segment_offset_in: i64,
    segment_size_in: c_int,
    staged_fallback_in: c_int,
    part_number: c_int,
    staged_file_exists: c_int,
    direct_write: c_int,
    force_direct_write: c_int,
    has_result_filename: c_int,
    completed_articles_in: c_int,
    total_size: c_int,
    out_eval: *mut crate::diskstatepp::RsArticleLoadEval,
) -> c_int {
    let eval = crate::diskstatepp::eval_article_load_state(
        format_version,
        completed != 0,
        fields,
        status_in,
        segment_offset_in,
        segment_size_in,
        staged_fallback_in,
        part_number,
        staged_file_exists != 0,
        direct_write != 0,
        force_direct_write != 0,
        has_result_filename != 0,
        completed_articles_in,
        total_size,
    );
    if !out_eval.is_null() {
        *out_eval = eval;
    }
    eval.valid
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_reconcile_direct_write_output(
    output_initialized: c_int,
    has_own_out_file: c_int,
    has_out_tmp: c_int,
    has_completed_dest: c_int,
    success_articles: c_int,
    completed: c_int,
    output_filename_empty: c_int,
    out_new_output_initialized: *mut c_int,
    out_pick_path_kind: *mut c_int,
) {
    let (new_init, pick_kind) = crate::diskstatepp::reconcile_direct_write_output(
        output_initialized != 0,
        has_own_out_file != 0,
        has_out_tmp != 0,
        has_completed_dest != 0,
        success_articles,
        completed != 0,
        output_filename_empty != 0,
    );
    if !out_new_output_initialized.is_null() {
        *out_new_output_initialized = if new_init { 1 } else { 0 };
    }
    if !out_pick_path_kind.is_null() {
        *out_pick_path_kind = pick_kind;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_classify_temp_file(
    filename: *const c_char,
    out_file_id: *mut c_int,
    out_part: *mut c_int,
) -> c_int {
    let (kind, id, part) =
        crate::diskstatepp::classify_temp_file(crate::diskstatepp::cstr_to_bytes(filename));
    if !out_file_id.is_null() {
        *out_file_id = id;
    }
    if !out_part.is_null() {
        *out_part = part;
    }
    kind
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_is_orphan_queue_file(
    filename: *const c_char,
    sorted_file_ids: *const c_int,
    file_ids_len: usize,
    sorted_nzb_ids: *const c_int,
    nzb_ids_len: usize,
) -> c_int {
    let file_ids = if sorted_file_ids.is_null() || file_ids_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(sorted_file_ids, file_ids_len)
    };
    let nzb_ids = if sorted_nzb_ids.is_null() || nzb_ids_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(sorted_nzb_ids, nzb_ids_len)
    };
    if crate::diskstatepp::is_orphan_queue_file(
        crate::diskstatepp::cstr_to_bytes(filename),
        file_ids,
        nzb_ids,
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_match_servers(
    server_names: *const *const c_char,
    server_hosts: *const *const c_char,
    server_ports: *const c_int,
    server_users: *const *const c_char,
    server_count: usize,
    ref_state_ids: *const c_int,
    ref_names: *const *const c_char,
    ref_hosts: *const *const c_char,
    ref_ports: *const c_int,
    ref_users: *const *const c_char,
    ref_count: usize,
    out_server_state_ids: *mut c_int,
) -> c_int {
    if server_count > 0
        && (server_names.is_null()
            || server_hosts.is_null()
            || server_ports.is_null()
            || server_users.is_null()
            || out_server_state_ids.is_null())
    {
        return 0;
    }
    if ref_count > 0
        && (ref_state_ids.is_null()
            || ref_names.is_null()
            || ref_hosts.is_null()
            || ref_ports.is_null()
            || ref_users.is_null())
    {
        return 0;
    }

    let mut servers = Vec::with_capacity(server_count);
    for i in 0..server_count {
        servers.push(crate::diskstatepp::ServerMatchInput {
            name: crate::diskstatepp::cstr_to_bytes(*server_names.add(i)),
            host: crate::diskstatepp::cstr_to_bytes(*server_hosts.add(i)),
            port: *server_ports.add(i),
            user: crate::diskstatepp::cstr_to_bytes(*server_users.add(i)),
        });
    }

    let mut refs = Vec::with_capacity(ref_count);
    for i in 0..ref_count {
        refs.push(crate::diskstatepp::ServerRefEntry {
            state_id: *ref_state_ids.add(i),
            name: crate::diskstatepp::cstr_to_bytes(*ref_names.add(i)),
            host: crate::diskstatepp::cstr_to_bytes(*ref_hosts.add(i)),
            port: *ref_ports.add(i),
            user: crate::diskstatepp::cstr_to_bytes(*ref_users.add(i)),
            matched: false,
            perfect: false,
        });
    }

    let out_ids = if server_count == 0 {
        &mut [][..]
    } else {
        std::slice::from_raw_parts_mut(out_server_state_ids, server_count)
    };
    if crate::diskstatepp::match_servers(&servers, &mut refs, out_ids) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_sanitize_log_text(buf: *mut c_char, len: usize) {
    if buf.is_null() || len == 0 {
        return;
    }
    let slice = std::slice::from_raw_parts_mut(buf as *mut u8, len);
    crate::diskstatepp::sanitize_log_text_in_place(slice);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_parse_log_line(
    line: *const c_char,
    out_time: *mut c_int,
    out_kind: *mut c_int,
    out_text_offset: *mut usize,
) -> c_int {
    if let Some((time, kind, offset)) =
        crate::diskstatepp::parse_log_line(crate::diskstatepp::cstr_to_bytes(line))
    {
        if !out_time.is_null() {
            *out_time = time;
        }
        if !out_kind.is_null() {
            *out_kind = kind;
        }
        if !out_text_offset.is_null() {
            *out_text_offset = offset;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_sanitise_post_stage(
    stage: c_int,
    pt_executing_script: c_int,
    pt_finished: c_int,
    pt_queued: c_int,
    dest_dir_exists: c_int,
    unpack_tried: c_int,
    par_status: c_int,
    ps_skipped: c_int,
    out_stage: *mut c_int,
    out_request_par_check: *mut c_int,
) {
    let (new_stage, req_par) = crate::diskstatepp::sanitise_post_stage(
        stage,
        pt_executing_script,
        pt_finished,
        pt_queued,
        dest_dir_exists != 0,
        unpack_tried != 0,
        par_status,
        ps_skipped,
    );
    if !out_stage.is_null() {
        *out_stage = new_stage;
    }
    if !out_request_par_check.is_null() {
        *out_request_par_check = if req_par { 1 } else { 0 };
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_classify_file_event(
    is_file_completed: c_int,
    is_file_deleted: c_int,
    dupe_deleted: c_int,
    delete_status: c_int,
    ds_none: c_int,
    ds_health: c_int,
    has_post_info: c_int,
    download_completed_ignore_paused: c_int,
    download_completed_all: c_int,
) -> c_int {
    crate::diskstatepp::classify_file_event(
        is_file_completed != 0,
        is_file_deleted != 0,
        dupe_deleted != 0,
        delete_status,
        ds_none,
        ds_health,
        has_post_info != 0,
        download_completed_ignore_paused != 0,
        download_completed_all != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_eval_nzb_completed(
    dupe_hint: c_int,
    dh_redownload_auto: c_int,
    keep_history: c_int,
    avoid_history: c_int,
    dupe_check: c_int,
    dupe_mode: c_int,
    dm_force: c_int,
    fails_over: c_int,
    delete_status: c_int,
    ds_none: c_int,
    ds_health: c_int,
    ds_bad: c_int,
    ds_scan: c_int,
    ds_copy: c_int,
    out_add_to_history: *mut c_int,
    out_call_dupe_completed: *mut c_int,
    out_enqueue_deleted_script: *mut c_int,
    out_need_save: *mut c_int,
) {
    let (add_hist, call_dupe, enq_del, need_save) = crate::diskstatepp::eval_nzb_completed(
        dupe_hint,
        dh_redownload_auto,
        keep_history,
        avoid_history != 0,
        dupe_check != 0,
        dupe_mode,
        dm_force,
        fails_over != 0,
        delete_status,
        ds_none,
        ds_health,
        ds_bad,
        ds_scan,
        ds_copy,
    );
    if !out_add_to_history.is_null() {
        *out_add_to_history = if add_hist { 1 } else { 0 };
    }
    if !out_call_dupe_completed.is_null() {
        *out_call_dupe_completed = if call_dupe { 1 } else { 0 };
    }
    if !out_enqueue_deleted_script.is_null() {
        *out_enqueue_deleted_script = if enq_del { 1 } else { 0 };
    }
    if !out_need_save.is_null() {
        *out_need_save = if need_save { 1 } else { 0 };
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_can_run_more_jobs(
    job_stages: *const c_int,
    job_count: usize,
    pt_loading_pars: c_int,
    pt_verifying_repaired: c_int,
    pt_repairing: c_int,
    post_strategy: c_int,
    out_allow_par: *mut c_int,
) -> c_int {
    let stages = if job_stages.is_null() || job_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(job_stages, job_count)
    };
    let (can_run, allow_par) = crate::diskstatepp::can_run_more_jobs(
        stages,
        pt_loading_pars,
        pt_verifying_repaired,
        pt_repairing,
        post_strategy,
    );
    if !out_allow_par.is_null() {
        *out_allow_par = if allow_par { 1 } else { 0 };
    }
    if can_run {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_pick_next_post_job(
    candidates: *const crate::diskstatepp::RsPostJobCandidate,
    count: usize,
    allow_par: c_int,
    has_active_unpack: c_int,
    pause_post_process: c_int,
) -> c_int {
    if candidates.is_null() || count == 0 {
        return -1;
    }
    let slice = std::slice::from_raw_parts(candidates, count);
    crate::diskstatepp::pick_next_post_job(
        slice,
        allow_par != 0,
        has_active_unpack != 0,
        pause_post_process != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_eval_post_stages(
    want_unpack: c_int,
    unpack_status: c_int,
    delete_status: c_int,
    par_status: c_int,
    cleanup_status: c_int,
    has_ext_cleanup_disk: c_int,
    health: c_int,
    move_status: c_int,
    in_inter_dir: c_int,
    direct_unpack_status: c_int,
    opt_post_download_rename: c_int,
    post_download_renaming_status: c_int,
    has_dest_dir: c_int,
    has_name: c_int,
    opt_rename_after_unpack: c_int,
    post_unpack_renaming_status: c_int,
    out_decision: *mut crate::diskstatepp::RsPostStageDecision,
) {
    let dec = crate::diskstatepp::eval_post_stages(
        want_unpack != 0,
        unpack_status,
        delete_status,
        par_status,
        cleanup_status,
        has_ext_cleanup_disk != 0,
        health,
        move_status,
        in_inter_dir != 0,
        direct_unpack_status,
        opt_post_download_rename != 0,
        post_download_renaming_status,
        has_dest_dir != 0,
        has_name != 0,
        opt_rename_after_unpack != 0,
        post_unpack_renaming_status,
    );
    if !out_decision.is_null() {
        *out_decision = dec;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_calc_post_pause_state(
    job_stages: *const c_int,
    job_count: usize,
    par_pause_queue: c_int,
    unpack_pause_queue: c_int,
    script_pause_queue: c_int,
) -> c_int {
    let stages = if job_stages.is_null() || job_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(job_stages, job_count)
    };
    if crate::diskstatepp::calc_post_pause_state(
        stages,
        par_pause_queue != 0,
        unpack_pause_queue != 0,
        script_pause_queue != 0,
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_scanner_next_pass(
    nzb_dir_interval: c_int,
    nzb_dir_file_age: c_int,
    has_scan_script: c_int,
    current_pass: c_int,
    out_next_interval: *mut c_int,
    out_next_pass: *mut c_int,
) {
    let (next_int, next_pass) = crate::diskstatepp::scanner_next_pass(
        nzb_dir_interval,
        nzb_dir_file_age,
        has_scan_script != 0,
        current_pass,
    );
    if !out_next_interval.is_null() {
        *out_next_interval = next_int;
    }
    if !out_next_pass.is_null() {
        *out_next_pass = next_pass;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_can_process_extension(
    full_filename: *const c_char,
) -> c_int {
    if crate::diskstatepp::can_process_extension(crate::diskstatepp::cstr_to_bytes(full_filename)) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_eval_file_age(
    old_size: i64,
    new_size: i64,
    last_change: i64,
    current_time: i64,
    file_age: c_int,
    out_size: *mut i64,
    out_last_change: *mut i64,
) -> c_int {
    let (can_proc, sz, lc) = crate::diskstatepp::eval_file_age(
        old_size,
        new_size,
        last_change,
        current_time,
        file_age,
    );
    if !out_size.is_null() {
        *out_size = sz;
    }
    if !out_last_change.is_null() {
        *out_last_change = lc;
    }
    if can_proc {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_should_drop_scan_file(
    last_change: i64,
    current_time: i64,
    nzb_dir_interval: c_int,
    nzb_dir_file_age: c_int,
) -> c_int {
    if crate::diskstatepp::should_drop_scan_file(
        last_change,
        current_time,
        nzb_dir_interval,
        nzb_dir_file_age,
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_classify_missed_articles(
    missed_articles: c_int,
    total_articles: c_int,
    is_cut_short: c_int,
) -> c_int {
    crate::diskstatepp::classify_missed_articles(
        missed_articles,
        total_articles,
        is_cut_short != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_is_nzb_xml_prefix(
    buf: *const u8,
    len: usize,
) -> c_int {
    if buf.is_null() || len == 0 {
        return 0;
    }
    let slice = std::slice::from_raw_parts(buf, len);
    if crate::diskstatepp::is_nzb_xml_prefix(slice) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_normalize_external_nzb_name(
    nzb_name: *const c_char,
    valid_name: *const c_char,
    is_nzb: c_int,
) -> RsBuf {
    let out = crate::diskstatepp::normalize_external_nzb_name(
        crate::diskstatepp::cstr_to_bytes(nzb_name),
        crate::diskstatepp::cstr_to_bytes(valid_name),
        is_nzb != 0,
    );
    into_buf(out)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_twin_sample_indices(
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
    if let Some(indices) = crate::diskstatepp::twin_sample_indices(
        p_counts,
        p_bytes,
        d_counts,
        d_bytes,
        sample_count,
    ) {
        for (i, (f_idx, s_idx)) in indices.into_iter().enumerate() {
            *out_file_indices.add(i) = f_idx;
            *out_seg_indices.add(i) = s_idx;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_twin_select_primary(
    queued: *const u8,
    is_dupe: *const u8,
    scores: *const c_int,
    count: usize,
) -> c_int {
    if count == 0 || queued.is_null() || is_dupe.is_null() || scores.is_null() {
        return -1;
    }
    crate::diskstatepp::twin_select_primary(
        std::slice::from_raw_parts(queued, count),
        std::slice::from_raw_parts(is_dupe, count),
        std::slice::from_raw_parts(scores, count),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dsp_twin_resolve_kind(
    is_primary: c_int,
    is_dupe: c_int,
    by_par2: c_int,
    by_files: *const c_char,
    listed: c_int,
    same_print: c_int,
    sampleable: c_int,
    sampled_now: c_int,
    sampled_val: *const c_char,
) -> RsBuf {
    let out = crate::diskstatepp::twin_resolve_kind(
        is_primary != 0,
        is_dupe != 0,
        by_par2 != 0,
        crate::diskstatepp::cstr_to_bytes(by_files),
        listed != 0,
        same_print != 0,
        sampleable != 0,
        sampled_now != 0,
        crate::diskstatepp::cstr_to_bytes(sampled_val),
    );
    into_buf(out)
}

// ============================================================================
// DupeSearch, Fleet, DonorHealth, Newznab, FeedCoordinator, FeedInfo &
// FeedFile Hot-Path FFI (`rust/src/dupefeed.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_lower_key(key: *const c_char) -> RsBuf {
    into_buf(crate::dupefeed::lower_key(crate::diskstatepp::cstr_to_bytes(key)))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_saved_history_backup(
    has_pick: c_int,
    pick_full_hash: u32,
    candidate_delete_status: c_int,
    candidate_full_hash: u32,
    ds_dupe: c_int,
    ds_copy: c_int,
) -> c_int {
    if crate::dupefeed::saved_history_backup(
        has_pick != 0,
        pick_full_hash,
        candidate_delete_status,
        candidate_full_hash,
        ds_dupe,
        ds_copy,
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_placement_available(
    has_backup: c_int,
    is_nzb_kind: c_int,
    deleting: c_int,
    delete_status: c_int,
    ds_none: c_int,
    ds_manual: c_int,
    ds_dupe: c_int,
    ds_copy: c_int,
    mark_status: c_int,
    ks_bad: c_int,
    dupe_mode: c_int,
    dm_force: c_int,
    in_history: c_int,
    whole_backup: c_int,
    has_post_info: c_int,
    is_dupe_success: c_int,
    has_pick: c_int,
    pick_full_hash: u32,
    backup_full_hash: u32,
    queued_file_exists: c_int,
    health: c_int,
    critical_health: c_int,
    health_check_is_dupe: c_int,
    has_marked_alive: c_int,
    marked_alive_val: c_int,
) -> c_int {
    if crate::dupefeed::placement_available(
        has_backup != 0,
        is_nzb_kind != 0,
        deleting != 0,
        delete_status,
        ds_none,
        ds_manual,
        ds_dupe,
        ds_copy,
        mark_status,
        ks_bad,
        dupe_mode,
        dm_force,
        in_history != 0,
        whole_backup != 0,
        has_post_info != 0,
        is_dupe_success != 0,
        has_pick != 0,
        pick_full_hash,
        backup_full_hash,
        queued_file_exists != 0,
        health,
        critical_health,
        health_check_is_dupe != 0,
        has_marked_alive != 0,
        marked_alive_val,
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_sort_verified_postings(
    total_bytes: *const i64,
    grabs: *const c_int,
    dates: *const i64,
    count: usize,
    pick_bytes: i64,
    out_order: *mut usize,
) {
    if count == 0 || total_bytes.is_null() || grabs.is_null() || dates.is_null() || out_order.is_null() {
        return;
    }
    crate::dupefeed::sort_verified_postings(
        std::slice::from_raw_parts(total_bytes, count),
        std::slice::from_raw_parts(grabs, count),
        std::slice::from_raw_parts(dates, count),
        pick_bytes,
        std::slice::from_raw_parts_mut(out_order, count),
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_eval_member_rerank(
    base: c_int,
    wanted_score: c_int,
    dead_score: c_int,
    old_score: c_int,
    alive: f64,
    all_dead: c_int,
    out_score: *mut c_int,
    out_percent: *mut c_int,
    out_add_alive_zero: *mut c_int,
) {
    let (score, percent, add_zero) = crate::dupefeed::eval_member_rerank(
        base,
        wanted_score,
        dead_score,
        old_score,
        alive,
        all_dead != 0,
    );
    if !out_score.is_null() {
        *out_score = score;
    }
    if !out_percent.is_null() {
        *out_percent = percent;
    }
    if !out_add_alive_zero.is_null() {
        *out_add_alive_zero = if add_zero { 1 } else { 0 };
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_eval_pick_gone(
    has_pick: c_int,
    deleting: c_int,
    delete_status: c_int,
    ds_manual: c_int,
    pick_eff_key: *const c_char,
    job_dupe_key: *const c_char,
    pick_norm_name: *const c_char,
    job_norm_name: *const c_char,
) -> c_int {
    crate::dupefeed::eval_pick_gone(
        has_pick != 0,
        deleting != 0,
        delete_status,
        ds_manual,
        crate::diskstatepp::cstr_to_bytes(pick_eff_key),
        crate::diskstatepp::cstr_to_bytes(job_dupe_key),
        crate::diskstatepp::cstr_to_bytes(pick_norm_name),
        crate::diskstatepp::cstr_to_bytes(job_norm_name),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_format_pending_meta(
    title: *const c_char,
    indexer: *const c_char,
    grabs: c_int,
    size: i64,
    date: i64,
) -> RsBuf {
    let out = crate::dupefeed::format_pending_meta(
        crate::diskstatepp::cstr_to_bytes(title),
        crate::diskstatepp::cstr_to_bytes(indexer),
        grabs,
        size,
        date,
    );
    into_buf(out)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_parse_pending_meta(
    meta: *const c_char,
    len: usize,
    out_view: *mut crate::dupefeed::RsPendingMetaView,
) {
    if out_view.is_null() {
        return;
    }
    let slice = if meta.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(meta as *const u8, len)
    };
    *out_view = crate::dupefeed::parse_pending_meta(slice);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_parse_dupesearch_state_line(
    line: *const c_char,
    len: usize,
    now: i64,
    state_ttl_sec: i64,
    out_line: *mut crate::dupefeed::RsDupeSearchStateLine,
) -> c_int {
    if out_line.is_null() || line.is_null() || len == 0 {
        return 0;
    }
    let slice = std::slice::from_raw_parts(line as *const u8, len);
    let parsed = crate::dupefeed::parse_dupesearch_state_line(slice, now, state_ttl_sec);
    let ok = parsed.valid;
    *out_line = parsed;
    ok
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_eval_fleet_member_health(
    checked: c_int,
    result_complete: c_int,
    present: c_int,
    missing: c_int,
    error: c_int,
    min_known: c_int,
    min_alive_pct: c_int,
    out_alive_pct: *mut c_int,
    out_dead: *mut c_int,
    out_member_complete: *mut c_int,
) {
    let (alive, dead, complete) = crate::dupefeed::eval_fleet_member_health(
        checked != 0,
        result_complete != 0,
        present,
        missing,
        error,
        min_known,
        min_alive_pct,
    );
    if !out_alive_pct.is_null() {
        *out_alive_pct = alive;
    }
    if !out_dead.is_null() {
        *out_dead = if dead { 1 } else { 0 };
    }
    if !out_member_complete.is_null() {
        *out_member_complete = if complete { 1 } else { 0 };
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_rank_fleet_candidates(
    has_error: *const u8,
    same_as: *const c_int,
    dead: *const u8,
    alive: *const c_int,
    total_bytes: *const i64,
    count: usize,
    out_ranked: *mut usize,
) -> usize {
    if count == 0
        || has_error.is_null()
        || same_as.is_null()
        || dead.is_null()
        || alive.is_null()
        || total_bytes.is_null()
        || out_ranked.is_null()
    {
        return 0;
    }
    crate::dupefeed::rank_fleet_candidates(
        std::slice::from_raw_parts(has_error, count),
        std::slice::from_raw_parts(same_as, count),
        std::slice::from_raw_parts(dead, count),
        std::slice::from_raw_parts(alive, count),
        std::slice::from_raw_parts(total_bytes, count),
        std::slice::from_raw_parts_mut(out_ranked, count),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_eval_fleet_outcome_reason(
    any_usable: c_int,
    any_alive: c_int,
    any_unknown: c_int,
    on_disk: c_int,
) -> c_int {
    crate::dupefeed::eval_fleet_outcome_reason(
        any_usable != 0,
        any_alive != 0,
        any_unknown != 0,
        on_disk != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_donor_health_alive(
    present: c_int,
    missing: c_int,
    error: c_int,
    min_known: c_int,
) -> f64 {
    crate::dupefeed::donor_health_alive(present, missing, error, min_known)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_donor_health_dead_probe(
    present: c_int,
    missing: c_int,
    min_known: c_int,
) -> c_int {
    if crate::dupefeed::donor_health_dead_probe(present, missing, min_known) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_donor_sample_count(
    total_ids: usize,
    percent: c_int,
    minimum: c_int,
    maximum: c_int,
) -> usize {
    crate::dupefeed::donor_sample_count(total_ids, percent, minimum, maximum)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_adjust_pipeline_depth(
    depth: c_int,
    took_ms: i64,
    pipeline_max: c_int,
) -> c_int {
    crate::dupefeed::adjust_pipeline_depth(depth, took_ms, pipeline_max)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_eval_article_votes(
    votes: *const c_int,
    count: usize,
) -> c_int {
    if votes.is_null() || count == 0 {
        return 0;
    }
    crate::dupefeed::eval_article_votes(std::slice::from_raw_parts(votes, count))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_resolve_unsettled_vote(
    votes: *const c_int,
    count: usize,
) -> c_int {
    if votes.is_null() || count == 0 {
        return 0;
    }
    crate::dupefeed::resolve_unsettled_vote(std::slice::from_raw_parts(votes, count))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_url_encode(text: *const c_char, len: usize) -> RsBuf {
    let slice = if text.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(text as *const u8, len)
    };
    into_buf(crate::dupefeed::url_encode(slice))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_digits_only(text: *const c_char, len: usize) -> RsBuf {
    let slice = if text.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(text as *const u8, len)
    };
    into_buf(crate::dupefeed::digits_only(slice))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_days_from_civil(
    year: i64,
    month: c_int,
    day: c_int,
) -> i64 {
    crate::dupefeed::days_from_civil(year, month, day)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_parse_newznab_date(
    text: *const c_char,
    len: usize,
) -> i64 {
    let slice = if text.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(text as *const u8, len)
    };
    crate::dupefeed::parse_newznab_date(slice)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_build_newznab_url(
    base: *const c_char,
    param_keys: *const *const c_char,
    param_vals: *const *const c_char,
    param_count: usize,
    api_key: *const c_char,
) -> RsBuf {
    let mut keys: Vec<&[u8]> = Vec::with_capacity(param_count);
    let mut vals: Vec<&[u8]> = Vec::with_capacity(param_count);
    if !param_keys.is_null() && !param_vals.is_null() {
        for i in 0..param_count {
            keys.push(crate::diskstatepp::cstr_to_bytes(*param_keys.add(i)));
            vals.push(crate::diskstatepp::cstr_to_bytes(*param_vals.add(i)));
        }
    }
    let out = crate::dupefeed::build_newznab_url(
        crate::diskstatepp::cstr_to_bytes(base),
        &keys,
        &vals,
        crate::diskstatepp::cstr_to_bytes(api_key),
    );
    into_buf(out)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_mask_newznab_text(
    text: *const c_char,
    len: usize,
) -> RsBuf {
    let slice = if text.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(text as *const u8, len)
    };
    into_buf(crate::dupefeed::mask_newznab_text(slice))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_normalize_feed_nzb_name(
    filename: *const c_char,
) -> RsBuf {
    into_buf(crate::dupefeed::normalize_feed_nzb_name(
        crate::diskstatepp::cstr_to_bytes(filename),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_format_dupe_status_mask(dupe_status: c_int) -> RsBuf {
    into_buf(crate::dupefeed::format_dupe_status_mask(dupe_status))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_should_start_feed(
    interval_min: c_int,
    next_update: i64,
    current_time: i64,
    fetch: c_int,
    status: c_int,
    fs_running: c_int,
    out_set_force: *mut c_int,
) -> c_int {
    let (start, set_force) = crate::dupefeed::should_start_feed(
        interval_min,
        next_update,
        current_time,
        fetch != 0,
        status,
        fs_running,
    );
    if !out_set_force.is_null() {
        *out_set_force = if set_force { 1 } else { 0 };
    }
    if start {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_calc_feed_next_interval(
    success: c_int,
    interval_min: c_int,
    last_interval_sec: c_int,
    out_last_interval_sec: *mut c_int,
) -> c_int {
    let (interval_sec, new_last) = crate::dupefeed::calc_feed_next_interval(
        success != 0,
        interval_min,
        last_interval_sec,
    );
    if !out_last_interval_sec.is_null() {
        *out_last_interval_sec = new_last;
    }
    interval_sec
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_is_feed_cache_expired(
    last_usage: i64,
    cache_time_sec: c_int,
    cur_time: i64,
) -> c_int {
    if crate::dupefeed::is_feed_cache_expired(last_usage, cache_time_sec, cur_time) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_parse_prefixed_int(value: *const c_char) -> c_int {
    crate::dupefeed::parse_prefixed_int(crate::diskstatepp::cstr_to_bytes(value))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_build_feed_dupe_key(
    rage_id_str: *const c_char,
    tvdb_id_str: *const c_char,
    tvmaze_id_str: *const c_char,
    series: *const c_char,
    item_rage_id: c_int,
    item_tvdb_id: c_int,
    item_tvmaze_id: c_int,
    item_imdb_id: c_int,
    season_num: c_int,
    episode_num: c_int,
    season: *const c_char,
    episode: *const c_char,
) -> RsBuf {
    let out = crate::dupefeed::build_feed_dupe_key(
        crate::diskstatepp::cstr_to_bytes(rage_id_str),
        crate::diskstatepp::cstr_to_bytes(tvdb_id_str),
        crate::diskstatepp::cstr_to_bytes(tvmaze_id_str),
        crate::diskstatepp::cstr_to_bytes(series),
        item_rage_id,
        item_tvdb_id,
        item_tvmaze_id,
        item_imdb_id,
        season_num,
        episode_num,
        crate::diskstatepp::cstr_to_bytes(season),
        crate::diskstatepp::cstr_to_bytes(episode),
    );
    into_buf(out)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_parse_feed_subject(title: *const c_char) -> RsBuf {
    into_buf(crate::dupefeed::parse_feed_subject(
        crate::diskstatepp::cstr_to_bytes(title),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_df_extract_size_from_description(
    description: *const c_char,
) -> i64 {
    crate::dupefeed::extract_size_from_description(crate::diskstatepp::cstr_to_bytes(description))
}

// ============================================================================
// StatMeter, WebProcessor, UrlCoordinator, DirectRenamer, NzbFile,
// QueueScript, PostScript, ScanScript, ScriptConfig & ScriptController FFI
// (`rust/src/scripturlstat.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_eval_interval_check(
    cur_time: i64,
    last_check: i64,
    last_time_offset: i64,
    start_download: i64,
    stand_by: c_int,
    out_clock_delta: *mut i64,
    out_adjust_download: *mut c_int,
    out_adjust_time_offset: *mut c_int,
) -> c_int {
    let (adj_clocks, delta, adj_dl, adj_tz) = crate::scripturlstat::eval_interval_check(
        cur_time,
        last_check,
        last_time_offset,
        start_download,
        stand_by != 0,
    );
    if !out_clock_delta.is_null() {
        *out_clock_delta = delta;
    }
    if !out_adjust_download.is_null() {
        *out_adjust_download = if adj_dl { 1 } else { 0 };
    }
    if !out_adjust_time_offset.is_null() {
        *out_adjust_time_offset = if adj_tz { 1 } else { 0 };
    }
    if adj_clocks {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_calc_total_stat(
    cur_time: i64,
    start_server: i64,
    start_download: i64,
    paused_from: i64,
    stand_by: c_int,
    out_up_time_sec: *mut c_int,
    out_dn_time_sec: *mut c_int,
) {
    let (up, dn) = crate::scripturlstat::calc_total_stat(
        cur_time,
        start_server,
        start_download,
        paused_from,
        stand_by != 0,
    );
    if !out_up_time_sec.is_null() {
        *out_up_time_sec = up;
    }
    if !out_dn_time_sec.is_null() {
        *out_dn_time_sec = dn;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_calc_current_download_speed(
    stand_by: c_int,
    cur_time: i64,
    speed_start_time: c_int,
    speed_total_bytes: i64,
) -> i64 {
    crate::scripturlstat::calc_current_download_speed(
        stand_by != 0,
        cur_time,
        speed_start_time,
        speed_total_bytes,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_calc_momentary_download_speed(
    cur_time: i64,
    cur_sec_time: i64,
    cur_sec_bytes: i64,
) -> i64 {
    crate::scripturlstat::calc_momentary_download_speed(cur_time, cur_sec_time, cur_sec_bytes)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_add_speed_reading(
    bytes: c_int,
    cur_time: i64,
    cur_sec_time: *mut i64,
    cur_sec_bytes: *mut i64,
    speed_bytes: *mut i32,
    speed_time: *mut i32,
    speed_bytes_index: *mut c_int,
    speed_total_bytes: *mut i64,
    speed_start_time: *mut c_int,
    speed_correction: *mut i64,
    all_bytes: *mut i64,
) {
    if cur_sec_time.is_null()
        || cur_sec_bytes.is_null()
        || speed_bytes.is_null()
        || speed_time.is_null()
        || speed_bytes_index.is_null()
        || speed_total_bytes.is_null()
        || speed_start_time.is_null()
        || speed_correction.is_null()
        || all_bytes.is_null()
    {
        return;
    }
    let sb = &mut *(speed_bytes as *mut [i32; crate::scripturlstat::SPEEDMETER_SLOTS]);
    let st = &mut *(speed_time as *mut [i32; crate::scripturlstat::SPEEDMETER_SLOTS]);
    crate::scripturlstat::add_speed_reading(
        bytes,
        cur_time,
        &mut *cur_sec_time,
        &mut *cur_sec_bytes,
        sb,
        st,
        &mut *speed_bytes_index,
        &mut *speed_total_bytes,
        &mut *speed_start_time,
        &mut *speed_correction,
        &mut *all_bytes,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_calc_quota_usage(
    loc_time: i64,
    first_day: c_int,
    bytes_per_days: *const i64,
    days_len: usize,
    elapsed_slots: c_int,
    out_month_bytes: *mut i64,
    out_day_bytes: *mut i64,
) {
    let slice = if bytes_per_days.is_null() || days_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(bytes_per_days, days_len)
    };
    let (month_b, day_b) =
        crate::scripturlstat::calc_quota_usage(loc_time, first_day, slice, elapsed_slots);
    if !out_month_bytes.is_null() {
        *out_month_bytes = month_b;
    }
    if !out_day_bytes.is_null() {
        *out_day_bytes = day_b;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_eval_quota_status(
    monthly_quota_mb: c_int,
    daily_quota_mb: c_int,
    month_bytes: i64,
    day_bytes: i64,
    was_quota_reached: c_int,
    out_action: *mut c_int,
) -> c_int {
    let (reached, action) = crate::scripturlstat::eval_quota_status(
        monthly_quota_mb,
        daily_quota_mb,
        month_bytes,
        day_bytes,
        was_quota_reached != 0,
    );
    if !out_action.is_null() {
        *out_action = action;
    }
    if reached {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_web_classify_get_dispatch(url: *const c_char) -> c_int {
    crate::scripturlstat::web_classify_get_dispatch(crate::diskstatepp::cstr_to_bytes(url))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_web_is_ignorable_404(base_name: *const c_char) -> c_int {
    if crate::scripturlstat::web_is_ignorable_404(crate::diskstatepp::cstr_to_bytes(base_name)) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_web_detect_content_type(
    filename: *const c_char,
) -> *const c_char {
    match crate::scripturlstat::web_detect_content_type(crate::diskstatepp::cstr_to_bytes(filename))
    {
        Some(ct) => ct.as_ptr() as *const c_char,
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_select_next_url(
    kinds: *const c_int,
    url_statuses: *const c_int,
    delete_statuses: *const c_int,
    priorities: *const c_int,
    count: usize,
    nk_url: c_int,
    ls_none: c_int,
    ds_none: c_int,
) -> c_int {
    if count == 0
        || kinds.is_null()
        || url_statuses.is_null()
        || delete_statuses.is_null()
        || priorities.is_null()
    {
        return -1;
    }
    crate::scripturlstat::select_next_url(
        std::slice::from_raw_parts(kinds, count),
        std::slice::from_raw_parts(url_statuses, count),
        std::slice::from_raw_parts(delete_statuses, count),
        std::slice::from_raw_parts(priorities, count),
        nk_url,
        ls_none,
        ds_none,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_is_hanging_url_download(
    now: i64,
    last_update_time: i64,
    url_timeout: c_int,
    status: c_int,
    ad_running: c_int,
) -> c_int {
    if crate::scripturlstat::is_hanging_url_download(
        now,
        last_update_time,
        url_timeout,
        status,
        ad_running,
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_eval_direct_rename_article(
    article_size: c_int,
    article_count: usize,
    old_par_file: c_int,
    new_par_file: c_int,
    file_paused: c_int,
    file_size: i64,
    file_success_size: i64,
    file_failed_size: i64,
    file_missed_size: i64,
    par_size: i64,
    par_current_success_size: i64,
    par_current_failed_size: i64,
    par_failed_size: i64,
    remaining_par_count: c_int,
    out_delta: *mut crate::scripturlstat::RsDirectRenameParDelta,
) {
    if out_delta.is_null() {
        return;
    }
    *out_delta = crate::scripturlstat::eval_direct_rename_article(
        article_size,
        article_count,
        old_par_file != 0,
        new_par_file != 0,
        file_paused != 0,
        file_size,
        file_success_size,
        file_failed_size,
        file_missed_size,
        par_size,
        par_current_success_size,
        par_current_failed_size,
        par_failed_size,
        remaining_par_count,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_select_pars_to_unpause(
    ids: *const c_int,
    set_ids: *const *const c_char,
    completed: *const u8,
    in_file_list: *const u8,
    sizes: *const i64,
    count: usize,
    out_file_ids: *mut c_int,
) -> usize {
    if count == 0
        || ids.is_null()
        || set_ids.is_null()
        || completed.is_null()
        || in_file_list.is_null()
        || sizes.is_null()
        || out_file_ids.is_null()
    {
        return 0;
    }
    let mut set_id_slices: Vec<&[u8]> = Vec::with_capacity(count);
    for i in 0..count {
        set_id_slices.push(crate::diskstatepp::cstr_to_bytes(*set_ids.add(i)));
    }
    crate::scripturlstat::select_pars_to_unpause(
        std::slice::from_raw_parts(ids, count),
        &set_id_slices,
        std::slice::from_raw_parts(completed, count),
        std::slice::from_raw_parts(in_file_list, count),
        std::slice::from_raw_parts(sizes, count),
        std::slice::from_raw_parts_mut(out_file_ids, count),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_extract_password_from_filename(
    filename: *const c_char,
    out_start: *mut usize,
    out_len: *mut usize,
) -> c_int {
    let bytes = crate::diskstatepp::cstr_to_bytes(filename);
    if let Some(pw) = crate::scripturlstat::extract_password_from_filename(bytes) {
        let start = (pw.as_ptr() as usize) - (bytes.as_ptr() as usize);
        if !out_start.is_null() {
            *out_start = start;
        }
        if !out_len.is_null() {
            *out_len = pw.len();
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_sanitize_segment_message_id(
    tag_content: *const c_char,
) -> RsBuf {
    into_buf(crate::scripturlstat::sanitize_segment_message_id(
        crate::diskstatepp::cstr_to_bytes(tag_content),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_calc_nzb_hashes(
    skip_flags: *const u8,
    article_counts: *const usize,
    file_count: usize,
    message_ids: *const *const c_char,
    total_articles: usize,
    out_full_hash: *mut u32,
    out_filtered_hash: *mut u32,
) {
    let mut msg_slices: Vec<&[u8]> = Vec::with_capacity(total_articles);
    if !message_ids.is_null() {
        for i in 0..total_articles {
            msg_slices.push(crate::diskstatepp::cstr_to_bytes(*message_ids.add(i)));
        }
    }
    let skips = if skip_flags.is_null() || file_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(skip_flags, file_count)
    };
    let counts = if article_counts.is_null() || file_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(article_counts, file_count)
    };
    let (full, filt) =
        crate::scripturlstat::calc_nzb_hashes(skips, counts, &msg_slices);
    if !out_full_hash.is_null() {
        *out_full_hash = full;
    }
    if !out_filtered_hash.is_null() {
        *out_filtered_hash = filt;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_parse_queue_script_cmd(
    msg_text: *const c_char,
    event: c_int,
    qe_nzb_downloaded: c_int,
    out_cmd: *mut crate::scripturlstat::RsQueueScriptCmd,
) -> c_int {
    let cmd = crate::scripturlstat::parse_queue_script_cmd(
        crate::diskstatepp::cstr_to_bytes(msg_text),
        event,
        qe_nzb_downloaded,
    );
    let kind = cmd.kind;
    if !out_cmd.is_null() {
        *out_cmd = cmd;
    }
    kind
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_should_skip_queue_event_interval(
    event: c_int,
    qe_file_downloaded: c_int,
    event_interval: c_int,
    cur_time: i64,
    queue_script_time: i64,
) -> c_int {
    if crate::scripturlstat::should_skip_queue_event_interval(
        event,
        qe_file_downloaded,
        event_interval,
        cur_time,
        queue_script_time,
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_is_usable_queue_script(
    is_queue_script: c_int,
    queue_events: *const c_char,
    script_name: *const c_char,
    event: c_int,
    qe_url_completed: c_int,
    param_names: *const *const c_char,
    param_values: *const *const c_char,
    param_count: usize,
    url_extensions: *const c_char,
) -> c_int {
    let mut pnames: Vec<&[u8]> = Vec::with_capacity(param_count);
    let mut pvals: Vec<&[u8]> = Vec::with_capacity(param_count);
    if !param_names.is_null() && !param_values.is_null() {
        for i in 0..param_count {
            pnames.push(crate::diskstatepp::cstr_to_bytes(*param_names.add(i)));
            pvals.push(crate::diskstatepp::cstr_to_bytes(*param_values.add(i)));
        }
    }
    if crate::scripturlstat::is_usable_queue_script(
        is_queue_script != 0,
        crate::diskstatepp::cstr_to_bytes(queue_events),
        crate::diskstatepp::cstr_to_bytes(script_name),
        event,
        qe_url_completed,
        &pnames,
        &pvals,
        crate::diskstatepp::cstr_to_bytes(url_extensions),
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_should_ignore_queued_script_event(
    has_nzb_info: c_int,
    delete_status: c_int,
    ds_none: c_int,
    mark_status: c_int,
    ks_bad: c_int,
    event: c_int,
    qe_nzb_deleted: c_int,
    qe_nzb_marked: c_int,
) -> c_int {
    if crate::scripturlstat::should_ignore_queued_script_event(
        has_nzb_info != 0,
        delete_status,
        ds_none,
        mark_status,
        ks_bad,
        event,
        qe_nzb_deleted,
        qe_nzb_marked,
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_build_enabled_script_comma_list(
    param_names: *const *const c_char,
    param_values: *const *const c_char,
    param_count: usize,
) -> RsBuf {
    let mut pnames: Vec<&[u8]> = Vec::with_capacity(param_count);
    let mut pvals: Vec<&[u8]> = Vec::with_capacity(param_count);
    if !param_names.is_null() && !param_values.is_null() {
        for i in 0..param_count {
            pnames.push(crate::diskstatepp::cstr_to_bytes(*param_names.add(i)));
            pvals.push(crate::diskstatepp::cstr_to_bytes(*param_values.add(i)));
        }
    }
    into_buf(crate::scripturlstat::build_enabled_script_comma_list(
        &pnames, &pvals,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_analyse_post_script_exit_code(
    exit_code: c_int,
    par_status: c_int,
    ps_skipped: c_int,
    parcheck_enabled: c_int,
    sr_none: c_int,
    sr_failure: c_int,
    sr_success: c_int,
    out_message_action: *mut c_int,
    out_request_par_check: *mut c_int,
) -> c_int {
    let (status, action, req_par) = crate::scripturlstat::analyse_post_script_exit_code(
        exit_code,
        par_status,
        ps_skipped,
        parcheck_enabled != 0,
        sr_none,
        sr_failure,
        sr_success,
    );
    if !out_message_action.is_null() {
        *out_message_action = action;
    }
    if !out_request_par_check.is_null() {
        *out_request_par_check = if req_par { 1 } else { 0 };
    }
    status
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_parse_post_script_cmd(
    msg_text: *const c_char,
    out_cmd: *mut crate::scripturlstat::RsPostScriptCmd,
) -> c_int {
    let cmd = crate::scripturlstat::parse_post_script_cmd(crate::diskstatepp::cstr_to_bytes(
        msg_text,
    ));
    let kind = cmd.kind;
    if !out_cmd.is_null() {
        *out_cmd = cmd;
    }
    kind
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_parse_scan_script_cmd(
    msg_text: *const c_char,
    out_cmd: *mut crate::scripturlstat::RsScanScriptCmd,
) -> c_int {
    let cmd = crate::scripturlstat::parse_scan_script_cmd(crate::diskstatepp::cstr_to_bytes(
        msg_text,
    ));
    let kind = cmd.kind;
    if !out_cmd.is_null() {
        *out_cmd = cmd;
    }
    kind
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_build_cipher_baseline(
    names: *const *const c_char,
    values: *const *const c_char,
    count: usize,
) -> RsBuf {
    let mut name_slices: Vec<&[u8]> = Vec::with_capacity(count);
    let mut val_slices: Vec<&[u8]> = Vec::with_capacity(count);
    if !names.is_null() && !values.is_null() {
        for i in 0..count {
            name_slices.push(crate::diskstatepp::cstr_to_bytes(*names.add(i)));
            val_slices.push(crate::diskstatepp::cstr_to_bytes(*values.add(i)));
        }
    }
    into_buf(crate::scripturlstat::build_cipher_baseline(
        &name_slices,
        &val_slices,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_is_nzbget_internal_env_var(var: *const c_char) -> c_int {
    if crate::scripturlstat::is_nzbget_internal_env_var(crate::diskstatepp::cstr_to_bytes(var)) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_normalize_special_env_var_name(
    varname: *const c_char,
) -> RsBuf {
    into_buf(crate::scripturlstat::normalize_special_env_var_name(
        crate::diskstatepp::cstr_to_bytes(varname),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_classify_script_output_line(
    line: *mut c_char,
    out_kind: *mut c_int,
    out_offset: *mut usize,
) -> c_int {
    if line.is_null() {
        return 0;
    }
    let bytes = crate::diskstatepp::cstr_to_bytes(line);
    let (trimmed_len, kind, offset) = crate::scripturlstat::classify_script_output_line(bytes);
    *line.add(trimmed_len) = 0;
    if trimmed_len == 0 {
        return 0;
    }
    if !out_kind.is_null() {
        *out_kind = kind;
    }
    if !out_offset.is_null() {
        *out_offset = offset;
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_sus_check_env_size_limit(
    env_vars: *const *const c_char,
    count: usize,
    ptr_size: usize,
    limit: usize,
    out_exceeded_total: *mut usize,
) -> c_int {
    if env_vars.is_null() || count == 0 || limit == 0 {
        return 0;
    }
    let mut lens: Vec<usize> = Vec::with_capacity(count);
    for i in 0..count {
        let p = *env_vars.add(i);
        if !p.is_null() {
            lens.push(crate::diskstatepp::cstr_to_bytes(p).len());
        }
    }
    if let Some(total) = crate::scripturlstat::check_env_size_limit(&lens, ptr_size, limit) {
        if !out_exceeded_total.is_null() {
            *out_exceeded_total = total;
        }
        1
    } else {
        0
    }
}

// ============================================================================
// XmlRpc, RemoteServer, Connection, WebDownloader, Options, Cleanup, Rename,
// ArchiveProcessor, Log & NString hot-path FFI (rpcconnopt.rs)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rpc_is_rpc_request(url: *const c_char) -> c_int {
    if crate::rpcconnopt::rpc_is_rpc_request(crate::diskstatepp::cstr_to_bytes(url)) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rpc_log_slice_range(
    messages_len: c_int,
    front_id: c_int,
    id_from: c_int,
    nr_entries: c_int,
    out_start: *mut c_int,
    out_nr_entries: *mut c_int,
) {
    let (start, eff) =
        crate::rpcconnopt::rpc_log_slice_range(messages_len, front_id, id_from, nr_entries);
    if !out_start.is_null() {
        *out_start = start;
    }
    if !out_nr_entries.is_null() {
        *out_nr_entries = eff;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rpc_list_files_match_and_progress(
    nzb_id: c_int,
    file_nzb_id: c_int,
    file_id: c_int,
    id_start: c_int,
    id_end: c_int,
    size: i64,
    remaining_size: i64,
    missed_size: i64,
    failed_size: i64,
    success_size: i64,
    out_progress: *mut c_int,
) -> c_int {
    if let Some(progress) = crate::rpcconnopt::rpc_list_files_match_and_progress(
        nzb_id,
        file_nzb_id,
        file_id,
        id_start,
        id_end,
        size,
        remaining_size,
        missed_size,
        failed_size,
        success_size,
    ) {
        if !out_progress.is_null() {
            *out_progress = progress;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rpc_detect_group_status(
    has_post_info: c_int,
    post_stage: c_int,
    has_queue_script_job: c_int,
    queue_script_active: c_int,
    direct_unpack_status: c_int,
    active_downloads: c_int,
    kind: c_int,
    paused_size: i64,
    remaining_size: i64,
) -> *const c_char {
    crate::rpcconnopt::rpc_detect_group_status(
        has_post_info != 0,
        post_stage,
        has_queue_script_job != 0,
        queue_script_active != 0,
        direct_unpack_status,
        active_downloads,
        kind,
        paused_size,
        remaining_size,
    )
    .as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rpc_lookup_edit_command(
    edit_command: *const c_char,
) -> c_int {
    crate::rpcconnopt::rpc_lookup_edit_command(crate::diskstatepp::cstr_to_bytes(edit_command))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rpc_validate_edit_queue_args(
    action: c_int,
    args: *const c_char,
    parsed_int_ok: c_int,
) -> c_int {
    if crate::rpcconnopt::rpc_validate_edit_queue_args(
        action,
        crate::diskstatepp::cstr_to_bytes(args),
        parsed_int_ok != 0,
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rpc_parse_write_log_kind(kind: *const c_char) -> c_int {
    crate::rpcconnopt::rpc_parse_write_log_kind(crate::diskstatepp::cstr_to_bytes(kind))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rpc_detect_history_dup_status(
    dup_status: c_int,
) -> *const c_char {
    crate::rpcconnopt::rpc_detect_history_dup_status(dup_status).as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rpc_parse_update_branch(
    branch_name: *const c_char,
) -> c_int {
    crate::rpcconnopt::rpc_parse_update_branch(crate::diskstatepp::cstr_to_bytes(branch_name))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rpc_save_config_parse_bool(
    val: *const c_char,
    def_val: c_int,
) -> c_int {
    let opt = if val.is_null() {
        None
    } else {
        Some(crate::diskstatepp::cstr_to_bytes(val))
    };
    if crate::rpcconnopt::rpc_save_config_parse_bool(opt, def_val != 0) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rpc_save_config_check_global_conflicts(
    dupe_search_val: *const c_char,
    health_check_val: *const c_char,
    flush_queue_val: *const c_char,
    direct_write_val: *const c_char,
    unpack_pause_queue_val: *const c_char,
    direct_unpack_val: *const c_char,
) -> c_int {
    let opt = |p: *const c_char| {
        if p.is_null() {
            None
        } else {
            Some(crate::diskstatepp::cstr_to_bytes(p))
        }
    };
    crate::rpcconnopt::rpc_save_config_check_global_conflicts(
        opt(dupe_search_val),
        opt(health_check_val),
        opt(flush_queue_val),
        opt(direct_write_val),
        opt(unpack_pause_queue_val),
        opt(direct_unpack_val),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_remote_classify_signature(
    sig_ptr: *const c_char,
    nzb_magic_be: u32,
) -> c_int {
    if sig_ptr.is_null() {
        return 0;
    }
    let slice = std::slice::from_raw_parts(sig_ptr.cast::<u8>(), 4);
    let arr: [u8; 4] = [slice[0], slice[1], slice[2], slice[3]];
    crate::rpcconnopt::remote_classify_signature(&arr, nzb_magic_be)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_remote_parse_web_request_line(
    sig_ptr: *const c_char,
    line: *const c_char,
    out_http_method: *mut c_int,
    out_url_offset: *mut usize,
    out_url_len: *mut usize,
) -> c_int {
    if sig_ptr.is_null() || line.is_null() {
        return 0;
    }
    let slice = std::slice::from_raw_parts(sig_ptr.cast::<u8>(), 4);
    let arr: [u8; 4] = [slice[0], slice[1], slice[2], slice[3]];
    let line_bytes = crate::diskstatepp::cstr_to_bytes(line);
    if let Some((method, offset, len)) =
        crate::rpcconnopt::remote_parse_web_request_line(&arr, line_bytes)
    {
        if !out_http_method.is_null() {
            *out_http_method = method;
        }
        if !out_url_offset.is_null() {
            *out_url_offset = offset;
        }
        if !out_url_len.is_null() {
            *out_url_len = len;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_conn_read_line_chunk(
    buf_ptr: *const c_char,
    buf_avail: c_int,
    max_out: c_int,
    out_found_newline: *mut c_int,
) -> c_int {
    if buf_ptr.is_null() || buf_avail <= 0 || max_out <= 0 {
        if !out_found_newline.is_null() {
            *out_found_newline = 0;
        }
        return 0;
    }
    let slice = std::slice::from_raw_parts(buf_ptr.cast::<u8>(), buf_avail as usize);
    let (copy_len, found) = crate::rpcconnopt::conn_read_line_chunk(slice, max_out as usize);
    if !out_found_newline.is_null() {
        *out_found_newline = if found { 1 } else { 0 };
    }
    copy_len as c_int
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_conn_classify_accept_errno(
    err: c_int,
    eintr: c_int,
    econnaborted: c_int,
    eproto: c_int,
    enetdown: c_int,
    enoprotoopt: c_int,
    ehostdown: c_int,
    ehostunreach: c_int,
    eopnotsupp: c_int,
    enetunreach: c_int,
    enonet: c_int,
    emfile: c_int,
    enfile: c_int,
    enobufs: c_int,
    enomem: c_int,
) -> c_int {
    crate::rpcconnopt::conn_classify_accept_errno(
        err,
        eintr,
        econnaborted,
        eproto,
        enetdown,
        enoprotoopt,
        ehostdown,
        ehostunreach,
        eopnotsupp,
        enetunreach,
        enonet,
        emfile,
        enfile,
        enobufs,
        enomem,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_webdl_resolve_protocol_port(
    is_valid: c_int,
    protocol: *const c_char,
    port: c_int,
    tls_disabled: c_int,
    out_port: *mut c_int,
    out_tls: *mut c_int,
) -> c_int {
    let (code, resolved_port, is_tls) = crate::rpcconnopt::webdl_resolve_protocol_port(
        is_valid != 0,
        crate::diskstatepp::cstr_to_bytes(protocol),
        port,
        tls_disabled != 0,
    );
    if !out_port.is_null() {
        *out_port = resolved_port;
    }
    if !out_tls.is_null() {
        *out_tls = if is_tls { 1 } else { 0 };
    }
    code
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_webdl_check_body_chunk(
    chunk_len: c_int,
    content_len: c_int,
    written_len: c_int,
    gzip: c_int,
    confirmed_length: c_int,
) -> c_int {
    crate::rpcconnopt::webdl_check_body_chunk(
        chunk_len,
        content_len,
        written_len,
        gzip != 0,
        confirmed_length != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_options_find_var_reference(
    s: *const c_char,
    out_dollar_pos: *mut usize,
    out_var_len: *mut usize,
) -> c_int {
    if let Some((pos, len)) =
        crate::rpcconnopt::options_find_var_reference(crate::diskstatepp::cstr_to_bytes(s))
    {
        if !out_dollar_pos.is_null() {
            *out_dollar_pos = pos;
        }
        if !out_var_len.is_null() {
            *out_var_len = len;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_options_trim_and_check_config_line(
    line: *mut c_char,
) -> c_int {
    if line.is_null() {
        return 1;
    }
    let bytes = crate::diskstatepp::cstr_to_bytes(line);
    let (new_len, skip) = crate::rpcconnopt::options_trim_and_check_config_line(bytes);
    *line.add(new_len) = 0;
    if skip { 1 } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_options_lookup_scheduler_command(
    cmd: *const c_char,
) -> c_int {
    crate::rpcconnopt::options_lookup_scheduler_command(crate::diskstatepp::cstr_to_bytes(cmd))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_options_validate_scheduler_param(
    task_command: c_int,
    param: *const c_char,
    split_cmd_empty: c_int,
) -> c_int {
    let opt = if param.is_null() {
        None
    } else {
        Some(crate::diskstatepp::cstr_to_bytes(param))
    };
    crate::rpcconnopt::options_validate_scheduler_param(task_command, opt, split_cmd_empty != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_move_controller_decide_outcome(
    ok: c_int,
    is_stopped: c_int,
    out_move_status: *mut c_int,
) -> c_int {
    let (action, status) =
        crate::rpcconnopt::move_controller_decide_outcome(ok != 0, is_stopped != 0);
    if !out_move_status.is_null() {
        *out_move_status = status;
    }
    action
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_cleanup_controller_decide_outcome(
    ok: c_int,
    deleted: c_int,
    out_cleanup_status: *mut c_int,
) -> c_int {
    let (action, status) =
        crate::rpcconnopt::cleanup_controller_decide_outcome(ok != 0, deleted != 0);
    if !out_cleanup_status.is_null() {
        *out_cleanup_status = status;
    }
    action
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_rename_controller_complete_action(
    kind: c_int,
    renamed_count: c_int,
    has_damaged_pars: c_int,
    remaining_par_count: c_int,
    has_missed_files: c_int,
    par_status: c_int,
    out_request_extra_pars: *mut c_int,
    out_request_par_check: *mut c_int,
) -> c_int {
    let (initial_status, req_extra, req_par) =
        crate::rpcconnopt::rename_controller_complete_action(
            kind,
            renamed_count,
            has_damaged_pars != 0,
            remaining_par_count,
            has_missed_files != 0,
            par_status,
        );
    if !out_request_extra_pars.is_null() {
        *out_request_extra_pars = if req_extra { 1 } else { 0 };
    }
    if !out_request_par_check.is_null() {
        *out_request_par_check = if req_par { 1 } else { 0 };
    }
    initial_status
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_archive_is_nzb_extension(ext: *const c_char) -> c_int {
    if crate::rpcconnopt::archive_is_nzb_extension(crate::diskstatepp::cstr_to_bytes(ext)) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_archive_decide_dispose_action(
    config_action: c_int,
    non_nzb_file_count: c_int,
) -> c_int {
    crate::rpcconnopt::archive_decide_dispose_action(config_action, non_nzb_file_count)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_log_should_rotate(
    rawtime: i64,
    last_written: i64,
    write_log_mode: c_int,
) -> c_int {
    if crate::rpcconnopt::log_should_rotate(rawtime, last_written, write_log_mode) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_log_init_message_disposition(
    target: c_int,
    out_write_to_file: *mut c_int,
    out_keep_in_screen: *mut c_int,
) {
    let (write_to_file, keep_in_screen) =
        crate::rpcconnopt::log_init_message_disposition(target);
    if !out_write_to_file.is_null() {
        *out_write_to_file = if write_to_file { 1 } else { 0 };
    }
    if !out_keep_in_screen.is_null() {
        *out_keep_in_screen = if keep_in_screen { 1 } else { 0 };
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_wstring_decode_utf8_to_utf16(
    utf8: *const c_char,
    out: *mut u16,
    out_cap: usize,
) -> usize {
    if utf8.is_null() || out.is_null() || out_cap == 0 {
        return 0;
    }
    let bytes = crate::diskstatepp::cstr_to_bytes(utf8);
    let out_slice = std::slice::from_raw_parts_mut(out, out_cap);
    crate::rpcconnopt::wstring_decode_utf8_to_utf16(bytes, out_slice)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_rco_string_builder_calc_reserve_capacity(
    old_capacity: c_int,
    requested_capacity: c_int,
    exact: c_int,
) -> c_int {
    crate::rpcconnopt::string_builder_calc_reserve_capacity(
        old_capacity,
        requested_capacity,
        exact != 0,
    )
}

// ============================================================================
// Frontend, NCurses, Maintenance, ExtensionManager, NntpServer & SystemHealth FFI
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_frontend_is_any_ip(control_ip: *const c_char) -> c_int {
    if control_ip.is_null() {
        return 0;
    }
    let bytes = crate::diskstatepp::cstr_to_bytes(control_ip);
    c_int::from(crate::frontexthealth::frontend_is_any_ip(bytes))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_frontend_calc_log_start_index(
    needed_first_id: c_int,
    first_msg_id: c_int,
    out_skip: *mut c_int,
) -> c_int {
    let (start, skip) =
        crate::frontexthealth::frontend_calc_log_start_index(needed_first_id, first_msg_id);
    if !out_skip.is_null() {
        *out_skip = c_int::from(skip);
    }
    start
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_frontend_calc_remaining_hms(
    remaining_size: i64,
    current_speed: i64,
    stand_by: c_int,
    pause_download: c_int,
    out_h: *mut c_int,
    out_m: *mut c_int,
    out_s: *mut c_int,
) -> c_int {
    match crate::frontexthealth::frontend_calc_remaining_hms(
        remaining_size,
        current_speed,
        stand_by != 0,
        pause_download != 0,
    ) {
        Some((h, m, s)) => {
            if !out_h.is_null() {
                *out_h = h;
            }
            if !out_m.is_null() {
                *out_m = m;
            }
            if !out_s.is_null() {
                *out_s = s;
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_ncurses_calc_window_sizes(
    nr_rows: c_int,
    nr_columns: c_int,
    max_screen_width: c_int,
    queue_win_pct: c_int,
    queue_size: c_int,
    out_clamped_cols: *mut c_int,
    out_queue_win_height: *mut c_int,
    out_queue_win_client_height: *mut c_int,
    out_messages_win_top: *mut c_int,
    out_messages_win_height: *mut c_int,
    out_messages_win_client_height: *mut c_int,
) {
    let (cols, qh, qch, mtop, mh, mch) = crate::frontexthealth::ncurses_calc_window_sizes(
        nr_rows,
        nr_columns,
        max_screen_width,
        queue_win_pct,
        queue_size,
    );
    if !out_clamped_cols.is_null() {
        *out_clamped_cols = cols;
    }
    if !out_queue_win_height.is_null() {
        *out_queue_win_height = qh;
    }
    if !out_queue_win_client_height.is_null() {
        *out_queue_win_client_height = qch;
    }
    if !out_messages_win_top.is_null() {
        *out_messages_win_top = mtop;
    }
    if !out_messages_win_height.is_null() {
        *out_messages_win_height = mh;
    }
    if !out_messages_win_client_height.is_null() {
        *out_messages_win_client_height = mch;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_ncurses_calc_scroll_and_selection(
    entry: c_int,
    queue_size: c_int,
    scroll_offset: c_int,
    client_height: c_int,
    out_selected_entry: *mut c_int,
    out_scroll_offset: *mut c_int,
) {
    let (sel, off) = crate::frontexthealth::ncurses_calc_scroll_and_selection(
        entry,
        queue_size,
        scroll_offset,
        client_height,
    );
    if !out_selected_entry.is_null() {
        *out_selected_entry = sel;
    }
    if !out_scroll_offset.is_null() {
        *out_scroll_offset = off;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_ncurses_calc_top_header_time(
    time_sec: c_int,
    out_d: *mut c_int,
    out_h: *mut c_int,
    out_m: *mut c_int,
    out_s: *mut c_int,
) {
    let (d, h, m, s) = crate::frontexthealth::ncurses_calc_top_header_time(time_sec);
    if !out_d.is_null() {
        *out_d = d;
    }
    if !out_h.is_null() {
        *out_h = h;
    }
    if !out_m.is_null() {
        *out_m = m;
    }
    if !out_s.is_null() {
        *out_s = s;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_ncurses_resolve_group_pause_action(
    action: c_int,
    remaining_size: i64,
    paused_size: i64,
    remaining_par_count: c_int,
    last_pause_pars: c_int,
    last_edit_entry: c_int,
    selected_entry: c_int,
    out_last_pause_pars: *mut c_int,
) -> c_int {
    let (resolved, new_last) = crate::frontexthealth::ncurses_resolve_group_pause_action(
        action,
        remaining_size,
        paused_size,
        remaining_par_count,
        last_pause_pars != 0,
        last_edit_entry,
        selected_entry,
    );
    if !out_last_pause_pars.is_null() {
        *out_last_pause_pars = c_int::from(new_last);
    }
    resolved
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_ncurses_step_rate_input(
    key: c_int,
    key_backspace: c_int,
    inout_index: *mut c_int,
    inout_value: *mut c_int,
) -> c_int {
    if inout_index.is_null() || inout_value.is_null() {
        return 0;
    }
    let (step, new_idx, new_val) = crate::frontexthealth::ncurses_step_rate_input(
        key,
        key_backspace,
        *inout_index,
        *inout_value,
    );
    *inout_index = new_idx;
    *inout_value = new_val;
    step
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_maintenance_extract_package_info_str(
    json: *const c_char,
    key: *const c_char,
    max_len: usize,
    out_start: *mut usize,
    out_len: *mut usize,
) -> c_int {
    if json.is_null() || key.is_null() {
        return 0;
    }
    let json_bytes = crate::diskstatepp::cstr_to_bytes(json);
    let key_bytes = crate::diskstatepp::cstr_to_bytes(key);
    match crate::frontexthealth::maintenance_extract_package_info_str(
        json_bytes,
        key_bytes,
        max_len,
    ) {
        Some((start, len)) => {
            if !out_start.is_null() {
                *out_start = start;
            }
            if !out_len.is_null() {
                *out_len = len;
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_maintenance_classify_update_message(
    text: *const c_char,
) -> c_int {
    if text.is_null() {
        return 0;
    }
    let bytes = crate::diskstatepp::cstr_to_bytes(text);
    crate::frontexthealth::maintenance_classify_update_message(bytes)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_maintenance_classify_update_info_message(
    text: *const c_char,
) -> c_int {
    if text.is_null() {
        return 0;
    }
    let bytes = crate::diskstatepp::cstr_to_bytes(text);
    crate::frontexthealth::maintenance_classify_update_info_message(bytes)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_maintenance_decode_hex_signature(
    hex_sig: *const c_char,
    out_sig: *mut u8,
) -> c_int {
    if hex_sig.is_null() || out_sig.is_null() {
        return 0;
    }
    let bytes = crate::diskstatepp::cstr_to_bytes(hex_sig);
    let out_arr = &mut *out_sig.cast::<[u8; 256]>();
    c_int::from(crate::frontexthealth::maintenance_decode_hex_signature(
        bytes, out_arr,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_ext_should_skip_dir_entry(
    filename: *const c_char,
) -> c_int {
    if filename.is_null() {
        return 1;
    }
    let bytes = crate::diskstatepp::cstr_to_bytes(filename);
    c_int::from(crate::frontexthealth::ext_should_skip_dir_entry(bytes))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_ext_get_stem_len(
    filename: *const c_char,
    filename_len: usize,
) -> usize {
    if filename.is_null() || filename_len == 0 {
        return 0;
    }
    let bytes = std::slice::from_raw_parts(filename.cast::<u8>(), filename_len);
    crate::frontexthealth::ext_get_stem_len(bytes)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_ext_is_in_list(
    extensions_list: *const c_char,
    target_name: *const c_char,
) -> c_int {
    if extensions_list.is_null() || target_name.is_null() {
        return 0;
    }
    let list_bytes = crate::diskstatepp::cstr_to_bytes(extensions_list);
    let name_bytes = crate::diskstatepp::cstr_to_bytes(target_name);
    c_int::from(crate::frontexthealth::ext_is_in_list(
        list_bytes, name_bytes,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_nntp_classify_command(
    line: *const c_char,
    out_arg_offset: *mut usize,
) -> c_int {
    if line.is_null() {
        return 0;
    }
    let bytes = crate::diskstatepp::cstr_to_bytes(line);
    let (cmd, off) = crate::frontexthealth::nntp_classify_command(bytes);
    if !out_arg_offset.is_null() {
        *out_arg_offset = off;
    }
    cmd
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_nntp_parse_message_id(
    msg_id: *const c_char,
    out_name_len: *mut c_int,
    out_part: *mut c_int,
    out_offset: *mut i64,
    out_size: *mut c_int,
    out_serv_offset: *mut c_int,
) -> c_int {
    if msg_id.is_null() {
        return 0;
    }
    let bytes = crate::diskstatepp::cstr_to_bytes(msg_id);
    match crate::frontexthealth::nntp_parse_message_id(bytes) {
        Some((name_len, part, offset, size, serv_off)) => {
            if !out_name_len.is_null() {
                *out_name_len = name_len;
            }
            if !out_part.is_null() {
                *out_part = part;
            }
            if !out_offset.is_null() {
                *out_offset = offset;
            }
            if !out_size.is_null() {
                *out_size = size;
            }
            if !out_serv_offset.is_null() {
                *out_serv_offset = serv_off;
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_nntp_server_in_list(
    serv_list: *const c_char,
    server_id: c_int,
) -> c_int {
    if serv_list.is_null() {
        return 0;
    }
    let bytes = crate::diskstatepp::cstr_to_bytes(serv_list);
    c_int::from(crate::frontexthealth::nntp_server_in_list(bytes, server_id))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_nntp_calc_send_chunks(
    size: c_int,
    speed: c_int,
    elapsed_ticks: i64,
    out_expected_time: *mut i64,
    out_chunk_num: *mut c_int,
    out_chunk_size: *mut c_int,
    out_pause: *mut c_int,
) {
    let (exp, cnum, csize, pause) =
        crate::frontexthealth::nntp_calc_send_chunks(size, speed, elapsed_ticks);
    if !out_expected_time.is_null() {
        *out_expected_time = exp;
    }
    if !out_chunk_num.is_null() {
        *out_chunk_num = cnum;
    }
    if !out_chunk_size.is_null() {
        *out_chunk_size = csize;
    }
    if !out_pause.is_null() {
        *out_pause = pause;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_health_check_password(
    password: *const c_char,
    password_len: usize,
) -> c_int {
    let bytes = if password.is_null() || password_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(password.cast::<u8>(), password_len)
    };
    crate::frontexthealth::health_check_password(bytes)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_health_valid_port(port: c_int) -> c_int {
    c_int::from(crate::frontexthealth::health_valid_port(port))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_health_validate_control_ip(
    ip: *const c_char,
    ip_len: usize,
    is_win32: c_int,
) -> c_int {
    let bytes = if ip.is_null() || ip_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(ip.cast::<u8>(), ip_len)
    };
    crate::frontexthealth::health_validate_control_ip(bytes, is_win32 != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_health_validate_user_pass_pair(
    user: *const c_char,
    user_len: usize,
    pass: *const c_char,
    pass_len: usize,
) -> c_int {
    let u = if user.is_null() || user_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(user.cast::<u8>(), user_len)
    };
    let p = if pass.is_null() || pass_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(pass.cast::<u8>(), pass_len)
    };
    crate::frontexthealth::health_validate_user_pass_pair(u, p)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_health_validate_form_auth(
    form_auth: c_int,
    secure_control: c_int,
    no_control_pass: c_int,
    no_add_user: c_int,
    no_restricted_user: c_int,
) -> c_int {
    crate::frontexthealth::health_validate_form_auth(
        form_auth != 0,
        secure_control != 0,
        no_control_pass != 0,
        no_add_user != 0,
        no_restricted_user != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_health_validate_article_cache(
    val: c_int,
    ptr_size: usize,
    direct_write: c_int,
) -> c_int {
    crate::frontexthealth::health_validate_article_cache(val, ptr_size, direct_write != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_health_validate_write_buffer(val: c_int) -> c_int {
    crate::frontexthealth::health_validate_write_buffer(val)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_health_validate_server_port(
    active: c_int,
    port: c_int,
    tls: c_int,
) -> c_int {
    crate::frontexthealth::health_validate_server_port(active != 0, port, tls != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_health_validate_server_connections(
    active: c_int,
    max_conn: c_int,
    dupe_search: c_int,
) -> c_int {
    crate::frontexthealth::health_validate_server_connections(
        active != 0,
        max_conn,
        dupe_search != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_health_validate_server_retention(
    active: c_int,
    retention: c_int,
) -> c_int {
    crate::frontexthealth::health_validate_server_retention(active != 0, retention)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feh_health_validate_article_read_chunk_size(
    bytes: c_int,
    out_kb_val: *mut c_int,
) -> c_int {
    let (code, kb) = crate::frontexthealth::health_validate_article_read_chunk_size(bytes);
    if !out_kb_val.is_null() {
        *out_kb_val = kb;
    }
    code
}

// ============================================================================
// System, NServ, DupeSearch, Connect, PostRenamer & SystemHealth Hot-Path FFI
// (`rust/src/sysnservhealth.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_sys_extract_python_version(
    line: *const c_char,
    len: usize,
    out_start: *mut usize,
    out_len: *mut usize,
) -> c_int {
    let s = if line.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(line.cast::<u8>(), len)
    };
    match crate::sysnservhealth::sys_extract_python_version(s) {
        Some((st, ln)) => {
            if !out_start.is_null() {
                *out_start = st;
            }
            if !out_len.is_null() {
                *out_len = ln;
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_sys_parse_unpacker_version(
    line: *const c_char,
    len: usize,
    out_start: *mut usize,
    out_len: *mut usize,
) -> c_int {
    let s = if line.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(line.cast::<u8>(), len)
    };
    match crate::sysnservhealth::sys_parse_unpacker_version(s) {
        Some((st, ln)) => {
            if !out_start.is_null() {
                *out_start = st;
            }
            if !out_len.is_null() {
                *out_len = ln;
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_sys_parse_par_version(
    line: *const c_char,
    len: usize,
    out_pos: *mut usize,
) -> c_int {
    let s = if line.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(line.cast::<u8>(), len)
    };
    match crate::sysnservhealth::sys_parse_par_version(s) {
        Some(pos) => {
            if !out_pos.is_null() {
                *out_pos = pos;
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_os_trim_quotes(
    s: *const c_char,
    len: usize,
    out_start: *mut usize,
    out_len: *mut usize,
) {
    let bytes = if s.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(s.cast::<u8>(), len)
    };
    let (st, ln) = crate::sysnservhealth::os_trim_quotes(bytes);
    if !out_start.is_null() {
        *out_start = st;
    }
    if !out_len.is_null() {
        *out_len = ln;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_os_parse_sw_vers_field(
    output: *const c_char,
    output_len: usize,
    prefix: *const c_char,
    prefix_len: usize,
    out_start: *mut usize,
    out_len: *mut usize,
) -> c_int {
    let out_bytes = if output.is_null() || output_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(output.cast::<u8>(), output_len)
    };
    let pre_bytes = if prefix.is_null() || prefix_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(prefix.cast::<u8>(), prefix_len)
    };
    match crate::sysnservhealth::os_parse_sw_vers_field(out_bytes, pre_bytes) {
        Some((st, ln)) => {
            if !out_start.is_null() {
                *out_start = st;
            }
            if !out_len.is_null() {
                *out_len = ln;
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_os_map_windows_build(build_num: i64) -> *const c_char {
    match crate::sysnservhealth::os_map_windows_build(build_num) {
        Some(cstr) => cstr.as_ptr(),
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_cpu_canonical_arch(
    arch: *const c_char,
    len: usize,
) -> *const c_char {
    let bytes = if arch.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(arch.cast::<u8>(), len)
    };
    match crate::sysnservhealth::cpu_canonical_arch(bytes) {
        Some(cstr) => cstr.as_ptr(),
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_cpu_parse_cpuinfo_line(
    line: *const c_char,
    len: usize,
    out_val_offset: *mut usize,
) -> c_int {
    let bytes = if line.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(line.cast::<u8>(), len)
    };
    match crate::sysnservhealth::cpu_parse_cpuinfo_line(bytes) {
        Some(off) => {
            if !out_val_offset.is_null() {
                *out_val_offset = off;
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_yenc_validate_open_range(
    size: c_int,
    offset: i64,
    file_size: i64,
    out_eff_size: *mut c_int,
) -> c_int {
    let (ok, eff) = crate::sysnservhealth::yenc_validate_open_range(size, offset, file_size);
    if !out_eff_size.is_null() {
        *out_eff_size = eff;
    }
    ok
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_yenc_calc_segment_buffers(
    size: c_int,
    out_reserve: *mut c_int,
    out_inbuf: *mut c_int,
) {
    let (res, inb) = crate::sysnservhealth::yenc_calc_segment_buffers(size);
    if !out_reserve.is_null() {
        *out_reserve = res;
    }
    if !out_inbuf.is_null() {
        *out_inbuf = inb;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_nzbgen_is_nzb_file(filename: *const c_char) -> c_int {
    let s = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    i32::from(crate::sysnservhealth::nzbgen_is_nzb_file(s))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_nzbgen_calc_segment_count_and_size(
    file_size: i64,
    segment_size: c_int,
    seg_offset: i64,
    out_seg_count: *mut c_int,
    out_cur_seg_size: *mut c_int,
) {
    let (cnt, sz) = crate::sysnservhealth::nzbgen_calc_segment_count_and_size(
        file_size,
        segment_size,
        seg_offset,
    );
    if !out_seg_count.is_null() {
        *out_seg_count = cnt;
    }
    if !out_cur_seg_size.is_null() {
        *out_cur_seg_size = sz;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_nserv_verbosity_flags(
    verbosity: c_int,
    out_mute_info_warn_err: *mut c_int,
    out_mute_detail: *mut c_int,
    out_enable_debug: *mut c_int,
) {
    let (m1, m2, dbg) = crate::sysnservhealth::nserv_verbosity_flags(verbosity);
    if !out_mute_info_warn_err.is_null() {
        *out_mute_info_warn_err = i32::from(m1);
    }
    if !out_mute_detail.is_null() {
        *out_mute_detail = i32::from(m2);
    }
    if !out_enable_debug.is_null() {
        *out_enable_debug = i32::from(dbg);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_nntp_health_cap_for(
    connections: c_int,
    max_cap: c_int,
) -> c_int {
    crate::sysnservhealth::nntp_health_cap_for(connections, max_cap)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_nntp_health_is_bracketed(
    msg_id: *const c_char,
    len: usize,
) -> c_int {
    let s = if msg_id.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(msg_id.cast::<u8>(), len)
    };
    i32::from(crate::sysnservhealth::nntp_health_is_bracketed(s))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_nntp_health_classify_stat_reply(
    line: *const c_char,
) -> c_int {
    let s = if line.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(line).to_bytes()
    };
    crate::sysnservhealth::nntp_health_classify_stat_reply(s)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_http_get_calc_watchdog_wait(
    deadline_ms: i64,
    now_ms: i64,
    file_size: i64,
    max_bytes: usize,
    out_wait_ms: *mut i64,
) -> c_int {
    let (stop, wait) = crate::sysnservhealth::http_get_calc_watchdog_wait(
        deadline_ms,
        now_ms,
        file_size,
        max_bytes,
    );
    if !out_wait_ms.is_null() {
        *out_wait_ms = wait;
    }
    i32::from(stop)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_xml_reader_local_name_offset(name: *const c_char) -> usize {
    let s = if name.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(name).to_bytes()
    };
    crate::sysnservhealth::xml_reader_local_name_offset(s)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_http_client_find_header_colon(
    line: *const c_char,
    len: usize,
    out_colon_pos: *mut usize,
) -> c_int {
    let s = if line.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(line.cast::<u8>(), len)
    };
    match crate::sysnservhealth::http_client_find_header_colon(s) {
        Some(pos) => {
            if !out_colon_pos.is_null() {
                *out_colon_pos = pos;
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_speed_test_calc_mbps(bytes: u32, time_sec: f64) -> f64 {
    crate::sysnservhealth::speed_test_calc_mbps(bytes, time_sec)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_speed_test_is_required_more(
    dataset_len: usize,
    curr_median: f64,
    prev_p25: f64,
) -> c_int {
    i32::from(crate::sysnservhealth::speed_test_is_required_more(
        dataset_len,
        curr_median,
        prev_p25,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_speed_test_exceeded_max_failures(
    failures: c_int,
    iterations: c_int,
) -> c_int {
    i32::from(crate::sysnservhealth::speed_test_exceeded_max_failures(
        failures, iterations,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_post_renamer_classify_plan(
    is_disc: c_int,
    is_ambiguous: c_int,
    can_rename: c_int,
    actions_empty: c_int,
    target_obfuscated: c_int,
) -> c_int {
    crate::sysnservhealth::post_renamer_classify_plan(
        is_disc != 0,
        is_ambiguous != 0,
        can_rename != 0,
        actions_empty != 0,
        target_obfuscated != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_post_renamer_decide_final_status(
    is_stopped: c_int,
    any_failed: c_int,
    any_renamed: c_int,
) -> c_int {
    crate::sysnservhealth::post_renamer_decide_final_status(
        is_stopped != 0,
        any_failed != 0,
        any_renamed != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_health_validate_par_buffer(
    par_repair: c_int,
    buffer_mb: c_int,
) -> c_int {
    crate::sysnservhealth::health_validate_par_buffer(par_repair != 0, buffer_mb)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_health_validate_par_threads(
    par_repair: c_int,
    threads: c_int,
    hw_threads: usize,
) -> c_int {
    crate::sysnservhealth::health_validate_par_threads(par_repair != 0, threads, hw_threads)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_health_validate_par_time_limit(
    par_repair: c_int,
    limit: c_int,
) -> c_int {
    crate::sysnservhealth::health_validate_par_time_limit(par_repair != 0, limit)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_health_validate_rotate_log(
    is_rotate: c_int,
    rotate_log: c_int,
) -> c_int {
    crate::sysnservhealth::health_validate_rotate_log(is_rotate != 0, rotate_log)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_health_validate_time_correction(val: c_int) -> c_int {
    i32::from(crate::sysnservhealth::health_validate_time_correction(val))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_snh_health_validate_feed_url(
    url: *const c_char,
    len: usize,
) -> c_int {
    let s = if url.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(url.cast::<u8>(), len)
    };
    crate::sysnservhealth::health_validate_feed_url(s)
}

// ============================================================================
// Extension, Utility, NServ & SystemHealth Hot-Path FFI (`rust/src/extutilhealth.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_data_analytics_compute(
    measurements: *mut f64,
    len: usize,
    out_min: *mut f64,
    out_max: *mut f64,
    out_avg: *mut f64,
    out_median: *mut f64,
    out_p25: *mut f64,
    out_p75: *mut f64,
    out_p90: *mut f64,
) -> c_int {
    if measurements.is_null() || len == 0 {
        return 0;
    }
    let slice = std::slice::from_raw_parts_mut(measurements, len);
    match crate::extutilhealth::data_analytics_compute(slice) {
        Some((min, max, avg, med, p25, p75, p90)) => {
            if !out_min.is_null() {
                *out_min = min;
            }
            if !out_max.is_null() {
                *out_max = max;
            }
            if !out_avg.is_null() {
                *out_avg = avg;
            }
            if !out_median.is_null() {
                *out_median = med;
            }
            if !out_p25.is_null() {
                *out_p25 = p25;
            }
            if !out_p75.is_null() {
                *out_p75 = p75;
            }
            if !out_p90.is_null() {
                *out_p90 = p90;
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_data_analytics_percentile(
    sorted: *const f64,
    len: usize,
    percentile: f64,
) -> f64 {
    let slice = if sorted.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(sorted, len)
    };
    crate::extutilhealth::data_analytics_percentile(slice, percentile)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_benchmark_limit_by_free_space(
    requested: u64,
    available: u64,
    reserve: u64,
) -> u64 {
    crate::extutilhealth::benchmark_limit_by_free_space(requested, available, reserve)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_benchmark_calc_write_chunk(
    block_size: usize,
    max_bytes: u64,
    write_bytes: u64,
) -> usize {
    crate::extutilhealth::benchmark_calc_write_chunk(block_size, max_bytes, write_bytes)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_unrar_overwrite_switch(mode: c_int) -> *const c_char {
    crate::extutilhealth::unrar_overwrite_switch(mode).as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_unrar_is_part1_or_unsplit_rar(
    filename: *const c_char,
    len: usize,
) -> c_int {
    let s = if filename.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(filename.cast::<u8>(), len)
    };
    i32::from(crate::extutilhealth::unrar_is_part1_or_unsplit_rar(s))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_unrar_decode_exit_code(
    ec: c_int,
    out_msg: *mut *const c_char,
) -> c_int {
    let (ok, msg) = crate::extutilhealth::unrar_decode_exit_code(ec);
    if !out_msg.is_null() {
        *out_msg = msg.as_ptr();
    }
    i32::from(ok)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_sevenzip_overwrite_switch(mode: c_int) -> *const c_char {
    crate::extutilhealth::sevenzip_overwrite_switch(mode).as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_sevenzip_decode_exit_code(
    ec: c_int,
    out_msg: *mut *const c_char,
) -> c_int {
    let (ok, msg) = crate::extutilhealth::sevenzip_decode_exit_code(ec);
    if !out_msg.is_null() {
        *out_msg = msg.as_ptr();
    }
    i32::from(ok)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_service_should_work_now(
    service_interval: c_int,
    service_now: c_int,
    cur_time: i64,
    last_work: i64,
) -> c_int {
    i32::from(crate::extutilhealth::service_should_work_now(
        service_interval,
        service_now,
        cur_time,
        last_work,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_service_calc_remaining_wait(
    service_interval: c_int,
    service_now: c_int,
    service_sleep: c_int,
    cur_time: i64,
    last_work: i64,
    cur_wait_interval: c_int,
    out_remaining: *mut c_int,
    out_wait_interval: *mut c_int,
) {
    let (rem, wait) = crate::extutilhealth::service_calc_remaining_wait(
        service_interval,
        service_now,
        service_sleep,
        cur_time,
        last_work,
        cur_wait_interval,
    );
    if !out_remaining.is_null() {
        *out_remaining = rem;
    }
    if !out_wait_interval.is_null() {
        *out_wait_interval = wait;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_disk_service_interval(
    waiting_required_dir: c_int,
    disk_space_mb: c_int,
    downloading: c_int,
    sleep_val: c_int,
) -> c_int {
    crate::extutilhealth::disk_service_interval(
        waiting_required_dir != 0,
        disk_space_mb,
        downloading != 0,
        sleep_val,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_disk_service_is_low_space(
    available_bytes: i64,
    disk_space_mb: c_int,
) -> c_int {
    i32::from(crate::extutilhealth::disk_service_is_low_space(
        available_bytes,
        disk_space_mb,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_nserv_frontend_calc_start_index(
    needed_first_id: c_int,
    first_msg_id: c_int,
    out_need_skip: *mut c_int,
) -> c_int {
    let (start, skip) =
        crate::extutilhealth::nserv_frontend_calc_start_index(needed_first_id, first_msg_id);
    if !out_need_skip.is_null() {
        *out_need_skip = i32::from(skip);
    }
    start
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_cmd_script_classify_exit_code(exit_code: c_int) -> c_int {
    crate::extutilhealth::cmd_script_classify_exit_code(exit_code)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_sched_script_strip_ext_len(
    base_name: *const c_char,
    len: usize,
) -> usize {
    let s = if base_name.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(base_name.cast::<u8>(), len)
    };
    crate::extutilhealth::sched_script_strip_ext_len(s)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_nzb_script_match_param_prefix(
    param_name: *const c_char,
    strip_prefix: *const c_char,
    out_offset: *mut usize,
) -> c_int {
    let pname = if param_name.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(param_name).to_bytes()
    };
    let sprefix = if strip_prefix.is_null() {
        None
    } else {
        Some(CStr::from_ptr(strip_prefix).to_bytes())
    };
    match crate::extutilhealth::nzb_script_match_param_prefix(pname, sprefix) {
        Some(off) => {
            if !out_offset.is_null() {
                *out_offset = off;
            }
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_feed_script_is_success(exit_code: c_int) -> c_int {
    i32::from(crate::extutilhealth::feed_script_is_success(exit_code))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_validate_nzb_dir_interval(
    interval: c_int,
) -> c_int {
    crate::extutilhealth::health_validate_nzb_dir_interval(interval)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_validate_nzb_dir_file_age(age: c_int) -> c_int {
    crate::extutilhealth::health_validate_nzb_dir_file_age(age)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_validate_dupe_search(
    dupe_search: c_int,
    dupe_check: c_int,
    health_check_is_dupe: c_int,
) -> c_int {
    crate::extutilhealth::health_validate_dupe_search(
        dupe_search != 0,
        dupe_check != 0,
        health_check_is_dupe != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_validate_ext_list_len(len: usize) -> c_int {
    i32::from(crate::extutilhealth::health_validate_ext_list_len(len))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_strip_script_ext_len(
    name: *const c_char,
    len: usize,
) -> usize {
    let s = if name.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(name.cast::<u8>(), len)
    };
    crate::extutilhealth::health_strip_script_ext_len(s)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_validate_event_interval(val: c_int) -> c_int {
    i32::from(crate::extutilhealth::health_validate_event_interval(val))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_validate_primary_server(
    levels: *const i32,
    actives: *const u8,
    count: usize,
) -> c_int {
    let lvls = if levels.is_null() || count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(levels, count)
    };
    let acts = if actives.is_null() || count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(actives, count)
    };
    crate::extutilhealth::health_validate_primary_server(lvls, acts)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_validate_update_interval(v: c_int) -> c_int {
    i32::from(crate::extutilhealth::health_validate_update_interval(v))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_validate_scheduler_time(
    h: c_int,
    m: c_int,
) -> c_int {
    crate::extutilhealth::health_validate_scheduler_time(h, m)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_validate_scheduler_weekdays(bits: c_int) -> c_int {
    i32::from(crate::extutilhealth::health_validate_scheduler_weekdays(bits))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_validate_category_unpack(
    global_unpack: c_int,
    cat_unpack: c_int,
) -> c_int {
    crate::extutilhealth::health_validate_category_unpack(global_unpack != 0, cat_unpack != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_euh_health_severity_to_str(
    severity: c_int,
) -> *const c_char {
    crate::extutilhealth::health_severity_to_str(severity).as_ptr()
}

// ============================================================================
// ArticleDownloader, DirectUnpack, UnpackController, Repair & ParChecker Hot-Path FFI
// (`rust/src/dlunpackrepair.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_article_dl_is_retention_failure(
    retention_days: c_int,
    file_time: i64,
    cur_time: i64,
    out_age_days: *mut c_int,
) -> c_int {
    let (fail, age) = crate::dlunpackrepair::article_dl_is_retention_failure(
        retention_days,
        file_time,
        cur_time,
    );
    if !out_age_days.is_null() {
        *out_age_days = age;
    }
    i32::from(fail)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_article_dl_all_servers_on_level_failed(
    level: c_int,
    cand_ids: *const i32,
    cand_levels: *const i32,
    cand_groups: *const i32,
    cand_avail: *const u8,
    cand_count: usize,
    failed_ids: *const i32,
    failed_levels: *const i32,
    failed_groups: *const i32,
    failed_count: usize,
) -> c_int {
    let c_ids = if cand_ids.is_null() || cand_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(cand_ids, cand_count)
    };
    let c_lvls = if cand_levels.is_null() || cand_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(cand_levels, cand_count)
    };
    let c_grps = if cand_groups.is_null() || cand_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(cand_groups, cand_count)
    };
    let c_avl = if cand_avail.is_null() || cand_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(cand_avail, cand_count)
    };
    let f_ids = if failed_ids.is_null() || failed_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(failed_ids, failed_count)
    };
    let f_lvls = if failed_levels.is_null() || failed_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(failed_levels, failed_count)
    };
    let f_grps = if failed_groups.is_null() || failed_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(failed_groups, failed_count)
    };
    i32::from(crate::dlunpackrepair::article_dl_all_servers_on_level_failed(
        level, c_ids, c_lvls, c_grps, c_avl, f_ids, f_lvls, f_grps,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_article_dl_resolve_final_status(
    status: c_int,
    is_duplicate: c_int,
    is_stopped: c_int,
    ad_finished: c_int,
    ad_failed: c_int,
    ad_retry: c_int,
    ad_fatal_error: c_int,
) -> c_int {
    crate::dlunpackrepair::article_dl_resolve_final_status(
        status,
        is_duplicate != 0,
        is_stopped != 0,
        ad_finished,
        ad_failed,
        ad_retry,
        ad_fatal_error,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_article_dl_should_throttle_bandwidth(
    is_stopped: c_int,
    speed_limit: f32,
    cur_speed: f32,
    mom_speed: f32,
) -> c_int {
    i32::from(crate::dlunpackrepair::article_dl_should_throttle_bandwidth(
        is_stopped != 0,
        speed_limit,
        cur_speed,
        mom_speed,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_article_dl_classify_decode_status(
    raw_article: c_int,
    decoder_status: c_int,
    is_yenc: c_int,
    range_exceeded: c_int,
) -> c_int {
    crate::dlunpackrepair::article_dl_classify_decode_status(
        raw_article != 0,
        decoder_status,
        is_yenc != 0,
        range_exceeded != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_direct_unpack_resolve_final_status(
    processed: c_int,
    unpack_ok: c_int,
    interrupted: c_int,
    cancelled: c_int,
    ns_none: c_int,
    ns_success: c_int,
    ns_failure: c_int,
) -> c_int {
    crate::dlunpackrepair::direct_unpack_resolve_final_status(
        processed != 0,
        unpack_ok != 0,
        interrupted != 0,
        cancelled != 0,
        ns_none,
        ns_success,
        ns_failure,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_direct_unpack_use_inter_dir(
    dest_dir: *const c_char,
    inter_dir: *const c_char,
    sep: c_char,
) -> c_int {
    let d = if dest_dir.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(dest_dir).to_bytes()
    };
    let i = if inter_dir.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(inter_dir).to_bytes()
    };
    i32::from(crate::dlunpackrepair::direct_unpack_use_inter_dir(
        d,
        i,
        sep as u8,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_direct_unpack_is_main_archive(
    filename: *const c_char,
) -> c_int {
    let s = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    i32::from(crate::dlunpackrepair::direct_unpack_is_main_archive(s))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_direct_unpack_is_insert_disk_prompt(
    buf: *const c_char,
    i: usize,
    ch: c_int,
) -> c_int {
    if i <= 35 || ch != c_int::from(b' ') || buf.is_null() {
        return 0;
    }
    let len = i.saturating_add(1);
    let raw = std::slice::from_raw_parts(buf.cast::<u8>(), len);
    let s = match raw.iter().position(|&b| b == 0) {
        Some(pos) => &raw[..pos],
        None => raw,
    };
    i32::from(crate::dlunpackrepair::direct_unpack_is_insert_disk_prompt(
        s, i, ch,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_unpack_ctrl_should_skip_second_attempt(
    unpack_tried: c_int,
    par_repaired: c_int,
    password_empty: c_int,
    pass_file_empty: c_int,
    pass_list_tried: c_int,
) -> c_int {
    i32::from(
        crate::dlunpackrepair::unpack_ctrl_should_skip_second_attempt(
            unpack_tried != 0,
            par_repaired != 0,
            password_empty != 0,
            pass_file_empty != 0,
            pass_list_tried != 0,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_unpack_ctrl_eval_unrar_exit(
    exit_code: c_int,
    all_ok_received: c_int,
    terminated: c_int,
    auto_terminated: c_int,
    prev_password_error: c_int,
    out_unpack_ok: *mut c_int,
    out_start_error: *mut c_int,
    out_space_error: *mut c_int,
    out_password_error: *mut c_int,
) {
    let (ok, start_err, space_err, pass_err) = crate::dlunpackrepair::unpack_ctrl_eval_unrar_exit(
        exit_code,
        all_ok_received != 0,
        terminated != 0,
        auto_terminated != 0,
        prev_password_error != 0,
    );
    if !out_unpack_ok.is_null() {
        *out_unpack_ok = i32::from(ok);
    }
    if !out_start_error.is_null() {
        *out_start_error = i32::from(start_err);
    }
    if !out_space_error.is_null() {
        *out_space_error = i32::from(space_err);
    }
    if !out_password_error.is_null() {
        *out_password_error = i32::from(pass_err);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_unpack_ctrl_classify_archive_file(
    filename: *const c_char,
) -> c_int {
    let s = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    crate::dlunpackrepair::unpack_ctrl_classify_archive_file(s)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_unpack_ctrl_resolve_fail_status(
    space_error: c_int,
    password_error: c_int,
    decrypt_error: c_int,
    us_space: c_int,
    us_password: c_int,
    us_failure: c_int,
) -> c_int {
    crate::dlunpackrepair::unpack_ctrl_resolve_fail_status(
        space_error != 0,
        password_error != 0,
        decrypt_error != 0,
        us_space,
        us_password,
        us_failure,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_unpack_ctrl_has_completed_archive_ext(
    filename: *const c_char,
) -> c_int {
    let s = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    i32::from(crate::dlunpackrepair::unpack_ctrl_has_completed_archive_ext(s))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_repair_ctrl_classify_file_crc_status(
    cf_status: c_int,
    reprocess: c_int,
    has_segments: c_int,
    cf_success: c_int,
    cf_failure: c_int,
    cf_partial: c_int,
    fs_success: c_int,
    fs_failure: c_int,
    fs_partial: c_int,
    fs_unknown: c_int,
) -> c_int {
    crate::dlunpackrepair::repair_ctrl_classify_file_crc_status(
        cf_status,
        reprocess != 0,
        has_segments != 0,
        cf_success,
        cf_failure,
        cf_partial,
        fs_success,
        fs_failure,
        fs_partial,
        fs_unknown,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_repair_ctrl_estimate_twin_vol_blocks(
    entry_size: i64,
    twin_block_size: u64,
) -> c_int {
    crate::dlunpackrepair::repair_ctrl_estimate_twin_vol_blocks(entry_size, twin_block_size)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_repair_ctrl_estimate_par_blocks_from_size(
    file_size: i64,
    block_size: i64,
) -> c_int {
    crate::dlunpackrepair::repair_ctrl_estimate_par_blocks_from_size(file_size, block_size)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_repair_ctrl_match_strict_par_name(
    filename: *const c_char,
    main_base: *const c_char,
) -> c_int {
    let f = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    let b = if main_base.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(main_base).to_bytes()
    };
    i32::from(crate::dlunpackrepair::repair_ctrl_match_strict_par_name(
        f, b,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_repair_ctrl_resolve_par_status(
    checker_status: c_int,
    cur_par_status: c_int,
    ps_repaired: c_int,
    ps_repair_not_needed: c_int,
    ps_repair_possible: c_int,
    nzb_ps_skipped: c_int,
    nzb_ps_success: c_int,
    nzb_ps_repair_possible: c_int,
    nzb_ps_failure: c_int,
    out_set_par_repaired: *mut c_int,
    out_par_repaired_val: *mut c_int,
) -> c_int {
    let (status, set_rep, rep_val) = crate::dlunpackrepair::repair_ctrl_resolve_par_status(
        checker_status,
        cur_par_status,
        ps_repaired,
        ps_repair_not_needed,
        ps_repair_possible,
        nzb_ps_skipped,
        nzb_ps_success,
        nzb_ps_repair_possible,
        nzb_ps_failure,
    );
    if !out_set_par_repaired.is_null() {
        *out_set_par_repaired = i32::from(set_rep);
    }
    if !out_par_repaired_val.is_null() {
        *out_par_repaired_val = i32::from(rep_val);
    }
    status
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_parchecker_calc_threads_and_mem(
    par_threads: c_int,
    cpu_cores: c_int,
    effective_mem_mb: c_int,
    out_threads: *mut c_int,
    out_mem: *mut c_int,
) {
    let (t, m) = crate::dlunpackrepair::parchecker_calc_threads_and_mem(
        par_threads,
        cpu_cores,
        effective_mem_mb,
    );
    if !out_threads.is_null() {
        *out_threads = t;
    }
    if !out_mem.is_null() {
        *out_mem = m;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_parchecker_calc_pad_bytes(
    block_size: u64,
    block_count: u32,
    file_size: u64,
) -> u64 {
    crate::dlunpackrepair::parchecker_calc_pad_bytes(block_size, block_count, file_size)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_parchecker_compare_extra_file_stems(
    base_name1: *const c_char,
    base_name2: *const c_char,
    par_stem: *const c_char,
) -> c_int {
    let b1 = if base_name1.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(base_name1).to_bytes()
    };
    let b2 = if base_name2.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(base_name2).to_bytes()
    };
    let ps = if par_stem.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(par_stem).to_bytes()
    };
    i32::from(crate::dlunpackrepair::parchecker_compare_extra_file_stems(
        b1, b2, ps,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_health_classify_log_file_mode(
    write_log: c_int,
    wl_none: c_int,
    wl_rotate: c_int,
    wl_reset: c_int,
    path_empty: c_int,
    is_null_or_console: c_int,
    has_parent_path: c_int,
) -> c_int {
    crate::dlunpackrepair::health_classify_log_file_mode(
        write_log,
        wl_none,
        wl_rotate,
        wl_reset,
        path_empty != 0,
        is_null_or_console != 0,
        has_parent_path != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_health_classify_cert_store(
    path_empty: c_int,
    cert_check: c_int,
) -> c_int {
    crate::dlunpackrepair::health_classify_cert_store(path_empty != 0, cert_check != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_health_classify_lock_file(
    path_empty: c_int,
    daemon_mode: c_int,
) -> c_int {
    crate::dlunpackrepair::health_classify_lock_file(path_empty != 0, daemon_mode != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dur_health_alert_matches(
    src1: *const c_char,
    src1_len: usize,
    cat1: *const c_char,
    cat1_len: usize,
    src2: *const c_char,
    src2_len: usize,
    cat2: *const c_char,
    cat2_len: usize,
) -> c_int {
    let s1 = if src1.is_null() || src1_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(src1.cast::<u8>(), src1_len)
    };
    let c1 = if cat1.is_null() || cat1_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(cat1.cast::<u8>(), cat1_len)
    };
    let s2 = if src2.is_null() || src2_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(src2.cast::<u8>(), src2_len)
    };
    let c2 = if cat2.is_null() || cat2_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(cat2.cast::<u8>(), cat2_len)
    };
    i32::from(crate::dlunpackrepair::health_alert_matches(s1, c1, s2, c2))
}

// ============================================================================
// ArticleWriter, Extension, Validators, Unpack, Thread, Xml, Json, Network,
// Observer, WorkState, NewsServer, OpenSSL & Utf8 FFI (`rust/src/artwritercore.rs`)
// ============================================================================

unsafe fn write_bytes_to_c_buf(src: &[u8], out_buf: *mut c_char, out_cap: usize) -> usize {
    if !out_buf.is_null() && out_cap > 0 {
        let copy_len = src.len().min(out_cap - 1);
        std::ptr::copy_nonoverlapping(src.as_ptr(), out_buf.cast::<u8>(), copy_len);
        *out_buf.add(copy_len) = 0;
    }
    src.len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_format_result_filename(
    tmp_dir: *const c_char,
    sep: c_char,
    file_id: c_int,
    part_num: c_int,
    out_buf: *mut c_char,
    out_cap: usize,
) -> usize {
    let dir = if tmp_dir.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(tmp_dir).to_bytes()
    };
    let formatted = crate::artwritercore::article_writer_format_result_filename(
        dir,
        sep as u8,
        file_id,
        part_num,
    );
    write_bytes_to_c_buf(&formatted, out_buf, out_cap)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_format_direct_out_tmp(
    dest_dir: *const c_char,
    sep: c_char,
    file_id: c_int,
    out_buf: *mut c_char,
    out_cap: usize,
) -> usize {
    let dir = if dest_dir.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(dest_dir).to_bytes()
    };
    let formatted = crate::artwritercore::article_writer_format_direct_out_tmp(
        dir,
        sep as u8,
        file_id,
    );
    write_bytes_to_c_buf(&formatted, out_buf, out_cap)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_is_within_dest_dir(
    nzb_dest_dir: *const c_char,
    nzb_dest_dir_len: usize,
    output_filename: *const c_char,
    output_filename_len: usize,
    sep1: c_char,
    sep2: c_char,
) -> c_int {
    let dir = if nzb_dest_dir.is_null() || nzb_dest_dir_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(nzb_dest_dir.cast::<u8>(), nzb_dest_dir_len)
    };
    let out = if output_filename.is_null() || output_filename_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(output_filename.cast::<u8>(), output_filename_len)
    };
    i32::from(crate::artwritercore::article_writer_is_within_dest_dir(
        dir,
        out,
        sep1 as u8,
        sep2 as u8,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_writer_classify_completion_status(
    total_articles: c_int,
    success_articles: c_int,
    missed_articles: c_int,
    failed_articles: c_int,
) -> c_int {
    crate::artwritercore::article_writer_classify_completion_status(
        total_articles,
        success_articles,
        missed_articles,
        failed_articles,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_article_cache_can_flush_file(
    cached_articles: c_int,
    active_downloads: c_int,
    skip_disk_write: c_int,
    flush_everything: c_int,
) -> c_int {
    i32::from(crate::artwritercore::article_cache_can_flush_file(
        cached_articles,
        active_downloads,
        skip_disk_write != 0,
        flush_everything != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_memory_monitor_tier_name(
    tier: c_int,
    constrained: c_int,
    critical: c_int,
) -> *const c_char {
    crate::artwritercore::memory_monitor_tier_name(tier, constrained, critical).as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_memory_monitor_effective_downloads_limit(
    tier: c_int,
    configured_limit: c_int,
    constrained: c_int,
    critical: c_int,
) -> c_int {
    crate::artwritercore::memory_monitor_effective_downloads_limit(
        tier,
        configured_limit,
        constrained,
        critical,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_xml_bool_to_str(value: c_int) -> *const c_char {
    crate::artwritercore::xml_bool_to_str(value != 0).as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_extension_select_kind_flag(
    post: c_int,
    scan: c_int,
    queue: c_int,
    scheduler: c_int,
    feed: c_int,
    which: c_int,
) -> c_int {
    i32::from(crate::artwritercore::extension_select_kind_flag(
        post != 0,
        scan != 0,
        queue != 0,
        scheduler != 0,
        feed != 0,
        which,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_health_status_should_include(is_ok: c_int) -> c_int {
    i32::from(crate::artwritercore::health_status_should_include(
        is_ok != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_health_find_duplicate_feed_url(
    url_ptrs: *const *const c_char,
    url_lens: *const usize,
    count: usize,
    out_first_idx: *mut usize,
    out_dup_idx: *mut usize,
) -> c_int {
    if url_ptrs.is_null() || url_lens.is_null() || count == 0 {
        return 0;
    }
    let ptrs = std::slice::from_raw_parts(url_ptrs, count);
    let lens = std::slice::from_raw_parts(url_lens, count);
    let mut slices: Vec<&[u8]> = Vec::with_capacity(count);
    for i in 0..count {
        if ptrs[i].is_null() || lens[i] == 0 {
            slices.push(&[]);
        } else {
            slices.push(std::slice::from_raw_parts(ptrs[i].cast::<u8>(), lens[i]));
        }
    }
    if let Some((first_idx, dup_idx)) =
        crate::artwritercore::health_find_duplicate_feed_url(&slices)
    {
        if !out_first_idx.is_null() {
            *out_first_idx = first_idx;
        }
        if !out_dup_idx.is_null() {
            *out_dup_idx = dup_idx;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_health_find_duplicate_category_name(
    name_ptrs: *const *const c_char,
    name_lens: *const usize,
    count: usize,
    out_dup_idx: *mut usize,
) -> c_int {
    if name_ptrs.is_null() || name_lens.is_null() || count == 0 {
        return 0;
    }
    let ptrs = std::slice::from_raw_parts(name_ptrs, count);
    let lens = std::slice::from_raw_parts(name_lens, count);
    let mut slices: Vec<&[u8]> = Vec::with_capacity(count);
    for i in 0..count {
        if ptrs[i].is_null() || lens[i] == 0 {
            slices.push(&[]);
        } else {
            slices.push(std::slice::from_raw_parts(ptrs[i].cast::<u8>(), lens[i]));
        }
    }
    if let Some(dup_idx) = crate::artwritercore::health_find_duplicate_category_name(&slices) {
        if !out_dup_idx.is_null() {
            *out_dup_idx = dup_idx;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_health_scheduler_tasks_reserve_count(
    count: usize,
) -> usize {
    crate::artwritercore::health_scheduler_tasks_reserve_count(count)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_unpack_make_password_arg(
    password: *const c_char,
    password_len: usize,
    out_buf: *mut c_char,
    out_cap: usize,
) -> usize {
    let pw = if password.is_null() || password_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(password.cast::<u8>(), password_len)
    };
    let formatted = crate::artwritercore::unpack_make_password_arg(pw);
    write_bytes_to_c_buf(&formatted, out_buf, out_cap)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_unpack_validate_tool_status(
    tool_empty: c_int,
    has_err: c_int,
    exists: c_int,
) -> c_int {
    crate::artwritercore::unpack_validate_tool_status(
        tool_empty != 0,
        has_err != 0,
        exists != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_thread_adjust_count_on_kill(
    terminated: c_int,
    cur_count: c_int,
) -> c_int {
    crate::artwritercore::thread_adjust_count_on_kill(terminated != 0, cur_count)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_json_has_error(err_val: c_int) -> c_int {
    i32::from(crate::artwritercore::json_has_error(err_val))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_network_is_http_status_ok(status_code: c_int) -> c_int {
    i32::from(crate::artwritercore::network_is_http_status_ok(status_code))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_observer_find_index(
    observers: *const usize,
    count: usize,
    target: usize,
) -> isize {
    let obs = if observers.is_null() || count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(observers, count)
    };
    crate::artwritercore::observer_find_index(obs, target)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_work_state_should_notify() -> c_int {
    i32::from(crate::artwritercore::work_state_should_notify())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_news_server_format_default_name(
    name: *const c_char,
    id: c_int,
    out_buf: *mut c_char,
    out_cap: usize,
) -> usize {
    let n = if name.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(name).to_bytes()
    };
    let formatted = crate::artwritercore::news_server_format_default_name(n, id);
    write_bytes_to_c_buf(&formatted, out_buf, out_cap)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_openssl_should_stop_thread(is_libressl: c_int) -> c_int {
    i32::from(crate::artwritercore::openssl_should_stop_thread(
        is_libressl != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_awc_utf8_is_empty_len(len: usize) -> c_int {
    i32::from(crate::artwritercore::utf8_is_empty_len(len))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_conn_select_ai_family(
    ip_version: c_int,
    ip_v4: c_int,
    ip_v6: c_int,
    af_inet: c_int,
    af_inet6: c_int,
    af_unspec: c_int,
) -> c_int {
    crate::connclientmain::conn_select_ai_family(
        ip_version, ip_v4, ip_v6, af_inet, af_inet6, af_unspec,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_conn_is_unix_domain_path(host: *const c_char) -> c_int {
    let h = if host.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(host).to_bytes()
    };
    i32::from(crate::connclientmain::conn_is_unix_domain_path(h))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_conn_recv_buffered_chunk_len(
    size: c_int,
    buf_avail: c_int,
) -> c_int {
    crate::connclientmain::conn_recv_buffered_chunk_len(size, buf_avail)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_conn_poll_timeout_ms(timeout_sec: c_int) -> c_int {
    crate::connclientmain::conn_poll_timeout_ms(timeout_sec)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_conn_poll_revents_ready(revents: c_int, mask: c_int) -> c_int {
    i32::from(crate::connclientmain::conn_poll_revents_ready(
        revents, mask,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_remote_client_is_wildcard_control_ip(
    control_ip: *const c_char,
) -> c_int {
    let ip = if control_ip.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(control_ip).to_bytes()
    };
    i32::from(crate::connclientmain::remote_client_is_wildcard_control_ip(
        ip,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_remote_client_is_http_url(
    nzb_content: *const c_char,
) -> c_int {
    let s = if nzb_content.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(nzb_content).to_bytes()
    };
    i32::from(crate::connclientmain::remote_client_is_http_url(s))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_remote_client_calc_file_completed_pct(
    remaining_size: i64,
    total_size: i64,
) -> c_int {
    crate::connclientmain::remote_client_calc_file_completed_pct(remaining_size, total_size)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_remote_client_calc_hms(
    sec: i64,
    out_h: *mut c_int,
    out_m: *mut c_int,
    out_s: *mut c_int,
) {
    let (h, m, s) = crate::connclientmain::remote_client_calc_hms(sec);
    if !out_h.is_null() {
        *out_h = h;
    }
    if !out_m.is_null() {
        *out_m = m;
    }
    if !out_s.is_null() {
        *out_s = s;
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_remote_client_calc_avg_speed(
    all_bytes: i64,
    download_time_sec: i64,
) -> c_int {
    crate::connclientmain::remote_client_calc_avg_speed(all_bytes, download_time_sec)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_remote_client_format_server_state(
    download_paused: c_int,
    download2_paused: c_int,
    download_standby: c_int,
    post_job_count: u32,
    post_paused: c_int,
    out_buf: *mut c_char,
    out_cap: usize,
) -> usize {
    let formatted = crate::connclientmain::remote_client_format_server_state(
        download_paused != 0,
        download2_paused != 0,
        download_standby != 0,
        post_job_count,
        post_paused != 0,
    );
    write_bytes_to_c_buf(&formatted, out_buf, out_cap)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_remote_client_align4(len: c_int) -> c_int {
    crate::connclientmain::remote_client_align4(len)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_nzbget_is_valid_umask(umask_val: u32) -> c_int {
    i32::from(crate::connclientmain::nzbget_is_valid_umask(umask_val))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_nzbget_should_start_secure_server(
    secure_control: c_int,
    is_unix: c_int,
    control_ip: *const c_char,
) -> c_int {
    let ip = if control_ip.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(control_ip).to_bytes()
    };
    i32::from(crate::connclientmain::nzbget_should_start_secure_server(
        secure_control != 0,
        is_unix != 0,
        ip,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_nzbget_should_stop_standalone(
    server_mode: c_int,
    queue_more: c_int,
    url_more: c_int,
    post_more: c_int,
) -> c_int {
    i32::from(crate::connclientmain::nzbget_should_stop_standalone(
        server_mode != 0,
        queue_more != 0,
        url_more != 0,
        post_more != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_nzbget_webget_exit_code(
    status: c_int,
    ad_finished: c_int,
) -> c_int {
    crate::connclientmain::nzbget_webget_exit_code(status, ad_finished)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_nzbget_sigverify_exit_code(ok: c_int) -> c_int {
    crate::connclientmain::nzbget_sigverify_exit_code(ok != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_twin_check_parse_fingerprint_files(
    fingerprint: *const c_char,
) -> c_int {
    let fp = if fingerprint.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(fingerprint).to_bytes()
    };
    crate::connclientmain::twin_check_parse_fingerprint_files(fp)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_twin_check_format_identity(
    files_param: *const c_char,
    posting: *const c_char,
) -> RsBuf {
    let fp = if files_param.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(files_param).to_bytes()
    };
    let post = if posting.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(posting).to_bytes()
    };
    into_buf(crate::connclientmain::twin_check_format_identity(fp, post))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_twin_check_is_dupe_item(
    queued: c_int,
    remaining_size: i64,
    paused_size: i64,
    delete_status: c_int,
    ds_dupe: c_int,
    ds_copy: c_int,
) -> c_int {
    i32::from(crate::connclientmain::twin_check_is_dupe_item(
        queued != 0,
        remaining_size,
        paused_size,
        delete_status,
        ds_dupe,
        ds_copy,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_twin_check_service_interval(nudged: c_int) -> c_int {
    crate::connclientmain::twin_check_service_interval(nudged != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_queue_editor_clamp_move_target(
    entry: c_int,
    offset: c_int,
    size: c_int,
    out_new_entry: *mut c_int,
) -> c_int {
    if let Some(target) = crate::connclientmain::queue_editor_clamp_move_target(entry, offset, size)
    {
        if !out_new_entry.is_null() {
            *out_new_entry = target;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_queue_editor_eval_group_delete_flags(
    action: c_int,
    ea_park_delete: c_int,
    ea_dupe_delete: c_int,
    ea_final_delete: c_int,
    keep_history: c_int,
    unpack_cleaned_up_disk: c_int,
    current_success_articles: c_int,
    out_parking: *mut c_int,
    out_avoid_history: *mut c_int,
    out_set_dupe_status: *mut c_int,
    out_cleanup_disk: *mut c_int,
) {
    let (park, avoid, dupe, clean) = crate::connclientmain::queue_editor_eval_group_delete_flags(
        action,
        ea_park_delete,
        ea_dupe_delete,
        ea_final_delete,
        keep_history,
        unpack_cleaned_up_disk != 0,
        current_success_articles,
    );
    if !out_parking.is_null() {
        *out_parking = i32::from(park);
    }
    if !out_avoid_history.is_null() {
        *out_avoid_history = i32::from(avoid);
    }
    if !out_set_dupe_status.is_null() {
        *out_set_dupe_status = i32::from(dupe);
    }
    if !out_cleanup_disk.is_null() {
        *out_cleanup_disk = i32::from(clean);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_queue_editor_calc_move_groups_to_offset(
    queue_ids: *const c_int,
    queue_len: usize,
    target_id: c_int,
    move_id: c_int,
    before: c_int,
    item_list_size: usize,
) -> c_int {
    let ids = if queue_ids.is_null() || queue_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(queue_ids, queue_len)
    };
    crate::connclientmain::queue_editor_calc_move_groups_to_offset(
        ids,
        target_id,
        move_id,
        before != 0,
        item_list_size,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_ncurses_calc_msg_lines(
    text_len: c_int,
    screen_width: c_int,
    out_win_width: *mut c_int,
    out_msg_lines: *mut c_int,
) {
    let (ww, ml) = crate::connclientmain::ncurses_calc_msg_lines(text_len, screen_width);
    if !out_win_width.is_null() {
        *out_win_width = ww;
    }
    if !out_msg_lines.is_null() {
        *out_msg_lines = ml;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ccm_ncurses_sanitize_message_chars(
    buf: *mut c_char,
    len: usize,
) {
    if buf.is_null() || len == 0 {
        return;
    }
    let slice = std::slice::from_raw_parts_mut(buf.cast::<u8>(), len);
    crate::connclientmain::ncurses_sanitize_message_chars(slice);
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_ncurses_select_edit_queue_keybar(
    selected_entry: c_int,
    queue_size: c_int,
) -> *const c_char {
    crate::connclientmain::ncurses_select_edit_queue_keybar(selected_entry, queue_size)
        .as_ptr()
        .cast::<c_char>()
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_ncurses_calc_group_name_len(
    screen_width: c_int,
    calc_col_width: c_int,
    col_files: c_int,
    col_total: c_int,
    col_left: c_int,
) -> c_int {
    crate::connclientmain::ncurses_calc_group_name_len(
        screen_width,
        calc_col_width != 0,
        col_files,
        col_total,
        col_left,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ccm_ncurses_next_queue_win_pct(
    cur_pct: c_int,
    is_edit_queue: c_int,
) -> c_int {
    crate::connclientmain::ncurses_next_queue_win_pct(cur_pct, is_edit_queue != 0)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_nntp_map_ip_version(
    ip_version: c_int,
    ip_v4: c_int,
    ip_v6: c_int,
    ip_auto: c_int,
) -> c_int {
    crate::nntptlsweb::nntp_map_ip_version(ip_version, ip_v4, ip_v6, ip_auto)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_tls_should_verify_peer(
    has_store: c_int,
    cert_verif_level: c_int,
    cv_none: c_int,
) -> c_int {
    i32::from(crate::nntptlsweb::tls_should_verify_peer(
        has_store != 0,
        cert_verif_level,
        cv_none,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_tls_is_explicit_handshake_rejection(
    err_lib: c_int,
    err_lib_ssl: c_int,
    reason: c_int,
    r_handshake_fail: c_int,
    r_proto_ver: c_int,
    r_no_shared_cipher: c_int,
    r_unsupported_proto: c_int,
) -> c_int {
    i32::from(crate::nntptlsweb::tls_is_explicit_handshake_rejection(
        err_lib,
        err_lib_ssl,
        reason,
        r_handshake_fail,
        r_proto_ver,
        r_no_shared_cipher,
        r_unsupported_proto,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_tls_should_check_host(
    cert_verif_level: c_int,
    cv_minimal: c_int,
    host_empty: c_int,
) -> c_int {
    i32::from(crate::nntptlsweb::tls_should_check_host(
        cert_verif_level,
        cv_minimal,
        host_empty != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ntw_tls_classify_want_io(
    ssl_err: c_int,
    want_read: c_int,
    want_write: c_int,
    out_want_read: *mut c_int,
    out_want_write: *mut c_int,
) -> c_int {
    let (retry, r, w) = crate::nntptlsweb::tls_classify_want_io(ssl_err, want_read, want_write);
    if !out_want_read.is_null() {
        *out_want_read = i32::from(r);
    }
    if !out_want_write.is_null() {
        *out_want_write = i32::from(w);
    }
    i32::from(retry)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ntw_webdl_calc_initial_retries(
    url_retries: c_int,
    retry_enabled: c_int,
    out_rem_dl: *mut c_int,
    out_rem_conn: *mut c_int,
) {
    let (dl, conn) = crate::nntptlsweb::webdl_calc_initial_retries(url_retries, retry_enabled != 0);
    if !out_rem_dl.is_null() {
        *out_rem_dl = dl;
    }
    if !out_rem_conn.is_null() {
        *out_rem_conn = conn;
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_webdl_should_wait_before_retry(
    status: c_int,
    ad_failed: c_int,
    ad_connect_error: c_int,
    rem_dl: c_int,
    rem_conn: c_int,
    is_stopped: c_int,
    force: c_int,
    pause_download: c_int,
) -> c_int {
    i32::from(crate::nntptlsweb::webdl_should_wait_before_retry(
        status,
        ad_failed,
        ad_connect_error,
        rem_dl,
        rem_conn,
        is_stopped != 0,
        force != 0,
        pause_download != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ntw_webdl_is_default_http_port(
    protocol: *const c_char,
    port: c_int,
) -> c_int {
    i32::from(crate::nntptlsweb::webdl_is_default_http_port(
        input(protocol),
        port,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_webdl_is_header_end_line(first_ch: c_int) -> c_int {
    i32::from(crate::nntptlsweb::webdl_is_header_end_line(first_ch as u8))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ntw_webdl_strip_query_len(url: *const c_char) -> usize {
    crate::nntptlsweb::webdl_strip_query_len(input(url))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_server_pool_can_advance_level(
    norm_level: c_int,
    level: c_int,
    active: c_int,
    optional: c_int,
    blocked: c_int,
) -> c_int {
    i32::from(crate::nntptlsweb::server_pool_can_advance_level(
        norm_level,
        level,
        active != 0,
        optional != 0,
        blocked != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_server_pool_match_want_server(
    has_want: c_int,
    cand_id: c_int,
    want_id: c_int,
    want_group: c_int,
    cand_group: c_int,
) -> c_int {
    i32::from(crate::nntptlsweb::server_pool_match_want_server(
        has_want != 0,
        cand_id,
        want_id,
        want_group,
        cand_group,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_server_pool_match_ignore_server(
    cand_id: c_int,
    ign_id: c_int,
    ign_group: c_int,
    cand_group: c_int,
    ign_norm_level: c_int,
    cand_norm_level: c_int,
) -> c_int {
    i32::from(crate::nntptlsweb::server_pool_match_ignore_server(
        cand_id,
        ign_id,
        ign_group,
        cand_group,
        ign_norm_level,
        cand_norm_level,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_server_pool_calc_remaining_block_sec(
    block_time: i64,
    retry_interval: c_int,
    cur_time: i64,
) -> c_int {
    crate::nntptlsweb::server_pool_calc_remaining_block_sec(block_time, retry_interval, cur_time)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_remote_server_select_port(
    tls: c_int,
    secure_port: c_int,
    control_port: c_int,
) -> c_int {
    crate::nntptlsweb::remote_server_select_port(tls != 0, secure_port, control_port)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ntw_remote_server_should_warn_cap(
    active_count: c_int,
    max_processors: c_int,
    now: i64,
    last_warn: i64,
    out_should_warn: *mut c_int,
) -> c_int {
    let (at_cap, should_warn) = crate::nntptlsweb::remote_server_should_warn_cap(
        active_count,
        max_processors,
        now,
        last_warn,
    );
    if !out_should_warn.is_null() {
        *out_should_warn = i32::from(should_warn);
    }
    i32::from(at_cap)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_web_proc_max_post_content_len(authorized: c_int) -> c_int {
    crate::nntptlsweb::web_proc_max_post_content_len(authorized != 0)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_web_proc_is_content_len_valid(
    content_len: c_int,
    authorized: c_int,
) -> c_int {
    i32::from(crate::nntptlsweb::web_proc_is_content_len_valid(
        content_len,
        authorized != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_web_proc_should_use_gzip(
    gzip_enabled: c_int,
    body_len: c_int,
    max_uncompressed_size: c_int,
) -> c_int {
    i32::from(crate::nntptlsweb::web_proc_should_use_gzip(
        gzip_enabled != 0,
        body_len,
        max_uncompressed_size,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ntw_web_proc_default_resource(
    url: *const c_char,
) -> *const c_char {
    crate::nntptlsweb::web_proc_default_resource(input(url))
        .as_ptr()
        .cast::<c_char>()
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_xmlrpc_mib_per_sec(bytes: u64, ms: f64) -> f64 {
    crate::nntptlsweb::xmlrpc_mib_per_sec(bytes, ms)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ntw_xmlrpc_check_user_access_denied(
    user_access: c_int,
    ua_add: c_int,
    ua_restricted: c_int,
    method_name: *const c_char,
) -> c_int {
    crate::nntptlsweb::xmlrpc_check_user_access_denied(
        user_access,
        ua_add,
        ua_restricted,
        input(method_name),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ntw_xmlrpc_ex_par_status(extra_par_blocks: c_int) -> *const c_char {
    crate::nntptlsweb::xmlrpc_ex_par_status(extra_par_blocks)
        .as_ptr()
        .cast::<c_char>()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ntw_xmlrpc_parse_dupe_mode(
    dupe_mode_str: *const c_char,
    dm_score: c_int,
    dm_all: c_int,
    dm_force: c_int,
) -> c_int {
    crate::nntptlsweb::xmlrpc_parse_dupe_mode(input(dupe_mode_str), dm_score, dm_all, dm_force)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ntw_xmlrpc_is_http_url(content: *const c_char) -> c_int {
    i32::from(crate::nntptlsweb::xmlrpc_is_http_url(input(content)))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ntw_xmlrpc_parse_category_source(
    cat_src_str: *const c_char,
    cs_nzb_file: c_int,
    cs_feed_file: c_int,
    cs_auto: c_int,
) -> c_int {
    crate::nntptlsweb::xmlrpc_parse_category_source(
        input(cat_src_str),
        cs_nzb_file,
        cs_feed_file,
        cs_auto,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ntw_xmlrpc_validate_reset_counter(
    counter: *const c_char,
) -> c_int {
    crate::nntptlsweb::xmlrpc_validate_reset_counter(input(counter))
}

// ============================================================================
// feedurlqueue FFI exports (nzbget_rs_fuq_*)
// ============================================================================

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_feed_coord_should_cleanup_history(
    cur_time: i64,
    last_cleanup: i64,
) -> c_int {
    i32::from(crate::feedurlqueue::feed_coord_should_cleanup_history(
        cur_time,
        last_cleanup,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_feed_coord_calc_wait_interval(
    url_force: c_int,
    feeds_empty: c_int,
) -> c_int {
    crate::feedurlqueue::feed_coord_calc_wait_interval(url_force != 0, feeds_empty != 0)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_feed_coord_is_hanging_download(
    cur_time: i64,
    last_update: i64,
    url_timeout: c_int,
    status: c_int,
    ad_running: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::feed_coord_is_hanging_download(
        cur_time,
        last_update,
        url_timeout,
        status,
        ad_running,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fuq_feed_coord_resolve_category(
    feed_cat: *const c_char,
    cat_source: c_int,
    cs_nzb_file: c_int,
    cs_auto: c_int,
    item_cat: *const c_char,
) -> *const c_char {
    const EMPTY: &[u8] = b"\0";
    let fc = input(feed_cat);
    let ic = input(item_cat);
    let res = crate::feedurlqueue::feed_coord_resolve_category(
        fc,
        cat_source,
        cs_nzb_file,
        cs_auto,
        ic,
    );
    if res.is_empty() {
        EMPTY.as_ptr().cast::<c_char>()
    } else if std::ptr::eq(res.as_ptr(), ic.as_ptr()) {
        item_cat
    } else {
        feed_cat
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fuq_feed_coord_classify_item_history(
    first_fetch: c_int,
    backlog: c_int,
    has_history: c_int,
    hs_unknown: c_int,
    hs_backlog: c_int,
    hs_fetched: c_int,
    out_should_add: *mut c_int,
) -> c_int {
    let (status, should_add) = crate::feedurlqueue::feed_coord_classify_item_history(
        first_fetch != 0,
        backlog != 0,
        has_history != 0,
        hs_unknown,
        hs_backlog,
        hs_fetched,
    );
    if !out_should_add.is_null() {
        *out_should_add = i32::from(should_add);
    }
    status
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_feed_coord_calc_history_border_date(
    oldest_update: i64,
    feed_history_days: c_int,
) -> i64 {
    crate::feedurlqueue::feed_coord_calc_history_border_date(oldest_update, feed_history_days)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_url_coord_should_start_download(
    has_nzb: c_int,
    pause_download: c_int,
    url_force: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::url_coord_should_start_download(
        has_nzb != 0,
        pause_download != 0,
        url_force != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_url_coord_calc_sleep_interval(download_started: c_int) -> c_int {
    crate::feedurlqueue::url_coord_calc_sleep_interval(download_started != 0)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_url_coord_is_completed_status(
    status: c_int,
    ad_finished: c_int,
    ad_failed: c_int,
    ad_retry: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::url_coord_is_completed_status(
        status,
        ad_finished,
        ad_failed,
        ad_retry,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_url_coord_resolve_scan_fail_status(
    add_status: c_int,
    as_failed: c_int,
    ls_scan_failed: c_int,
    ls_scan_skipped: c_int,
) -> c_int {
    crate::feedurlqueue::url_coord_resolve_scan_fail_status(
        add_status,
        as_failed,
        ls_scan_failed,
        ls_scan_skipped,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_can_start_article(
    has_more: c_int,
    can_proceed: c_int,
    download_held: c_int,
    extra_priority: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::queue_coord_can_start_article(
        has_more != 0,
        can_proceed != 0,
        download_held != 0,
        extra_priority != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_next_standby_wait(wait_interval: c_int) -> c_int {
    crate::feedurlqueue::queue_coord_next_standby_wait(wait_interval)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_active_sleep_interval(
    download_started: c_int,
) -> c_int {
    crate::feedurlqueue::queue_coord_active_sleep_interval(download_started != 0)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_is_smaller_dupe_file(
    size1: i64,
    idx1: c_int,
    size2: i64,
    idx2: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::queue_coord_is_smaller_dupe_file(
        size1, idx1, size2, idx2,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_is_higher_nzb_priority(
    has_file: c_int,
    nzb_extra: c_int,
    file_nzb_extra: c_int,
    nzb_prio: c_int,
    file_nzb_prio: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::queue_coord_is_higher_nzb_priority(
        has_file != 0,
        nzb_extra != 0,
        file_nzb_extra != 0,
        nzb_prio,
        file_nzb_prio,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_should_analyze_first_article(
    part_num: c_int,
    raw_article: c_int,
    direct_rename: c_int,
    dupe_fallback_enabled: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::queue_coord_should_analyze_first_article(
        part_num,
        raw_article != 0,
        direct_rename != 0,
        dupe_fallback_enabled != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_is_article_completed_status(
    status: c_int,
    ad_finished: c_int,
    ad_failed: c_int,
    ad_retry: c_int,
    ad_fatal_error: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::queue_coord_is_article_completed_status(
        status,
        ad_finished,
        ad_failed,
        ad_retry,
        ad_fatal_error,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_should_use_article_filename(
    file_naming: c_int,
    nf_article: c_int,
    nf_auto: c_int,
    is_obfuscated: c_int,
    many_dupe_files: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::queue_coord_should_use_article_filename(
        file_naming,
        nf_article,
        nf_auto,
        is_obfuscated != 0,
        many_dupe_files != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_should_delete_file_obj(
    file_completed: c_int,
    file_deleted: c_int,
    active_downloads: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::queue_coord_should_delete_file_obj(
        file_completed != 0,
        file_deleted != 0,
        active_downloads,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_should_skip_health_check(
    health_check: c_int,
    hc_none: c_int,
    hc_park: c_int,
    par_scan: c_int,
    ps_dupe: c_int,
    health_paused: c_int,
    delete_status: c_int,
    ds_health: c_int,
    health: c_int,
    critical_health: c_int,
    success_articles: c_int,
    total_articles: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::queue_coord_should_skip_health_check(
        health_check,
        hc_none,
        hc_park,
        par_scan,
        ps_dupe,
        health_paused != 0,
        delete_status,
        ds_health,
        health,
        critical_health,
        success_articles,
        total_articles,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_resolve_download_critical_health(
    critical_est: c_int,
    critical_exact: c_int,
    health_check: c_int,
    hc_dupe: c_int,
    certainly_parless: c_int,
    has_size_twin: c_int,
) -> c_int {
    crate::feedurlqueue::queue_coord_resolve_download_critical_health(
        critical_est,
        critical_exact,
        health_check,
        hc_dupe,
        certainly_parless != 0,
        has_size_twin != 0,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_is_queued_dupe_active(
    deleting: c_int,
    delete_status: c_int,
    ds_none: c_int,
    active_downloads: c_int,
    has_post_info: c_int,
    post_working: c_int,
    post_stage: c_int,
    pt_queued: c_int,
    extra_priority: c_int,
    self_completed_empty: c_int,
    direct_unpack_status: c_int,
    ns_running: c_int,
) -> c_int {
    i32::from(crate::feedurlqueue::queue_coord_is_queued_dupe_active(
        deleting != 0,
        delete_status,
        ds_none,
        active_downloads,
        has_post_info != 0,
        post_working != 0,
        post_stage,
        pt_queued,
        extra_priority != 0,
        self_completed_empty != 0,
        direct_unpack_status,
        ns_running,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_queue_coord_should_abort_dead_pick_failover(
    cur_success: c_int,
    dupe_recovered: c_int,
    recovered_at_start: c_int,
    min_alive_samples: c_int,
) -> c_int {
    i32::from(
        crate::feedurlqueue::queue_coord_should_abort_dead_pick_failover(
            cur_success,
            dupe_recovered,
            recovered_at_start,
            min_alive_samples,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_options_normalize_time_correction(raw_val: c_int) -> c_int {
    crate::feedurlqueue::options_normalize_time_correction(raw_val)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fuq_options_clamp_32bit_memory_limits(
    is_32bit: c_int,
    article_cache: c_int,
    par_buffer: c_int,
    out_article_cache: *mut c_int,
    out_par_buffer: *mut c_int,
) -> c_int {
    let (ac, pb, err) = crate::feedurlqueue::options_clamp_32bit_memory_limits(
        is_32bit != 0,
        article_cache,
        par_buffer,
    );
    if !out_article_cache.is_null() {
        *out_article_cache = ac;
    }
    if !out_par_buffer.is_null() {
        *out_par_buffer = pb;
    }
    err
}

#[no_mangle]
pub extern "C" fn nzbget_rs_fuq_prepost_should_complete_after_direct_rename(
    is_stopped: c_int,
    direct_rename_status: c_int,
    ts_success: c_int,
    has_post_info: c_int,
    delete_status: c_int,
    ds_none: c_int,
    completed_not_empty: c_int,
    download_completed: c_int,
    raw_article: c_int,
    skip_write: c_int,
    skip_disk_write: c_int,
) -> c_int {
    i32::from(
        crate::feedurlqueue::prepost_should_complete_after_direct_rename(
            is_stopped != 0,
            direct_rename_status,
            ts_success,
            has_post_info != 0,
            delete_status,
            ds_none,
            completed_not_empty != 0,
            download_completed != 0,
            raw_article != 0,
            skip_write != 0,
            skip_disk_write != 0,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fuq_repair_ctrl_calc_par_check_times(
    cur_dl_sec: c_int,
    start_dl_sec: c_int,
    cur_time: i64,
    par_time: i64,
    out_wait_time: *mut c_int,
    out_par_sec: *mut c_int,
) {
    let (wt, ps) = crate::feedurlqueue::repair_ctrl_calc_par_check_times(
        cur_dl_sec,
        start_dl_sec,
        cur_time,
        par_time,
    );
    if !out_wait_time.is_null() {
        *out_wait_time = wt;
    }
    if !out_par_sec.is_null() {
        *out_par_sec = ps;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fuq_direct_unpack_eval_unrar_result(
    exit_code: c_int,
    all_ok_msg: c_int,
    terminated: c_int,
    cancelled: c_int,
    stopped: c_int,
    out_unpack_ok: *mut c_int,
    out_interrupted: *mut c_int,
) {
    let (ok, intr) = crate::feedurlqueue::direct_unpack_eval_unrar_result(
        exit_code,
        all_ok_msg != 0,
        terminated != 0,
        cancelled != 0,
        stopped != 0,
    );
    if !out_unpack_ok.is_null() {
        *out_unpack_ok = i32::from(ok);
    }
    if !out_interrupted.is_null() {
        *out_interrupted = i32::from(intr);
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_parchecker_select_match_type(
    file_status: c_int,
    fs_success: c_int,
    fs_partial: c_int,
    match_full: c_int,
    match_partial: c_int,
    match_none: c_int,
) -> c_int {
    crate::parunpackpost::parchecker_select_match_type(
        file_status,
        fs_success,
        fs_partial,
        match_full,
        match_partial,
        match_none,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_parchecker_should_run_forced_full_check(
    status: c_int,
    ps_repair_not_needed: c_int,
    par_quick: c_int,
    force_repair: c_int,
    is_stopped: c_int,
) -> c_int {
    i32::from(crate::parunpackpost::parchecker_should_run_forced_full_check(
        status,
        ps_repair_not_needed,
        par_quick != 0,
        force_repair != 0,
        is_stopped != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_parchecker_should_add_missing_files(
    has_damaged: c_int,
    is_stopped: c_int,
    missing_file_count: u32,
    added_split: c_int,
    res: c_int,
    e_repair_possible: c_int,
    par_scan: c_int,
    ps_extended: c_int,
    ps_dupe: c_int,
) -> c_int {
    i32::from(crate::parunpackpost::parchecker_should_add_missing_files(
        has_damaged != 0,
        is_stopped != 0,
        missing_file_count,
        added_split != 0,
        res,
        e_repair_possible,
        par_scan,
        ps_extended,
        ps_dupe,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_parchecker_calc_missing_blocks(
    missing_block_count: u32,
    recovery_packet_count: usize,
) -> c_int {
    crate::parunpackpost::parchecker_calc_missing_blocks(
        missing_block_count,
        recovery_packet_count,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_parchecker_should_abort_extra_scan(
    only_missing: c_int,
    missing_file_count: u32,
    missing_block_count: u32,
) -> c_int {
    crate::parunpackpost::parchecker_should_abort_extra_scan(
        only_missing != 0,
        missing_file_count,
        missing_block_count,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_parchecker_should_print_stage_msg(
    stage: c_int,
    pt_verifying_repaired: c_int,
    par_quick: c_int,
) -> c_int {
    i32::from(crate::parunpackpost::parchecker_should_print_stage_msg(
        stage,
        pt_verifying_repaired,
        par_quick != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_unpack_ctrl_should_try_par_before_pass_file(
    unpack_ok: c_int,
    has_par_files: c_int,
    password_err: c_int,
    par_status: c_int,
    ps_skipped: c_int,
) -> c_int {
    i32::from(
        crate::parunpackpost::unpack_ctrl_should_try_par_before_pass_file(
            unpack_ok != 0,
            has_par_files != 0,
            password_err != 0,
            par_status,
            ps_skipped,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_unpack_ctrl_should_try_password_file(
    unpack_ok: c_int,
    start_err: c_int,
    space_err: c_int,
    decrypt_err: c_int,
    password_err: c_int,
    terminated: c_int,
    auto_terminated: c_int,
    password_empty: c_int,
    pass_file_empty: c_int,
) -> c_int {
    i32::from(crate::parunpackpost::unpack_ctrl_should_try_password_file(
        unpack_ok != 0,
        start_err != 0,
        space_err != 0,
        decrypt_err != 0,
        password_err != 0,
        terminated != 0,
        auto_terminated != 0,
        password_empty != 0,
        pass_file_empty != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pup_unpack_ctrl_eval_sevenzip_exit(
    exit_code: c_int,
    all_ok_msg: c_int,
    terminated: c_int,
    auto_terminated: c_int,
    out_unpack_ok: *mut c_int,
    out_start_err: *mut c_int,
) {
    let (ok, start_err) = crate::parunpackpost::unpack_ctrl_eval_sevenzip_exit(
        exit_code,
        all_ok_msg != 0,
        terminated != 0,
        auto_terminated != 0,
    );
    if !out_unpack_ok.is_null() {
        *out_unpack_ok = i32::from(ok);
    }
    if !out_start_err.is_null() {
        *out_start_err = i32::from(start_err);
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_unpack_ctrl_will_retry_with_pass(
    decrypt_err: c_int,
    password_err: c_int,
    password_empty: c_int,
    pass_file_not_empty: c_int,
) -> c_int {
    i32::from(crate::parunpackpost::unpack_ctrl_will_retry_with_pass(
        decrypt_err != 0,
        password_err != 0,
        password_empty != 0,
        pass_file_not_empty != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_unpack_ctrl_is_stopped_for_reload(
    auto_terminated: c_int,
    post_deleted: c_int,
    is_stopped: c_int,
    terminated: c_int,
) -> c_int {
    i32::from(crate::parunpackpost::unpack_ctrl_is_stopped_for_reload(
        auto_terminated != 0,
        post_deleted != 0,
        is_stopped != 0,
        terminated != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_unpack_ctrl_should_request_par_after_fail(
    unpack_ok: c_int,
    par_status: c_int,
    ps_skipped: c_int,
    par_full: c_int,
    start_err: c_int,
    space_err: c_int,
    password_err: c_int,
    terminated: c_int,
    is_stopped: c_int,
    auto_terminated: c_int,
    has_par_files: c_int,
) -> c_int {
    i32::from(
        crate::parunpackpost::unpack_ctrl_should_request_par_after_fail(
            unpack_ok != 0,
            par_status,
            ps_skipped,
            par_full != 0,
            start_err != 0,
            space_err != 0,
            password_err != 0,
            terminated != 0,
            is_stopped != 0,
            auto_terminated != 0,
            has_par_files != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_unpack_ctrl_calc_force_repair_on_request(
    password_empty: c_int,
    pass_file_empty: c_int,
    pass_list_tried: c_int,
    decrypt_err: c_int,
    password_err: c_int,
    par_status: c_int,
    ps_skipped: c_int,
) -> c_int {
    i32::from(
        crate::parunpackpost::unpack_ctrl_calc_force_repair_on_request(
            password_empty != 0,
            pass_file_empty != 0,
            pass_list_tried != 0,
            decrypt_err != 0,
            password_err != 0,
            par_status,
            ps_skipped,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_history_coord_calc_min_time(
    cur_time: i64,
    keep_history_days: c_int,
) -> i64 {
    crate::parunpackpost::history_coord_calc_min_time(cur_time, keep_history_days)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_history_coord_should_count_parked_file(
    status: c_int,
    cf_none: c_int,
    cf_partial: c_int,
    is_last: c_int,
) -> c_int {
    i32::from(crate::parunpackpost::history_coord_should_count_parked_file(
        status,
        cf_none,
        cf_partial,
        is_last != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pup_history_coord_should_recheck_failed_articles(
    daf_none: c_int,
    hc_dupe: c_int,
    kind_nzb: c_int,
    failed_articles: c_int,
    text_status: *const c_char,
    has_dupe_recheck_param: c_int,
) -> c_int {
    let ts = if text_status.is_null() {
        &b""[..]
    } else {
        CStr::from_ptr(text_status).to_bytes()
    };
    i32::from(
        crate::parunpackpost::history_coord_should_recheck_failed_articles(
            daf_none != 0,
            hc_dupe != 0,
            kind_nzb != 0,
            failed_articles,
            ts,
            has_dupe_recheck_param != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_history_coord_should_delete_dest_dir(
    is_nzb: c_int,
    delete_status: c_int,
    ds_none: c_int,
    par_status: c_int,
    ps_failure: c_int,
    unpack_status: c_int,
    us_failure: c_int,
    us_password: c_int,
    dir_exists: c_int,
) -> c_int {
    i32::from(crate::parunpackpost::history_coord_should_delete_dest_dir(
        is_nzb != 0,
        delete_status,
        ds_none,
        par_status,
        ps_failure,
        unpack_status,
        us_failure,
        us_password,
        dir_exists != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_history_coord_should_erase_on_delete(
    final_delete: c_int,
    dupe_check: c_int,
    is_url: c_int,
) -> c_int {
    i32::from(crate::parunpackpost::history_coord_should_erase_on_delete(
        final_delete != 0,
        dupe_check != 0,
        is_url != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pup_history_coord_is_temporary_output_name(
    filename: *const c_char,
) -> c_int {
    let s = if filename.is_null() {
        &b""[..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    i32::from(crate::parunpackpost::history_coord_is_temporary_output_name(s))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pup_history_coord_parse_dupe_backup_flag(
    text: *const c_char,
    ds_dupe: c_int,
    ds_manual: c_int,
) -> c_int {
    let s = if text.is_null() {
        &b""[..]
    } else {
        CStr::from_ptr(text).to_bytes()
    };
    crate::parunpackpost::history_coord_parse_dupe_backup_flag(s, ds_dupe, ds_manual)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_scanner_select_service_interval(
    requested_scan: c_int,
    nzb_dir_interval_opt: c_int,
    cur_interval: c_int,
    now_val: c_int,
    sleep_val: c_int,
) -> c_int {
    crate::parunpackpost::scanner_select_service_interval(
        requested_scan != 0,
        nzb_dir_interval_opt,
        cur_interval,
        now_val,
        sleep_val,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pup_scanner_should_skip_item(
    filename: *const c_char,
    processed_dir: *const c_char,
    broken_dir: *const c_char,
) -> c_int {
    let f = if filename.is_null() {
        &b""[..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    let p = if processed_dir.is_null() {
        &b""[..]
    } else {
        CStr::from_ptr(processed_dir).to_bytes()
    };
    let b = if broken_dir.is_null() {
        &b""[..]
    } else {
        CStr::from_ptr(broken_dir).to_bytes()
    };
    i32::from(crate::parunpackpost::scanner_should_skip_item(f, p, b))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pup_scanner_is_absolute_nzb_name(
    nzb_name: *const c_char,
) -> c_int {
    let s = if nzb_name.is_null() {
        &b""[..]
    } else {
        CStr::from_ptr(nzb_name).to_bytes()
    };
    i32::from(crate::parunpackpost::scanner_is_absolute_nzb_name(s))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_queue_editor_is_history_or_post_without_ids(
    action: c_int,
    ea_post_delete: c_int,
    ea_history_delete: c_int,
    ea_history_set_name: c_int,
    has_id_list: c_int,
) -> c_int {
    i32::from(
        crate::parunpackpost::queue_editor_is_history_or_post_without_ids(
            action,
            ea_post_delete,
            ea_history_delete,
            ea_history_set_name,
            has_id_list != 0,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pup_nzb_info_strip_nzb_ext_len(
    nicename: *const c_char,
    remove_ext: c_int,
) -> usize {
    let s = if nicename.is_null() {
        &b""[..]
    } else {
        CStr::from_ptr(nicename).to_bytes()
    };
    crate::parunpackpost::nzb_info_strip_nzb_ext(s, remove_ext != 0).len()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pup_nzb_info_calc_active_downloads_sec(
    old_active: c_int,
    new_active: c_int,
    is_nzb_kind: c_int,
    cur_time: i64,
    download_sec: c_int,
    download_start_time: i64,
    download_start_sec: c_int,
    out_download_sec: *mut c_int,
    out_download_start_time: *mut i64,
    out_download_start_sec: *mut c_int,
    out_changed: *mut c_int,
) {
    let (sec, start_time, start_sec, changed) =
        crate::parunpackpost::nzb_info_calc_active_downloads_sec(
            old_active,
            new_active,
            is_nzb_kind != 0,
            cur_time,
            download_sec,
            download_start_time,
            download_start_sec,
        );
    if !out_download_sec.is_null() {
        *out_download_sec = sec;
    }
    if !out_download_start_time.is_null() {
        *out_download_start_time = start_time;
    }
    if !out_download_start_sec.is_null() {
        *out_download_start_sec = start_sec;
    }
    if !out_changed.is_null() {
        *out_changed = i32::from(changed);
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_file_info_blocks_download_completion(
    paused: c_int,
    ignore_paused_pars: c_int,
    par_file: c_int,
    deleted: c_int,
) -> c_int {
    i32::from(
        crate::parunpackpost::file_info_blocks_download_completion(
            paused != 0,
            ignore_paused_pars != 0,
            par_file != 0,
            deleted != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pup_history_coord_can_attempt_requeue_file(
    status: c_int,
    cf_success: c_int,
    cf_failure: c_int,
    reset_failed: c_int,
    file_id: c_int,
) -> c_int {
    i32::from(
        crate::parunpackpost::history_coord_can_attempt_requeue_file(
            status,
            cf_success,
            cf_failure,
            reset_failed != 0,
            file_id,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_content_source_range_valid(
    offset: i64,
    size: i64,
    total_size: i64,
) -> c_int {
    i32::from(crate::contentdupenntp::content_source_range_valid(
        offset, size, total_size,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cdn_content_map_calc_decrypt_cipher_bounds(
    inner_offset: i64,
    inner_size: i64,
    inner_size_total: i64,
    block_size: i64,
    cipher_stream_size: i64,
    out_empty: *mut c_int,
    out_cipher_from: *mut i64,
    out_cipher_to: *mut i64,
) -> c_int {
    let (valid, empty, from, to) =
        crate::contentdupenntp::content_map_calc_decrypt_cipher_bounds(
            inner_offset,
            inner_size,
            inner_size_total,
            block_size,
            cipher_stream_size,
        );
    if !out_empty.is_null() {
        *out_empty = i32::from(empty);
    }
    if !out_cipher_from.is_null() {
        *out_cipher_from = from;
    }
    if !out_cipher_to.is_null() {
        *out_cipher_to = to;
    }
    i32::from(valid)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_content_mapper_is_compressible_archive(
    format: c_int,
    mf_rar: c_int,
    mf_zip: c_int,
    mf_seven_zip: c_int,
) -> c_int {
    i32::from(
        crate::contentdupenntp::content_mapper_is_compressible_archive(
            format,
            mf_rar,
            mf_zip,
            mf_seven_zip,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_content_mapper_rar_cipher_geometry_valid(
    inner_size: i64,
    cipher_total: i64,
) -> c_int {
    i32::from(
        crate::contentdupenntp::content_mapper_rar_cipher_geometry_valid(inner_size, cipher_total),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_content_mapper_rar_calc_plain_chunk(
    packed_size: i64,
    inner_size: i64,
    cipher_base: i64,
) -> i64 {
    crate::contentdupenntp::content_mapper_rar_calc_plain_chunk(
        packed_size,
        inner_size,
        cipher_base,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cdn_content_mapper_rar_infer_unknown_packed(
    unknown_volume: c_int,
    inner_size: i64,
    known_packed: i64,
    out_inferred_packed: *mut i64,
) -> c_int {
    let (status, inferred) = crate::contentdupenntp::content_mapper_rar_infer_unknown_packed(
        unknown_volume,
        inner_size,
        known_packed,
    );
    if !out_inferred_packed.is_null() {
        *out_inferred_packed = inferred;
    }
    status
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_dupesearch_classify_search_retry_reason(
    queries: c_int,
    failed: c_int,
    verified_empty: c_int,
    fetched_empty: c_int,
    refused: c_int,
    fetch_err: c_int,
    not_tried: c_int,
) -> c_int {
    crate::contentdupenntp::dupesearch_classify_search_retry_reason(
        queries,
        failed,
        verified_empty != 0,
        fetched_empty != 0,
        refused,
        fetch_err,
        not_tried,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_dupesearch_calc_fast_donor_count(
    dupe_fast_donors: c_int,
    live_count: usize,
    cap: c_int,
) -> usize {
    crate::contentdupenntp::dupesearch_calc_fast_donor_count(dupe_fast_donors, live_count, cap)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_dupesearch_alive_to_percent(alive: f64) -> c_int {
    crate::contentdupenntp::dupesearch_alive_to_percent(alive)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_dupesearch_fast_donor_alive_param(
    is_dead: c_int,
    alive: f64,
) -> c_int {
    crate::contentdupenntp::dupesearch_fast_donor_alive_param(is_dead != 0, alive)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cdn_dupe_fallback_is_par_file(
    par_file: c_int,
    first_content: c_int,
    fc_par2: c_int,
    filename_confirmed: c_int,
    filename: *const c_char,
) -> c_int {
    let fname = if filename.is_null() {
        &b""[..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    i32::from(crate::contentdupenntp::dupe_fallback_is_par_file(
        par_file != 0,
        first_content,
        fc_par2,
        filename_confirmed != 0,
        fname,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_dupe_fallback_should_skip_first_content(
    content: c_int,
    fc_par2: c_int,
    fc_unknown: c_int,
    part_number: c_int,
    has_extension: c_int,
) -> c_int {
    i32::from(
        crate::contentdupenntp::dupe_fallback_should_skip_first_content(
            content,
            fc_par2,
            fc_unknown,
            part_number,
            has_extension != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_dupe_fallback_is_nzb_or_file_ineligible(
    file_deleted: c_int,
    nzb_deleting: c_int,
    nzb_parking: c_int,
    delete_status: c_int,
    ds_none: c_int,
    dupe_mode: c_int,
    dm_force: c_int,
) -> c_int {
    i32::from(
        crate::contentdupenntp::dupe_fallback_is_nzb_or_file_ineligible(
            file_deleted != 0,
            nzb_deleting != 0,
            nzb_parking != 0,
            delete_status,
            ds_none,
            dupe_mode,
            dm_force,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cdn_dupe_fallback_eval_par_defer(
    par_defer: c_int,
    part_number: c_int,
    dupe_par_defer_state: c_int,
    dp_deferred: c_int,
    dp_lifted: c_int,
    par_cannot_cover: c_int,
    damage_certain: c_int,
    damage_projected: c_int,
    out_lift_defer_state: *mut c_int,
    out_log_lifted: *mut c_int,
) -> c_int {
    let (defer, lift, log) = crate::contentdupenntp::dupe_fallback_eval_par_defer(
        par_defer != 0,
        part_number,
        dupe_par_defer_state,
        dp_deferred,
        dp_lifted,
        par_cannot_cover != 0,
        damage_certain != 0,
        damage_projected != 0,
    );
    if !out_lift_defer_state.is_null() {
        *out_lift_defer_state = i32::from(lift);
    }
    if !out_log_lifted.is_null() {
        *out_log_lifted = i32::from(log);
    }
    i32::from(defer)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cdn_dupe_fallback_classify_first_head(
    head_ptr: *const u8,
    head_len: usize,
) -> c_int {
    if head_ptr.is_null() || head_len < 8 {
        return 0;
    }
    let slice = std::slice::from_raw_parts(head_ptr, head_len);
    crate::contentdupenntp::dupe_fallback_classify_first_head(slice)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_stream_repair_is_nzb_eligible_for_job(
    has_nzb: c_int,
    delete_status: c_int,
    ds_none: c_int,
    ds_health: c_int,
    deleting: c_int,
    parking: c_int,
    has_post_info: c_int,
) -> c_int {
    i32::from(
        crate::contentdupenntp::stream_repair_is_nzb_eligible_for_job(
            has_nzb != 0,
            delete_status,
            ds_none,
            ds_health,
            deleting != 0,
            parking != 0,
            has_post_info != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_dupecoord_same_content_hash(
    full1: u32,
    filtered1: u32,
    full2: u32,
    filtered2: u32,
) -> c_int {
    i32::from(crate::contentdupenntp::dupecoord_same_content_hash(
        full1, filtered1, full2, filtered2,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_dupecoord_should_keep_returned_dupe(
    health_check: c_int,
    hc_dupe: c_int,
    dupe_hint: c_int,
    dh_redownload_auto: c_int,
    current_success_articles: c_int,
    health: c_int,
    critical_health: c_int,
) -> c_int {
    i32::from(crate::contentdupenntp::dupecoord_should_keep_returned_dupe(
        health_check,
        hc_dupe,
        dupe_hint,
        dh_redownload_auto,
        current_success_articles,
        health,
        critical_health,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cdn_stream_repair_check_fetched_member_part(
    success: c_int,
    data_len: usize,
    offset: i64,
    fetched_file_size: i64,
    member_size: i64,
    out_mark_member_bad: *mut c_int,
) -> c_int {
    let (invalid, mark_bad) = crate::contentdupenntp::stream_repair_check_fetched_member_part(
        success != 0,
        data_len,
        offset,
        fetched_file_size,
        member_size,
    );
    if !out_mark_member_bad.is_null() {
        *out_mark_member_bad = i32::from(mark_bad);
    }
    i32::from(invalid)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cdn_stream_repair_eval_slow_rate(
    download_rate: i64,
    recovered: i64,
    remaining: i64,
    elapsed_sec: f64,
    timeout_sec: c_int,
    slow_repair_factor: c_int,
    out_repair_mb_s: *mut f64,
    out_remaining_mb: *mut f64,
    out_repair_sec: *mut f64,
    out_download_mb_s: *mut f64,
) -> c_int {
    let (is_slow, repair_mb_s, rem_mb, repair_sec, dl_mb_s) =
        crate::contentdupenntp::stream_repair_eval_slow_rate(
            download_rate,
            recovered,
            remaining,
            elapsed_sec,
            timeout_sec,
            slow_repair_factor,
        );
    if !out_repair_mb_s.is_null() {
        *out_repair_mb_s = repair_mb_s;
    }
    if !out_remaining_mb.is_null() {
        *out_remaining_mb = rem_mb;
    }
    if !out_repair_sec.is_null() {
        *out_repair_sec = repair_sec;
    }
    if !out_download_mb_s.is_null() {
        *out_download_mb_s = dl_mb_s;
    }
    i32::from(is_slow)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_stream_repair_is_incomplete_nzb_hole(
    missing: i64,
    encoded_size: i64,
) -> c_int {
    i32::from(crate::contentdupenntp::stream_repair_is_incomplete_nzb_hole(
        missing,
        encoded_size,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_stream_repair_materialize_article_valid(
    success: c_int,
    data_len: usize,
    offset: i64,
    fetched_file_size: i64,
    member_size: i64,
) -> c_int {
    i32::from(
        crate::contentdupenntp::stream_repair_materialize_article_valid(
            success != 0,
            data_len,
            offset,
            fetched_file_size,
            member_size,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cdn_stream_repair_credit_repaired_sizes(
    recovered_failed_size: i64,
    recovered_failed_par_size: i64,
    inout_failed_size: *mut i64,
    inout_success_size: *mut i64,
    inout_cur_failed_size: *mut i64,
    inout_cur_success_size: *mut i64,
    inout_par_failed_size: *mut i64,
    inout_par_success_size: *mut i64,
    inout_par_cur_failed_size: *mut i64,
    inout_par_cur_success_size: *mut i64,
) {
    if inout_failed_size.is_null()
        || inout_success_size.is_null()
        || inout_cur_failed_size.is_null()
        || inout_cur_success_size.is_null()
        || inout_par_failed_size.is_null()
        || inout_par_success_size.is_null()
        || inout_par_cur_failed_size.is_null()
        || inout_par_cur_success_size.is_null()
    {
        return;
    }
    let (nf, ns, ncf, ncs, npf, nps, npcf, npcs) =
        crate::contentdupenntp::stream_repair_credit_repaired_sizes(
            recovered_failed_size,
            recovered_failed_par_size,
            *inout_failed_size,
            *inout_success_size,
            *inout_cur_failed_size,
            *inout_cur_success_size,
            *inout_par_failed_size,
            *inout_par_success_size,
            *inout_par_cur_failed_size,
            *inout_par_cur_success_size,
        );
    *inout_failed_size = nf;
    *inout_success_size = ns;
    *inout_cur_failed_size = ncf;
    *inout_cur_success_size = ncs;
    *inout_par_failed_size = npf;
    *inout_par_success_size = nps;
    *inout_par_cur_failed_size = npcf;
    *inout_par_cur_success_size = npcs;
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_twin_check_eval_sample_verdict(
    is_stopped: c_int,
    compared: c_int,
    differ: c_int,
    sample_count: c_int,
) -> c_int {
    crate::contentdupenntp::twin_check_eval_sample_verdict(
        is_stopped != 0,
        compared,
        differ,
        sample_count,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_par_damage_calc_queued_volume_blocks(
    parsed_blocks: c_int,
    file_size: i64,
    block_size: u64,
) -> c_int {
    crate::contentdupenntp::par_damage_calc_queued_volume_blocks(
        parsed_blocks,
        file_size,
        block_size,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_par_damage_calc_completed_volume_blocks(
    parsed_blocks: c_int,
    disk_size: i64,
    block_size: u64,
) -> c_int {
    crate::contentdupenntp::par_damage_calc_completed_volume_blocks(
        parsed_blocks,
        disk_size,
        block_size,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_cdn_article_fetcher_eval_decoded_yenc(
    check_status: c_int,
    ds_finished: c_int,
    format: c_int,
    ef_yenc: c_int,
    begin_pos: i64,
    end_pos: i64,
    size: i64,
    data_len: usize,
    out_offset: *mut i64,
    out_file_size: *mut i64,
    out_success: *mut c_int,
) -> c_int {
    let (valid, offset, file_size, success) =
        crate::contentdupenntp::article_fetcher_eval_decoded_yenc(
            check_status,
            ds_finished,
            format,
            ef_yenc,
            begin_pos,
            end_pos,
            size,
            data_len,
        );
    if !out_offset.is_null() {
        *out_offset = offset;
    }
    if !out_file_size.is_null() {
        *out_file_size = file_size;
    }
    if !out_success.is_null() {
        *out_success = i32::from(success);
    }
    i32::from(valid)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_cdn_article_batch_can_claim_next(
    batch_id: c_int,
    cur_batch_id: c_int,
    cancelled: c_int,
    stopped: c_int,
    next_claim: usize,
    requests_len: usize,
    next_deliver: usize,
    window_parts: c_int,
    buffered_bytes: i64,
    max_buffered_bytes: i64,
) -> c_int {
    i32::from(crate::contentdupenntp::article_batch_can_claim_next(
        batch_id,
        cur_batch_id,
        cancelled != 0,
        stopped != 0,
        next_claim,
        requests_len,
        next_deliver,
        window_parts,
        buffered_bytes,
        max_buffered_bytes,
    ))
}

// ============================================================================
// DiskState, ArticleWriter, ArticleDownloader & PostProcess FFI (`diskartdlpost.rs`)
// ============================================================================

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_diskstate_validate_dupe_extras_header(
    scan_cnt: c_int,
    recovered_articles: c_int,
    recovered_holes: c_int,
) -> c_int {
    i32::from(crate::diskartdlpost::diskstate_validate_dupe_extras_header(
        scan_cnt,
        recovered_articles,
        recovered_holes,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_diskstate_validate_repair_job_header(
    scan_cnt: c_int,
    expected_cnt: c_int,
    file_id: c_int,
    par_file: c_int,
    failed_articles: c_int,
) -> c_int {
    i32::from(crate::diskartdlpost::diskstate_validate_repair_job_header(
        scan_cnt,
        expected_cnt,
        file_id,
        par_file,
        failed_articles,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_diskstate_should_apply_url_status(
    kind: c_int,
    nk_nzb: c_int,
    url_status: c_int,
    ls_failed: c_int,
) -> c_int {
    i32::from(crate::diskartdlpost::diskstate_should_apply_url_status(
        kind, nk_nzb, url_status, ls_failed,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_diskstate_normalize_script_status(
    status: c_int,
    format_version: c_int,
) -> c_int {
    crate::diskartdlpost::diskstate_normalize_script_status(status, format_version)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_diskstate_eval_restored_output_init(
    output_initialized: c_int,
    direct_write: c_int,
    force_direct_write: c_int,
    has_own_out_file: c_int,
    has_out_tmp: c_int,
    has_force_dest: c_int,
) -> c_int {
    i32::from(crate::diskartdlpost::diskstate_eval_restored_output_init(
        output_initialized,
        direct_write != 0,
        force_direct_write != 0,
        has_own_out_file != 0,
        has_out_tmp != 0,
        has_force_dest != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dad_diskstate_calc_staged_fallback(
    completed: c_int,
    article_status: c_int,
    ai_finished: c_int,
    dupe_fallback_round: c_int,
    has_result_filename: c_int,
    ai_undefined: c_int,
    out_saved_status: *mut c_int,
) -> c_int {
    let (staged, saved_status) = crate::diskartdlpost::diskstate_calc_staged_fallback(
        completed != 0,
        article_status,
        ai_finished,
        dupe_fallback_round,
        has_result_filename != 0,
        ai_undefined,
    );
    if !out_saved_status.is_null() {
        *out_saved_status = saved_status;
    }
    staged
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_diskstate_validate_article_count(
    size: c_int,
    has_articles: c_int,
    articles_len: c_int,
) -> c_int {
    i32::from(crate::diskartdlpost::diskstate_validate_article_count(
        size,
        has_articles != 0,
        articles_len,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_diskstate_need_staged_file_check(
    completed: c_int,
    format_version: c_int,
    status_int: c_int,
    ai_finished: c_int,
    segment_offset: i64,
    segment_size: c_int,
    staged_fallback: c_int,
    part_number: c_int,
    direct_write: c_int,
    force_direct_write: c_int,
) -> c_int {
    i32::from(crate::diskartdlpost::diskstate_need_staged_file_check(
        completed != 0,
        format_version,
        status_int,
        ai_finished,
        segment_offset,
        segment_size,
        staged_fallback,
        part_number,
        direct_write != 0,
        force_direct_write != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dad_diskstate_is_digits_only_filename(
    filename: *const c_char,
) -> c_int {
    let bytes = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    i32::from(crate::diskartdlpost::diskstate_is_digits_only_filename(
        bytes,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_diskstate_should_load_file_state(
    suffix: c_char,
    continue_partial: c_int,
    cache_was_active: c_int,
) -> c_int {
    i32::from(crate::diskartdlpost::diskstate_should_load_file_state(
        suffix as u8,
        continue_partial != 0,
        cache_was_active != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dad_diskstate_calc_volume_slot_size(
    slot_index: c_int,
    arr_size: c_int,
    out_target_size: *mut c_int,
) -> c_int {
    let (valid, target_size) =
        crate::diskartdlpost::diskstate_calc_volume_slot_size(slot_index, arr_size);
    if !out_target_size.is_null() {
        *out_target_size = target_size;
    }
    i32::from(valid)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_article_writer_should_check_dupe_file(
    format: c_int,
    ef_yenc: c_int,
    dupe_check: c_int,
    dupe_mode: c_int,
    dm_force: c_int,
    many_dupe_files: c_int,
) -> c_int {
    i32::from(
        crate::diskartdlpost::article_writer_should_check_dupe_file(
            format,
            ef_yenc,
            dupe_check != 0,
            dupe_mode,
            dm_force,
            many_dupe_files != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_article_writer_is_duplicate_dest(
    output_initialized: c_int,
    is_own_output_file: c_int,
    has_filename: c_int,
    dest_exists: c_int,
) -> c_int {
    i32::from(crate::diskartdlpost::article_writer_is_duplicate_dest(
        output_initialized != 0,
        is_own_output_file != 0,
        has_filename != 0,
        dest_exists != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_article_writer_can_use_article_cache(
    article_cache_mb: c_int,
    raw_article: c_int,
    format: c_int,
    ef_yenc: c_int,
    is_dupe_fallback: c_int,
    is_critical: c_int,
) -> c_int {
    i32::from(crate::diskartdlpost::article_writer_can_use_article_cache(
        article_cache_mb,
        raw_article != 0,
        format,
        ef_yenc,
        is_dupe_fallback != 0,
        is_critical != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dad_article_writer_advance_and_check_range(
    raw_article: c_int,
    inout_ptr: *mut c_int,
    len: c_int,
    article_size: c_int,
) -> c_int {
    if inout_ptr.is_null() {
        return 0;
    }
    let (new_ptr, exceeded) = crate::diskartdlpost::article_writer_advance_and_check_range(
        raw_article != 0,
        *inout_ptr,
        len,
        article_size,
    );
    *inout_ptr = new_ptr;
    i32::from(exceeded)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dad_article_writer_resolve_direct_write_ready(
    direct_write_opt: c_int,
    force_direct_write: c_int,
    output_initialized: c_int,
    has_own_out: c_int,
    has_force_dest: c_int,
    out_output_initialized: *mut c_int,
) -> c_int {
    let (new_init, direct_write) =
        crate::diskartdlpost::article_writer_resolve_direct_write_ready(
            direct_write_opt != 0,
            force_direct_write != 0,
            output_initialized != 0,
            has_own_out != 0,
            has_force_dest != 0,
        );
    if !out_output_initialized.is_null() {
        *out_output_initialized = i32::from(new_init);
    }
    i32::from(direct_write)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_article_writer_need_gap_fill(
    raw_article: c_int,
    direct_write: c_int,
    segment_offset: i64,
    out_pos: i64,
) -> c_int {
    i32::from(crate::diskartdlpost::article_writer_need_gap_fill(
        raw_article != 0,
        direct_write != 0,
        segment_offset,
        out_pos,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dad_article_cache_can_alloc(
    effective_limit: usize,
    allocated: usize,
    size: usize,
    out_was_empty: *mut c_int,
) -> c_int {
    let (can_alloc, was_empty) =
        crate::diskartdlpost::article_cache_can_alloc(effective_limit, allocated, size);
    if !out_was_empty.is_null() {
        *out_was_empty = i32::from(was_empty);
    }
    i32::from(can_alloc)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dad_memory_monitor_calc_critical_or_normal_par_buffer(
    tier: c_int,
    configured_mb: c_int,
    tier_critical: c_int,
    tier_normal: c_int,
    out_mb: *mut c_int,
) -> c_int {
    let (handled, mb) =
        crate::diskartdlpost::memory_monitor_calc_critical_or_normal_par_buffer(
            tier,
            configured_mb,
            tier_critical,
            tier_normal,
        );
    if !out_mb.is_null() {
        *out_mb = mb;
    }
    i32::from(handled)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dad_memory_monitor_resolve_tier_limits(
    next_tier: c_int,
    raw_sample: c_int,
    cfg_cache: u32,
    cfg_par: u32,
    cfg_limit: u32,
    tier_normal: c_int,
    tier_constrained: c_int,
    tier_critical: c_int,
    rs_eff_cache: u32,
    rs_eff_par: u32,
    out_eff_cache: *mut u32,
    out_eff_par: *mut u32,
    out_eff_limit: *mut u32,
) {
    let (eff_cache, eff_par, eff_limit) =
        crate::diskartdlpost::memory_monitor_resolve_tier_limits(
            next_tier,
            raw_sample,
            cfg_cache,
            cfg_par,
            cfg_limit,
            tier_normal,
            tier_constrained,
            tier_critical,
            rs_eff_cache,
            rs_eff_par,
        );
    if !out_eff_cache.is_null() {
        *out_eff_cache = eff_cache;
    }
    if !out_eff_par.is_null() {
        *out_eff_par = eff_par;
    }
    if !out_eff_limit.is_null() {
        *out_eff_limit = eff_limit;
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_article_dl_should_abort_for_work_state(
    is_stopped: c_int,
    pause_download: c_int,
    quota_reached: c_int,
    force: c_int,
    download_held: c_int,
    extra_priority: c_int,
    gen_changed: c_int,
) -> c_int {
    i32::from(
        crate::diskartdlpost::article_dl_should_abort_for_work_state(
            is_stopped != 0,
            pause_download != 0,
            quota_reached != 0,
            force != 0,
            download_held != 0,
            extra_priority != 0,
            gen_changed != 0,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dad_article_dl_classify_attempt_stat(
    status: c_int,
    ad_finished: c_int,
    ad_failed: c_int,
    ad_not_found: c_int,
    ad_crc_error: c_int,
    out_success: *mut c_int,
    out_failed: *mut c_int,
) -> c_int {
    let (record, success, failed) = crate::diskartdlpost::article_dl_classify_attempt_stat(
        status,
        ad_finished,
        ad_failed,
        ad_not_found,
        ad_crc_error,
    );
    if !out_success.is_null() {
        *out_success = success;
    }
    if !out_failed.is_null() {
        *out_failed = failed;
    }
    i32::from(record)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_article_dl_should_retry_same_server(
    connected: c_int,
    status: c_int,
    ad_failed: c_int,
    remained_retries: c_int,
    retention_failure: c_int,
) -> c_int {
    i32::from(crate::diskartdlpost::article_dl_should_retry_same_server(
        connected != 0,
        status,
        ad_failed,
        remained_retries,
        retention_failure != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dad_article_dl_should_advance_server_list(
    has_want_server: c_int,
    connected: c_int,
    retention_failure: c_int,
    optional_blocked: c_int,
    auth_rejected: c_int,
    out_push_failed_server: *mut c_int,
) -> c_int {
    let (advance, push_failed) = crate::diskartdlpost::article_dl_should_advance_server_list(
        has_want_server != 0,
        connected != 0,
        retention_failure != 0,
        optional_blocked != 0,
        auth_rejected != 0,
    );
    if !out_push_failed_server.is_null() {
        *out_push_failed_server = i32::from(push_failed);
    }
    i32::from(advance)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_article_dl_should_pace_critical_uncached(
    is_stopped: c_int,
    has_cached_buffer: c_int,
    is_critical: c_int,
    is_flushing: c_int,
) -> c_int {
    i32::from(
        crate::diskartdlpost::article_dl_should_pace_critical_uncached(
            is_stopped != 0,
            has_cached_buffer != 0,
            is_critical != 0,
            is_flushing != 0,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dad_direct_unpack_eval_post_run_actions(
    unpack_ok: c_int,
    cancelled: c_int,
    is_stopped: c_int,
    terminated: c_int,
    out_should_cleanup: *mut c_int,
) -> c_int {
    let (interrupted, should_cleanup) =
        crate::diskartdlpost::direct_unpack_eval_post_run_actions(
            unpack_ok != 0,
            cancelled != 0,
            is_stopped != 0,
            terminated != 0,
        );
    if !out_should_cleanup.is_null() {
        *out_should_cleanup = i32::from(should_cleanup);
    }
    i32::from(interrupted)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_direct_unpack_should_commit_extracted_archives(
    has_archives: c_int,
    status: c_int,
    ns_success: c_int,
    interrupted: c_int,
) -> c_int {
    i32::from(
        crate::diskartdlpost::direct_unpack_should_commit_extracted_archives(
            has_archives != 0,
            status,
            ns_success,
            interrupted != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_prepost_should_complete_interrupted_download(
    has_post_info: c_int,
    kind: c_int,
    nk_nzb: c_int,
    completed_files_empty: c_int,
    delete_status: c_int,
    ds_none: c_int,
    download_completed: c_int,
) -> c_int {
    i32::from(
        crate::diskartdlpost::prepost_should_complete_interrupted_download(
            has_post_info != 0,
            kind,
            nk_nzb,
            completed_files_empty != 0,
            delete_status,
            ds_none,
            download_completed != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_dad_prepost_is_skipped_delete_status(
    delete_status: c_int,
    ds_dupe: c_int,
    ds_copy: c_int,
    ds_good: c_int,
    ds_scan: c_int,
) -> c_int {
    i32::from(crate::diskartdlpost::prepost_is_skipped_delete_status(
        delete_status,
        ds_dupe,
        ds_copy,
        ds_good,
        ds_scan,
    ))
}

// ============================================================================
// DownloadInfo, QueueCoordinator & QueueEditor Hot-Path FFI (`queuecoordinfo.rs`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_server_stat_apply_op(
    cur_success: c_int,
    cur_failed: c_int,
    delta_success: c_int,
    delta_failed: c_int,
    stat_op: c_int,
    so_set: c_int,
    so_add: c_int,
    so_subtract: c_int,
    out_success: *mut c_int,
    out_failed: *mut c_int,
) {
    let (new_succ, new_fail) = crate::queuecoordinfo::server_stat_apply_op(
        cur_success,
        cur_failed,
        delta_success,
        delta_failed,
        stat_op,
        so_set,
        so_add,
        so_subtract,
    );
    if !out_success.is_null() {
        *out_success = new_succ;
    }
    if !out_failed.is_null() {
        *out_failed = new_fail;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_nzb_info_update_completed_stats(
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
    if inout_success_size.is_null()
        || inout_failed_size.is_null()
        || inout_failed_articles.is_null()
        || inout_success_articles.is_null()
        || inout_extra_priority.is_null()
        || inout_par_success_size.is_null()
        || inout_par_failed_size.is_null()
        || inout_remaining_par_count.is_null()
        || inout_paused_file_count.is_null()
    {
        return;
    }
    crate::queuecoordinfo::nzb_info_update_completed_stats(
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
pub unsafe extern "C" fn nzbget_rs_qci_nzb_info_update_deleted_stats(
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
    if inout_file_count.is_null()
        || inout_size.is_null()
        || inout_cur_success_size.is_null()
        || inout_failed_size.is_null()
        || inout_failed_articles.is_null()
        || inout_cur_failed_size.is_null()
        || inout_total_articles.is_null()
        || inout_cur_success_articles.is_null()
        || inout_cur_failed_articles.is_null()
        || inout_remaining_size.is_null()
        || inout_extra_priority.is_null()
        || inout_remaining_par_count.is_null()
        || inout_par_size.is_null()
        || inout_par_cur_success_size.is_null()
        || inout_par_failed_size.is_null()
        || inout_par_cur_failed_size.is_null()
        || inout_paused_file_count.is_null()
        || inout_paused_size.is_null()
    {
        return;
    }
    crate::queuecoordinfo::nzb_info_update_deleted_stats(
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
pub unsafe extern "C" fn nzbget_rs_qci_file_info_calc_paused_delta(
    cur_paused: c_int,
    new_paused: c_int,
    has_nzb: c_int,
    cur_paused_count: c_int,
    cur_paused_size: i64,
    remaining_size: i64,
    out_paused_count: *mut c_int,
    out_paused_size: *mut i64,
) -> c_int {
    let (changed, new_cnt, new_sz) = crate::queuecoordinfo::file_info_calc_paused_delta(
        cur_paused != 0,
        new_paused != 0,
        has_nzb != 0,
        cur_paused_count,
        cur_paused_size,
        remaining_size,
    );
    if !out_paused_count.is_null() {
        *out_paused_count = new_cnt;
    }
    if !out_paused_size.is_null() {
        *out_paused_size = new_sz;
    }
    i32::from(changed)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_file_info_calc_extra_priority_delta(
    cur_extra: c_int,
    new_extra: c_int,
    has_nzb: c_int,
    cur_nzb_extra: c_int,
    out_nzb_extra: *mut c_int,
) -> c_int {
    let (changed, new_extra_cnt) = crate::queuecoordinfo::file_info_calc_extra_priority_delta(
        cur_extra != 0,
        new_extra != 0,
        has_nzb != 0,
        cur_nzb_extra,
    );
    if !out_nzb_extra.is_null() {
        *out_nzb_extra = new_extra_cnt;
    }
    i32::from(changed)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qci_queue_coord_should_resave_completed_file_state(
    status: c_int,
    cf_partial: c_int,
    cf_failure: c_int,
    file_id: c_int,
) -> c_int {
    i32::from(
        crate::queuecoordinfo::queue_coord_should_resave_completed_file_state(
            status, cf_partial, cf_failure, file_id,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_queue_coord_resolve_desired_server_level(
    has_desired_server: c_int,
    has_server_obj: c_int,
    is_active: c_int,
    norm_level: c_int,
    out_level: *mut c_int,
) -> c_int {
    let (use_specific, level) = crate::queuecoordinfo::queue_coord_resolve_desired_server_level(
        has_desired_server != 0,
        has_server_obj != 0,
        is_active != 0,
        norm_level,
    );
    if !out_level.is_null() {
        *out_level = level;
    }
    i32::from(use_specific)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_queue_coord_calc_propagation_wait_log_mins(
    file_time: i64,
    propagation_delay: c_int,
    cur_date: i64,
    out_posted_min: *mut c_int,
    out_start_min: *mut c_int,
) {
    let (posted_min, start_min) = crate::queuecoordinfo::queue_coord_calc_propagation_wait_log_mins(
        file_time,
        propagation_delay,
        cur_date,
    );
    if !out_posted_min.is_null() {
        *out_posted_min = posted_min;
    }
    if !out_start_min.is_null() {
        *out_start_min = start_min;
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qci_queue_coord_should_fetch_first_article_for_rename(
    direct_rename: c_int,
    direct_rename_status: c_int,
    ts_running: c_int,
    all_first: c_int,
) -> c_int {
    i32::from(
        crate::queuecoordinfo::queue_coord_should_fetch_first_article_for_rename(
            direct_rename != 0,
            direct_rename_status,
            ts_running,
            all_first != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qci_queue_coord_is_misplaced_donor_article(
    status: c_int,
    ad_finished: c_int,
    dupe_fallback_round: c_int,
    decoded_file_size: i64,
    segment_aligned: c_int,
) -> c_int {
    i32::from(
        crate::queuecoordinfo::queue_coord_is_misplaced_donor_article(
            status,
            ad_finished,
            dupe_fallback_round,
            decoded_file_size,
            segment_aligned != 0,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_queue_coord_should_reset_dupe_failed_run(
    dupe_fallback_round: c_int,
    msg_id: *const c_char,
    orig_msg_id: *const c_char,
) -> c_int {
    let m = if msg_id.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(msg_id).to_bytes()
    };
    let o = if orig_msg_id.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(orig_msg_id).to_bytes()
    };
    i32::from(
        crate::queuecoordinfo::queue_coord_should_reset_dupe_failed_run(dupe_fallback_round, m, o),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_queue_coord_eval_dupe_recovery_and_cutover(
    dupe_fallback_round: c_int,
    dupe_proactive: c_int,
    msg_id: *const c_char,
    orig_msg_id: *const c_char,
    cur_file_recovered: c_int,
    cur_nzb_recovered: c_int,
    dupe_cutover: c_int,
    cutover_threshold: c_int,
    out_file_recovered: *mut c_int,
    out_nzb_recovered: *mut c_int,
    out_trigger_cutover: *mut c_int,
) -> c_int {
    let m = if msg_id.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(msg_id).to_bytes()
    };
    let o = if orig_msg_id.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(orig_msg_id).to_bytes()
    };
    let (recovered, next_file_rec, next_nzb_rec, trigger_cutover) =
        crate::queuecoordinfo::queue_coord_eval_dupe_recovery_and_cutover(
            dupe_fallback_round,
            dupe_proactive != 0,
            m,
            o,
            cur_file_recovered,
            cur_nzb_recovered,
            dupe_cutover != 0,
            cutover_threshold,
        );
    if !out_file_recovered.is_null() {
        *out_file_recovered = next_file_rec;
    }
    if !out_nzb_recovered.is_null() {
        *out_nzb_recovered = next_nzb_rec;
    }
    if !out_trigger_cutover.is_null() {
        *out_trigger_cutover = i32::from(trigger_cutover);
    }
    i32::from(recovered)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_queue_coord_advance_article_completion_counters(
    rem_file_size: i64,
    rem_nzb_size: i64,
    paused_size: i64,
    article_size: c_int,
    is_paused: c_int,
    completed_articles: c_int,
    total_articles: c_int,
    out_rem_file_size: *mut i64,
    out_rem_nzb_size: *mut i64,
    out_paused_size: *mut i64,
    out_completed_articles: *mut c_int,
) -> c_int {
    let (rf, rn, ps, ca, fc) =
        crate::queuecoordinfo::queue_coord_advance_article_completion_counters(
            rem_file_size,
            rem_nzb_size,
            paused_size,
            article_size,
            is_paused != 0,
            completed_articles,
            total_articles,
        );
    if !out_rem_file_size.is_null() {
        *out_rem_file_size = rf;
    }
    if !out_rem_nzb_size.is_null() {
        *out_rem_nzb_size = rn;
    }
    if !out_paused_size.is_null() {
        *out_paused_size = ps;
    }
    if !out_completed_articles.is_null() {
        *out_completed_articles = ca;
    }
    i32::from(fc)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_queue_coord_eval_file_completion_and_assembly(
    file_completed: c_int,
    parking: c_int,
    active_downloads: c_int,
    dupe_deleted: c_int,
    file_deleted: c_int,
    finished_before_cancel: c_int,
    out_file_completed: *mut c_int,
) -> c_int {
    let (fc, complete_parts) =
        crate::queuecoordinfo::queue_coord_eval_file_completion_and_assembly(
            file_completed != 0,
            parking != 0,
            active_downloads,
            dupe_deleted != 0,
            file_deleted != 0,
            finished_before_cancel != 0,
        );
    if !out_file_completed.is_null() {
        *out_file_completed = i32::from(fc);
    }
    i32::from(complete_parts)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_queue_coord_apply_assembly_failure_counters(
    success_articles: c_int,
    success_size: i64,
    is_par_file: c_int,
    inout_file_failed_articles: *mut c_int,
    inout_file_failed_size: *mut i64,
    inout_nzb_cur_failed_articles: *mut c_int,
    inout_nzb_cur_success_articles: *mut c_int,
    inout_nzb_cur_failed_size: *mut i64,
    inout_nzb_cur_success_size: *mut i64,
    inout_nzb_par_cur_failed_size: *mut i64,
    inout_nzb_par_cur_success_size: *mut i64,
) {
    if inout_file_failed_articles.is_null()
        || inout_file_failed_size.is_null()
        || inout_nzb_cur_failed_articles.is_null()
        || inout_nzb_cur_success_articles.is_null()
        || inout_nzb_cur_failed_size.is_null()
        || inout_nzb_cur_success_size.is_null()
        || inout_nzb_par_cur_failed_size.is_null()
        || inout_nzb_par_cur_success_size.is_null()
    {
        return;
    }
    crate::queuecoordinfo::queue_coord_apply_assembly_failure_counters(
        success_articles,
        success_size,
        is_par_file != 0,
        &mut *inout_file_failed_articles,
        &mut *inout_file_failed_size,
        &mut *inout_nzb_cur_failed_articles,
        &mut *inout_nzb_cur_success_articles,
        &mut *inout_nzb_cur_failed_size,
        &mut *inout_nzb_cur_success_size,
        &mut *inout_nzb_par_cur_failed_size,
        &mut *inout_nzb_par_cur_success_size,
    );
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_queue_coord_demote_article_counters(
    article_size: c_int,
    is_par_file: c_int,
    dupe_proactive: c_int,
    msg_id: *const c_char,
    orig_msg_id: *const c_char,
    inout_file_success_size: *mut i64,
    inout_file_failed_size: *mut i64,
    inout_file_success_articles: *mut c_int,
    inout_file_failed_articles: *mut c_int,
    inout_nzb_cur_success_size: *mut i64,
    inout_nzb_cur_failed_size: *mut i64,
    inout_nzb_par_cur_success_size: *mut i64,
    inout_nzb_par_cur_failed_size: *mut i64,
    inout_nzb_cur_success_articles: *mut c_int,
    inout_nzb_cur_failed_articles: *mut c_int,
    inout_file_dupe_recovered: *mut c_int,
    inout_nzb_dupe_recovered: *mut c_int,
) {
    if inout_file_success_size.is_null()
        || inout_file_failed_size.is_null()
        || inout_file_success_articles.is_null()
        || inout_file_failed_articles.is_null()
        || inout_nzb_cur_success_size.is_null()
        || inout_nzb_cur_failed_size.is_null()
        || inout_nzb_par_cur_success_size.is_null()
        || inout_nzb_par_cur_failed_size.is_null()
        || inout_nzb_cur_success_articles.is_null()
        || inout_nzb_cur_failed_articles.is_null()
        || inout_file_dupe_recovered.is_null()
        || inout_nzb_dupe_recovered.is_null()
    {
        return;
    }
    let m = if msg_id.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(msg_id).to_bytes()
    };
    let o = if orig_msg_id.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(orig_msg_id).to_bytes()
    };
    crate::queuecoordinfo::queue_coord_demote_article_counters(
        article_size,
        is_par_file != 0,
        dupe_proactive != 0,
        m,
        o,
        &mut *inout_file_success_size,
        &mut *inout_file_failed_size,
        &mut *inout_file_success_articles,
        &mut *inout_file_failed_articles,
        &mut *inout_nzb_cur_success_size,
        &mut *inout_nzb_cur_failed_size,
        &mut *inout_nzb_par_cur_success_size,
        &mut *inout_nzb_par_cur_failed_size,
        &mut *inout_nzb_cur_success_articles,
        &mut *inout_nzb_cur_failed_articles,
        &mut *inout_file_dupe_recovered,
        &mut *inout_nzb_dupe_recovered,
    );
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qci_queue_coord_should_delete_temp_article_file(
    has_result_filename: c_int,
    direct_write: c_int,
    force_direct_write: c_int,
    dupe_fallback_round: c_int,
) -> c_int {
    i32::from(
        crate::queuecoordinfo::queue_coord_should_delete_temp_article_file(
            has_result_filename != 0,
            direct_write != 0,
            force_direct_write != 0,
            dupe_fallback_round,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_queue_coord_should_defer_dupe_failover(
    fallback_enabled: c_int,
    attempted: c_int,
    recovered: c_int,
    failover_sample: c_int,
    defer_state: c_int,
    dp_deferred: c_int,
    par_cannot_cover: c_int,
    failed_articles: c_int,
    failover_checked: c_int,
    out_update_checked: *mut c_int,
) -> c_int {
    let (defer, update_checked) = crate::queuecoordinfo::queue_coord_should_defer_dupe_failover(
        fallback_enabled != 0,
        attempted,
        recovered,
        failover_sample,
        defer_state,
        dp_deferred,
        par_cannot_cover != 0,
        failed_articles,
        failover_checked,
    );
    if !out_update_checked.is_null() {
        *out_update_checked = i32::from(update_checked);
    }
    i32::from(defer)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qci_queue_coord_should_park_dead_without_backup(
    fallback_enabled: c_int,
    defer_state: c_int,
    dp_deferred: c_int,
    attempted: c_int,
    recovered: c_int,
    tried: c_int,
    projected_sample: c_int,
    files_tried: c_int,
    far_below: c_int,
    below: c_int,
) -> c_int {
    i32::from(
        crate::queuecoordinfo::queue_coord_should_park_dead_without_backup(
            fallback_enabled != 0,
            defer_state,
            dp_deferred,
            attempted,
            recovered,
            tried,
            projected_sample,
            files_tried,
            far_below != 0,
            below != 0,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_queue_coord_is_partly_alive_failover_ready(
    backup_alive: c_int,
    projected: c_int,
    backup_lead: c_int,
    critical: c_int,
    failover_warranted: c_int,
    out_projected_below_critical: *mut c_int,
) -> c_int {
    let (ready, below_crit) = crate::queuecoordinfo::queue_coord_is_partly_alive_failover_ready(
        backup_alive,
        projected,
        backup_lead,
        critical,
        failover_warranted != 0,
    );
    if !out_projected_below_critical.is_null() {
        *out_projected_below_critical = i32::from(below_crit);
    }
    i32::from(ready)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qci_queue_coord_should_wait_for_borrowing_on_par_damage(
    fallback_enabled: c_int,
    raw_article: c_int,
    dupe_mode: c_int,
    dm_force: c_int,
    has_par2: c_int,
    defer_state: c_int,
    dp_deferred: c_int,
    attempted: c_int,
    failover_sample: c_int,
    recovered: c_int,
) -> c_int {
    i32::from(
        crate::queuecoordinfo::queue_coord_should_wait_for_borrowing_on_par_damage(
            fallback_enabled != 0,
            raw_article != 0,
            dupe_mode,
            dm_force,
            has_par2 != 0,
            defer_state,
            dp_deferred,
            attempted,
            failover_sample,
            recovered,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_qci_queue_coord_eval_discard_direct_rename_file(
    is_par_file: c_int,
    completed_articles: c_int,
    active_downloads: c_int,
    server_mode: c_int,
    has_articles: c_int,
    continue_partial: c_int,
    cached_articles: c_int,
    out_discard_infos: *mut c_int,
) -> c_int {
    let (discard_par, discard_infos) =
        crate::queuecoordinfo::queue_coord_eval_discard_direct_rename_file(
            is_par_file != 0,
            completed_articles,
            active_downloads,
            server_mode != 0,
            has_articles != 0,
            continue_partial != 0,
            cached_articles,
        );
    if !out_discard_infos.is_null() {
        *out_discard_infos = i32::from(discard_infos);
    }
    i32::from(discard_par)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qci_queue_coord_validate_split_file_state(
    active_downloads: c_int,
    completed_articles: c_int,
    has_post_info: c_int,
    same_nzb: c_int,
) -> c_int {
    crate::queuecoordinfo::queue_coord_validate_split_file_state(
        active_downloads,
        completed_articles,
        has_post_info != 0,
        same_nzb != 0,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_qci_group_sorter_compare_values(
    sort_criteria: c_int,
    sort_order: c_int,
    so_descending: c_int,
    cmp_name: c_int,
    size1: i64,
    size2: i64,
    rem1: i64,
    rem2: i64,
    min_time1: i64,
    min_time2: i64,
    cmp_cat: c_int,
    prio1: c_int,
    prio2: c_int,
) -> c_int {
    i32::from(crate::queuecoordinfo::group_sorter_compare_values(
        sort_criteria,
        sort_order,
        so_descending,
        cmp_name,
        size1,
        size2,
        rem1,
        rem2,
        min_time1,
        min_time2,
        cmp_cat,
        prio1,
        prio2,
    ))
}

// ============================================================================
// ParChecker, UnpackController, DupeArticleFallback, DupeCoordinator & NzbFile
// Hot-Path C ABI (rust/src/pardupenzb.rs)
// ============================================================================

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_parchecker_accumulate_worst_status(
    all_status: c_int,
    set_status: c_int,
) -> c_int {
    crate::pardupenzb::parchecker_accumulate_worst_status(all_status, set_status)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_parchecker_should_try_more_recovery(
    has_damaged_files: c_int,
    is_stopped: c_int,
    res: c_int,
    e_repair_not_possible: c_int,
    require_dupe_scan: c_int,
    par_scan: c_int,
    ps_dupe: c_int,
) -> c_int {
    i32::from(crate::pardupenzb::parchecker_should_try_more_recovery(
        has_damaged_files != 0,
        is_stopped != 0,
        res,
        e_repair_not_possible,
        require_dupe_scan != 0,
        par_scan,
        ps_dupe,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_parchecker_eval_post_verify_action(
    res: c_int,
    e_success: c_int,
    e_repair_possible: c_int,
    has_damaged_files: c_int,
    par_repair: c_int,
    ps_failed: c_int,
    ps_repair_not_needed: c_int,
    ps_repair_possible: c_int,
    out_action_code: *mut c_int,
) -> c_int {
    let (status, action) = crate::pardupenzb::parchecker_eval_post_verify_action(
        res,
        e_success,
        e_repair_possible,
        has_damaged_files != 0,
        par_repair != 0,
        ps_failed,
        ps_repair_not_needed,
        ps_repair_possible,
    );
    if !out_action_code.is_null() {
        *out_action_code = action;
    }
    status
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_parchecker_eval_cancel_or_fail_outcome(
    is_stopped: c_int,
    stage: c_int,
    pt_repairing: c_int,
    cur_status: c_int,
    ps_failed: c_int,
    ps_repair_possible: c_int,
    err_msg_empty: c_int,
    res: c_int,
    out_outcome_kind: *mut c_int,
    out_use_cmd_err: *mut c_int,
) -> c_int {
    let (status, kind, use_cmd) = crate::pardupenzb::parchecker_eval_cancel_or_fail_outcome(
        is_stopped != 0,
        stage,
        pt_repairing,
        cur_status,
        ps_failed,
        ps_repair_possible,
        err_msg_empty != 0,
        res,
    );
    if !out_outcome_kind.is_null() {
        *out_outcome_kind = kind;
    }
    if !out_use_cmd_err.is_null() {
        *out_use_cmd_err = i32::from(use_cmd);
    }
    status
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_parchecker_is_verified_complete_target(
    status: c_int,
    ps_repaired: c_int,
    ps_repair_not_needed: c_int,
    has_complete_file: c_int,
    complete_eq_target: c_int,
) -> c_int {
    i32::from(crate::pardupenzb::parchecker_is_verified_complete_target(
        status,
        ps_repaired,
        ps_repair_not_needed,
        has_complete_file != 0,
        complete_eq_target != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_parchecker_calc_dupe_blocks_delta(
    prev_used_blocks: c_int,
    was_blocks_missing2: u32,
    blocks_missing2: u32,
    was_blocks_missing_total: u32,
    out_total_found: *mut u32,
) -> c_int {
    let (new_used, total_found) = crate::pardupenzb::parchecker_calc_dupe_blocks_delta(
        prev_used_blocks,
        was_blocks_missing2,
        blocks_missing2,
        was_blocks_missing_total,
    );
    if !out_total_found.is_null() {
        *out_total_found = total_found;
    }
    new_used
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_parchecker_eval_extra_file_step(
    was_files_missing: u32,
    now_files_missing: u32,
    was_blocks_missing: u32,
    now_blocks_missing: u32,
    external_dir: c_int,
    prev_files_added: c_int,
    out_file_added: *mut c_int,
    out_block_added: *mut c_int,
    out_blocks_delta: *mut u32,
    out_register_parred: *mut c_int,
) -> c_int {
    let (fa, ba, nfa, delta, reg) = crate::pardupenzb::parchecker_eval_extra_file_step(
        was_files_missing,
        now_files_missing,
        was_blocks_missing,
        now_blocks_missing,
        external_dir != 0,
        prev_files_added != 0,
    );
    if !out_file_added.is_null() {
        *out_file_added = i32::from(fa);
    }
    if !out_block_added.is_null() {
        *out_block_added = i32::from(ba);
    }
    if !out_blocks_delta.is_null() {
        *out_blocks_delta = delta;
    }
    if !out_register_parred.is_null() {
        *out_register_parred = i32::from(reg);
    }
    i32::from(nfa)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_parchecker_eval_signal_done(
    stage: c_int,
    pt_verifying_sources: c_int,
    available: c_int,
    total: c_int,
    verifying_extra_files: c_int,
    ignore: c_int,
    prev_has_damaged_files: c_int,
    out_has_damaged_files: *mut c_int,
    out_bad_blocks: *mut c_int,
) -> c_int {
    let (report, has_damaged, bad_blocks) = crate::pardupenzb::parchecker_eval_signal_done(
        stage,
        pt_verifying_sources,
        available,
        total,
        verifying_extra_files != 0,
        ignore != 0,
        prev_has_damaged_files != 0,
    );
    if !out_has_damaged_files.is_null() {
        *out_has_damaged_files = i32::from(has_damaged);
    }
    if !out_bad_blocks.is_null() {
        *out_bad_blocks = bad_blocks;
    }
    i32::from(report)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_parchecker_check_verify_data_file_preconditions(
    stage: c_int,
    pt_verifying_sources: c_int,
    target_exists: c_int,
    has_verification_packet: c_int,
    file_size: i64,
    block_count: u64,
) -> c_int {
    crate::pardupenzb::parchecker_check_verify_data_file_preconditions(
        stage,
        pt_verifying_sources,
        target_exists != 0,
        has_verification_packet != 0,
        file_size,
        block_count,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_parchecker_calc_dumb_crc_read_size(
    cnt: c_int,
    buffer_size: c_int,
    start: i64,
    end: i64,
) -> c_int {
    crate::pardupenzb::parchecker_calc_dumb_crc_read_size(cnt, buffer_size, start, end)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_unpack_ctrl_classify_run_branch(
    unpack: c_int,
    has_files: c_int,
    par_status: c_int,
    ps_skipped: c_int,
    par_rename_status: c_int,
    rs_skipped: c_int,
    has_par_files: c_int,
    has_renamed_archive_files: c_int,
) -> c_int {
    crate::pardupenzb::unpack_ctrl_classify_run_branch(
        unpack != 0,
        has_files != 0,
        par_status,
        ps_skipped,
        par_rename_status,
        rs_skipped,
        has_par_files != 0,
        has_renamed_archive_files != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_unpack_ctrl_eval_rar_unpack_mode(
    has_not_unpacked_rar_files: c_int,
    unpack_dir_created: c_int,
    direct_unpack_status: c_int,
    ns_success: c_int,
    out_log_kind: *mut c_int,
) -> c_int {
    let (run_unrar, log_kind) = crate::pardupenzb::unpack_ctrl_eval_rar_unpack_mode(
        has_not_unpacked_rar_files != 0,
        unpack_dir_created != 0,
        direct_unpack_status,
        ns_success,
    );
    if !out_log_kind.is_null() {
        *out_log_kind = log_kind;
    }
    i32::from(run_unrar)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_unpack_ctrl_should_log_exit_error(
    unpack_ok: c_int,
    exit_code: c_int,
    will_retry_with_pass: c_int,
    is_stopped: c_int,
    is_terminated: c_int,
) -> c_int {
    i32::from(crate::pardupenzb::unpack_ctrl_should_log_exit_error(
        unpack_ok != 0,
        exit_code,
        will_retry_with_pass != 0,
        is_stopped != 0,
        is_terminated != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_unpack_ctrl_accumulate_split_piece(
    filename: *const c_char,
    dest_base_name: *const c_char,
    segment_size: i64,
    first_segment_size: i64,
    inout_count: *mut c_int,
    inout_min: *mut c_int,
    inout_max: *mut c_int,
    inout_dif_size_min: *mut c_int,
) -> c_int {
    let f = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    let b = if dest_base_name.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(dest_base_name).to_bytes()
    };
    i32::from(crate::pardupenzb::unpack_ctrl_accumulate_split_piece(
        f,
        b,
        segment_size,
        first_segment_size,
        &mut *inout_count,
        &mut *inout_min,
        &mut *inout_max,
        &mut *inout_dif_size_min,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_unpack_ctrl_classify_cleanup_archive_file(
    is_dir: c_int,
    inter_dir: c_int,
    in_extracted_files: c_int,
    is_downloaded_file: c_int,
    has_splitted_files: c_int,
    in_joined_files: c_int,
    filename: *const c_char,
) -> c_int {
    let f = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    crate::pardupenzb::unpack_ctrl_classify_cleanup_archive_file(
        is_dir != 0,
        inter_dir != 0,
        in_extracted_files != 0,
        is_downloaded_file != 0,
        has_splitted_files != 0,
        in_joined_files != 0,
        f,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_dupe_fallback_should_defer_to_par_header(
    has_nzb_info: c_int,
    direct_rename_status: c_int,
    ts_running: c_int,
    all_first: c_int,
    par_status: c_int,
    ps_failure: c_int,
    par_size: i64,
) -> c_int {
    crate::pardupenzb::dupe_fallback_should_defer_to_par_header(
        has_nzb_info != 0,
        direct_rename_status,
        ts_running,
        all_first != 0,
        par_status,
        ps_failure,
        par_size,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_dupe_fallback_is_borrowed_article(
    donor_id: c_int,
    fallback_round: c_int,
) -> c_int {
    i32::from(crate::pardupenzb::dupe_fallback_is_borrowed_article(
        donor_id,
        fallback_round,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_dupe_fallback_is_valid_par2_block_spec(
    has_sums: c_int,
    block_size: u64,
    max_par2_block_size: u64,
    crc_count: usize,
    file_size: i64,
) -> c_int {
    i32::from(crate::pardupenzb::dupe_fallback_is_valid_par2_block_spec(
        has_sums != 0,
        block_size,
        max_par2_block_size,
        crc_count,
        file_size,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_dupe_fallback_calc_article_block_range(
    segment_offset: i64,
    segment_size: c_int,
    block_size: u64,
    out_first: *mut i64,
    out_last: *mut i64,
) {
    let (first, last) = crate::pardupenzb::dupe_fallback_calc_article_block_range(
        segment_offset,
        segment_size,
        block_size,
    );
    if !out_first.is_null() {
        *out_first = first;
    }
    if !out_last.is_null() {
        *out_last = last;
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_dupe_fallback_compute_pin_lead_state(
    contributors: *const c_int,
    contributors_len: usize,
    cur_file_lead_donor_id: c_int,
    out_lead_snapshot: *mut c_int,
    out_next_lead: *mut c_int,
    out_donor_count: *mut c_int,
    out_new_file_lead: *mut c_int,
) {
    let slice = if contributors.is_null() || contributors_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(contributors, contributors_len)
    };
    let (lead, next, cnt, file_lead) =
        crate::pardupenzb::dupe_fallback_compute_pin_lead_state(slice, cur_file_lead_donor_id);
    if !out_lead_snapshot.is_null() {
        *out_lead_snapshot = lead;
    }
    if !out_next_lead.is_null() {
        *out_next_lead = next;
    }
    if !out_donor_count.is_null() {
        *out_donor_count = cnt;
    }
    if !out_new_file_lead.is_null() {
        *out_new_file_lead = file_lead;
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_dupe_fallback_should_vacate_ghost_lead(
    lead_nzb_id: c_int,
    is_barred: c_int,
    in_donors: c_int,
) -> c_int {
    i32::from(crate::pardupenzb::dupe_fallback_should_vacate_ghost_lead(
        lead_nzb_id,
        is_barred != 0,
        in_donors != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_dupe_fallback_compare_donors(
    twin1: c_int,
    score1: c_int,
    id1: c_int,
    twin2: c_int,
    score2: c_int,
    id2: c_int,
) -> c_int {
    i32::from(crate::pardupenzb::dupe_fallback_compare_donors(
        twin1 != 0,
        score1,
        id1,
        twin2 != 0,
        score2,
        id2,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_dupecoord_check_history_good_or_score_skip(
    is_hk_nzb: c_int,
    is_hk_dup: c_int,
    hist_dupe_mode: c_int,
    dm_force: c_int,
    hist_mark_status: c_int,
    ks_good: c_int,
    hist_dup_status: c_int,
    ds_good: c_int,
    ds_success: c_int,
    nzb_dupe_mode: c_int,
    dm_score: c_int,
    nzb_dupe_score: c_int,
    hist_dupe_score: c_int,
    same_name_or_key: c_int,
    out_good: *mut c_int,
) -> c_int {
    let (skip, good) = crate::pardupenzb::dupecoord_check_history_good_or_score_skip(
        is_hk_nzb != 0,
        is_hk_dup != 0,
        hist_dupe_mode,
        dm_force,
        hist_mark_status,
        ks_good,
        hist_dup_status,
        ds_good,
        ds_success,
        nzb_dupe_mode,
        dm_score,
        nzb_dupe_score,
        hist_dupe_score,
        same_name_or_key != 0,
    );
    if !out_good.is_null() {
        *out_good = i32::from(good);
    }
    i32::from(skip)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_dupecoord_classify_same_content_history_item(
    is_hk_nzb: c_int,
    is_hk_dup: c_int,
    same_hash: c_int,
    delete_status: c_int,
    ds_dupe: c_int,
    ds_copy: c_int,
    ds_manual: c_int,
    is_dupe_success: c_int,
    dup_status: c_int,
    dup_ds_success: c_int,
    dup_ds_good: c_int,
    dup_ds_failed: c_int,
    dup_ds_bad: c_int,
    out_failed: *mut c_int,
) -> c_int {
    let (succeeded, failed) = crate::pardupenzb::dupecoord_classify_same_content_history_item(
        is_hk_nzb != 0,
        is_hk_dup != 0,
        same_hash != 0,
        delete_status,
        ds_dupe,
        ds_copy,
        ds_manual,
        is_dupe_success != 0,
        dup_status,
        dup_ds_success,
        dup_ds_good,
        dup_ds_failed,
        dup_ds_bad,
    );
    if !out_failed.is_null() {
        *out_failed = i32::from(failed);
    }
    i32::from(succeeded)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_dupecoord_resolve_skip_action(
    feed_id: c_int,
    dupe_hint: c_int,
    dh_none: c_int,
    same_content: c_int,
    ds_manual: c_int,
    ds_copy: c_int,
    ds_good: c_int,
    out_delete_disk: *mut c_int,
) -> c_int {
    let (del_status, del_disk) = crate::pardupenzb::dupecoord_resolve_skip_action(
        feed_id,
        dupe_hint,
        dh_none,
        same_content != 0,
        ds_manual,
        ds_copy,
        ds_good,
    );
    if !out_delete_disk.is_null() {
        *out_delete_disk = i32::from(del_disk);
    }
    del_status
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_dupecoord_accum_queue_dupe_status(
    cur_statuses: c_int,
    same_name_or_key: c_int,
    success_articles: c_int,
    failed_articles: c_int,
    ds_downloading: c_int,
    ds_queued: c_int,
) -> c_int {
    crate::pardupenzb::dupecoord_accum_queue_dupe_status(
        cur_statuses,
        same_name_or_key != 0,
        success_articles,
        failed_articles,
        ds_downloading,
        ds_queued,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_pnd_dupecoord_accum_dup_history_status(
    cur_statuses: c_int,
    same_name_or_key: c_int,
    dup_status: c_int,
    dup_ds_success: c_int,
    dup_ds_good: c_int,
    dup_ds_failed: c_int,
    dup_ds_bad: c_int,
    ds_success: c_int,
    ds_failure: c_int,
) -> c_int {
    crate::pardupenzb::dupecoord_accum_dup_history_status(
        cur_statuses,
        same_name_or_key != 0,
        dup_status,
        dup_ds_success,
        dup_ds_good,
        dup_ds_failed,
        dup_ds_bad,
        ds_success,
        ds_failure,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_pnd_nzbfile_accumulate_file_stats(
    filename: *const c_char,
    file_total_articles: c_int,
    file_missed_articles: c_int,
    file_size: i64,
    file_remaining_size: i64,
    file_missed_size: i64,
    inout_nzb_file_count: *mut c_int,
    inout_nzb_total_articles: *mut c_int,
    inout_nzb_failed_articles: *mut c_int,
    inout_nzb_cur_failed_articles: *mut c_int,
    inout_nzb_size: *mut i64,
    inout_nzb_remaining_size: *mut i64,
    inout_nzb_failed_size: *mut i64,
    inout_nzb_cur_failed_size: *mut i64,
    inout_nzb_par_size: *mut i64,
    inout_nzb_par_failed_size: *mut i64,
    inout_nzb_par_cur_failed_size: *mut i64,
    inout_nzb_remaining_par_count: *mut c_int,
) -> c_int {
    let f = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    i32::from(crate::pardupenzb::nzbfile_accumulate_file_stats(
        f,
        file_total_articles,
        file_missed_articles,
        file_size,
        file_remaining_size,
        file_missed_size,
        &mut *inout_nzb_file_count,
        &mut *inout_nzb_total_articles,
        &mut *inout_nzb_failed_articles,
        &mut *inout_nzb_cur_failed_articles,
        &mut *inout_nzb_size,
        &mut *inout_nzb_remaining_size,
        &mut *inout_nzb_failed_size,
        &mut *inout_nzb_cur_failed_size,
        &mut *inout_nzb_par_size,
        &mut *inout_nzb_par_failed_size,
        &mut *inout_nzb_par_cur_failed_size,
        &mut *inout_nzb_remaining_par_count,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_history_coord_classify_service_work_item(
    kind: c_int,
    hk_dup: c_int,
    hk_nzb: c_int,
    item_time: i64,
    min_time: i64,
    dupe_check: c_int,
) -> c_int {
    crate::histscanrepair::history_coord_classify_service_work_item(
        kind,
        hk_dup,
        hk_nzb,
        item_time,
        min_time,
        dupe_check != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_history_coord_resolve_post_history_direct_statuses(
    direct_rename_status: c_int,
    ts_running: c_int,
    ts_failure: c_int,
    direct_unpack_status: c_int,
    ns_running: c_int,
    ns_none: c_int,
    out_new_rename_status: *mut c_int,
    out_new_unpack_status: *mut c_int,
) {
    let (r, u) = crate::histscanrepair::history_coord_resolve_post_history_direct_statuses(
        direct_rename_status,
        ts_running,
        ts_failure,
        direct_unpack_status,
        ns_running,
        ns_none,
    );
    *out_new_rename_status = r;
    *out_new_unpack_status = u;
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_history_coord_should_sample_completed_file(
    status: c_int,
    cf_partial: c_int,
    cf_failure: c_int,
    file_id: c_int,
    out_is_partial: *mut c_int,
) -> c_int {
    let (sample, is_partial) = crate::histscanrepair::history_coord_should_sample_completed_file(
        status, cf_partial, cf_failure, file_id,
    );
    *out_is_partial = i32::from(is_partial);
    i32::from(sample)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_history_coord_should_sample_article(
    is_partial: c_int,
    article_status: c_int,
    ai_failed: c_int,
    message_id: *const c_char,
) -> c_int {
    let m = if message_id.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(message_id).to_bytes()
    };
    i32::from(crate::histscanrepair::history_coord_should_sample_article(
        is_partial != 0,
        article_status,
        ai_failed,
        m,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_history_coord_calc_move_to_queue_reset(
    unpack_cleaned_up_disk: c_int,
    post_total_sec: c_int,
    unpack_sec: c_int,
    has_main_pars: c_int,
    par_sec: c_int,
    move_status: c_int,
    ms_failure: c_int,
    ms_none: c_int,
    out_new_post_total_sec: *mut c_int,
    out_new_move_status: *mut c_int,
) {
    let (p, m) = crate::histscanrepair::history_coord_calc_move_to_queue_reset(
        unpack_cleaned_up_disk != 0,
        post_total_sec,
        unpack_sec,
        has_main_pars != 0,
        par_sec,
        move_status,
        ms_failure,
        ms_none,
    );
    *out_new_post_total_sec = p;
    *out_new_move_status = m;
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_history_coord_resolve_url_redownload_dupe_hint(
    cur_dupe_hint: c_int,
    dh_none: c_int,
    dh_redownload_manual: c_int,
) -> c_int {
    crate::histscanrepair::history_coord_resolve_url_redownload_dupe_hint(
        cur_dupe_hint,
        dh_none,
        dh_redownload_manual,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_history_coord_classify_return_action(
    kind: c_int,
    hk_url: c_int,
    hk_nzb: c_int,
    parked_file_count: c_int,
) -> c_int {
    crate::histscanrepair::history_coord_classify_return_action(
        kind,
        hk_url,
        hk_nzb,
        parked_file_count,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_history_coord_eval_requeue_loaded_file(
    status: c_int,
    cf_none: c_int,
    cf_failure: c_int,
    cf_partial: c_int,
    reset_failed: c_int,
    state_loaded: c_int,
    remaining_size: i64,
) -> c_int {
    i32::from(crate::histscanrepair::history_coord_eval_requeue_loaded_file(
        status,
        cf_none,
        cf_failure,
        cf_partial,
        reset_failed != 0,
        state_loaded != 0,
        remaining_size,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_history_coord_has_retryable_article(
    statuses_ptr: *const c_int,
    statuses_len: usize,
    reset_failed: c_int,
    ai_undefined: c_int,
    ai_failed: c_int,
) -> c_int {
    let slice = if statuses_ptr.is_null() || statuses_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(statuses_ptr, statuses_len)
    };
    i32::from(crate::histscanrepair::history_coord_has_retryable_article(
        slice,
        reset_failed != 0,
        ai_undefined,
        ai_failed,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_history_coord_reset_articles_header(
    all_failed: c_int,
    is_par_file: c_int,
    file_size: i64,
    file_missed_size: i64,
    file_total_articles: c_int,
    file_missed_articles: c_int,
    cur_file_failed_size: i64,
    cur_file_failed_articles: c_int,
    cur_file_remaining_size: i64,
    cur_file_completed_articles: c_int,
    file_success_size: i64,
    file_success_articles: c_int,
    cur_nzb_failed_size: i64,
    cur_nzb_success_size: i64,
    cur_nzb_failed_articles: c_int,
    cur_nzb_success_articles: c_int,
    cur_nzb_par_failed_size: i64,
    cur_nzb_par_success_size: i64,
    out_file_failed_size: *mut i64,
    out_file_failed_articles: *mut c_int,
    out_file_remaining_size: *mut i64,
    out_file_completed_articles: *mut c_int,
    out_nzb_failed_size: *mut i64,
    out_nzb_success_size: *mut i64,
    out_nzb_failed_articles: *mut c_int,
    out_nzb_success_articles: *mut c_int,
    out_nzb_par_failed_size: *mut i64,
    out_nzb_par_success_size: *mut i64,
) {
    let res = crate::histscanrepair::history_coord_reset_articles_header(
        all_failed != 0,
        is_par_file != 0,
        file_size,
        file_missed_size,
        file_total_articles,
        file_missed_articles,
        cur_file_failed_size,
        cur_file_failed_articles,
        cur_file_remaining_size,
        cur_file_completed_articles,
        file_success_size,
        file_success_articles,
        cur_nzb_failed_size,
        cur_nzb_success_size,
        cur_nzb_failed_articles,
        cur_nzb_success_articles,
        cur_nzb_par_failed_size,
        cur_nzb_par_success_size,
    );
    *out_file_failed_size = res.0;
    *out_file_failed_articles = res.1;
    *out_file_remaining_size = res.2;
    *out_file_completed_articles = res.3;
    *out_nzb_failed_size = res.4;
    *out_nzb_success_size = res.5;
    *out_nzb_failed_articles = res.6;
    *out_nzb_success_articles = res.7;
    *out_nzb_par_failed_size = res.8;
    *out_nzb_par_success_size = res.9;
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_history_coord_apply_article_reset_delta(
    pa_status: c_int,
    ai_failed: c_int,
    ai_undefined: c_int,
    ai_finished: c_int,
    pa_size: c_int,
    cur_completed_articles: c_int,
    cur_remaining_size: i64,
    cur_failed_articles: c_int,
    cur_failed_size: i64,
    cur_success_articles: c_int,
    cur_success_size: i64,
    out_completed_articles: *mut c_int,
    out_remaining_size: *mut i64,
    out_failed_articles: *mut c_int,
    out_failed_size: *mut i64,
    out_success_articles: *mut c_int,
    out_success_size: *mut i64,
) {
    let res = crate::histscanrepair::history_coord_apply_article_reset_delta(
        pa_status,
        ai_failed,
        ai_undefined,
        ai_finished,
        pa_size,
        cur_completed_articles,
        cur_remaining_size,
        cur_failed_articles,
        cur_failed_size,
        cur_success_articles,
        cur_success_size,
    );
    *out_completed_articles = res.0;
    *out_remaining_size = res.1;
    *out_failed_articles = res.2;
    *out_failed_size = res.3;
    *out_success_articles = res.4;
    *out_success_size = res.5;
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_scanner_should_run_scan_script(
    scan_script: c_int,
    extension: *const c_char,
) -> c_int {
    let ext = if extension.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(extension).to_bytes()
    };
    i32::from(crate::histscanrepair::scanner_should_run_scan_script(
        scan_script != 0,
        ext,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_scanner_classify_incoming_extension(
    ran_scan_script: c_int,
    exists: c_int,
    extension: *const c_char,
    out_rename_to_processed: *mut c_int,
) -> c_int {
    let ext = if extension.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(extension).to_bytes()
    };
    let (ren, act) = crate::histscanrepair::scanner_classify_incoming_extension(
        ran_scan_script != 0,
        exists != 0,
        ext,
    );
    *out_rename_to_processed = i32::from(ren);
    act
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_scanner_classify_add_file_outcome(
    ok: c_int,
    has_url_info: c_int,
) -> c_int {
    crate::histscanrepair::scanner_classify_add_file_outcome(ok != 0, has_url_info != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_scanner_resolve_archive_add_status(
    has_url_info: c_int,
    statuses_ptr: *const c_int,
    statuses_len: usize,
    as_success: c_int,
    as_skipped: c_int,
) -> c_int {
    let slice = if statuses_ptr.is_null() || statuses_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(statuses_ptr, statuses_len)
    };
    crate::histscanrepair::scanner_resolve_archive_add_status(
        has_url_info != 0,
        slice,
        as_success,
        as_skipped,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_repair_ctrl_should_consider_twin_candidate(
    is_self: c_int,
    kind: c_int,
    nk_nzb: c_int,
    same_name_or_key: c_int,
    queued_filename: *const c_char,
    has_print: c_int,
    own_print_matches: c_int,
) -> c_int {
    let q = if queued_filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(queued_filename).to_bytes()
    };
    i32::from(
        crate::histscanrepair::repair_ctrl_should_consider_twin_candidate(
            is_self != 0,
            kind,
            nk_nzb,
            same_name_or_key != 0,
            q,
            has_print != 0,
            own_print_matches != 0,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_repair_ctrl_sum_and_check_twin_blocks(
    vol_blocks_ptr: *const c_int,
    vol_blocks_len: usize,
    block_needed: c_int,
    out_available: *mut c_int,
) -> c_int {
    let slice = if vol_blocks_ptr.is_null() || vol_blocks_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(vol_blocks_ptr, vol_blocks_len)
    };
    let (avail, ok) =
        crate::histscanrepair::repair_ctrl_sum_and_check_twin_blocks(slice, block_needed);
    *out_available = avail;
    i32::from(ok)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_repair_ctrl_calc_dupe_source_extra_blocks(
    cur_history_extra: c_int,
    used_blocks: c_int,
    cur_total_extra: c_int,
    out_new_history_extra: *mut c_int,
    out_new_total_extra: *mut c_int,
) {
    let (h, t) = crate::histscanrepair::repair_ctrl_calc_dupe_source_extra_blocks(
        cur_history_extra,
        used_blocks,
        cur_total_extra,
    );
    *out_new_history_extra = h;
    *out_new_total_extra = t;
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_repair_ctrl_should_use_par_quick(
    par_quick_opt: c_int,
    force_par_full: c_int,
    dupe_repaired: c_int,
) -> c_int {
    i32::from(crate::histscanrepair::repair_ctrl_should_use_par_quick(
        par_quick_opt != 0,
        force_par_full != 0,
        dupe_repaired != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_repair_ctrl_eval_stage_transition(
    cur_post_stage: c_int,
    new_stage: c_int,
    pt_repairing: c_int,
    pt_verifying_repaired: c_int,
    current_time: i64,
    repair_time: i64,
    cur_repair_sec: c_int,
    out_set_repair_time: *mut c_int,
    out_new_repair_sec: *mut c_int,
) -> c_int {
    let (changed, set_rt, new_sec) = crate::histscanrepair::repair_ctrl_eval_stage_transition(
        cur_post_stage,
        new_stage,
        pt_repairing,
        pt_verifying_repaired,
        current_time,
        repair_time,
        cur_repair_sec,
    );
    *out_set_repair_time = i32::from(set_rt);
    *out_new_repair_sec = new_sec;
    i32::from(changed)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_prepost_classify_nzb_downloaded_entry(
    skip_script_processing: c_int,
    skip_disk_write: c_int,
    opt_skip_write: c_int,
    delete_status: c_int,
    ds_health: c_int,
    ds_bad: c_int,
    has_post_info: c_int,
    opt_raw_article: c_int,
    par_status: c_int,
    ps_none: c_int,
    par_check_opt: c_int,
    pc_always: c_int,
    pc_force: c_int,
    out_enqueue_deleted_script: *mut c_int,
    out_set_par_skipped: *mut c_int,
) -> c_int {
    let (route, enq, skip_par) = crate::histscanrepair::prepost_classify_nzb_downloaded_entry(
        skip_script_processing != 0,
        skip_disk_write != 0,
        opt_skip_write != 0,
        delete_status,
        ds_health,
        ds_bad,
        has_post_info != 0,
        opt_raw_article != 0,
        par_status,
        ps_none,
        par_check_opt,
        pc_always,
        pc_force,
    );
    *out_enqueue_deleted_script = i32::from(enq);
    *out_set_par_skipped = i32::from(skip_par);
    route
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_prepost_resolve_nzb_deleted_route(
    delete_status: c_int,
    ds_none: c_int,
    ds_manual: c_int,
    ds_health: c_int,
    ds_bad: c_int,
    out_new_delete_status: *mut c_int,
) -> c_int {
    let (new_st, call_dl) = crate::histscanrepair::prepost_resolve_nzb_deleted_route(
        delete_status,
        ds_none,
        ds_manual,
        ds_health,
        ds_bad,
    );
    *out_new_delete_status = new_st;
    i32::from(call_dl)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_prepost_classify_delete_cleanup_mode(
    cleanup_disk: c_int,
    delete_status: c_int,
    ds_dupe: c_int,
    parking: c_int,
    skip_disk_write: c_int,
    opt_skip_write: c_int,
) -> c_int {
    crate::histscanrepair::prepost_classify_delete_cleanup_mode(
        cleanup_disk != 0,
        delete_status,
        ds_dupe,
        parking != 0,
        skip_disk_write != 0,
        opt_skip_write != 0,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_prepost_classify_request_par_action(
    request_par_check: c_int,
    par_status: c_int,
    ps_skipped: c_int,
    force_repair: c_int,
    par_full: c_int,
    par_check_opt: c_int,
    pc_manual: c_int,
    cur_force_par_full: c_int,
    file_list_empty: c_int,
    out_new_force_par_full: *mut c_int,
) -> c_int {
    let (act, new_force) = crate::histscanrepair::prepost_classify_request_par_action(
        request_par_check != 0,
        par_status,
        ps_skipped,
        force_repair != 0,
        par_full != 0,
        par_check_opt,
        pc_manual,
        cur_force_par_full != 0,
        file_list_empty != 0,
    );
    *out_new_force_par_full = i32::from(new_force);
    act
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_prepost_should_skip_par_cannot_cover_check(
    health: c_int,
    critical_health: c_int,
) -> c_int {
    i32::from(crate::histscanrepair::prepost_should_skip_par_cannot_cover_check(
        health,
        critical_health,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_prepost_calc_job_protected_damage(
    decoded_file_size: i64,
    holes_total_size: i64,
    failed_size: i64,
    missed_size: i64,
) -> i64 {
    crate::histscanrepair::prepost_calc_job_protected_damage(
        decoded_file_size,
        holes_total_size,
        failed_size,
        missed_size,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_prepost_is_damage_beyond_par(
    protected_damage: i64,
    par_size: i64,
    par_current_failed_size: i64,
) -> c_int {
    i32::from(crate::histscanrepair::prepost_is_damage_beyond_par(
        protected_damage,
        par_size,
        par_current_failed_size,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_prepost_should_check_unrecovered_stream_skip(
    par_cannot_cover: c_int,
    stream_repair_done: c_int,
    stream_repair_recovered: c_int,
    dupe_recovered_bytes: i64,
    par_check_opt: c_int,
    pc_auto: c_int,
    par_status: c_int,
    ps_none: c_int,
    delete_status: c_int,
    ds_none: c_int,
) -> c_int {
    i32::from(
        crate::histscanrepair::prepost_should_check_unrecovered_stream_skip(
            par_cannot_cover != 0,
            stream_repair_done != 0,
            stream_repair_recovered != 0,
            dupe_recovered_bytes,
            par_check_opt,
            pc_auto,
            par_status,
            ps_none,
            delete_status,
            ds_none,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_prepost_classify_skipped_par_health(
    par_status: c_int,
    ps_skipped: c_int,
    par_scan_opt: c_int,
    ps_dupe: c_int,
    health: c_int,
    critical_health: c_int,
    failed_size: i64,
    par_failed_size: i64,
) -> c_int {
    crate::histscanrepair::prepost_classify_skipped_par_health(
        par_status,
        ps_skipped,
        par_scan_opt,
        ps_dupe,
        health,
        critical_health,
        failed_size,
        par_failed_size,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_hsr_prepost_eval_want_unpack_and_rar_rename(
    has_unpack_param: c_int,
    unpack_param_val: *const c_char,
    unpack_status: c_int,
    us_none: c_int,
    delete_status: c_int,
    ds_none: c_int,
    rar_rename_status: c_int,
    rs_none: c_int,
    opt_rar_rename: c_int,
    out_want_unpack: *mut c_int,
    out_unpack: *mut c_int,
) -> c_int {
    let val = if unpack_param_val.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(unpack_param_val).to_bytes()
    };
    let (w, u, r) = crate::histscanrepair::prepost_eval_want_unpack_and_rar_rename(
        has_unpack_param != 0,
        val,
        unpack_status,
        us_none,
        delete_status,
        ds_none,
        rar_rename_status,
        rs_none,
        opt_rar_rename != 0,
    );
    *out_want_unpack = i32::from(w);
    *out_unpack = i32::from(u);
    i32::from(r)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_hsr_prepost_classify_file_downloaded_direct_unpack(
    opt_direct_unpack: c_int,
    opt_raw_article: c_int,
    opt_skip_write: c_int,
    skip_disk_write: c_int,
    direct_unpack_status: c_int,
    ns_none: c_int,
    ns_running: c_int,
    direct_rename_status: c_int,
    ts_running: c_int,
    has_unpack_thread: c_int,
) -> c_int {
    crate::histscanrepair::prepost_classify_file_downloaded_direct_unpack(
        opt_direct_unpack != 0,
        opt_raw_article != 0,
        opt_skip_write != 0,
        skip_disk_write != 0,
        direct_unpack_status,
        ns_none,
        ns_running,
        direct_rename_status,
        ts_running,
        has_unpack_thread != 0,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stq_stream_repair_is_first_donor_part_valid(
    has_first: c_int,
    success: c_int,
    offset: i64,
) -> c_int {
    i32::from(
        crate::streamtwinqueue::stream_repair_is_first_donor_part_valid(
            has_first != 0,
            success != 0,
            offset,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_step_part_for_offset(
    offset: i64,
    fetched_offset: i64,
    fetched_len: usize,
    index: c_int,
    ranges_len: usize,
    out_new_index: *mut c_int,
) -> c_int {
    let (status, new_idx) = crate::streamtwinqueue::stream_repair_step_part_for_offset(
        offset,
        fetched_offset,
        fetched_len,
        index,
        ranges_len,
    );
    *out_new_index = new_idx;
    status
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_calc_prefetch_bounds(
    first_est: c_int,
    last_est: c_int,
    ranges_len: usize,
    out_first: *mut c_int,
    out_last: *mut c_int,
) {
    let (f, l) =
        crate::streamtwinqueue::stream_repair_calc_prefetch_bounds(first_est, last_est, ranges_len);
    *out_first = f;
    *out_last = l;
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_calc_donor_read_chunk(
    pos: i64,
    fetched_offset: i64,
    fetched_data_len: usize,
    end_offset: i64,
    out_from: *mut i64,
    out_len: *mut i64,
) {
    let (from, len) = crate::streamtwinqueue::stream_repair_calc_donor_read_chunk(
        pos,
        fetched_offset,
        fetched_data_len,
        end_offset,
    );
    *out_from = from;
    *out_len = len;
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_is_par2_job_or_file(
    is_par_file: c_int,
    filename: *const c_char,
) -> c_int {
    let fname = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    i32::from(crate::streamtwinqueue::stream_repair_is_par2_job_or_file(
        is_par_file != 0,
        fname,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stq_stream_repair_is_donor_nzb_pre_eligible(
    is_self: c_int,
    kind: c_int,
    nk_nzb: c_int,
    dupe_mode: c_int,
    dm_force: c_int,
) -> c_int {
    i32::from(
        crate::streamtwinqueue::stream_repair_is_donor_nzb_pre_eligible(
            is_self != 0,
            kind,
            nk_nzb,
            dupe_mode,
            dm_force,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_should_skip_donor_candidate(
    own_full_hash: u32,
    donor_full_hash: u32,
    queued_filename: *const c_char,
) -> c_int {
    let qname = if queued_filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(queued_filename).to_bytes()
    };
    i32::from(
        crate::streamtwinqueue::stream_repair_should_skip_donor_candidate(
            own_full_hash,
            donor_full_hash,
            qname,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_eval_donor_outcome_bail(
    outcome: c_int,
    ro_productive: c_int,
    ro_unproductive: c_int,
    donor_proven: c_int,
    cur_consecutive_failures: c_int,
    donor_failure_bail: c_int,
    unproven_donor_bail: c_int,
    out_new_consecutive_failures: *mut c_int,
) -> c_int {
    let (next, bail) = crate::streamtwinqueue::stream_repair_eval_donor_outcome_bail(
        outcome,
        ro_productive,
        ro_unproductive,
        donor_proven != 0,
        cur_consecutive_failures,
        donor_failure_bail,
        unproven_donor_bail,
    );
    *out_new_consecutive_failures = next;
    i32::from(bail)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stq_stream_repair_resolve_repair_file_outcome(
    patched: c_int,
    spent_fetches: c_int,
    ro_productive: c_int,
    ro_unproductive: c_int,
    ro_no_cost: c_int,
) -> c_int {
    crate::streamtwinqueue::stream_repair_resolve_repair_file_outcome(
        patched != 0,
        spent_fetches != 0,
        ro_productive,
        ro_unproductive,
        ro_no_cost,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stq_stream_repair_calc_probe_replacement_count(
    missing: c_int,
    reserve_len: usize,
    max_probe_fetches: c_int,
    attempted_len: usize,
) -> c_int {
    crate::streamtwinqueue::stream_repair_calc_probe_replacement_count(
        missing,
        reserve_len,
        max_probe_fetches,
        attempted_len,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stq_stream_repair_pick_replacement_index(
    reserve_len: usize,
    k: c_int,
    replacements: c_int,
) -> usize {
    crate::streamtwinqueue::stream_repair_pick_replacement_index(reserve_len, k, replacements)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_is_probe_article_valid(
    success: c_int,
    data_len: usize,
    file_size: i64,
    offset: i64,
    target_decoded_size: i64,
    out_is_missing: *mut c_int,
) -> c_int {
    let (valid, missing) = crate::streamtwinqueue::stream_repair_is_probe_article_valid(
        success != 0,
        data_len,
        file_size,
        offset,
        target_decoded_size,
    );
    *out_is_missing = i32::from(missing);
    i32::from(valid)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_clip_hole_to_fetched(
    hole_offset: i64,
    hole_size: i64,
    fetched_offset: i64,
    fetched_len: usize,
    out_from: *mut i64,
    out_to: *mut i64,
) -> c_int {
    if let Some((from, to)) = crate::streamtwinqueue::stream_repair_clip_hole_to_fetched(
        hole_offset,
        hole_size,
        fetched_offset,
        fetched_len,
    ) {
        *out_from = from;
        *out_to = to;
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_classify_remaining_hole_target(
    interrupted: c_int,
    decoded_file_size: i64,
    holes_empty: c_int,
    is_par_file: c_int,
    failed_articles: c_int,
    failed_size: i64,
    missed_size: i64,
    inout_recovered_articles: *mut c_int,
    inout_recovered_failed_size: *mut i64,
    inout_recovered_failed_par_size: *mut i64,
) -> c_int {
    let (status, rec_arts, rec_fsize, rec_pfsize) =
        crate::streamtwinqueue::stream_repair_classify_remaining_hole_target(
            interrupted != 0,
            decoded_file_size,
            holes_empty != 0,
            is_par_file != 0,
            failed_articles,
            failed_size,
            missed_size,
            *inout_recovered_articles,
            *inout_recovered_failed_size,
            *inout_recovered_failed_par_size,
        );
    *inout_recovered_articles = rec_arts;
    *inout_recovered_failed_size = rec_fsize;
    *inout_recovered_failed_par_size = rec_pfsize;
    status
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_patch_from_donor_set_step_failure(
    chunk: i64,
    min_chunk: i64,
    inout_size: *mut i64,
    inout_pos: *mut i64,
    inout_failed_pieces: *mut c_int,
    max_failed_pieces: c_int,
) -> c_int {
    let (action, new_sz, new_pos, new_failed) =
        crate::streamtwinqueue::stream_repair_patch_from_donor_set_step_failure(
            chunk,
            min_chunk,
            *inout_size,
            *inout_pos,
            *inout_failed_pieces,
            max_failed_pieces,
        );
    *inout_size = new_sz;
    *inout_pos = new_pos;
    *inout_failed_pieces = new_failed;
    action
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_encrypted_clip_live_hole(
    hole_offset: i64,
    hole_size: i64,
    live_holes: *const crate::dupefallback::NzbgetRsByteRange,
    live_holes_len: usize,
    last_computable: i64,
    block_size: i64,
    inout_uncomputable_tail: *mut i64,
    out_hole_from: *mut i64,
    out_patch_to: *mut i64,
    out_block_from: *mut i64,
    out_block_to: *mut i64,
) -> c_int {
    let slice = if live_holes.is_null() || live_holes_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(live_holes, live_holes_len)
    };
    let (ok, new_tail, h_from, p_to, b_from, b_to) =
        crate::streamtwinqueue::stream_repair_encrypted_clip_live_hole(
            hole_offset,
            hole_size,
            slice,
            last_computable,
            block_size,
            *inout_uncomputable_tail,
        );
    *inout_uncomputable_tail = new_tail;
    *out_hole_from = h_from;
    *out_patch_to = p_to;
    *out_block_from = b_from;
    *out_block_to = b_to;
    i32::from(ok)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_encrypted_is_prev_block_present(
    block_from: i64,
    block_size: i64,
    live_holes: *const crate::dupefallback::NzbgetRsByteRange,
    live_holes_len: usize,
) -> c_int {
    let slice = if live_holes.is_null() || live_holes_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(live_holes, live_holes_len)
    };
    i32::from(
        crate::streamtwinqueue::stream_repair_encrypted_is_prev_block_present(
            block_from, block_size, slice,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_stream_repair_resolve_completion_par_action(
    recovered_bytes: i64,
    holes_remain: c_int,
    par_rename_status: c_int,
    rs_none: c_int,
    par_status: c_int,
    ps_skipped: c_int,
    par_check_opt: c_int,
    pc_manual: c_int,
    ps_manual: c_int,
    ps_none: c_int,
    out_reset_par_rename: *mut c_int,
    out_set_par_status: *mut c_int,
    out_new_par_status: *mut c_int,
    out_force_par_full: *mut c_int,
    out_set_request_par_check_false: *mut c_int,
    out_check_main_pars: *mut c_int,
) {
    let (rst_ren, set_ps, new_ps, force_full, req_false, chk_pars) =
        crate::streamtwinqueue::stream_repair_resolve_completion_par_action(
            recovered_bytes,
            holes_remain != 0,
            par_rename_status,
            rs_none,
            par_status,
            ps_skipped,
            par_check_opt,
            pc_manual,
            ps_manual,
            ps_none,
        );
    *out_reset_par_rename = i32::from(rst_ren);
    *out_set_par_status = i32::from(set_ps);
    *out_new_par_status = new_ps;
    *out_force_par_full = i32::from(force_full);
    *out_set_request_par_check_false = i32::from(req_false);
    *out_check_main_pars = i32::from(chk_pars);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_dupe_stream_repair_is_extracted_entry_valid(
    entry_count: usize,
    max_entries: usize,
    size: i64,
    max_bytes: i64,
    inout_extracted_bytes: *mut i64,
    inner_size: i64,
    out_is_size_match: *mut c_int,
) -> c_int {
    let (valid, next_bytes, is_match) =
        crate::streamtwinqueue::dupe_stream_repair_is_extracted_entry_valid(
            entry_count,
            max_entries,
            size,
            max_bytes,
            *inout_extracted_bytes,
            inner_size,
        );
    *inout_extracted_bytes = next_bytes;
    *out_is_size_match = i32::from(is_match);
    i32::from(valid)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stq_dupe_stream_repair_calc_abs_size_diff(
    size1: i64,
    size2: i64,
) -> i64 {
    crate::streamtwinqueue::dupe_stream_repair_calc_abs_size_diff(size1, size2)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stq_twin_check_eval_index_fetch_retry(
    fetched: c_int,
    known: c_int,
    complete: c_int,
    next_attempts: c_int,
    max_attempts: c_int,
) -> c_int {
    i32::from(crate::streamtwinqueue::twin_check_eval_index_fetch_retry(
        fetched != 0,
        known != 0,
        complete != 0,
        next_attempts,
        max_attempts,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_twin_check_is_fetched_entry_part_valid(
    success: c_int,
    offset: i64,
    data_len: usize,
    max_size: i64,
    cur_data_len: usize,
    out_new_data_len: *mut usize,
) -> c_int {
    let (valid, new_len) = crate::streamtwinqueue::twin_check_is_fetched_entry_part_valid(
        success != 0,
        offset,
        data_len,
        max_size,
        cur_data_len,
    );
    *out_new_data_len = new_len;
    i32::from(valid)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_twin_check_is_print_usable(print: *const c_char) -> c_int {
    let bytes = if print.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(print).to_bytes()
    };
    i32::from(crate::streamtwinqueue::twin_check_is_print_usable(bytes))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stq_twin_check_is_listless_from_counts(
    has_sigs: c_int,
    attempts: c_int,
    max_attempts: c_int,
) -> c_int {
    i32::from(
        crate::streamtwinqueue::twin_check_is_listless_from_counts(has_sigs, attempts, max_attempts),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_twin_check_eval_sampleable(
    primary_ok: c_int,
    is_primary: c_int,
    is_dupe: c_int,
    print_empty: c_int,
    primary_print_empty: c_int,
    by_par2: c_int,
    has_sampled: c_int,
    sampled_of: *const c_char,
    primary_id: *const c_char,
    out_sampled_now: *mut c_int,
) -> c_int {
    let s_of = if sampled_of.is_null() {
        None
    } else {
        Some(CStr::from_ptr(sampled_of).to_bytes())
    };
    let p_id = if primary_id.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(primary_id).to_bytes()
    };
    let (sampleable, sampled_now) = crate::streamtwinqueue::twin_check_eval_sampleable(
        primary_ok != 0,
        is_primary != 0,
        is_dupe != 0,
        print_empty != 0,
        primary_print_empty != 0,
        by_par2 != 0,
        has_sampled != 0,
        s_of,
        p_id,
    );
    *out_sampled_now = i32::from(sampled_now);
    i32::from(sampleable)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stq_queue_editor_resolve_auto_sort_order(
    sort_order: c_int,
    so_auto: c_int,
    so_descending: c_int,
    sort_criteria: c_int,
    sc_priority: c_int,
) -> c_int {
    crate::streamtwinqueue::queue_editor_resolve_auto_sort_order(
        sort_order,
        so_auto,
        so_descending,
        sort_criteria,
        sc_priority,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stq_queue_editor_flip_sort_order(
    sort_order: c_int,
    so_descending: c_int,
    so_ascending: c_int,
) -> c_int {
    crate::streamtwinqueue::queue_editor_flip_sort_order(sort_order, so_descending, so_ascending)
}

#[no_mangle]
pub extern "C" fn nzbget_rs_stq_queue_editor_resolve_prepare_list_offset(
    action: c_int,
    ea_file_move_top: c_int,
    ea_group_move_top: c_int,
    ea_file_move_bottom: c_int,
    ea_group_move_bottom: c_int,
    max_id: c_int,
    cur_offset: c_int,
) -> c_int {
    crate::streamtwinqueue::queue_editor_resolve_prepare_list_offset(
        action,
        ea_file_move_top,
        ea_group_move_top,
        ea_file_move_bottom,
        ea_group_move_bottom,
        max_id,
        cur_offset,
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_queue_editor_calc_move_sweep_bounds(
    nr_entries: c_int,
    offset: c_int,
    le_zero_forward: c_int,
    out_start: *mut c_int,
    out_end: *mut c_int,
    out_step: *mut c_int,
) {
    let (s, e, st) = crate::streamtwinqueue::queue_editor_calc_move_sweep_bounds(
        nr_entries,
        offset,
        le_zero_forward != 0,
    );
    *out_start = s;
    *out_end = e;
    *out_step = st;
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_stq_queue_editor_eval_align_group_step(
    selected: c_int,
    has_last_nzb: c_int,
    num: u32,
    last_num: u32,
    out_new_last_num: *mut u32,
) -> c_int {
    let (mv, new_last) = crate::streamtwinqueue::queue_editor_eval_align_group_step(
        selected != 0,
        has_last_nzb != 0,
        num,
        last_num,
    );
    *out_new_last_num = new_last;
    i32::from(mv)
}

// ============================================================================
// DirectUnpack, Cleanup, UnpackController, UrlCoordinator, FeedCoordinator & RemoteServer Hot-Path FFI (`unpackcleanurlfeed.rs`)
// ============================================================================

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_direct_unpack_should_clear_working(
    has_post_info: c_int,
    has_live_repair_thread: c_int,
) -> c_int {
    i32::from(
        crate::unpackcleanurlfeed::direct_unpack_should_clear_working(
            has_post_info != 0,
            has_live_repair_thread != 0,
        ),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ucf_direct_unpack_use_final_dir(
    use_inter_dir: c_int,
    final_dir: *const c_char,
) -> c_int {
    let bytes = if final_dir.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(final_dir).to_bytes()
    };
    i32::from(crate::unpackcleanurlfeed::direct_unpack_use_final_dir(
        use_inter_dir != 0,
        bytes,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ucf_direct_unpack_is_progress_dots_line(
    buf: *const c_char,
) -> c_int {
    let bytes = if buf.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(buf).to_bytes()
    };
    i32::from(crate::unpackcleanurlfeed::direct_unpack_is_progress_dots_line(bytes))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_direct_unpack_should_set_cancelled(
    has_prepost: c_int,
    prepost_stopped: c_int,
) -> c_int {
    i32::from(
        crate::unpackcleanurlfeed::direct_unpack_should_set_cancelled(
            has_prepost != 0,
            prepost_stopped != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_direct_unpack_should_try_break(
    is_exec_thread: c_int,
    cancelled_or_auto_terminated: c_int,
    has_active_process: c_int,
) -> c_int {
    i32::from(crate::unpackcleanurlfeed::direct_unpack_should_try_break(
        is_exec_thread != 0,
        cancelled_or_auto_terminated != 0,
        has_active_process != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ucf_direct_unpack_has_dest_changed(
    has_nzb: c_int,
    cur_dest: *const c_char,
    nzb_dest: *const c_char,
    cur_final: *const c_char,
    nzb_final: *const c_char,
) -> c_int {
    let cd = if cur_dest.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(cur_dest).to_bytes()
    };
    let nd = if nzb_dest.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(nzb_dest).to_bytes()
    };
    let cf = if cur_final.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(cur_final).to_bytes()
    };
    let nf = if nzb_final.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(nzb_final).to_bytes()
    };
    i32::from(crate::unpackcleanurlfeed::direct_unpack_has_dest_changed(
        has_nzb != 0,
        cd,
        nd,
        cf,
        nf,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ucf_direct_unpack_matches_waiting_file(
    waiting_file: *const c_char,
    downloaded_filename: *const c_char,
) -> c_int {
    let wf = if waiting_file.is_null() {
        None
    } else {
        Some(CStr::from_ptr(waiting_file).to_bytes())
    };
    let df = if downloaded_filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(downloaded_filename).to_bytes()
    };
    i32::from(crate::unpackcleanurlfeed::direct_unpack_matches_waiting_file(wf, df))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_direct_unpack_can_proceed(
    is_archive: c_int,
    failed_articles: c_int,
) -> c_int {
    i32::from(crate::unpackcleanurlfeed::direct_unpack_can_proceed(
        is_archive != 0,
        failed_articles,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ucf_direct_unpack_calc_extra_time(
    extra_start_time: i64,
    cur_time: i64,
    cur_unpack_sec: c_int,
    cur_post_total_sec: c_int,
    out_new_unpack_sec: *mut c_int,
    out_new_post_total_sec: *mut c_int,
) -> c_int {
    let (applied, u, p) = crate::unpackcleanurlfeed::direct_unpack_calc_extra_time(
        extra_start_time,
        cur_time,
        cur_unpack_sec,
        cur_post_total_sec,
    );
    *out_new_unpack_sec = u;
    *out_new_post_total_sec = p;
    i32::from(applied)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ucf_move_ctrl_is_dot_file(
    filename: *const c_char,
) -> c_int {
    let bytes = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    i32::from(crate::unpackcleanurlfeed::move_ctrl_is_dot_file(bytes))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_cleanup_ctrl_should_delete_entry(
    is_dir: c_int,
    matches_ext: c_int,
) -> c_int {
    i32::from(crate::unpackcleanurlfeed::cleanup_ctrl_should_delete_entry(
        is_dir != 0,
        matches_ext != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ucf_unpack_ctrl_eval_unpack_param(
    param_val: *const c_char,
) -> c_int {
    let pv = if param_val.is_null() {
        None
    } else {
        Some(CStr::from_ptr(param_val).to_bytes())
    };
    i32::from(crate::unpackcleanurlfeed::unpack_ctrl_eval_unpack_param(pv))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_unpack_ctrl_has_any_archive_files(
    has_rar: c_int,
    has_7z: c_int,
    has_7z_multi: c_int,
    has_splitted: c_int,
) -> c_int {
    i32::from(
        crate::unpackcleanurlfeed::unpack_ctrl_has_any_archive_files(
            has_rar != 0,
            has_7z != 0,
            has_7z_multi != 0,
            has_splitted != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_unpack_ctrl_can_continue_password_loop(
    unpack_ok: c_int,
    start_err: c_int,
    space_err: c_int,
    decrypt_err: c_int,
    password_err: c_int,
) -> c_int {
    i32::from(
        crate::unpackcleanurlfeed::unpack_ctrl_can_continue_password_loop(
            unpack_ok != 0,
            start_err != 0,
            space_err != 0,
            decrypt_err != 0,
            password_err != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_unpack_ctrl_should_resume_before_password_try(
    is_stopped: c_int,
    auto_terminated: c_int,
) -> c_int {
    i32::from(
        crate::unpackcleanurlfeed::unpack_ctrl_should_resume_before_password_try(
            is_stopped != 0,
            auto_terminated != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_unpack_ctrl_eval_pass_list_tried(
    is_stopped: c_int,
    auto_terminated: c_int,
) -> c_int {
    i32::from(crate::unpackcleanurlfeed::unpack_ctrl_eval_pass_list_tried(
        is_stopped != 0,
        auto_terminated != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ucf_unpack_ctrl_is_split_first_piece(
    filename: *const c_char,
) -> c_int {
    let bytes = if filename.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filename).to_bytes()
    };
    i32::from(crate::unpackcleanurlfeed::unpack_ctrl_is_split_first_piece(
        bytes,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ucf_unpack_ctrl_calc_join_progress(
    cur_written: i64,
    cnt: c_int,
    total_size: i64,
    out_new_written: *mut i64,
) -> c_int {
    let (w, p) = crate::unpackcleanurlfeed::unpack_ctrl_calc_join_progress(
        cur_written,
        cnt,
        total_size,
    );
    *out_new_written = w;
    p
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_unpack_ctrl_is_join_size_mismatch(
    ok: c_int,
    written: i64,
    total_size: i64,
) -> c_int {
    i32::from(crate::unpackcleanurlfeed::unpack_ctrl_is_join_size_mismatch(
        ok != 0,
        written,
        total_size,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_unpack_ctrl_is_downloaded_file_allowed(
    use_temp_unpack_dir: c_int,
    downloaded_empty: c_int,
) -> c_int {
    i32::from(
        crate::unpackcleanurlfeed::unpack_ctrl_is_downloaded_file_allowed(
            use_temp_unpack_dir != 0,
            downloaded_empty != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_unpack_ctrl_should_move_from_temp_unpack(
    unpack_ok: c_int,
    use_temp_unpack_dir: c_int,
) -> c_int {
    i32::from(
        crate::unpackcleanurlfeed::unpack_ctrl_should_move_from_temp_unpack(
            unpack_ok != 0,
            use_temp_unpack_dir != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_unpack_ctrl_should_cleanup_archive_disks(
    unpack_ok: c_int,
    move_ok: c_int,
    unpack_cleanup_disk: c_int,
) -> c_int {
    i32::from(
        crate::unpackcleanurlfeed::unpack_ctrl_should_cleanup_archive_disks(
            unpack_ok != 0,
            move_ok != 0,
            unpack_cleanup_disk != 0,
        ),
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_url_coord_is_wake_action(
    action: c_int,
    ea_url_added: c_int,
    ea_url_returned: c_int,
) -> c_int {
    i32::from(crate::unpackcleanurlfeed::url_coord_is_wake_action(
        action,
        ea_url_added,
        ea_url_returned,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_url_coord_resolve_delete_status(
    cur_delete_status: c_int,
    ds_none: c_int,
    ds_manual: c_int,
) -> c_int {
    crate::unpackcleanurlfeed::url_coord_resolve_delete_status(
        cur_delete_status,
        ds_none,
        ds_manual,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_feed_coord_can_check_feeds(
    pause_download: c_int,
    force: c_int,
    url_force: c_int,
) -> c_int {
    i32::from(crate::unpackcleanurlfeed::feed_coord_can_check_feeds(
        pause_download != 0,
        force != 0,
        url_force != 0,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_feed_coord_use_feed_id_temp_file(
    id: c_int,
    preview: c_int,
) -> c_int {
    i32::from(crate::unpackcleanurlfeed::feed_coord_use_feed_id_temp_file(
        id,
        preview != 0,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ucf_feed_coord_can_use_cache(
    cache_time_sec: c_int,
    cache_id: *const c_char,
) -> c_int {
    let bytes = if cache_id.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(cache_id).to_bytes()
    };
    i32::from(crate::unpackcleanurlfeed::feed_coord_can_use_cache(
        cache_time_sec,
        bytes,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ucf_feed_coord_matches_existing_feed(
    url1: *const c_char,
    url2: *const c_char,
    filter1: *const c_char,
    filter2: *const c_char,
    last_update: i64,
) -> c_int {
    let u1 = if url1.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(url1).to_bytes()
    };
    let u2 = if url2.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(url2).to_bytes()
    };
    let f1 = if filter1.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filter1).to_bytes()
    };
    let f2 = if filter2.is_null() {
        &[][..]
    } else {
        CStr::from_ptr(filter2).to_bytes()
    };
    i32::from(crate::unpackcleanurlfeed::feed_coord_matches_existing_feed(
        u1,
        u2,
        f1,
        f2,
        last_update,
    ))
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_feed_coord_resolve_preview_item_status(
    has_history: c_int,
    history_status: c_int,
    first_fetch: c_int,
    backlog: c_int,
    is_backlog: c_int,
    is_new: c_int,
) -> c_int {
    crate::unpackcleanurlfeed::feed_coord_resolve_preview_item_status(
        has_history != 0,
        history_status,
        first_fetch != 0,
        backlog != 0,
        is_backlog,
        is_new,
    )
}

#[no_mangle]
pub extern "C" fn nzbget_rs_ucf_remote_server_is_accept_busy_retry(
    bind: c_int,
    has_accepted_conn: c_int,
    accept_busy: c_int,
    is_stopped: c_int,
) -> c_int {
    i32::from(
        crate::unpackcleanurlfeed::remote_server_is_accept_busy_retry(
            bind != 0,
            has_accepted_conn != 0,
            accept_busy != 0,
            is_stopped != 0,
        ),
    )
}










