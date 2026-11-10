//! The binary remote protocol's frame (BinRpc.cpp): the checks
//! BinRpcProcessor makes on a request's base - credentials compared as the
//! C++ strcmp compared them, the request type, its size - the request names
//! for the log, the bool responses and the check of a request's trailing
//! data length. Integers on the wire are big-endian (htonl/ntohl). Reading
//! from and writing to the connection, and what each command does, stay C++.

use std::ffi::CStr;
use std::mem::MaybeUninit;

/// NZBMESSAGE_SIGNATURE ("nzb1").
pub const SIGNATURE: i32 = 0x6E7A_6231;
/// NZBREQUESTPASSWORDSIZE.
const PASSWORD_SIZE: usize = 32;
/// sizeof(SNzbRequestBase): signature, struct size, type, user, password.
pub const BASE_SIZE: usize = 12 + 2 * PASSWORD_SIZE;
const USER_AT: usize = 12;
const PASSWORD_AT: usize = USER_AT + PASSWORD_SIZE;
/// rrHistory, the last request type.
pub const LAST_TYPE: u32 = 14;

/// g_MessageRequestNames.
const NAMES: [&CStr; LAST_TYPE as usize + 1] = [
    c"N/A",
    c"Download",
    c"Pause/Unpause",
    c"List",
    c"Set download rate",
    c"Dump debug",
    c"Edit queue",
    c"Log",
    c"Quit",
    c"Reload",
    c"Version",
    c"Post-queue",
    c"Write log",
    c"Scan",
    c"History",
];

fn be32(base: &[u8; BASE_SIZE], at: usize) -> u32 {
    u32::from_be_bytes(base[at..at + 4].try_into().unwrap())
}

/// A C string in the base from `at`: strcmp read on past an unterminated
/// field (the user name into the password); the base's end stops it here.
fn c_field(base: &[u8; BASE_SIZE], at: usize) -> &[u8] {
    let rest = &base[at..];
    &rest[..rest.iter().position(|&b| b == 0).unwrap_or(rest.len())]
}

/// BinRpcProcessor::Execute's credential check: true when the request may
/// go on (an empty ControlUsername isn't compared).
pub fn credentials_ok(
    base: &[u8; BASE_SIZE],
    control_user: &[u8],
    control_password: &[u8],
) -> bool {
    !((!control_user.is_empty() && c_field(base, USER_AT) != control_user)
        || c_field(base, PASSWORD_AT) != control_password)
}

/// The request type (ntohl(m_type)).
pub fn request_type(base: &[u8; BASE_SIZE]) -> u32 {
    be32(base, 8)
}

/// The request's name in the debug line ("Unknown" past rrHistory).
pub fn request_name(kind: u32) -> &'static CStr {
    NAMES.get(kind as usize).copied().unwrap_or(c"Unknown")
}

/// Dispatch's size check: Some((expected, received)) when a known request
/// type comes with another struct size than g_MessageRequestSizes gives.
pub fn size_error(base: &[u8; BASE_SIZE], sizes: &[u32]) -> Option<(u32, u32)> {
    let kind = request_type(base);
    let size = be32(base, 4);
    if (1..=LAST_TYPE).contains(&kind) {
        let expected = sizes.get(kind as usize).copied()?;
        if expected != size {
            return Some((expected, size));
        }
    }
    None
}

/// SendBoolResponse's bytes: SNzbDownloadResponse (signature, struct size,
/// success, text length) then the text and its NUL.
pub fn bool_response(success: bool, text: &[u8]) -> Vec<u8> {
    let text = &text[..text.iter().position(|&b| b == 0).unwrap_or(text.len())];
    let mut out = Vec::with_capacity(16 + text.len() + 1);
    out.extend_from_slice(&(SIGNATURE as u32).to_be_bytes());
    out.extend_from_slice(&16u32.to_be_bytes());
    out.extend_from_slice(&(success as u32).to_be_bytes());
    out.extend_from_slice(&((text.len() + 1) as u32).to_be_bytes());
    out.extend_from_slice(text);
    out.push(0);
    out
}

/// ReceiveTrailing's check of the client's data length.
pub fn trailing_ok(len: i64, max: i64) -> bool {
    !(len < 0 || len > max)
}

/// One entry of a list response as the remote client reads it (the existing wire layout): `ints.len()` big-endian ints at `at` into
/// `ints` (host order), and the start of each string, whose length is in
/// field `fields[k]`, into `starts[k]`. Returns the offset after the entry
/// (the header and every length in `fields`), or None (outputs untouched)
/// when the entry or one of its strings, up to its NUL, doesn't fit in
/// `data`.
pub fn read_entry(
    data: &[u8],
    at: usize,
    ints: &mut [u32],
    fields: &[usize],
    starts: &mut [usize],
) -> Option<usize> {
    let header = at.checked_add(ints.len().checked_mul(4)?)?;
    let raw = data.get(at..header)?;
    let value = |k: usize| u32::from_be_bytes(raw[4 * k..4 * k + 4].try_into().unwrap()) as usize;
    let mut next = header;
    for &field in fields {
        if field >= ints.len() {
            return None;
        }
        let start = next;
        // the string ends within the data
        crate::bytes::find_byte(data.get(start..)?, 0)?;
        next = next.checked_add(value(field))?;
    }
    if next > data.len() {
        return None;
    }
    let mut start = header;
    for (k, &field) in fields.iter().enumerate() {
        if let Some(out) = starts.get_mut(k) {
            *out = start;
        }
        start += value(field);
    }
    for (k, out) in ints.iter_mut().enumerate() {
        *out = value(k) as u32;
    }
    Some(next)
}

/// A string of an entry: `len` bytes at `data`, without a NUL (the NUL is
/// the encoder's). `data` may be null for length 0.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Text {
    pub data: *const u8,
    pub len: usize,
}

impl Text {
    pub const EMPTY: Text = Text {
        data: std::ptr::null(),
        len: 0,
    };

    pub fn new(bytes: &[u8]) -> Text {
        Text {
            data: bytes.as_ptr(),
            len: bytes.len(),
        }
    }

    /// # Safety
    /// `data` is readable for `len` bytes (or null with `len` 0) while the
    /// result is used.
    pub unsafe fn bytes<'a>(&self) -> &'a [u8] {
        if self.len == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(self.data, self.len)
        }
    }
}

/// The most ints in an entry's header (an NZB's) and the most strings.
pub const MAX_HEADER: usize = 17;
pub const MAX_STRINGS: usize = 5;

/// One kind of a list response's entries (List, Log, PostQueue, History):
/// a header of big-endian ints, among them the lengths (with their NULs) of
/// the strings that follow, each with its NUL, then zeros to a 4-byte
/// boundary counted in the length at `PAD`, as the C++ aligned the entries
/// (its buffer started aligned, so the offset decides). The values are in
/// host order here; fields the entry doesn't set are 0.
pub trait Entry: Copy {
    /// The ints in the header.
    const HEADER: usize;
    /// The header index of each string's length, in the strings' order.
    const FIELDS: &'static [usize];
    /// The index of the length that takes the padding.
    const PAD: usize;
    /// Sets the header's ints other than the strings' lengths (h is 0).
    fn header(&self, h: &mut [u32; MAX_HEADER]);
    fn texts(&self) -> &[Text];
    /// The entry as read: the header's ints and the strings.
    fn from_wire(h: &[u32; MAX_HEADER], texts: &[Text; MAX_STRINGS]) -> Self;
}

/// An NZB of the queue (SNzbListResponseNzbEntry).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NzbEntry {
    pub id: u32,
    pub kind: u32,
    pub size_lo: u32,
    pub size_hi: u32,
    pub remaining_size_lo: u32,
    pub remaining_size_hi: u32,
    pub paused_size_lo: u32,
    pub paused_size_hi: u32,
    pub paused_count: u32,
    pub remaining_par_count: u32,
    pub priority: u32,
    pub matched: u32,
    /// filename, name, dest dir, category, queued filename
    pub texts: [Text; 5],
}

/// A parameter of an NZB (SNzbListResponsePPPEntry); `nzb_index` counts from 1.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ParamEntry {
    pub nzb_index: u32,
    /// name, value
    pub texts: [Text; 2],
}

/// A file of an NZB (SNzbListResponseFileEntry); `nzb_index` counts from 1.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FileEntry {
    pub id: u32,
    pub nzb_index: u32,
    pub size_lo: u32,
    pub size_hi: u32,
    pub remaining_size_lo: u32,
    pub remaining_size_hi: u32,
    pub paused: u32,
    pub filename_confirmed: u32,
    pub active_downloads: u32,
    pub matched: u32,
    /// subject, filename
    pub texts: [Text; 2],
}

/// A log message (SNzbLogResponseEntry).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LogEntry {
    pub id: u32,
    pub kind: u32,
    pub time: u32,
    pub texts: [Text; 1],
}

/// A post-processing job (SNzbPostQueueResponseEntry).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PostEntry {
    pub id: u32,
    pub stage: u32,
    pub stage_progress: u32,
    pub file_progress: u32,
    pub start_time: u32,
    pub stage_time: u32,
    /// nzb filename, name, dest dir, progress label
    pub texts: [Text; 4],
}

/// A history item (SNzbHistoryResponseEntry).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct HistoryEntry {
    pub id: u32,
    pub kind: u32,
    pub time: u32,
    pub size_lo: u32,
    pub size_hi: u32,
    pub file_count: u32,
    pub par_status: u32,
    pub script_status: u32,
    pub url_status: u32,
    /// name
    pub texts: [Text; 1],
}

fn texts_from<const N: usize>(texts: &[Text; MAX_STRINGS]) -> [Text; N] {
    std::array::from_fn(|k| texts[k])
}

impl Entry for NzbEntry {
    const HEADER: usize = 17;
    const FIELDS: &'static [usize] = &[12, 13, 14, 15, 16];
    const PAD: usize = 16;
    fn header(&self, h: &mut [u32; MAX_HEADER]) {
        h[..12].copy_from_slice(&[
            self.id,
            self.kind,
            self.size_lo,
            self.size_hi,
            self.remaining_size_lo,
            self.remaining_size_hi,
            self.paused_size_lo,
            self.paused_size_hi,
            self.paused_count,
            self.remaining_par_count,
            self.priority,
            self.matched,
        ]);
    }
    fn texts(&self) -> &[Text] {
        &self.texts
    }
    fn from_wire(h: &[u32; MAX_HEADER], texts: &[Text; MAX_STRINGS]) -> Self {
        NzbEntry {
            id: h[0],
            kind: h[1],
            size_lo: h[2],
            size_hi: h[3],
            remaining_size_lo: h[4],
            remaining_size_hi: h[5],
            paused_size_lo: h[6],
            paused_size_hi: h[7],
            paused_count: h[8],
            remaining_par_count: h[9],
            priority: h[10],
            matched: h[11],
            texts: *texts,
        }
    }
}

impl Entry for ParamEntry {
    const HEADER: usize = 3;
    const FIELDS: &'static [usize] = &[1, 2];
    const PAD: usize = 2;
    fn header(&self, h: &mut [u32; MAX_HEADER]) {
        h[0] = self.nzb_index;
    }
    fn texts(&self) -> &[Text] {
        &self.texts
    }
    fn from_wire(h: &[u32; MAX_HEADER], texts: &[Text; MAX_STRINGS]) -> Self {
        ParamEntry {
            nzb_index: h[0],
            texts: texts_from(texts),
        }
    }
}

impl Entry for FileEntry {
    const HEADER: usize = 12;
    const FIELDS: &'static [usize] = &[10, 11];
    const PAD: usize = 11;
    fn header(&self, h: &mut [u32; MAX_HEADER]) {
        h[..10].copy_from_slice(&[
            self.id,
            self.nzb_index,
            self.size_lo,
            self.size_hi,
            self.remaining_size_lo,
            self.remaining_size_hi,
            self.paused,
            self.filename_confirmed,
            self.active_downloads,
            self.matched,
        ]);
    }
    fn texts(&self) -> &[Text] {
        &self.texts
    }
    fn from_wire(h: &[u32; MAX_HEADER], texts: &[Text; MAX_STRINGS]) -> Self {
        FileEntry {
            id: h[0],
            nzb_index: h[1],
            size_lo: h[2],
            size_hi: h[3],
            remaining_size_lo: h[4],
            remaining_size_hi: h[5],
            paused: h[6],
            filename_confirmed: h[7],
            active_downloads: h[8],
            matched: h[9],
            texts: texts_from(texts),
        }
    }
}

impl Entry for LogEntry {
    const HEADER: usize = 4;
    const FIELDS: &'static [usize] = &[3];
    const PAD: usize = 3;
    fn header(&self, h: &mut [u32; MAX_HEADER]) {
        h[..3].copy_from_slice(&[self.id, self.kind, self.time]);
    }
    fn texts(&self) -> &[Text] {
        &self.texts
    }
    fn from_wire(h: &[u32; MAX_HEADER], texts: &[Text; MAX_STRINGS]) -> Self {
        LogEntry {
            id: h[0],
            kind: h[1],
            time: h[2],
            texts: texts_from(texts),
        }
    }
}

impl Entry for PostEntry {
    const HEADER: usize = 10;
    const FIELDS: &'static [usize] = &[6, 7, 8, 9];
    const PAD: usize = 9;
    fn header(&self, h: &mut [u32; MAX_HEADER]) {
        h[..6].copy_from_slice(&[
            self.id,
            self.stage,
            self.stage_progress,
            self.file_progress,
            self.start_time,
            self.stage_time,
        ]);
    }
    fn texts(&self) -> &[Text] {
        &self.texts
    }
    fn from_wire(h: &[u32; MAX_HEADER], texts: &[Text; MAX_STRINGS]) -> Self {
        PostEntry {
            id: h[0],
            stage: h[1],
            stage_progress: h[2],
            file_progress: h[3],
            start_time: h[4],
            stage_time: h[5],
            texts: texts_from(texts),
        }
    }
}

impl Entry for HistoryEntry {
    const HEADER: usize = 10;
    const FIELDS: &'static [usize] = &[3];
    const PAD: usize = 3;
    fn header(&self, h: &mut [u32; MAX_HEADER]) {
        h[..3].copy_from_slice(&[self.id, self.kind, self.time]);
        h[4..10].copy_from_slice(&[
            self.size_lo,
            self.size_hi,
            self.file_count,
            self.par_status,
            self.script_status,
            self.url_status,
        ]);
    }
    fn texts(&self) -> &[Text] {
        &self.texts
    }
    fn from_wire(h: &[u32; MAX_HEADER], texts: &[Text; MAX_STRINGS]) -> Self {
        HistoryEntry {
            id: h[0],
            kind: h[1],
            time: h[2],
            size_lo: h[4],
            size_hi: h[5],
            file_count: h[6],
            par_status: h[7],
            script_status: h[8],
            url_status: h[9],
            texts: texts_from(texts),
        }
    }
}

// Check the POD layouts on both 32-bit and 64-bit targets. The text
// arrays follow the integers at the next pointer-aligned offset.
const _: () = {
    use std::mem::{align_of, offset_of, size_of};
    const fn texts_at(ints: usize) -> usize {
        let align = align_of::<Text>();
        (4 * ints + align - 1) & !(align - 1)
    }
    assert!(size_of::<Text>() == 2 * size_of::<usize>());
    assert!(offset_of!(NzbEntry, texts) == texts_at(12));
    assert!(offset_of!(ParamEntry, texts) == texts_at(1));
    assert!(offset_of!(FileEntry, texts) == texts_at(10));
    assert!(offset_of!(LogEntry, texts) == texts_at(3));
    assert!(offset_of!(PostEntry, texts) == texts_at(6));
    assert!(offset_of!(HistoryEntry, texts) == texts_at(9));
    assert!(size_of::<NzbEntry>() == texts_at(12) + 5 * size_of::<Text>());
    assert!(size_of::<ParamEntry>() == texts_at(1) + 2 * size_of::<Text>());
    assert!(size_of::<FileEntry>() == texts_at(10) + 2 * size_of::<Text>());
    assert!(size_of::<LogEntry>() == texts_at(3) + size_of::<Text>());
    assert!(size_of::<PostEntry>() == texts_at(6) + 4 * size_of::<Text>());
    assert!(size_of::<HistoryEntry>() == texts_at(9) + size_of::<Text>());
};

/// The size of one entry with its strings and padding: the offset after the
/// header is the same for every entry, so each is 4-aligned by itself.
/// Saturates.
unsafe fn entry_len<E: Entry>(entry: &E) -> usize {
    let mut n = 4 * E::HEADER;
    for text in entry.texts() {
        n = n.saturating_add(text.len).saturating_add(1);
    }
    n.saturating_add(3) & !3
}

/// The bytes `encode` writes for `entries`, saturating above MAX_RESPONSE.
///
/// # Safety
/// The entries' strings are readable for their lengths.
pub unsafe fn encoded_len<E: Entry>(entries: &[E]) -> u64 {
    entries
        .iter()
        .fold(0u64, |n, e| n.saturating_add(entry_len(e) as u64))
}

/// Writes the entries into `out`, which is `encoded_len` bytes at least:
/// the header's ints big-endian, each string's length with its NUL in its
/// field, the strings with their NULs, and zeros to a 4-byte boundary
/// counted in the field at `E::PAD`. Returns the bytes written, all of them
/// initialized (the rest of `out` isn't touched).
///
/// # Safety
/// The entries' strings are readable for their lengths, each less than 2^31.
pub unsafe fn encode<E: Entry>(entries: &[E], out: &mut [MaybeUninit<u8>]) -> usize {
    let base = out.as_mut_ptr().cast::<u8>();
    let mut at = 0;
    for entry in entries {
        let mut h = [0u32; MAX_HEADER];
        entry.header(&mut h);
        let texts = entry.texts();
        let mut body = 4 * E::HEADER;
        for (text, &field) in texts.iter().zip(E::FIELDS) {
            h[field] = (text.len + 1) as u32;
            body += text.len + 1;
        }
        let pad = body.wrapping_neg() & 3;
        h[E::PAD] = h[E::PAD].wrapping_add(pad as u32);
        assert!(
            body + pad <= out.len() - at,
            "the entries don't fit the response"
        );
        let mut p = base.add(at);
        for value in &h[..E::HEADER] {
            p.cast::<u32>().write_unaligned(value.to_be());
            p = p.add(4);
        }
        for text in texts {
            // A null pointer is permitted for an empty string, but even a
            // zero-byte copy requires non-null pointers.
            if text.len != 0 {
                std::ptr::copy_nonoverlapping(text.data, p, text.len);
            }
            p.add(text.len).write(0);
            p = p.add(text.len + 1);
        }
        // Padding is at most three bytes. Explicit stores avoid a libc
        // memset call for each entry while initializing every padding byte.
        if pad & 2 != 0 {
            p.cast::<u16>().write_unaligned(0);
        }
        if pad & 1 != 0 {
            p.add(pad - 1).write(0);
        }
        at += body + pad;
    }
    at
}

/// The largest response (an int, as the C++ counted it).
pub const MAX_RESPONSE: u64 = i32::MAX as u64;

/// EditQueue's check of the client's counts and lengths against each other
/// and the trailing data's length (64-bit sums).
pub fn edit_sizes_ok(ids: i32, names: i32, names_len: i32, text_len: i32, data_len: u32) -> bool {
    ids >= 0
        && names >= 0
        && text_len >= 0
        && names_len >= 0
        && ids <= 10_000_000
        && names <= names_len
        && i64::from(ids) * 4 + i64::from(text_len) + i64::from(names_len) == i64::from(data_len)
}

/// EditQueue's trailing data after the size check: the text, then
/// `ids_out.len()` big-endian ids, then `names` NUL-terminated names to the
/// data's end. Checks that the parts fit and that each name ends within the
/// names' part, then writes the ids to `ids_out` in host order; false (with
/// `ids_out` untouched) when they don't. Nothing is allocated, whatever the
/// counts.
pub fn edit_split(data: &[u8], text_len: usize, ids_out: &mut [u32], names: usize) -> bool {
    let Some(ids_end) = ids_out
        .len()
        .checked_mul(4)
        .and_then(|n| n.checked_add(text_len))
    else {
        return false;
    };
    let Some(id_bytes) = data.get(text_len..ids_end) else {
        return false;
    };
    let mut at = ids_end;
    for _ in 0..names {
        let Some(len) = data
            .get(at..)
            .and_then(|rest| crate::bytes::find_byte(rest, 0))
        else {
            return false;
        };
        at += len + 1;
    }
    for (out, id) in ids_out.iter_mut().zip(id_bytes.as_chunks::<4>().0) {
        *out = u32::from_be_bytes(*id);
    }
    true
}

/// Download's test of the request's content for a URL to fetch rather than
/// an NZB: "http:/" or "https:/" in any case, as strncasecmp compared them.
pub fn is_url(content: &[u8]) -> bool {
    let starts = |prefix: &[u8]| {
        content
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    starts(b"http:/") || starts(b"https:/")
}

/// The C-string URL predicate without scanning the complete NZB body.
/// At each byte, a mismatch (including NUL) stops the read before the next
/// byte. At most seven prefix bytes are read.
///
/// # Safety
/// content is null or points to a readable NUL-terminated string.
pub unsafe fn is_url_cstr(content: *const u8) -> bool {
    if content.is_null() {
        return false;
    }
    for (at, &expected) in b"http".iter().enumerate() {
        if !content.add(at).read().eq_ignore_ascii_case(&expected) {
            return false;
        }
    }
    let next = content.add(4).read();
    if next == b':' {
        content.add(5).read() == b'/'
    } else if next.eq_ignore_ascii_case(&b's') {
        content.add(5).read() == b':' && content.add(6).read() == b'/'
    } else {
        false
    }
}

/// One entry of a list response as the remote client reads it (the
/// reverse of `encode`): the entry at `at`, its strings pointing into
/// `data` (up to their NULs), and the offset after it (the header and every
/// string's length in its field). None when the entry, or one of its strings
/// up to its NUL, doesn't fit in `data`. With `LENS` the strings' `len` is
/// their length up to the NUL (the data is searched for it); without, it
/// is 0 and the NUL is looked for at the end of the string's field first,
/// where an encoder puts it: the strings are used as C strings.
pub fn read<E: Entry, const LENS: bool>(data: &[u8], at: usize) -> Option<(E, usize)> {
    let header_end = at.checked_add(4 * E::HEADER)?;
    let raw = data.get(at..header_end)?;
    let mut h = [0u32; MAX_HEADER];
    for (value, word) in h.iter_mut().zip(raw.as_chunks::<4>().0) {
        *value = u32::from_be_bytes(*word);
    }
    // Validate the complete declared entry once. Every subsequent string
    // start/end is a nonnegative prefix of this checked total, so it fits
    // without repeating the offset and remaining-length checks per field.
    let end = E::FIELDS
        .iter()
        .take(MAX_STRINGS)
        .try_fold(header_end, |end, &field| end.checked_add(h[field] as usize))?;
    if end > data.len() {
        return None;
    }
    let mut texts = [Text::EMPTY; MAX_STRINGS];
    let mut next = header_end;
    for (text, &field) in texts.iter_mut().zip(E::FIELDS) {
        // SAFETY: next is a prefix of the checked total end <= data.len().
        // Keep the whole remainder for NUL searches, including malformed
        // length fields whose terminator follows their declared extent.
        let rest = unsafe { data.get_unchecked(next..) };
        let field_len = h[field] as usize;
        text.data = rest.as_ptr();
        if LENS {
            text.len = crate::bytes::find_byte(rest, 0)?;
        } else if field_len == 0 || unsafe { *rest.get_unchecked(field_len - 1) } != 0 {
            // SAFETY above: with field_len > 0, next + field_len <= end.
            crate::bytes::find_byte(rest, 0)?;
        }
        next += field_len;
    }
    Some((E::from_wire(&h, &texts), end))
}

/// Up to `max` entries read from `*at` on into `out` (as `read` does
/// without lengths, the strings pointing into `data`), `*at` after them.
/// Fewer than `max` when an entry doesn't fit in `data`: the count read,
/// `*at` after them.
///
/// # Safety
/// `out` is writable for `max` entries (it may be uninitialized).
pub unsafe fn read_many<E: Entry>(data: &[u8], at: &mut usize, out: *mut E, max: usize) -> usize {
    for k in 0..max {
        let Some((entry, next)) = read::<E, false>(data, *at) else {
            return k;
        };
        out.add(k).write(entry);
        *at = next;
    }
    max
}

/// Decodes up to `nr_entries` `LogEntry` records from `data` (used by `Frontend::RequestMessages`),
/// invoking `emit(id, kind, time, text_ptr)` where `text_ptr` is a valid NUL-terminated pointer inside `data`.
pub fn decode_log_response_entries<F>(data: &[u8], nr_entries: u32, mut emit: F)
where
    F: FnMut(u32, u32, u32, *const u8),
{
    let mut at = 0usize;
    for _ in 0..nr_entries {
        let Some((entry, next)) = read::<LogEntry, false>(data, at) else {
            break;
        };
        emit(entry.id, entry.kind, entry.time, entry.texts[0].data);
        at = next;
    }
}

fn append_listing(out: &mut Vec<u8>, bytes: &[u8]) {
    crate::util::reserve_bytes(out, bytes.len());
    out.extend_from_slice(bytes);
}

fn push_listing(out: &mut Vec<u8>, byte: u8) {
    crate::util::reserve_bytes(out, 1);
    out.push(byte);
}

/// An int as printf's %i printed it.
fn push_int(out: &mut Vec<u8>, value: i32) {
    let mut digits = [0u8; 11];
    let mut at = digits.len();
    let mut rest = value.unsigned_abs();
    loop {
        at -= 1;
        digits[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if value < 0 {
        at -= 1;
        digits[at] = b'-';
    }
    append_listing(out, &digits[at..]);
}

const MAX_LISTING_INITIAL_CAPACITY: usize = 1024 * 1024;

fn listing_capacity(input_len: usize) -> usize {
    // Preserve the estimate for ordinary responses, but avoid an eager
    // reservation above the 32-bit Vec limit for a large received buffer.
    // Formatting grows the buffer only when output actually needs it.
    input_len
        .saturating_add(input_len / 2)
        .saturating_add(64)
        .min(MAX_LISTING_INITIAL_CAPACITY)
}

/// The lines of a listing: each of the first `count` entries `data` holds
/// from its start, through `line`, which adds the entry's text and its
/// newline or nothing. The listing ends at an entry that doesn't fit the
/// data, whatever `count` says.
fn listing<E: Entry>(data: &[u8], count: u32, line: impl Fn(&mut Vec<u8>, &E)) -> Vec<u8> {
    // a line is about as long as its entry; the buffer grows if it isn't
    let mut out = Vec::with_capacity(listing_capacity(data.len()));
    let mut at = 0;
    for _ in 0..count {
        let Some((entry, next)) = read::<E, true>(data, at) else {
            break;
        };
        line(&mut out, &entry);
        at = next;
    }
    // into_buf appends the FFI terminator after this return. Prevent that
    // final byte from triggering an unrepresentable amortized doubling.
    crate::util::reserve_bytes(&mut out, 1);
    out
}

/// Message::EKind's labels in the remote client's log listing.
const LOG_LABELS: [&[u8]; 5] = [
    b"[INFO] ",
    b"[WARNING] ",
    b"[ERROR] ",
    b"[DEBUG] ",
    b"[DETAIL] ",
];

/// The remote client's log listing: "[INFO] text" and so on for each
/// message; an unknown kind has no line.
pub fn log_listing(data: &[u8], count: u32) -> Vec<u8> {
    listing(data, count, |out, e: &LogEntry| {
        if let Some(label) = LOG_LABELS.get(e.kind as usize) {
            append_listing(out, label);
            // SAFETY: the text points into `data`, up to a NUL within it
            append_listing(out, unsafe { e.texts[0].bytes() });
            push_listing(out, b'\n');
        }
    })
}

/// PostInfo::EStage's names in the post-processing listing.
const STAGE_NAMES: [&[u8]; 15] = [
    b"",
    b", Loading Pars",
    b", Verifying source files",
    b", Repairing",
    b", Verifying repaired files",
    b", Par-Renaming",
    b", Rar-Renaming",
    b", Unpacking",
    b", Cleaning up",
    b", Moving",
    b", Post-unpack renaming",
    b", Executing postprocess-script",
    b"",
    b", Post-download renaming",
    b", Repairing from duplicates",
];
/// PostInfo::ptExecutingScript.
const EXECUTING_SCRIPT: u32 = 11;

/// The remote client's post-processing listing: "[id] name, stage, n%" (no
/// percentage while a script runs or before progress). An unknown stage has
/// no name.
pub fn post_queue_listing(data: &[u8], count: u32) -> Vec<u8> {
    listing(data, count, |out, e: &PostEntry| {
        push_listing(out, b'[');
        push_int(out, e.id as i32);
        append_listing(out, b"] ");
        // SAFETY: the name points into `data`, up to a NUL within it
        append_listing(out, unsafe { e.texts[1].bytes() });
        append_listing(
            out,
            STAGE_NAMES.get(e.stage as usize).copied().unwrap_or(b""),
        );
        let progress = e.stage_progress as i32;
        if progress > 0 && e.stage != EXECUTING_SCRIPT {
            append_listing(out, b", ");
            push_int(out, progress / 10);
            push_listing(out, b'%');
        }
        push_listing(out, b'\n');
    })
}

/// The remote client's history listing: "[id] name (...)" (HistoryInfo::EKind:
/// 1 an NZB, 2 a URL, 3 a hidden duplicate; no line for other kinds).
/// Unknown statuses have no text.
pub fn history_listing(data: &[u8], count: u32) -> Vec<u8> {
    const PAR: [&[u8]; 6] = [
        b"",
        b"",
        b", Par failed",
        b", Par successful",
        b", Repair possible",
        b", Repair needed",
    ];
    const SCRIPT: [&[u8]; 4] = [
        b"",
        b", Script status unknown",
        b", Script failed",
        b", Script successful",
    ];
    const URL: [&[u8]; 7] = [
        b"",
        b"",
        b"Url download successful",
        b"Url download failed",
        b"",
        b"Nzb scan skipped",
        b"Nzb scan failed",
    ];
    let text = |table: &[&'static [u8]], k: u32| table.get(k as usize).copied().unwrap_or(b"");
    listing(data, count, |out, e: &HistoryEntry| {
        if !matches!(e.kind, 1..=3) {
            return;
        }
        push_listing(out, b'[');
        push_int(out, e.id as i32);
        append_listing(out, b"] ");
        // SAFETY: the name points into `data`, up to a NUL within it
        append_listing(out, unsafe { e.texts[0].bytes() });
        if e.kind == 2 {
            append_listing(out, b" (URL, ");
            append_listing(out, text(&URL, e.url_status));
        } else {
            let hidden = e.kind == 3;
            append_listing(out, b" (");
            if hidden {
                append_listing(out, b"Hidden, ");
            } else {
                // BString<100>: never cut, an int's text is short
                push_int(out, e.file_count as i32);
                append_listing(out, b" files, ");
            }
            crate::util::format_size_into(out, ((e.size_hi as i64) << 32) + e.size_lo as i64);
            if !hidden {
                append_listing(out, text(&PAR, e.par_status));
                append_listing(out, text(&SCRIPT, e.script_status));
            }
        }
        append_listing(out, b")\n");
    })
}

/// FFI descriptor for a single file entry in `RemoteClient::RequestServerList(files=true)`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FileListingEntry {
    pub id: i32,
    pub nzb_name: *const u8,
    pub filename: *const u8,
    pub size: i64,
    pub remaining_size: i64,
    pub active_downloads: i32,
    pub paused: i32,
    pub matched: i32,
}

/// FFI descriptor for a group parameter (`name=value`) in `RemoteClient::RequestServerList(groups=true)`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GroupParamEntry {
    pub name: *const u8,
    pub value: *const u8,
}

/// FFI descriptor for a group (NZB/URL) entry in `RemoteClient::RequestServerList(groups=true)`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GroupListingEntry {
    pub id: i32,
    pub kind: i32,
    pub priority: i32,
    pub name: *const u8,
    pub category: *const u8,
    pub file_count: i32,
    pub remaining_size: i64,
    pub paused_size: i64,
    pub active_downloads: i32,
    pub matched: i32,
    pub params: *const GroupParamEntry,
    pub param_count: usize,
}

/// FFI descriptor for the server status summary tail of `RemoteClient::RequestServerList`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StatusSummarySpec {
    pub include_remaining_size: i32,
    pub remaining_size: i64,
    pub download_rate: u32,
    pub download_paused: u32,
    pub download2_paused: u32,
    pub download_stand_by: u32,
    pub downloaded_bytes: i64,
    pub download_time_sec: u32,
    pub download_limit: u32,
    pub up_time_sec: u32,
    pub thread_count: u32,
    pub post_job_count: u32,
    pub scan_paused: u32,
    pub post_paused: u32,
}

fn push_two_digits(out: &mut Vec<u8>, value: i32) {
    if (0..10).contains(&value) {
        push_listing(out, b'0');
    }
    push_int(out, value);
}

fn push_hms(out: &mut Vec<u8>, total_sec: i64) {
    let (h, m, s) = remote_client_calc_hms(total_sec);
    push_two_digits(out, h);
    push_listing(out, b':');
    push_two_digits(out, m);
    push_listing(out, b':');
    push_two_digits(out, s);
}

unsafe fn cstr_bytes<'a>(ptr: *const u8) -> &'a [u8] {
    if ptr.is_null() {
        b""
    } else {
        std::ffi::CStr::from_ptr(ptr.cast()).to_bytes()
    }
}

/// Format the `files` section of `RemoteClient::RequestServerList`.
pub fn file_listing(entries: &[FileListingEntry], has_pattern: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        entries
            .len()
            .saturating_mul(80)
            .saturating_add(160)
            .min(MAX_LISTING_INITIAL_CAPACITY),
    );
    append_listing(
        &mut out,
        b"Queue List\n-----------------------------------\n",
    );

    let mut remaining: i64 = 0;
    let mut paused: i64 = 0;
    let mut matches: i32 = 0;
    let nr_file_entries = entries.len() as i32;

    for e in entries {
        let completed_pct = remote_client_calc_file_completed_pct(e.remaining_size, e.size);
        if e.paused != 0 {
            paused = paused.wrapping_add(e.remaining_size);
        } else {
            remaining = remaining.wrapping_add(e.remaining_size);
        }

        if !has_pattern || e.matched != 0 {
            push_listing(&mut out, b'[');
            push_int(&mut out, e.id);
            append_listing(&mut out, b"] ");
            append_listing(&mut out, unsafe { cstr_bytes(e.nzb_name) });
            push_listing(&mut out, b'/');
            append_listing(&mut out, unsafe { cstr_bytes(e.filename) });
            append_listing(&mut out, b" (");
            crate::util::format_size_into(&mut out, e.size);
            if completed_pct >= 0 {
                append_listing(&mut out, b", ");
                push_int(&mut out, completed_pct);
                push_listing(&mut out, b'%');
            }
            if e.active_downloads > 0 {
                append_listing(&mut out, b", ");
                push_int(&mut out, e.active_downloads);
                append_listing(
                    &mut out,
                    if e.active_downloads > 1 {
                        b" threads"
                    } else {
                        b" thread"
                    },
                );
            }
            push_listing(&mut out, b')');
            if e.paused != 0 {
                append_listing(&mut out, b" (paused)");
            }
            push_listing(&mut out, b'\n');
            matches += 1;
        }
    }

    if matches == 0 {
        append_listing(&mut out, b"No matches founds\n");
    }

    append_listing(&mut out, b"-----------------------------------\nFiles: ");
    push_int(&mut out, nr_file_entries);
    push_listing(&mut out, b'\n');
    if has_pattern {
        append_listing(&mut out, b"Matches: ");
        push_int(&mut out, matches);
        push_listing(&mut out, b'\n');
    }

    append_listing(&mut out, b"Remaining size: ");
    crate::util::format_size_into(&mut out, remaining);
    if paused > 0 {
        append_listing(&mut out, b" (+");
        crate::util::format_size_into(&mut out, paused);
        append_listing(&mut out, b" paused)\n");
    } else {
        push_listing(&mut out, b'\n');
    }
    crate::util::reserve_bytes(&mut out, 1);
    out
}

/// Format the `groups` section of `RemoteClient::RequestServerList`.
pub fn group_listing(entries: &[GroupListingEntry], has_pattern: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        entries
            .len()
            .saturating_mul(96)
            .saturating_add(160)
            .min(MAX_LISTING_INITIAL_CAPACITY),
    );
    append_listing(
        &mut out,
        b"Queue List\n-----------------------------------\n",
    );

    let mut remaining: i64 = 0;
    let mut paused: i64 = 0;
    let mut matches: i32 = 0;
    let mut nr_file_entries: i32 = 0;

    for e in entries {
        nr_file_entries = nr_file_entries.wrapping_add(e.file_count);
        let mut has_th = 0;
        let mut th_pl = 0;
        let unpaused_rem = unsafe {
            roc_remote_client_calc_group_unpaused_remaining(
                e.remaining_size,
                e.paused_size,
                e.active_downloads,
                &mut has_th,
                &mut th_pl,
            )
        };
        let has_threads = has_th != 0;
        let threads_plural = th_pl != 0;
        remaining = remaining.wrapping_add(unpaused_rem);
        if e.paused_size > 0 {
            paused = paused.wrapping_add(e.paused_size);
        }

        if !has_pattern || e.matched != 0 {
            push_listing(&mut out, b'[');
            push_int(&mut out, e.id);
            append_listing(&mut out, b"] ");
            if e.priority != 0 {
                push_listing(&mut out, b'[');
                if e.priority > 0 {
                    push_listing(&mut out, b'+');
                }
                push_int(&mut out, e.priority);
                append_listing(&mut out, b"] ");
            }
            append_listing(&mut out, unsafe { cstr_bytes(e.name) });
            append_listing(&mut out, b" (");
            if e.kind == 1 {
                // NzbInfo::nkUrl == 1
                append_listing(&mut out, b"URL");
            } else {
                push_int(&mut out, e.file_count);
                append_listing(
                    &mut out,
                    if e.file_count > 1 {
                        b" files"
                    } else {
                        b" file"
                    },
                );
            }
            append_listing(&mut out, b", ");
            crate::util::format_size_into(&mut out, unpaused_rem);
            if e.paused_size > 0 {
                append_listing(&mut out, b" + ");
                crate::util::format_size_into(&mut out, e.paused_size);
                append_listing(&mut out, b" paused");
            }
            if has_threads {
                append_listing(&mut out, b", ");
                push_int(&mut out, e.active_downloads);
                append_listing(
                    &mut out,
                    if threads_plural {
                        b" threads"
                    } else {
                        b" thread"
                    },
                );
            }
            push_listing(&mut out, b')');
            let cat = unsafe { cstr_bytes(e.category) };
            if !cat.is_empty() {
                append_listing(&mut out, b" (");
                append_listing(&mut out, cat);
                push_listing(&mut out, b')');
            }
            if !e.params.is_null() && e.param_count > 0 {
                let params = unsafe { std::slice::from_raw_parts(e.params, e.param_count) };
                let mut param_buf = Vec::new();
                for (idx, p) in params.iter().enumerate() {
                    param_buf.extend_from_slice(if idx == 0 { b" (" } else { b", " });
                    param_buf.extend_from_slice(unsafe { cstr_bytes(p.name) });
                    param_buf.push(b'=');
                    param_buf.extend_from_slice(unsafe { cstr_bytes(p.value) });
                }
                // Match BString<1024> truncation semantics for parameters
                if param_buf.len() > 1023 {
                    param_buf.truncate(1023);
                }
                if param_buf.len() < 1023 {
                    param_buf.push(b')');
                }
                append_listing(&mut out, &param_buf);
            }
            push_listing(&mut out, b'\n');
            matches += 1;
        }
    }

    if matches == 0 {
        append_listing(&mut out, b"No matches founds\n");
    }

    append_listing(&mut out, b"-----------------------------------\nGroups: ");
    push_int(&mut out, entries.len() as i32);
    push_listing(&mut out, b'\n');
    if has_pattern {
        append_listing(&mut out, b"Matches: ");
        push_int(&mut out, matches);
        push_listing(&mut out, b'\n');
    }
    append_listing(&mut out, b"Files: ");
    push_int(&mut out, nr_file_entries);
    push_listing(&mut out, b'\n');

    append_listing(&mut out, b"Remaining size: ");
    crate::util::format_size_into(&mut out, remaining);
    if paused > 0 {
        append_listing(&mut out, b" (+");
        crate::util::format_size_into(&mut out, paused);
        append_listing(&mut out, b" paused)\n");
    } else {
        push_listing(&mut out, b'\n');
    }
    crate::util::reserve_bytes(&mut out, 1);
    out
}

/// Format the server status summary section at the end of `RemoteClient::RequestServerList`.
pub fn status_summary(spec: &StatusSummarySpec) -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    if spec.include_remaining_size != 0 {
        append_listing(&mut out, b"Remaining size: ");
        crate::util::format_size_into(&mut out, spec.remaining_size);
        push_listing(&mut out, b'\n');
    }

    if remote_client_should_print_remaining_time(
        spec.download_rate,
        spec.download_paused,
        spec.download2_paused,
        spec.download_stand_by,
    ) {
        let remain_sec = spec.remaining_size / (spec.download_rate as i64);
        append_listing(&mut out, b"Remaining time: ");
        push_hms(&mut out, remain_sec);
        push_listing(&mut out, b'\n');
    }

    append_listing(&mut out, b"Current download rate: ");
    append_listing(
        &mut out,
        &crate::util::format_speed(spec.download_rate as i64),
    );
    push_listing(&mut out, b'\n');

    let avg_speed =
        remote_client_calc_avg_speed(spec.downloaded_bytes, spec.download_time_sec as i64);
    append_listing(&mut out, b"Session download rate: ");
    append_listing(&mut out, &crate::util::format_speed(avg_speed as i64));
    push_listing(&mut out, b'\n');

    if spec.download_limit > 0 {
        append_listing(&mut out, b"Speed limit: ");
        append_listing(
            &mut out,
            &crate::util::format_speed(spec.download_limit as i64),
        );
        push_listing(&mut out, b'\n');
    }

    append_listing(&mut out, b"Up time: ");
    push_hms(&mut out, spec.up_time_sec as i64);
    push_listing(&mut out, b'\n');

    append_listing(&mut out, b"Download time: ");
    push_hms(&mut out, spec.download_time_sec as i64);
    push_listing(&mut out, b'\n');

    append_listing(&mut out, b"Downloaded: ");
    crate::util::format_size_into(&mut out, spec.downloaded_bytes);
    push_listing(&mut out, b'\n');

    append_listing(&mut out, b"Threads running: ");
    push_int(&mut out, spec.thread_count as i32);
    push_listing(&mut out, b'\n');

    if spec.post_job_count > 0 {
        append_listing(&mut out, b"Post-jobs: ");
        push_int(&mut out, spec.post_job_count as i32);
        push_listing(&mut out, b'\n');
    }

    if spec.scan_paused != 0 {
        append_listing(&mut out, b"Scan state: Paused\n");
    }

    let server_state = remote_client_format_server_state(
        spec.download_paused != 0,
        spec.download2_paused != 0,
        spec.download_stand_by != 0,
        spec.post_job_count,
        spec.post_paused != 0,
    );
    append_listing(&mut out, b"Server state: ");
    append_listing(&mut out, &server_state);
    push_listing(&mut out, b'\n');
    crate::util::reserve_bytes(&mut out, 1);
    out
}

/// sizeof(SNzbDownloadRequest): SNzbRequestBase (76) + 3*512-byte strings + 7 int32 fields.
pub const DOWNLOAD_REQUEST_SIZE: usize = BASE_SIZE + 512 + 512 + 20 + 512 + 8;

/// Populate an `SNzbRequestBase` header in network byte order (`RemoteClient::InitMessageBase`).
pub fn init_message_base(
    out: &mut [u8; BASE_SIZE],
    request_type: i32,
    struct_size: i32,
    username: &[u8],
    password: &[u8],
) {
    out.fill(0);
    out[0..4].copy_from_slice(&(SIGNATURE as u32).to_be_bytes());
    out[4..8].copy_from_slice(&(struct_size as u32).to_be_bytes());
    out[8..12].copy_from_slice(&(request_type as u32).to_be_bytes());
    let ulen = username.len().min(PASSWORD_SIZE - 1);
    out[USER_AT..USER_AT + ulen].copy_from_slice(&username[..ulen]);
    let plen = password.len().min(PASSWORD_SIZE - 1);
    out[PASSWORD_AT..PASSWORD_AT + plen].copy_from_slice(&password[..plen]);
}

/// Populate an `SNzbDownloadRequest` packet in network byte order (`RemoteClient::RequestServerDownload`).
#[allow(clippy::too_many_arguments)]
pub fn init_download_request(
    out: &mut [u8; DOWNLOAD_REQUEST_SIZE],
    username: &[u8],
    password: &[u8],
    nzb_filename: &[u8],
    nzb_content: &[u8],
    is_url: bool,
    category: &[u8],
    add_first: bool,
    add_paused: bool,
    priority: i32,
    dupe_score: i32,
    dupe_mode: i32,
    dupe_key: &[u8],
    trailing_len: i32,
    auto_category: bool,
) {
    out.fill(0);
    let base_slice: &mut [u8; BASE_SIZE] = (&mut out[..BASE_SIZE]).try_into().unwrap();
    init_message_base(
        base_slice,
        1,
        DOWNLOAD_REQUEST_SIZE as i32,
        username,
        password,
    );
    let fname_src = if !nzb_filename.is_empty() {
        nzb_filename
    } else if !is_url {
        nzb_content
    } else {
        b""
    };
    let flen = fname_src.len().min(511);
    out[76..76 + flen].copy_from_slice(&fname_src[..flen]);
    let clen = category.len().min(511);
    out[588..588 + clen].copy_from_slice(&category[..clen]);
    out[1100..1104].copy_from_slice(&(add_first as u32).to_be_bytes());
    out[1104..1108].copy_from_slice(&(add_paused as u32).to_be_bytes());
    out[1108..1112].copy_from_slice(&(priority as u32).to_be_bytes());
    out[1112..1116].copy_from_slice(&(dupe_score as u32).to_be_bytes());
    out[1116..1120].copy_from_slice(&(dupe_mode as u32).to_be_bytes());
    let klen = dupe_key.len().min(511);
    out[1120..1120 + klen].copy_from_slice(&dupe_key[..klen]);
    out[1632..1636].copy_from_slice(&(trailing_len as u32).to_be_bytes());
    out[1636..1640].copy_from_slice(&(auto_category as u32).to_be_bytes());
}

/// Encode the trailing payload of `RemoteClient::RequestServerEditQueue`:
/// 4-byte aligned `text`, followed by big-endian `ids`, followed by 4-byte aligned NUL-separated `names`.
/// Returns `(payload, aligned_text_len, aligned_names_len)`.
pub fn encode_edit_queue_trailing(
    text: Option<&[u8]>,
    ids: &[i32],
    names: &[&[u8]],
) -> (Vec<u8>, i32, i32) {
    let text_len = match text {
        Some(t) => (t.len() + 1 + 3) & !3,
        None => 0,
    };
    let id_length = ids.len() * 4;
    let names_len = if names.is_empty() {
        0
    } else {
        (names.iter().map(|n| n.len() + 1).sum::<usize>() + 3) & !3
    };
    let total = text_len + id_length + names_len;
    let mut buf = vec![0u8; total];
    if let Some(t) = text {
        buf[..t.len()].copy_from_slice(t);
    }
    for (i, &id) in ids.iter().enumerate() {
        let off = text_len + i * 4;
        buf[off..off + 4].copy_from_slice(&(id as u32).to_be_bytes());
    }
    let mut off = text_len + id_length;
    for n in names {
        buf[off..off + n.len()].copy_from_slice(n);
        off += n.len() + 1;
    }
    crate::util::reserve_bytes(&mut buf, 1);
    (buf, text_len as i32, names_len as i32)
}

struct WireNzb {
    id: i32,
    kind: i32,
    priority: i32,
    name: *const u8,
    category: *const u8,
    remaining_size: i64,
    paused_size: i64,
    active_downloads: i32,
    matched: i32,
    params: Vec<GroupParamEntry>,
    files: Vec<FileListingEntry>,
}

fn parse_wire_queue(
    data: &[u8],
    nr_nzb: u32,
    nr_ppp: u32,
    nr_files: u32,
    has_pattern: bool,
) -> Vec<WireNzb> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut at = 0usize;
    let mut nzbs = Vec::new();
    for _ in 0..nr_nzb {
        let Some((e, next)) = read::<NzbEntry, false>(data, at) else {
            return nzbs;
        };
        at = next;
        nzbs.push(WireNzb {
            id: e.id as i32,
            kind: e.kind as i32,
            priority: e.priority as i32,
            name: e.texts[1].data,
            category: e.texts[3].data,
            remaining_size: ((e.remaining_size_hi as i64) << 32)
                | (e.remaining_size_lo as u32 as i64),
            paused_size: ((e.paused_size_hi as i64) << 32) | (e.paused_size_lo as u32 as i64),
            active_downloads: 0,
            matched: if !has_pattern || e.matched != 0 { 1 } else { 0 },
            params: Vec::new(),
            files: Vec::new(),
        });
    }
    for _ in 0..nr_ppp {
        let Some((e, next)) = read::<ParamEntry, false>(data, at) else {
            return nzbs;
        };
        at = next;
        let Some(nzb) = (e.nzb_index as usize)
            .checked_sub(1)
            .and_then(|idx| nzbs.get_mut(idx))
        else {
            continue;
        };
        let name_ptr = e.texts[0].data;
        let value_ptr = e.texts[1].data;
        let name_bytes = unsafe { cstr_bytes(name_ptr) };
        let value_bytes = unsafe { cstr_bytes(value_ptr) };
        let empty_val = value_bytes.is_empty();
        let pos = nzb
            .params
            .iter()
            .position(|p| unsafe { cstr_bytes(p.name) }.eq_ignore_ascii_case(name_bytes));
        if empty_val {
            if let Some(idx) = pos {
                nzb.params.remove(idx);
            }
        } else if let Some(idx) = pos {
            nzb.params[idx].value = value_ptr;
        } else {
            nzb.params.push(GroupParamEntry {
                name: name_ptr,
                value: value_ptr,
            });
        }
    }
    for _ in 0..nr_files {
        let Some((e, next)) = read::<FileEntry, false>(data, at) else {
            return nzbs;
        };
        at = next;
        let Some(nzb) = (e.nzb_index as usize)
            .checked_sub(1)
            .and_then(|idx| nzbs.get_mut(idx))
        else {
            continue;
        };
        nzb.active_downloads = nzb.active_downloads.wrapping_add(e.active_downloads as i32);
        nzb.files.push(FileListingEntry {
            id: e.id as i32,
            nzb_name: nzb.name,
            filename: e.texts[1].data,
            size: ((e.size_hi as i64) << 32) | (e.size_lo as u32 as i64),
            remaining_size: ((e.remaining_size_hi as i64) << 32)
                | (e.remaining_size_lo as u32 as i64),
            active_downloads: e.active_downloads as i32,
            paused: if e.paused != 0 { 1 } else { 0 },
            matched: if !has_pattern || e.matched != 0 { 1 } else { 0 },
        });
    }
    nzbs
}

/// Decode `SNzbListResponse` trailing wire bytes and format the `files` section of `RemoteClient::RequestServerList`.
pub fn file_listing_from_wire(
    data: &[u8],
    nr_nzb: u32,
    nr_ppp: u32,
    nr_files: u32,
    has_pattern: bool,
) -> Vec<u8> {
    let nzbs = parse_wire_queue(data, nr_nzb, nr_ppp, nr_files, has_pattern);
    let files: Vec<FileListingEntry> = nzbs.into_iter().flat_map(|n| n.files).collect();
    file_listing(&files, has_pattern)
}

/// Decode `SNzbListResponse` trailing wire bytes and format the `groups` section of `RemoteClient::RequestServerList`.
pub fn group_listing_from_wire(
    data: &[u8],
    nr_nzb: u32,
    nr_ppp: u32,
    nr_files: u32,
    has_pattern: bool,
) -> Vec<u8> {
    let nzbs = parse_wire_queue(data, nr_nzb, nr_ppp, nr_files, has_pattern);
    let groups: Vec<GroupListingEntry> = nzbs
        .iter()
        .map(|n| GroupListingEntry {
            id: n.id,
            kind: n.kind,
            priority: n.priority,
            name: n.name,
            category: n.category,
            file_count: n.files.len() as i32,
            remaining_size: n.remaining_size,
            paused_size: n.paused_size,
            active_downloads: n.active_downloads,
            matched: n.matched,
            params: if n.params.is_empty() {
                std::ptr::null()
            } else {
                n.params.as_ptr()
            },
            param_count: n.params.len(),
        })
        .collect();
    group_listing(&groups, has_pattern)
}

/// Computes `(start, nr_entries)` for `BinRpcProcessor::ExecuteLog` (`daemon/remote/BinRpc.cpp`).
pub fn calc_log_slice(msg_count: i32, lines: i32, id_from: u32, first_msg_id: u32) -> (i32, i32) {
    let mut nr_entries = lines.max(0);
    let mut start = msg_count;
    if nr_entries > 0 {
        if nr_entries > msg_count {
            nr_entries = msg_count;
        }
        start = msg_count - nr_entries;
    }
    if id_from > 0 && msg_count > 0 {
        start = (id_from.wrapping_sub(first_msg_id) as i32).max(0);
        nr_entries = (msg_count - start).max(0);
    }
    (start, nr_entries)
}

/// Parses the HTTP request line after the 4-byte `signature` (`"POST"`, `"OPTI"`, `"GET "`)
/// in `RequestProcessor::ServWebRequest` (`daemon/remote/RemoteServer.cpp`), terminating the URL
/// at the first space and returning `(http_method, url_offset)`.
pub fn parse_web_request_line(signature: &[u8], line: &mut [u8]) -> Option<(i32, usize)> {
    let line_len = line.iter().position(|&b| b == 0).unwrap_or(line.len());
    let (method, url_offset) = if signature.starts_with(b"POST") {
        if line_len == 0 {
            return None;
        }
        (0, 1) // hmPost = 0
    } else if signature.starts_with(b"OPTI") && line_len > 4 {
        (2, 4) // hmOptions = 2
    } else if signature.starts_with(b"GET ") {
        (1, 0) // hmGet = 1
    } else {
        return None;
    };
    if let Some(space) = line[url_offset..line_len].iter().position(|&b| b == b' ') {
        line[url_offset + space] = 0;
    }
    Some((method, url_offset))
}

/// Returns the static MIME content type for `filename` (`WebProcessor::DetectContentType`).
pub fn detect_content_type(filename: &[u8]) -> Option<&'static CStr> {
    const MAP: &[(&[u8], &CStr)] = &[
        (b".css", c"text/css"),
        (b".html", c"text/html"),
        (b".js", c"application/javascript"),
        (b".png", c"image/png"),
        (b".jpeg", c"image/jpeg"),
        (b".gif", c"image/gif"),
        (b".svg", c"image/svg+xml"),
        (b".json", c"application/json"),
    ];
    for &(ext, mime) in MAP {
        if crate::util::ends_with(filename, ext, false) {
            return Some(mime);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The credential check with the control names as C strings.

    fn base(kind: u32, size: u32, user: &[u8], password: &[u8]) -> [u8; BASE_SIZE] {
        let mut b = [0u8; BASE_SIZE];
        b[..4].copy_from_slice(&(SIGNATURE as u32).to_be_bytes());
        b[4..8].copy_from_slice(&size.to_be_bytes());
        b[8..12].copy_from_slice(&kind.to_be_bytes());
        b[USER_AT..USER_AT + user.len()].copy_from_slice(user);
        b[PASSWORD_AT..PASSWORD_AT + password.len()].copy_from_slice(password);
        b
    }

    #[test]
    fn credentials() {
        assert!(credentials_ok(
            &base(1, 0, b"nzbget", b"tegbzn6789"),
            b"nzbget",
            b"tegbzn6789"
        ));
        assert!(credentials_ok(
            &base(1, 0, b"anyone", b"tegbzn6789"),
            b"",
            b"tegbzn6789"
        ));
        assert!(!credentials_ok(
            &base(1, 0, b"nzbget", b"wrong"),
            b"nzbget",
            b"tegbzn6789"
        ));
        assert!(!credentials_ok(
            &base(1, 0, b"other", b"tegbzn6789"),
            b"nzbget",
            b"tegbzn6789"
        ));
        // an unterminated user name runs on into the password, as strcmp read it
        let full = [b'u'; PASSWORD_SIZE];
        let b = base(1, 0, &full, b"pw");
        assert!(!credentials_ok(&b, &full, b"pw"));
        assert!(credentials_ok(&b, &[&full[..], b"pw"].concat(), b"pw"));
        // No read beyond the base when neither field is terminated. The old
        // strcmp could read out of bounds here; the bounded port stops here.
        let b = base(1, 0, &full, &full);
        assert!(credentials_ok(&b, &[b'u'; 64], &full));
        assert!(!credentials_ok(&b, b"", &[b'u'; 33]));
        assert!(!credentials_ok(&b, b"", &[b'u'; 31]));
    }

    #[test]
    fn sizes_and_names() {
        let sizes: Vec<u32> = (0..=LAST_TYPE).map(|t| 100 + t).collect();
        assert_eq!(size_error(&base(3, 103, b"", b""), &sizes), None);
        assert_eq!(size_error(&base(3, 99, b"", b""), &sizes), Some((103, 99)));
        assert_eq!(size_error(&base(0, 99, b"", b""), &sizes), None);
        assert_eq!(size_error(&base(15, 99, b"", b""), &sizes), None);
        assert_eq!(request_name(14), c"History");
        assert_eq!(request_name(15), c"Unknown");
        assert_eq!(request_name(u32::MAX), c"Unknown");
        for kind in [0x7fff_ffff, 0x8000_0000, u32::MAX] {
            assert_eq!(size_error(&base(kind, 99, b"", b""), &sizes), None);
            assert_eq!(request_name(kind), c"Unknown");
        }
        assert_eq!(
            size_error(&base(14, u32::MAX, b"", b""), &sizes),
            Some((114, u32::MAX))
        );
    }

    #[test]
    fn entries() {
        let mut buf = Vec::new();
        reference_entry(&mut buf, &[7, 0, 9], &[(b"ab", 1)], 1);
        assert_eq!(buf, [0, 0, 0, 7, 0, 0, 0, 4, 0, 0, 0, 9, b'a', b'b', 0, 0]);
        // 33 bytes: 3 bytes of padding, counted in the empty string's length
        reference_entry(&mut buf, &[1, 0, 0], &[(b"xyz", 1), (b"", 2)], 2);
        assert_eq!(
            &buf[16..],
            [0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 4, b'x', b'y', b'z', 0, 0, 0, 0, 0]
        );

        // a log entry, by hand
        let log = [LogEntry {
            id: 7,
            kind: 2,
            time: 99,
            texts: [Text::new(b"ab")],
        }];
        assert_eq!(unsafe { encoded_len(&log) }, 20);
        let out = encode_all(&log);
        assert_eq!(
            out,
            [
                0,
                0,
                0,
                7,
                0,
                0,
                0,
                2,
                0,
                0,
                0,
                99,
                0,
                0,
                0,
                3 + 1,
                b'a',
                b'b',
                0,
                0
            ]
        );
        // nothing, an empty text (a NULL pointer too)
        assert_eq!(unsafe { encoded_len::<LogEntry>(&[]) }, 0);
        assert_eq!(unsafe { encode::<LogEntry>(&[], &mut []) }, 0);
        assert!(encode_all::<LogEntry>(&[]).is_empty());
        let empty = [LogEntry {
            id: 1,
            kind: 0,
            time: 0,
            texts: [Text::EMPTY],
        }];
        assert_eq!(unsafe { encoded_len(&empty) }, 20);
        let out = encode_all(&empty);
        assert_eq!(
            out,
            [
                0,
                0,
                0,
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
                0,
                0,
                1 + 3,
                0,
                0,
                0,
                0
            ]
        );
        // The saturating sum of lengths no response can hold.
        let huge = [LogEntry {
            id: 1,
            kind: 0,
            time: 0,
            texts: [Text {
                data: std::ptr::null(),
                len: usize::MAX,
            }],
        }; 2];
        assert!(unsafe { encoded_len(&huge) } > MAX_RESPONSE);
    }

    /// A small deterministic generator.
    struct Random(u64);

    impl Random {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn int(&mut self) -> u32 {
            if self.below(3) == 0 {
                self.next() as u32
            } else {
                self.below(20) as u32
            }
        }
        /// A text of any length 0..=9 (every padding), sometimes long, never a NUL.
        fn text(&mut self) -> Vec<u8> {
            let n = if self.below(8) == 0 {
                self.below(300)
            } else {
                self.below(10)
            };
            (0..n).map(|_| 1 + self.below(255) as u8).collect()
        }
    }

    /// The entry kinds with the encoder's reference (the old one-entry-at-a-time
    /// layout written out from the wire's order of ints) and the entries as
    /// they come back from the wire.
    fn reference_entry(
        buf: &mut Vec<u8>,
        ints: &[u32],
        strings: &[(&[u8], usize)],
        pad_field: usize,
    ) {
        let start = buf.len();
        let mut fields = ints.to_vec();
        for &(text, field) in strings {
            fields[field] = (text.len() + 1) as u32;
        }
        buf.resize(start + fields.len() * 4, 0);
        for &(text, _) in strings {
            buf.extend_from_slice(text);
            buf.push(0);
        }
        let pad = (4 - buf.len() % 4) % 4;
        if pad > 0 {
            buf.resize(buf.len() + pad, 0);
            fields[pad_field] += pad as u32;
        }
        for (k, f) in fields.iter().enumerate() {
            buf[start + 4 * k..start + 4 * k + 4].copy_from_slice(&f.to_be_bytes());
        }
    }

    fn encode_all<E: Entry>(entries: &[E]) -> Vec<u8> {
        unsafe {
            let n = encoded_len(entries) as usize;
            // not zeroed: every byte must be written by encode
            let mut out: Vec<u8> = Vec::with_capacity(n);
            out.spare_capacity_mut().fill(MaybeUninit::new(0xAA));
            assert_eq!(encode(entries, &mut out.spare_capacity_mut()[..n]), n);
            out.set_len(n);
            out
        }
    }

    #[test]
    fn batch_encoders_match_the_entry_by_entry_encoder() {
        let mut r = Random(88172645463325252);
        for round in 0..400 {
            let n = r.below(6);
            // NZBs
            let (mut texts, mut expected) = (Vec::new(), Vec::new());
            let mut nzbs = Vec::new();
            for _ in 0..n {
                let ints: Vec<u32> = (0..12).map(|_| r.int()).collect();
                let strings: Vec<Vec<u8>> = (0..5).map(|_| r.text()).collect();
                let mut wire = ints.clone();
                wire.extend([0; 5]);
                let pairs: Vec<(&[u8], usize)> = strings
                    .iter()
                    .enumerate()
                    .map(|(k, s)| (&s[..], 12 + k))
                    .collect();
                reference_entry(&mut expected, &wire, &pairs, 16);
                nzbs.push(NzbEntry {
                    id: ints[0],
                    kind: ints[1],
                    size_lo: ints[2],
                    size_hi: ints[3],
                    remaining_size_lo: ints[4],
                    remaining_size_hi: ints[5],
                    paused_size_lo: ints[6],
                    paused_size_hi: ints[7],
                    paused_count: ints[8],
                    remaining_par_count: ints[9],
                    priority: ints[10],
                    matched: ints[11],
                    texts: std::array::from_fn(|k| Text::new(&strings[k])),
                });
                texts.push(strings);
            }
            assert_eq!(encode_all(&nzbs), expected, "nzbs, round {round}");
            // parameters
            let (mut texts, mut expected, mut params) = (Vec::new(), Vec::new(), Vec::new());
            for _ in 0..n {
                let (index, a, b) = (r.int(), r.text(), r.text());
                reference_entry(&mut expected, &[index, 0, 0], &[(&a, 1), (&b, 2)], 2);
                texts.push((a, b));
                params.push(index);
            }
            let params: Vec<ParamEntry> = params
                .iter()
                .zip(&texts)
                .map(|(&i, (a, b))| ParamEntry {
                    nzb_index: i,
                    texts: [Text::new(a), Text::new(b)],
                })
                .collect();
            assert_eq!(encode_all(&params), expected, "params, round {round}");
            // files
            let (mut texts, mut expected, mut ints_of) = (Vec::new(), Vec::new(), Vec::new());
            for _ in 0..n {
                let ints: Vec<u32> = (0..10).map(|_| r.int()).collect();
                let (a, b) = (r.text(), r.text());
                let mut wire = ints.clone();
                wire.extend([0; 2]);
                reference_entry(&mut expected, &wire, &[(&a, 10), (&b, 11)], 11);
                texts.push((a, b));
                ints_of.push(ints);
            }
            let files: Vec<FileEntry> = ints_of
                .iter()
                .zip(&texts)
                .map(|(i, (a, b))| FileEntry {
                    id: i[0],
                    nzb_index: i[1],
                    size_lo: i[2],
                    size_hi: i[3],
                    remaining_size_lo: i[4],
                    remaining_size_hi: i[5],
                    paused: i[6],
                    filename_confirmed: i[7],
                    active_downloads: i[8],
                    matched: i[9],
                    texts: [Text::new(a), Text::new(b)],
                })
                .collect();
            assert_eq!(encode_all(&files), expected, "files, round {round}");
            // log
            let (mut texts, mut expected, mut ints_of) = (Vec::new(), Vec::new(), Vec::new());
            for _ in 0..n {
                let ints: Vec<u32> = (0..3).map(|_| r.int()).collect();
                let a = r.text();
                reference_entry(
                    &mut expected,
                    &[ints[0], ints[1], ints[2], 0],
                    &[(&a, 3)],
                    3,
                );
                texts.push(a);
                ints_of.push(ints);
            }
            let log: Vec<LogEntry> = ints_of
                .iter()
                .zip(&texts)
                .map(|(i, a)| LogEntry {
                    id: i[0],
                    kind: i[1],
                    time: i[2],
                    texts: [Text::new(a)],
                })
                .collect();
            assert_eq!(encode_all(&log), expected, "log, round {round}");
            // post-queue
            let (mut texts, mut expected, mut ints_of) = (Vec::new(), Vec::new(), Vec::new());
            for _ in 0..n {
                let ints: Vec<u32> = (0..6).map(|_| r.int()).collect();
                let t: Vec<Vec<u8>> = (0..4).map(|_| r.text()).collect();
                let mut wire = ints.clone();
                wire.extend([0; 4]);
                let pairs: Vec<(&[u8], usize)> =
                    t.iter().enumerate().map(|(k, s)| (&s[..], 6 + k)).collect();
                reference_entry(&mut expected, &wire, &pairs, 9);
                texts.push(t);
                ints_of.push(ints);
            }
            let jobs: Vec<PostEntry> = ints_of
                .iter()
                .zip(&texts)
                .map(|(i, t)| PostEntry {
                    id: i[0],
                    stage: i[1],
                    stage_progress: i[2],
                    file_progress: i[3],
                    start_time: i[4],
                    stage_time: i[5],
                    texts: std::array::from_fn(|k| Text::new(&t[k])),
                })
                .collect();
            assert_eq!(encode_all(&jobs), expected, "post-queue, round {round}");
            // history: the name's length is the fourth int
            let (mut texts, mut expected, mut ints_of) = (Vec::new(), Vec::new(), Vec::new());
            for _ in 0..n {
                let ints: Vec<u32> = (0..9).map(|_| r.int()).collect();
                let a = r.text();
                reference_entry(
                    &mut expected,
                    &[
                        ints[0], ints[1], ints[2], 0, ints[3], ints[4], ints[5], ints[6], ints[7],
                        ints[8],
                    ],
                    &[(&a, 3)],
                    3,
                );
                texts.push(a);
                ints_of.push(ints);
            }
            let history: Vec<HistoryEntry> = ints_of
                .iter()
                .zip(&texts)
                .map(|(i, a)| HistoryEntry {
                    id: i[0],
                    kind: i[1],
                    time: i[2],
                    size_lo: i[3],
                    size_hi: i[4],
                    file_count: i[5],
                    par_status: i[6],
                    script_status: i[7],
                    url_status: i[8],
                    texts: [Text::new(a)],
                })
                .collect();
            assert_eq!(encode_all(&history), expected, "history, round {round}");
        }
    }

    #[test]
    fn every_padding_lands_in_the_last_length() {
        // the strings' lengths 0..=7 of a log entry: header 16, text, NUL, zeros to 4
        for len in 0..8usize {
            let text = vec![b'x'; len];
            let out = encode_all(&[LogEntry {
                id: 1,
                kind: 2,
                time: 3,
                texts: [Text::new(&text)],
            }]);
            let padded = (len + 1 + 3) & !3;
            assert_eq!(out.len(), 16 + padded);
            assert_eq!(&out[12..16], &(padded as u32).to_be_bytes());
            assert_eq!(&out[16..16 + len], &text[..]);
            assert!(out[16 + len..].iter().all(|&b| b == 0));
        }
        // padding of a history entry sits in the name's length, in the middle of the header
        let out = encode_all(&[HistoryEntry {
            id: 1,
            kind: 2,
            time: 3,
            size_lo: 4,
            size_hi: 5,
            file_count: 6,
            par_status: 7,
            script_status: 8,
            url_status: 9,
            texts: [Text::new(b"abcd")],
        }]);
        assert_eq!(
            &out[..40],
            [
                0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 8, 0, 0, 0, 4, 0, 0, 0, 5, 0, 0, 0, 6,
                0, 0, 0, 7, 0, 0, 0, 8, 0, 0, 0, 9
            ]
        );
        assert_eq!(&out[40..], b"abcd\0\0\0\0");
    }

    #[test]
    fn edits() {
        assert!(edit_sizes_ok(2, 1, 3, 4, 15));
        assert!(!edit_sizes_ok(2, 1, 3, 4, 16));
        assert!(!edit_sizes_ok(-1, 0, 0, 4, 0));
        assert!(!edit_sizes_ok(0, 2, 1, 0, 1));
        assert!(!edit_sizes_ok(10_000_001, 0, 0, 0, 40_000_004));
        assert!(!edit_sizes_ok(i32::MAX, 0, 0, i32::MAX, u32::MAX));
        assert!(edit_sizes_ok(0, 0, 0, 0, 0));
        let data = b"tx\0\0\0\0\0\x07\xff\xff\xff\xfeab\0\0c\0";
        let mut ids = [0u32; 2];
        assert!(edit_split(data, 4, &mut ids, 3));
        assert_eq!(ids, [7, 0xffff_fffe]);
        // a name running to the end, or past it: the ids stay untouched
        let mut ids = [1u32; 2];
        assert!(!edit_split(&data[..17], 4, &mut ids, 3));
        assert!(!edit_split(data, 4, &mut ids, 4));
        assert!(!edit_split(&data[..8], 4, &mut ids, 0));
        assert_eq!(ids, [1, 1]);
        assert!(edit_split(b"", 0, &mut [], 0));
        assert!(!edit_split(b"", 1, &mut [], 0));
    }

    #[test]
    fn urls() {
        for yes in [
            &b"http://x"[..],
            b"HTTPS://x",
            b"http:/",
            b"hTtPs:/",
            b"http:/\0",
        ] {
            assert!(is_url(yes), "{yes:?}");
        }
        for no in [
            &b""[..],
            b"http:",
            b"https:",
            b"ftp://x",
            b"<?xml",
            b"http\0:/",
            b"https:\\",
        ] {
            assert!(!is_url(no), "{no:?}");
        }
    }

    /// The entries as read, strings as bytes.
    fn log_texts(data: &[u8], entries: &[LogEntry]) -> Vec<Vec<u8>> {
        let _ = data;
        entries
            .iter()
            .map(|e| unsafe { e.texts[0].bytes() }.to_vec())
            .collect()
    }

    #[test]
    fn client_entries() {
        // the encoder's output reads back
        let nzb = |id: u32, texts: &[&[u8]; 5]| NzbEntry {
            id,
            kind: 1,
            size_lo: 2,
            size_hi: 3,
            remaining_size_lo: 4,
            remaining_size_hi: 5,
            paused_size_lo: 6,
            paused_size_hi: 7,
            paused_count: 8,
            remaining_par_count: 9,
            priority: 10,
            matched: 11,
            texts: std::array::from_fn(|k| Text::new(texts[k])),
        };
        let buf = encode_all(&[
            nzb(7, &[b"ab", b"", b"xyz", b"1234", b"x"]),
            nzb(8, &[b"", b"", b"", b"", b""]),
        ]);
        let (first, next) = read::<NzbEntry, true>(&buf, 0).unwrap();
        assert_eq!(
            (
                first.id,
                first.kind,
                first.size_lo,
                first.size_hi,
                first.remaining_size_lo,
                first.remaining_size_hi,
                first.paused_size_lo,
                first.paused_size_hi,
                first.paused_count,
                first.remaining_par_count,
                first.priority,
                first.matched
            ),
            (7, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11)
        );
        let strings: Vec<&[u8]> = first.texts.iter().map(|t| unsafe { t.bytes() }).collect();
        assert_eq!(strings, [&b"ab"[..], b"", b"xyz", b"1234", b"x"]);
        // the first text starts right after the 68-byte header
        assert_eq!(first.texts[0].data, buf[68..].as_ptr());
        let (second, end) = read::<NzbEntry, true>(&buf, next).unwrap();
        assert_eq!((second.id, end), (8, buf.len()));
        assert!(second.texts.iter().all(|t| t.len == 0));

        // read_many: stops at the first entry that doesn't fit, with the ones before it read
        let mut out = [std::mem::MaybeUninit::<NzbEntry>::uninit(); 3];
        let mut at = 0;
        assert_eq!(
            unsafe { read_many(&buf, &mut at, out.as_mut_ptr().cast::<NzbEntry>(), 3) },
            2
        );
        assert_eq!(at, buf.len());
        assert_eq!(unsafe { out[1].assume_init() }.id, 8);
        at = 0;
        assert_eq!(
            unsafe {
                read_many(
                    &buf[..buf.len() - 4],
                    &mut at,
                    out.as_mut_ptr().cast::<NzbEntry>(),
                    3,
                )
            },
            1
        );
        assert_eq!(at, next);
        at = 0;
        assert_eq!(
            unsafe { read_many(&buf, &mut at, out.as_mut_ptr().cast::<NzbEntry>(), 1) },
            1
        );
        assert_eq!(at, next);
        assert_eq!(
            unsafe { read_many(&buf, &mut at, out.as_mut_ptr().cast::<NzbEntry>(), 0) },
            0
        );
        assert_eq!(at, next);
    }

    /// The previous reader, an entry at a time: the oracle of `read` for
    /// data that is cut or made up.
    fn reference_read(
        data: &[u8],
        at: usize,
        nints: usize,
        fields: &[usize],
    ) -> Option<(Vec<u32>, Vec<usize>, usize)> {
        let header = at.checked_add(nints.checked_mul(4)?)?;
        let raw = data.get(at..header)?;
        let value =
            |k: usize| u32::from_be_bytes(raw[4 * k..4 * k + 4].try_into().unwrap()) as usize;
        let mut next = header;
        for &field in fields {
            let start = next;
            data.get(start..)?.iter().position(|&b| b == 0)?;
            next = next.checked_add(value(field))?;
        }
        if next > data.len() {
            return None;
        }
        let mut start = header;
        let mut starts = Vec::new();
        for &field in fields {
            starts.push(start);
            start += value(field);
        }
        Some(((0..nints).map(|k| value(k) as u32).collect(), starts, next))
    }

    #[test]
    fn reading_matches_the_entry_by_entry_reader_on_cut_and_made_up_data() {
        let mut r = Random(1234567);
        for round in 0..3000 {
            // a real entry, cut anywhere, or random bytes with a small-length header
            let text = r.text();
            let history = [HistoryEntry {
                id: r.int(),
                kind: 1,
                time: 5,
                size_lo: 6,
                size_hi: 7,
                file_count: 8,
                par_status: 9,
                script_status: 10,
                url_status: 11,
                texts: [Text::new(&text)],
            }];
            let mut data = encode_all(&history);
            match r.below(3) {
                0 => data.truncate(r.below(data.len() + 1)),
                1 => {
                    let k = r.below(data.len());
                    data[k] = r.next() as u8;
                }
                _ => {
                    data = (0..r.below(60))
                        .map(|_| if r.below(3) == 0 { r.next() as u8 } else { 0 })
                        .collect();
                }
            }
            let at = if r.below(4) == 0 {
                r.below(data.len() + 3)
            } else {
                0
            };
            let ours = read::<HistoryEntry, true>(&data, at);
            // without lengths: the same entries, with the NUL looked for at the end of the field first
            match (&ours, read::<HistoryEntry, false>(&data, at)) {
                (None, None) => {}
                (Some((a, next)), Some((b, next2))) => {
                    assert_eq!(*next, next2, "round {round}");
                    assert_eq!(
                        (
                            a.id,
                            a.kind,
                            a.time,
                            a.size_lo,
                            a.size_hi,
                            a.file_count,
                            a.par_status,
                            a.script_status,
                            a.url_status
                        ),
                        (
                            b.id,
                            b.kind,
                            b.time,
                            b.size_lo,
                            b.size_hi,
                            b.file_count,
                            b.par_status,
                            b.script_status,
                            b.url_status
                        )
                    );
                    assert_eq!((a.texts[0].data, b.texts[0].len), (b.texts[0].data, 0));
                }
                _ => panic!("round {round}: the flavors differ on {data:?} at {at}"),
            }
            let theirs = reference_read(&data, at, 10, &[3]);
            match (ours, theirs) {
                (None, None) => {}
                (Some((e, next)), Some((ints, starts, next2))) => {
                    assert_eq!(next, next2, "round {round}");
                    assert_eq!(
                        [
                            e.id,
                            e.kind,
                            e.time,
                            e.size_lo,
                            e.size_hi,
                            e.file_count,
                            e.par_status,
                            e.script_status,
                            e.url_status
                        ],
                        [
                            ints[0], ints[1], ints[2], ints[4], ints[5], ints[6], ints[7], ints[8],
                            ints[9]
                        ]
                    );
                    assert_eq!(e.texts[0].data, data[starts[0]..].as_ptr(), "round {round}");
                    // the string is the C string at the start
                    let c_len = data[starts[0]..].iter().position(|&b| b == 0).unwrap();
                    assert_eq!(e.texts[0].len, c_len);
                }
                (a, b) => panic!(
                    "round {round}: {:?} vs {:?}, data {data:?}",
                    a.map(|x| x.1),
                    b.map(|x| x.2)
                ),
            }
        }
        // overflowing offsets
        assert!(read::<LogEntry, true>(&[], usize::MAX - 2).is_none());
        assert!(read::<LogEntry, true>(&[0; 16], usize::MAX).is_none());
        assert!(read::<LogEntry, true>(b"", 0).is_none());
        // The NUL at the end of the string's field is taken for the string's end; where it isn't
        // (an empty field, a field ending at a letter, past the data) the data is searched for one.
        let mut e = encode_all(&[LogEntry {
            id: 1,
            kind: 2,
            time: 3,
            texts: [Text::new(b"abc")],
        }]);
        assert!(read::<LogEntry, false>(&e, 0).is_some());
        e[12..16].copy_from_slice(&2u32.to_be_bytes());
        // field 2: ends at "b", the NUL is after: read, to the end of the field
        assert_eq!(read::<LogEntry, false>(&e, 0).unwrap().1, 18);
        e[12..16].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(read::<LogEntry, false>(&e, 0).unwrap().1, 16);
        // no NUL anywhere after the start
        let mut e = encode_all(&[LogEntry {
            id: 1,
            kind: 2,
            time: 3,
            texts: [Text::new(b"abc")],
        }]);
        e[19] = b'x';
        for len in [0u32, 1, 3, 4] {
            e[12..16].copy_from_slice(&len.to_be_bytes());
            assert!(read::<LogEntry, false>(&e, 0).is_none(), "field {len}");
            assert!(read::<LogEntry, true>(&e, 0).is_none(), "field {len}");
        }
        // a field past the data
        e[19] = 0;
        e[12..16].copy_from_slice(&5u32.to_be_bytes());
        assert!(read::<LogEntry, false>(&e, 0).is_none());
        // a length past the end, a string without its NUL, a length of 0 with a NUL later
        let mut e = encode_all(&[LogEntry {
            id: 1,
            kind: 2,
            time: 3,
            texts: [Text::new(b"abc")],
        }]);
        assert!(read::<LogEntry, true>(&e, 0).is_some());
        e[12..16].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(read::<LogEntry, true>(&e, 0).is_none());
        e[12..16].copy_from_slice(&4u32.to_be_bytes());
        assert!(read::<LogEntry, true>(&e[..19], 0).is_none());
        e[12..16].copy_from_slice(&0u32.to_be_bytes());
        let (short, next) = read::<LogEntry, true>(&e, 0).unwrap();
        assert_eq!((next, short.texts[0].len), (16, 3));
        assert_eq!(log_texts(&e, &[short]), [b"abc"]);
    }

    /// The previous line writers: the oracles of the listings.
    fn reference_log_line(kind: u32, text: &[u8]) -> Option<Vec<u8>> {
        let label: &[u8] = match kind {
            0 => b"[INFO] ",
            1 => b"[WARNING] ",
            2 => b"[ERROR] ",
            3 => b"[DEBUG] ",
            4 => b"[DETAIL] ",
            _ => return None,
        };
        Some([label, text, b"\n"].concat())
    }

    fn reference_post_line(id: u32, stage: u32, progress: u32, info_name: &[u8]) -> Vec<u8> {
        let names = [
            "",
            ", Loading Pars",
            ", Verifying source files",
            ", Repairing",
            ", Verifying repaired files",
            ", Par-Renaming",
            ", Rar-Renaming",
            ", Unpacking",
            ", Cleaning up",
            ", Moving",
            ", Post-unpack renaming",
            ", Executing postprocess-script",
            "",
            ", Post-download renaming",
            ", Repairing from duplicates",
        ];
        let mut line = format!("[{}] ", id as i32).into_bytes();
        line.extend_from_slice(info_name);
        line.extend_from_slice(names.get(stage as usize).copied().unwrap_or("").as_bytes());
        if (progress as i32) > 0 && stage != 11 {
            line.extend_from_slice(format!(", {}%", progress as i32 / 10).as_bytes());
        }
        line.push(b'\n');
        line
    }

    fn reference_history_line(e: &HistoryEntry, name: &[u8]) -> Option<Vec<u8>> {
        let par = [
            "",
            "",
            ", Par failed",
            ", Par successful",
            ", Repair possible",
            ", Repair needed",
        ];
        let script = [
            "",
            ", Script status unknown",
            ", Script failed",
            ", Script successful",
        ];
        let url = [
            "",
            "",
            "Url download successful",
            "Url download failed",
            "",
            "Nzb scan skipped",
            "Nzb scan failed",
        ];
        let mut line = format!("[{}] ", e.id as i32).into_bytes();
        line.extend_from_slice(name);
        let size = ((e.size_hi as i64) << 32) + e.size_lo as i64;
        let text = |t: &[&str], k: u32| t.get(k as usize).copied().unwrap_or("").to_string();
        let tail = match e.kind {
            1 => format!(
                " ({} files, {}{}{})",
                e.file_count as i32,
                String::from_utf8(crate::util::format_size(size)).unwrap(),
                text(&par, e.par_status),
                text(&script, e.script_status)
            ),
            3 => format!(
                " (Hidden, {})",
                String::from_utf8(crate::util::format_size(size)).unwrap()
            ),
            2 => format!(" (URL, {})", text(&url, e.url_status)),
            _ => return None,
        };
        line.extend_from_slice(tail.as_bytes());
        line.push(b'\n');
        Some(line)
    }

    #[test]
    fn listings_match_the_line_by_line_writers() {
        let mut r = Random(42);
        for round in 0..600 {
            let n = r.below(6);
            let texts: Vec<Vec<u8>> = (0..n).map(|_| r.text()).collect();
            // log
            let log: Vec<LogEntry> = texts
                .iter()
                .map(|t| LogEntry {
                    id: r.int(),
                    kind: r.below(7) as u32,
                    time: r.int(),
                    texts: [Text::new(t)],
                })
                .collect();
            let mut expected = Vec::new();
            for e in &log {
                expected.extend(
                    reference_log_line(e.kind, unsafe { e.texts[0].bytes() }).unwrap_or_default(),
                );
            }
            let data = encode_all(&log);
            assert_eq!(log_listing(&data, n as u32), expected, "log, round {round}");
            // more entries than the data holds: the listing ends with the data
            assert_eq!(log_listing(&data, n as u32 + 3), expected);
            // fewer: just those
            if n > 1 {
                let first = reference_log_line(log[0].kind, unsafe { log[0].texts[0].bytes() })
                    .unwrap_or_default();
                assert_eq!(log_listing(&data, 1), first);
            }
            // post-queue
            let jobs: Vec<PostEntry> = texts
                .iter()
                .map(|t| PostEntry {
                    id: r.int(),
                    stage: r.below(18) as u32,
                    stage_progress: if r.below(3) == 0 {
                        r.next() as u32
                    } else {
                        r.below(1200) as u32
                    },
                    file_progress: 0,
                    start_time: 0,
                    stage_time: 0,
                    texts: [
                        Text::new(b"f"),
                        Text::new(t),
                        Text::EMPTY,
                        Text::new(b"label"),
                    ],
                })
                .collect();
            let mut expected = Vec::new();
            for (e, t) in jobs.iter().zip(&texts) {
                expected.extend(reference_post_line(e.id, e.stage, e.stage_progress, t));
            }
            assert_eq!(
                post_queue_listing(&encode_all(&jobs), n as u32),
                expected,
                "post-queue, round {round}"
            );
            // history
            let history: Vec<HistoryEntry> = texts
                .iter()
                .map(|t| HistoryEntry {
                    id: r.int(),
                    kind: r.below(5) as u32,
                    time: r.int(),
                    size_lo: r.int(),
                    size_hi: if r.below(2) == 0 {
                        0
                    } else {
                        r.below(4) as u32
                    },
                    file_count: r.int(),
                    par_status: r.below(8) as u32,
                    script_status: r.below(6) as u32,
                    url_status: r.below(9) as u32,
                    texts: [Text::new(t)],
                })
                .collect();
            let mut expected = Vec::new();
            for (e, t) in history.iter().zip(&texts) {
                expected.extend(reference_history_line(e, t).unwrap_or_default());
            }
            assert_eq!(
                history_listing(&encode_all(&history), n as u32),
                expected,
                "history, round {round}"
            );
        }
        // fixed examples
        let post = |stage, progress| {
            encode_all(&[PostEntry {
                id: 3,
                stage,
                stage_progress: progress,
                file_progress: 0,
                start_time: 0,
                stage_time: 0,
                texts: [Text::EMPTY, Text::new(b"Show"), Text::EMPTY, Text::EMPTY],
            }])
        };
        assert_eq!(
            post_queue_listing(&post(7, 455), 1),
            b"[3] Show, Unpacking, 45%\n"
        );
        assert_eq!(
            post_queue_listing(&post(11, 455), 1),
            b"[3] Show, Executing postprocess-script\n"
        );
        assert_eq!(post_queue_listing(&post(12, 0), 1), b"[3] Show\n");
        assert_eq!(post_queue_listing(&post(99, 9), 1), b"[3] Show, 0%\n");
        let history = |kind, url_status| {
            encode_all(&[HistoryEntry {
                id: 4,
                kind,
                time: 0,
                size_lo: 2048,
                size_hi: 0,
                file_count: 12,
                par_status: 3,
                script_status: 3,
                url_status,
                texts: [Text::new(b"N")],
            }])
        };
        assert_eq!(
            history_listing(&history(1, 0), 1),
            b"[4] N (12 files, 2.00 KB, Par successful, Script successful)\n"
        );
        assert_eq!(
            history_listing(&history(3, 0), 1),
            b"[4] N (Hidden, 2.00 KB)\n"
        );
        assert_eq!(
            history_listing(&history(2, 6), 1),
            b"[4] N (URL, Nzb scan failed)\n"
        );
        assert_eq!(history_listing(&history(2, 70), 1), b"[4] N (URL, )\n");
        assert_eq!(history_listing(&history(0, 0), 1), b"");
        // an entry that doesn't fit ends the listing, with the lines before it kept
        let mut data = encode_all(&[
            LogEntry {
                id: 1,
                kind: 0,
                time: 0,
                texts: [Text::new(b"ok")],
            },
            LogEntry {
                id: 2,
                kind: 0,
                time: 0,
                texts: [Text::new(b"cut")],
            },
        ]);
        data.truncate(data.len() - 2);
        assert_eq!(log_listing(&data, 2), b"[INFO] ok\n");
        assert_eq!(log_listing(&data, 0), b"");
        assert_eq!(log_listing(b"", 5), b"");
        // an unknown kind has no line but doesn't end the listing
        let data = encode_all(&[
            LogEntry {
                id: 1,
                kind: 5,
                time: 0,
                texts: [Text::new(b"x")],
            },
            LogEntry {
                id: 2,
                kind: 4,
                time: 0,
                texts: [Text::new(b"")],
            },
        ]);
        assert_eq!(log_listing(&data, 2), b"[DETAIL] \n");
    }

    #[test]
    fn listing_growth_covers_32_bit_newline_and_final_nul() {
        let limit = i32::MAX as usize;
        let text_end = (1usize << 30) + 7; // INFO prefix plus a 1 GiB text
        assert!(text_end + 2 <= limit);
        assert!(text_end * 2 > limit);
        assert!(crate::util::byte_vec_needs_exact_growth(
            text_end, text_end, 1, limit
        ));
        let newline_end = text_end + 1;
        assert!(crate::util::byte_vec_needs_exact_growth(
            newline_end,
            newline_end,
            1,
            limit
        ));
        assert!(!crate::util::byte_vec_needs_exact_growth(
            text_end,
            text_end - 8,
            1,
            limit
        ));
        assert!(!crate::util::byte_vec_needs_exact_growth(
            1024, 1024, 1, limit
        ));
        assert!(!crate::util::byte_vec_needs_exact_growth(
            limit / 2,
            limit / 2,
            1,
            limit
        ));
    }

    #[test]
    fn listing_reservation_handles_32_bit_large_lengths() {
        // A valid input whose old eager reserve exceeded a 32-bit Vec limit,
        // without allocating a multi-gigabyte fixture.
        let input_len = 1_500_000_000usize;
        assert!(input_len <= i32::MAX as usize);
        assert!(input_len as u64 + input_len as u64 / 2 + 64 > i32::MAX as u64);
        assert!(listing_capacity(input_len) <= MAX_LISTING_INITIAL_CAPACITY);
        assert!(listing_capacity(usize::MAX) <= MAX_LISTING_INITIAL_CAPACITY);
        for len in [0usize, 1, 100, 4096, 100_000] {
            assert_eq!(listing_capacity(len), len + len / 2 + 64);
        }
    }

    #[test]
    fn listing_reservation_follows_output_growth_for_unused_input() {
        let payload = vec![b'x'; 2 * MAX_LISTING_INITIAL_CAPACITY];
        let data = encode_all(&[LogEntry {
            id: 1,
            kind: u32::MAX,
            time: 0,
            texts: [Text::new(&payload)],
        }]);
        let listing = log_listing(&data, 1);
        assert!(listing.is_empty());
        assert!(listing.capacity() <= MAX_LISTING_INITIAL_CAPACITY);
        // Formatting and growth still work when an actual line exceeds the
        // bounded initial reserve.
        let data = encode_all(&[LogEntry {
            id: 1,
            kind: 0,
            time: 0,
            texts: [Text::new(&payload)],
        }]);
        let listing = log_listing(&data, 1);
        assert!(listing.starts_with(b"[INFO] "));
        assert_eq!(&listing[7..listing.len() - 1], payload);
        assert_eq!(listing.last(), Some(&b'\n'));
    }

    #[test]
    fn ints_print_as_printf_did() {
        for v in [0, 1, -1, 9, 10, -10, 12345, i32::MAX, i32::MIN, -2147483647] {
            let mut out = Vec::new();
            push_int(&mut out, v);
            assert_eq!(out, v.to_string().as_bytes());
        }
    }

    #[test]
    fn responses() {
        let r = bool_response(true, b"ok");
        assert_eq!(
            r,
            [0x6e, 0x7a, 0x62, 0x31, 0, 0, 0, 16, 0, 0, 0, 1, 0, 0, 0, 3, b'o', b'k', 0]
        );
        assert!(
            trailing_ok(0, 10)
                && trailing_ok(10, 10)
                && !trailing_ok(-1, 10)
                && !trailing_ok(11, 10)
        );
        assert!(trailing_ok(0, 0));
        assert!(!trailing_ok(0, -1));
        assert!(!trailing_ok(i64::MIN, i64::MAX));
        assert!(trailing_ok(i64::MAX, i64::MAX));
        assert!(!trailing_ok(i64::MAX, i64::MAX - 1));
    }

    #[test]
    fn test_file_and_group_and_status_listings() {
        let entries = [
            FileListingEntry {
                id: 1,
                nzb_name: c"MyNzb".as_ptr().cast(),
                filename: c"part1.rar".as_ptr().cast(),
                size: 100 * 1024 * 1024,
                remaining_size: 50 * 1024 * 1024,
                active_downloads: 2,
                paused: 0,
                matched: 1,
            },
            FileListingEntry {
                id: 2,
                nzb_name: c"MyNzb".as_ptr().cast(),
                filename: c"part2.rar".as_ptr().cast(),
                size: 100 * 1024 * 1024,
                remaining_size: 100 * 1024 * 1024,
                active_downloads: 0,
                paused: 1,
                matched: 0,
            },
        ];
        let fl = String::from_utf8(file_listing(&entries, true)).unwrap();
        assert!(fl.contains("[1] MyNzb/part1.rar (100.00 MB, 50%, 2 threads)\n"));
        assert!(!fl.contains("part2.rar"));
        assert!(fl.contains("Files: 2\nMatches: 1\nRemaining size: 50.00 MB (+100.00 MB paused)\n"));

        let params = [GroupParamEntry {
            name: c"*Unpack:".as_ptr().cast(),
            value: c"yes".as_ptr().cast(),
        }];
        let groups = [GroupListingEntry {
            id: 10,
            kind: 0,
            priority: 50,
            name: c"MyNzb".as_ptr().cast(),
            category: c"movies".as_ptr().cast(),
            file_count: 2,
            remaining_size: 150 * 1024 * 1024,
            paused_size: 100 * 1024 * 1024,
            active_downloads: 1,
            matched: 1,
            params: params.as_ptr(),
            param_count: params.len(),
        }];
        let gl = String::from_utf8(group_listing(&groups, false)).unwrap();
        assert!(gl.contains("[10] [+50] MyNzb (2 files, 50.00 MB + 100.00 MB paused, 1 thread) (movies) (*Unpack:=yes)\n"));
        assert!(gl.contains("Groups: 1\nFiles: 2\nRemaining size: 50.00 MB (+100.00 MB paused)\n"));

        let status = StatusSummarySpec {
            include_remaining_size: 1,
            remaining_size: 1024 * 1024,
            download_rate: 512 * 1024,
            download_paused: 0,
            download2_paused: 0,
            download_stand_by: 0,
            downloaded_bytes: 10 * 1024 * 1024,
            download_time_sec: 10,
            download_limit: 1024 * 1024,
            up_time_sec: 3661,
            thread_count: 4,
            post_job_count: 2,
            scan_paused: 1,
            post_paused: 0,
        };
        let ss = String::from_utf8(status_summary(&status)).unwrap();
        assert!(ss.contains("Remaining size: 1.00 MB\n"));
        assert!(ss.contains("Remaining time: 00:00:02\n"));
        assert!(ss.contains("Up time: 01:01:01\n"));
        assert!(ss.contains("Post-jobs: 2\n"));
        assert!(ss.contains("Scan state: Paused\n"));
        assert!(ss.contains("Server state: Downloading, Post-Processing\n"));

        let mut base_buf = [0u8; BASE_SIZE];
        init_message_base(&mut base_buf, 3, 100, b"nzbget", b"secret");
        assert!(credentials_ok(&base_buf, b"nzbget", b"secret"));
        assert_eq!(request_type(&base_buf), 3);

        let mut dl_buf = [0u8; DOWNLOAD_REQUEST_SIZE];
        init_download_request(
            &mut dl_buf,
            b"nzbget",
            b"secret",
            b"",
            b"/tmp/test.nzb",
            false,
            b"movies",
            true,
            false,
            10,
            100,
            2,
            b"key1",
            42,
            true,
        );
        let dl_base: &[u8; BASE_SIZE] = (&dl_buf[..BASE_SIZE]).try_into().unwrap();
        assert!(credentials_ok(dl_base, b"nzbget", b"secret"));
        assert_eq!(request_type(dl_base), 1);
        assert_eq!(&dl_buf[76..76 + 13], b"/tmp/test.nzb");
        assert_eq!(&dl_buf[588..588 + 6], b"movies");

        let (trail, tlen, nlen) = encode_edit_queue_trailing(Some(b"hi"), &[7, 8], &[b"a", b"bc"]);
        assert_eq!((tlen, nlen), (4, 8));
        let mut parsed_ids = [0u32; 2];
        assert!(edit_sizes_ok(2, 2, nlen, tlen, trail.len() as u32));
        assert!(edit_split(&trail, tlen as usize, &mut parsed_ids, 2));
        assert_eq!(parsed_ids, [7, 8]);

        let nzb_wire = encode_all(&[NzbEntry {
            id: 10,
            kind: 0,
            size_lo: 200 * 1024 * 1024,
            size_hi: 0,
            remaining_size_lo: 150 * 1024 * 1024,
            remaining_size_hi: 0,
            paused_size_lo: 100 * 1024 * 1024,
            paused_size_hi: 0,
            paused_count: 1,
            remaining_par_count: 0,
            priority: 50,
            matched: 1,
            texts: [
                Text::new(b"MyNzb.nzb"),
                Text::new(b"MyNzb"),
                Text::new(b"/dst"),
                Text::new(b"movies"),
                Text::new(b""),
            ],
        }]);
        let ppp_wire = encode_all(&[
            ParamEntry {
                nzb_index: 1,
                texts: [Text::new(b"*Unpack:"), Text::new(b"no")],
            },
            ParamEntry {
                nzb_index: 1,
                texts: [Text::new(b"*unpack:"), Text::new(b"yes")],
            },
        ]);
        let files_wire = encode_all(&[
            FileEntry {
                id: 1,
                nzb_index: 1,
                size_lo: 100 * 1024 * 1024,
                size_hi: 0,
                remaining_size_lo: 50 * 1024 * 1024,
                remaining_size_hi: 0,
                paused: 0,
                filename_confirmed: 1,
                active_downloads: 1,
                matched: 1,
                texts: [Text::new(b"subj1"), Text::new(b"part1.rar")],
            },
            FileEntry {
                id: 2,
                nzb_index: 1,
                size_lo: 100 * 1024 * 1024,
                size_hi: 0,
                remaining_size_lo: 100 * 1024 * 1024,
                remaining_size_hi: 0,
                paused: 1,
                filename_confirmed: 1,
                active_downloads: 0,
                matched: 0,
                texts: [Text::new(b"subj2"), Text::new(b"part2.rar")],
            },
        ]);
        let mut full_wire = Vec::new();
        full_wire.extend_from_slice(&nzb_wire);
        full_wire.extend_from_slice(&ppp_wire);
        full_wire.extend_from_slice(&files_wire);

        let fl_wire = String::from_utf8(file_listing_from_wire(&full_wire, 1, 2, 2, true)).unwrap();
        assert!(fl_wire.contains("[1] MyNzb/part1.rar (100.00 MB, 50%, 1 thread)\n"));
        assert!(!fl_wire.contains("part2.rar"));

        let gl_wire =
            String::from_utf8(group_listing_from_wire(&full_wire, 1, 2, 2, false)).unwrap();
        assert_eq!(gl_wire, gl);
    }
}

/// Checks whether the remaining download time line should be printed in `RemoteClient::RequestServerList`.
#[inline]
pub fn remote_client_should_print_remaining_time(
    download_rate: u32,
    download_paused: u32,
    download2_paused: u32,
    download_standby: u32,
) -> bool {
    download_rate > 0 && download_paused == 0 && download2_paused == 0 && download_standby == 0
}

/// Computes unpaused remaining group bytes and active download thread flags for
/// `RemoteClient::RequestServerList`.
pub unsafe fn roc_remote_client_calc_group_unpaused_remaining(
    remaining_size: i64,
    paused_size: i64,
    active_downloads: std::ffi::c_int,
    out_has_threads: *mut std::ffi::c_int,
    out_threads_plural: *mut std::ffi::c_int,
) -> i64 {
    if !out_has_threads.is_null() {
        unsafe { *out_has_threads = if active_downloads > 0 { 1 } else { 0 } };
    }
    if !out_threads_plural.is_null() {
        unsafe { *out_threads_plural = if active_downloads > 1 { 1 } else { 0 } };
    }
    remaining_size - paused_size
}

/// Calculates the file completion percentage in `RemoteClient::RequestServerList` and
/// `NCursesFrontend::PrintFilename`.
pub fn remote_client_calc_file_completed_pct(remaining_size: i64, total_size: i64) -> i32 {
    if remaining_size < total_size && total_size != 0 {
        (100i64.wrapping_sub(remaining_size.wrapping_mul(100) / total_size)) as i32
    } else {
        -1
    }
}

/// Splits `sec` into `(hours, minutes, seconds)` for `RemoteClient::RequestServerList`
/// and `NCursesFrontend`.
pub fn remote_client_calc_hms(sec: i64) -> (i32, i32, i32) {
    let h = (sec / 3600) as i32;
    let m = ((sec % 3600) / 60) as i32;
    let s = (sec % 60) as i32;
    (h, m, s)
}

/// Calculates the session average download speed in `RemoteClient::RequestServerList`.
pub fn remote_client_calc_avg_speed(all_bytes: i64, download_time_sec: i64) -> i32 {
    if download_time_sec > 0 {
        (all_bytes / download_time_sec) as i32
    } else {
        0
    }
}

/// Formats `serverState` in `RemoteClient::RequestServerList`.
pub fn remote_client_format_server_state(
    download_paused: bool,
    download2_paused: bool,
    download_standby: bool,
    post_job_count: u32,
    post_paused: bool,
) -> Vec<u8> {
    let mut state = Vec::with_capacity(48);
    if download_paused || download2_paused {
        state.extend_from_slice(if download_standby {
            b"Paused"
        } else {
            b"Pausing"
        });
        if download_paused && download2_paused {
            state.extend_from_slice(b" (+2)");
        } else if download2_paused {
            state.extend_from_slice(b" (2)");
        }
    } else if !download_standby {
        state.extend_from_slice(b"Downloading");
    }

    if post_job_count > 0 || post_paused {
        if !state.is_empty() {
            state.extend_from_slice(b", Post-Processing");
        } else {
            state.extend_from_slice(b"Post-Processing");
        }
        if post_paused {
            state.extend_from_slice(b" paused");
        }
    }

    if state.is_empty() {
        state.extend_from_slice(b"Stand-By");
    }
    state
}

/// Fixed wire struct sizes for `ERemoteRequest` (`g_MessageRequestSizes` in `BinRpc.cpp`).
pub const MESSAGE_REQUEST_SIZES: [u32; (LAST_TYPE as usize) + 1] = [
    0, 1640, // rrDownload
    84,   // rrPauseUnpause
    604,  // rrList
    80,   // rrSetDownloadRate
    76,   // rrDumpDebug
    108,  // rrEditQueue
    84,   // rrLog
    76,   // rrShutdown
    76,   // rrReload
    76,   // rrVersion
    76,   // rrPostQueue
    84,   // rrWriteLog
    80,   // rrScan
    80,   // rrHistory
];

const LIST_RESPONSE_SIZE: usize = 92;
const TABLE_RESPONSE_SIZE: usize = 20;
const LIST_FILE_ENTRY_WIRE_SIZE: u32 = 48;
const LOG_ENTRY_WIRE_SIZE: u32 = 16;
const POST_ENTRY_WIRE_SIZE: u32 = 40;
const HISTORY_ENTRY_WIRE_SIZE: u32 = 40;

#[inline]
fn split_i64(val: i64) -> (u32, u32) {
    (
        ((val >> 32) & 0xffff_ffff) as u32,
        (val & 0xffff_ffff) as u32,
    )
}

#[inline]
fn join_i64(hi: u32, lo: u32) -> i64 {
    ((hi as i64) << 32) | (lo as u64 as i64)
}

fn c_slice(buf: &[u8]) -> &[u8] {
    &buf[..buf.iter().position(|&b| b == 0).unwrap_or(buf.len())]
}

/// Accumulates wire-encoded `Entry` items safely while string slices are live,
/// returning an empty slice if total length exceeds `MAX_RESPONSE`.
pub struct WireEntryEncoder {
    buf: Vec<u8>,
    overflowed: bool,
}

impl WireEntryEncoder {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            overflowed: false,
        }
    }

    /// # Safety
    /// All `Text` pointers inside `entry` must be readable for their lengths during this call.
    pub unsafe fn push<E: Entry>(&mut self, entry: &E) {
        if self.overflowed {
            return;
        }
        let len = entry_len(entry);
        let new_total = (self.buf.len() as u64).saturating_add(len as u64);
        if new_total > MAX_RESPONSE || self.buf.try_reserve(len).is_err() {
            self.overflowed = true;
            self.buf.clear();
            return;
        }
        let old_len = self.buf.len();
        let spare = &mut self.buf.spare_capacity_mut()[..len];
        let written = encode(std::slice::from_ref(entry), spare);
        debug_assert_eq!(written, len);
        self.buf.set_len(old_len + written);
    }

    pub fn finish(self) -> Vec<u8> {
        if self.overflowed {
            Vec::new()
        } else {
            self.buf
        }
    }
}

impl Default for WireEntryEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Mutable builder populated during `BinRpcProcessor::ExecuteList`.
pub struct BinRpcListBuilder {
    match_group: bool,
    #[cfg(unix)]
    regex: Option<crate::util::PosixRegEx>,
    pub regex_valid: bool,
    pub nr_nzb: u32,
    pub nr_ppp: u32,
    pub nr_files: u32,
    nzb_enc: WireEntryEncoder,
    ppp_enc: WireEntryEncoder,
    file_enc: WireEntryEncoder,
    pub remaining_size: i64,
    pub download_rate: u32,
    pub download_limit: i32,
    pub download_paused: i32,
    pub post_paused: i32,
    pub scan_paused: i32,
    pub thread_count: i32,
    pub post_job_count: i32,
    pub up_time_sec: i32,
    pub dn_time_sec: i32,
    pub stand_by: i32,
    pub downloaded_bytes: i64,
    pub state_populated: bool,
}

impl BinRpcListBuilder {
    #[cfg(unix)]
    pub fn new(match_mode: i32, match_group: bool, pattern: &[u8]) -> Self {
        let mut regex_valid = false;
        let regex = if match_mode == 3 {
            let mut pat_c = Vec::with_capacity(pattern.len() + 1);
            pat_c.extend_from_slice(pattern);
            pat_c.push(0);
            let rx = CStr::from_bytes_until_nul(&pat_c)
                .map(|c| crate::util::PosixRegEx::compile(c, 100))
                .unwrap_or_else(|_| crate::util::PosixRegEx::new_with(false, 100));
            regex_valid = rx.is_valid();
            Some(rx)
        } else {
            None
        };
        Self {
            match_group,
            regex,
            regex_valid,
            nr_nzb: 0,
            nr_ppp: 0,
            nr_files: 0,
            nzb_enc: WireEntryEncoder::new(),
            ppp_enc: WireEntryEncoder::new(),
            file_enc: WireEntryEncoder::new(),
            remaining_size: 0,
            download_rate: 0,
            download_limit: 0,
            download_paused: 0,
            post_paused: 0,
            scan_paused: 0,
            thread_count: 0,
            post_job_count: 0,
            up_time_sec: 0,
            dn_time_sec: 0,
            stand_by: 0,
            downloaded_bytes: 0,
            state_populated: false,
        }
    }

    #[cfg(unix)]
    #[allow(clippy::too_many_arguments)]
    pub fn push_nzb(
        &mut self,
        id: u32,
        kind: u32,
        size: i64,
        remaining_size: i64,
        paused_size: i64,
        paused_file_count: u32,
        remaining_par_count: u32,
        priority: u32,
        filename: &[u8],
        name: &[u8],
        dest_dir: &[u8],
        category: &[u8],
        queued_filename: &[u8],
    ) {
        self.nr_nzb = self.nr_nzb.wrapping_add(1);
        let group_matched = if self.match_group {
            match self.regex.as_mut() {
                None => 1,
                Some(rx) => {
                    let mut c = Vec::with_capacity(name.len() + 1);
                    c.extend_from_slice(name);
                    c.push(0);
                    u32::from(
                        CStr::from_bytes_until_nul(&c)
                            .map(|cs| rx.match_cstr(cs))
                            .unwrap_or(false),
                    )
                }
            }
        } else {
            0
        };
        let (size_hi, size_lo) = split_i64(size);
        let (rem_hi, rem_lo) = split_i64(remaining_size);
        let (paused_hi, paused_lo) = split_i64(paused_size);
        let entry = NzbEntry {
            id,
            kind,
            size_lo,
            size_hi,
            remaining_size_lo: rem_lo,
            remaining_size_hi: rem_hi,
            paused_size_lo: paused_lo,
            paused_size_hi: paused_hi,
            paused_count: paused_file_count,
            remaining_par_count,
            priority,
            matched: group_matched,
            texts: [
                Text::new(filename),
                Text::new(name),
                Text::new(dest_dir),
                Text::new(category),
                Text::new(queued_filename),
            ],
        };
        unsafe { self.nzb_enc.push(&entry) };
    }

    pub fn push_param(&mut self, owner_index: u32, name: &[u8], value: &[u8]) {
        self.nr_ppp = self.nr_ppp.wrapping_add(1);
        let entry = ParamEntry {
            nzb_index: owner_index,
            texts: [Text::new(name), Text::new(value)],
        };
        unsafe { self.ppp_enc.push(&entry) };
    }

    #[cfg(unix)]
    #[allow(clippy::too_many_arguments)]
    pub fn push_file(
        &mut self,
        id: u32,
        nzb_index: u32,
        size: i64,
        remaining_size: i64,
        paused: u32,
        filename_confirmed: u32,
        active_downloads: u32,
        nzb_name: &[u8],
        subject: &[u8],
        filename: &[u8],
    ) {
        self.nr_files = self.nr_files.wrapping_add(1);
        let matched = if !self.match_group {
            if let Some(rx) = self.regex.as_mut() {
                let base_fn = &filename[crate::paths::base_file_name(filename)..];
                let mut combined = Vec::with_capacity(nzb_name.len() + 1 + base_fn.len() + 1);
                combined.extend_from_slice(nzb_name);
                combined.push(b'/');
                combined.extend_from_slice(base_fn);
                combined.push(0);
                u32::from(
                    CStr::from_bytes_until_nul(&combined)
                        .map(|cs| rx.match_cstr(cs))
                        .unwrap_or(false),
                )
            } else {
                0
            }
        } else {
            0
        };
        let (size_hi, size_lo) = split_i64(size);
        let (rem_hi, rem_lo) = split_i64(remaining_size);
        let entry = FileEntry {
            id,
            nzb_index,
            size_lo,
            size_hi,
            remaining_size_lo: rem_lo,
            remaining_size_hi: rem_hi,
            paused,
            filename_confirmed,
            active_downloads,
            matched,
            texts: [Text::new(subject), Text::new(filename)],
        };
        unsafe { self.file_enc.push(&entry) };
    }
}

fn make_table_header(struct_size: usize, entry_wire_size: u32) -> Vec<u8> {
    let mut hdr = vec![0u8; struct_size];
    hdr[0..4].copy_from_slice(&(SIGNATURE as u32).to_be_bytes());
    hdr[4..8].copy_from_slice(&(struct_size as u32).to_be_bytes());
    hdr[8..12].copy_from_slice(&entry_wire_size.to_be_bytes());
    hdr
}

/// Serves one BinRPC request after the 4-byte `NZBMESSAGE_SIGNATURE` has been read (`BinRpcProcessor::Execute`).
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
pub fn serve_binrpc<RV, SD, LG, ACT, LST, LOGT, POSTT, HISTT>(
    remote_addr: &[u8],
    control_user: &[u8],
    control_password: &[u8],
    version: &[u8],
    control_port: i32,
    mut recv_exact: RV,
    mut send_all: SD,
    mut log_msg: LG,
    mut run_action: ACT,
    mut collect_list: LST,
    mut collect_log: LOGT,
    mut collect_post: POSTT,
    mut collect_history: HISTT,
) where
    RV: FnMut(&mut [u8]) -> bool,
    SD: FnMut(&[u8]) -> bool,
    LG: FnMut(i32, &[u8]),
    ACT: FnMut(i32, i32, i32, i32, i32, &[u8], &[u8], &[u8], &[u8], &[i32], &[&[u8]]) -> bool,
    LST: FnMut(bool, bool, &mut BinRpcListBuilder),
    LOGT: FnMut(i32, u32, &mut u32, &mut WireEntryEncoder),
    POSTT: FnMut(&mut u32, &mut WireEntryEncoder),
    HISTT: FnMut(bool, &mut u32, &mut WireEntryEncoder),
{
    let mut base = [0u8; BASE_SIZE];
    base[..4].copy_from_slice(&(SIGNATURE as u32).to_be_bytes());
    if !recv_exact(&mut base[4..]) {
        let mut w = Vec::with_capacity(64 + remote_addr.len());
        w.extend_from_slice(b"Non-RustyGet request received on port ");
        w.extend_from_slice(control_port.to_string().as_bytes());
        w.extend_from_slice(b" from ");
        w.extend_from_slice(remote_addr);
        log_msg(1, &w);
        return;
    }

    let kind = request_type(&base);
    if !credentials_ok(&base, control_user, control_password) {
        let mut w = Vec::with_capacity(96 + remote_addr.len());
        w.extend_from_slice(b"RustyGet request received on port ");
        w.extend_from_slice(control_port.to_string().as_bytes());
        w.extend_from_slice(b" from ");
        w.extend_from_slice(remote_addr);
        w.extend_from_slice(b", but username or password invalid");
        log_msg(1, &w);
        return;
    }

    {
        let rname = request_name(kind).to_bytes();
        let mut d = Vec::with_capacity(24 + rname.len() + remote_addr.len());
        d.extend_from_slice(rname);
        d.extend_from_slice(b" request received from ");
        d.extend_from_slice(remote_addr);
        log_msg(3, &d);
    }

    if let Some((expected, received)) = size_error(&base, &MESSAGE_REQUEST_SIZES) {
        let msg = format!(
            "Invalid size of request: expected {} Bytes, but received {} Bytes",
            expected as i32, received as i32
        );
        log_msg(2, msg.as_bytes());
        return;
    }

    let send_bool = |success: bool, text: &[u8], send: &mut SD| {
        let resp = bool_response(success, text);
        let _ = send(&resp[..16]);
        let _ = send(&resp[16..]);
    };

    let recv_extra = |buf: &mut [u8], recv: &mut RV, log: &mut LG| -> bool {
        if !buf.is_empty() && !recv(buf) {
            log(2, b"invalid request");
            return false;
        }
        true
    };

    let recv_trailing = |len: i64, max_len: i64, recv: &mut RV, log: &mut LG| -> Option<Vec<u8>> {
        if !trailing_ok(len, max_len) {
            let msg = format!("invalid request: data length {len}");
            log(2, msg.as_bytes());
            return None;
        }
        let n = len as usize;
        let mut buf = Vec::new();
        if buf.try_reserve_exact(n + 1).is_err() {
            let msg = format!("invalid request: no memory for {len} bytes");
            log(2, msg.as_bytes());
            return None;
        }
        buf.resize(n + 1, 0);
        if n > 0 && !recv(&mut buf[..n]) {
            log(2, b"invalid request");
            return None;
        }
        buf[n] = 0;
        Some(buf)
    };

    match kind {
        1 => {
            // rrDownload (1640 bytes)
            let mut extra = [0u8; 1640 - BASE_SIZE];
            if !recv_extra(&mut extra, &mut recv_exact, &mut log_msg) {
                return;
            }
            extra[511] = 0;
            extra[512 + 511] = 0;
            extra[1044 + 511] = 0;
            let nzb_filename = c_slice(&extra[0..512]);
            let category = c_slice(&extra[512..1024]);
            let add_first = i32::from_be_bytes(extra[1024..1028].try_into().unwrap());
            let add_paused = i32::from_be_bytes(extra[1028..1032].try_into().unwrap());
            let priority = i32::from_be_bytes(extra[1032..1036].try_into().unwrap());
            let dupe_score = i32::from_be_bytes(extra[1036..1040].try_into().unwrap());
            let dupe_mode = i32::from_be_bytes(extra[1040..1044].try_into().unwrap());
            let dupe_key = c_slice(&extra[1044..1556]);
            let buf_len = i32::from_be_bytes(extra[1556..1560].try_into().unwrap());
            let auto_category = i32::from_be_bytes(extra[1560..1564].try_into().unwrap());

            if buf_len <= 0 {
                log_msg(2, b"invalid request");
                return;
            }
            let Some(nzb_content) = recv_trailing(
                i64::from(buf_len),
                768 * 1024 * 1024,
                &mut recv_exact,
                &mut log_msg,
            ) else {
                log_msg(2, b"invalid request");
                return;
            };

            let is_url_flag = unsafe { is_url_cstr(nzb_content.as_ptr()) };
            let packed_flags = (i32::from(add_first != 0))
                | (i32::from(add_paused != 0) << 1)
                | (i32::from(auto_category != 0) << 2)
                | (i32::from(is_url_flag) << 3);
            let ok = run_action(
                1,
                packed_flags,
                priority,
                dupe_score,
                dupe_mode,
                nzb_filename,
                category,
                dupe_key,
                &nzb_content[..buf_len as usize],
                &[],
                &[],
            );
            let base_fn = &nzb_filename[crate::paths::base_file_name(nzb_filename)..];
            let mut reply = Vec::with_capacity(40 + base_fn.len());
            if ok {
                reply.extend_from_slice(b"Collection ");
                reply.extend_from_slice(base_fn);
                reply.extend_from_slice(b" added to queue");
            } else {
                reply.extend_from_slice(b"Download Request failed for ");
                reply.extend_from_slice(base_fn);
            }
            if reply.len() > 1023 {
                reply.truncate(1023);
            }
            send_bool(ok, &reply, &mut send_all);
        }
        2 => {
            // rrPauseUnpause (84 bytes)
            let mut extra = [0u8; 8];
            if !recv_extra(&mut extra, &mut recv_exact, &mut log_msg) {
                return;
            }
            let pause = i32::from_be_bytes(extra[0..4].try_into().unwrap());
            let action = i32::from_be_bytes(extra[4..8].try_into().unwrap());
            let _ = run_action(
                2,
                i32::from(pause != 0),
                action,
                0,
                0,
                b"",
                b"",
                b"",
                b"",
                &[],
                &[],
            );
            send_bool(
                true,
                b"Pause-/Unpause-Command completed successfully",
                &mut send_all,
            );
        }
        3 => {
            // rrList (604 bytes)
            let mut extra = [0u8; 604 - BASE_SIZE];
            if !recv_extra(&mut extra, &mut recv_exact, &mut log_msg) {
                return;
            }
            let want_files = i32::from_be_bytes(extra[0..4].try_into().unwrap()) != 0;
            let want_state = i32::from_be_bytes(extra[4..8].try_into().unwrap()) != 0;
            let match_mode = i32::from_be_bytes(extra[8..12].try_into().unwrap());
            let match_group = i32::from_be_bytes(extra[12..16].try_into().unwrap()) != 0;
            extra[16 + 511] = 0;
            let pattern = c_slice(&extra[16..528]);

            let mut builder = BinRpcListBuilder::new(match_mode, match_group, pattern);
            collect_list(want_files, want_state, &mut builder);

            let mut hdr = make_table_header(LIST_RESPONSE_SIZE, LIST_FILE_ENTRY_WIRE_SIZE);
            let mut entries = Vec::new();
            if want_files {
                hdr[72..76].copy_from_slice(&(i32::from(builder.regex_valid)).to_ne_bytes());
                hdr[76..80].copy_from_slice(&builder.nr_nzb.to_be_bytes());
                hdr[80..84].copy_from_slice(&builder.nr_ppp.to_be_bytes());
                hdr[84..88].copy_from_slice(&builder.nr_files.to_be_bytes());
                if !builder.nzb_enc.overflowed
                    && !builder.ppp_enc.overflowed
                    && !builder.file_enc.overflowed
                {
                    let total = (builder.nzb_enc.buf.len() as u64)
                        .saturating_add(builder.ppp_enc.buf.len() as u64)
                        .saturating_add(builder.file_enc.buf.len() as u64);
                    if total > 0 && total <= MAX_RESPONSE {
                        if entries.try_reserve_exact(total as usize).is_ok() {
                            entries.extend_from_slice(&builder.nzb_enc.buf);
                            entries.extend_from_slice(&builder.ppp_enc.buf);
                            entries.extend_from_slice(&builder.file_enc.buf);
                        }
                    }
                }
            }
            if want_state && builder.state_populated {
                let (rem_hi, rem_lo) = split_i64(builder.remaining_size);
                let (dl_hi, dl_lo) = split_i64(builder.downloaded_bytes);
                hdr[12..16].copy_from_slice(&rem_lo.to_be_bytes());
                hdr[16..20].copy_from_slice(&rem_hi.to_be_bytes());
                hdr[20..24].copy_from_slice(&builder.download_rate.to_be_bytes());
                hdr[24..28].copy_from_slice(&(builder.download_limit as u32).to_be_bytes());
                hdr[28..32]
                    .copy_from_slice(&(u32::from(builder.download_paused != 0)).to_be_bytes());
                hdr[36..40].copy_from_slice(&(u32::from(builder.stand_by != 0)).to_be_bytes());
                hdr[40..44].copy_from_slice(&(u32::from(builder.post_paused != 0)).to_be_bytes());
                hdr[44..48].copy_from_slice(&(u32::from(builder.scan_paused != 0)).to_be_bytes());
                hdr[48..52].copy_from_slice(&(builder.thread_count as u32).to_be_bytes());
                hdr[52..56].copy_from_slice(&(builder.post_job_count as u32).to_be_bytes());
                hdr[56..60].copy_from_slice(&(builder.up_time_sec as u32).to_be_bytes());
                hdr[60..64].copy_from_slice(&(builder.dn_time_sec as u32).to_be_bytes());
                hdr[64..68].copy_from_slice(&dl_lo.to_be_bytes());
                hdr[68..72].copy_from_slice(&dl_hi.to_be_bytes());
            }
            hdr[88..92].copy_from_slice(&(entries.len() as u32).to_be_bytes());
            let _ = send_all(&hdr);
            if !entries.is_empty() {
                let _ = send_all(&entries);
            }
        }
        4 => {
            // rrSetDownloadRate (80 bytes)
            let mut extra = [0u8; 4];
            if !recv_extra(&mut extra, &mut recv_exact, &mut log_msg) {
                return;
            }
            let rate = i32::from_be_bytes(extra);
            let _ = run_action(4, rate, 0, 0, 0, b"", b"", b"", b"", &[], &[]);
            send_bool(true, b"Rate-Command completed successfully", &mut send_all);
        }
        5 => {
            // rrDumpDebug
            let _ = run_action(5, 0, 0, 0, 0, b"", b"", b"", b"", &[], &[]);
            send_bool(true, b"Debug-Command completed successfully", &mut send_all);
        }
        6 => {
            // rrEditQueue (108 bytes)
            let mut extra = [0u8; 108 - BASE_SIZE];
            if !recv_extra(&mut extra, &mut recv_exact, &mut log_msg) {
                return;
            }
            let action = i32::from_be_bytes(extra[0..4].try_into().unwrap());
            let offset = i32::from_be_bytes(extra[4..8].try_into().unwrap());
            let match_mode = i32::from_be_bytes(extra[8..12].try_into().unwrap());
            let nr_ids = i32::from_be_bytes(extra[12..16].try_into().unwrap());
            let nr_names = i32::from_be_bytes(extra[16..20].try_into().unwrap());
            let names_len = i32::from_be_bytes(extra[20..24].try_into().unwrap());
            let text_len = i32::from_be_bytes(extra[24..28].try_into().unwrap());
            let data_len = u32::from_be_bytes(extra[28..32].try_into().unwrap());

            if !edit_sizes_ok(nr_ids, nr_names, names_len, text_len, data_len) {
                log_msg(2, b"Invalid struct size");
                return;
            }
            let Some(mut buf) = recv_trailing(
                i64::from(data_len),
                64 * 1024 * 1024,
                &mut recv_exact,
                &mut log_msg,
            ) else {
                return;
            };
            if text_len > 0 {
                buf[(text_len as usize) - 1] = 0;
            }
            if nr_ids <= 0 && nr_names <= 0 {
                send_bool(
                    false,
                    b"Edit-Command failed: no IDs/Names specified",
                    &mut send_all,
                );
                return;
            }
            let mut ids_u32 = vec![0u32; nr_ids.max(0) as usize];
            if !edit_split(
                &buf[..data_len as usize],
                text_len as usize,
                &mut ids_u32,
                nr_names.max(0) as usize,
            ) {
                log_msg(2, b"Invalid struct size");
                return;
            }
            let ids_i32: Vec<i32> = ids_u32.into_iter().map(|v| v as i32).collect();
            let mut name_slices: Vec<&[u8]> = Vec::with_capacity(nr_names.max(0) as usize);
            let mut at = (text_len as usize) + ids_i32.len() * 4;
            for _ in 0..(nr_names.max(0) as usize) {
                let rest = &buf[at..data_len as usize];
                let nul = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                name_slices.push(&rest[..nul]);
                at += nul + 1;
            }

            let offset_text: Vec<u8>;
            let arg_text: &[u8] = if action == 1 || action == 11 {
                // eaFileMoveOffset (1) || eaGroupMoveOffset (11)
                offset_text = offset.to_string().into_bytes();
                &offset_text
            } else if text_len > 0 {
                c_slice(&buf[..text_len as usize])
            } else {
                b""
            };

            let ok = run_action(
                6,
                action,
                match_mode,
                offset,
                0,
                arg_text,
                b"",
                b"",
                b"",
                &ids_i32,
                &name_slices,
            );
            send_bool(
                ok,
                if ok {
                    b"Edit-Command completed successfully"
                } else {
                    b"Edit-Command failed"
                },
                &mut send_all,
            );
        }
        7 => {
            // rrLog (84 bytes)
            let mut extra = [0u8; 8];
            if !recv_extra(&mut extra, &mut recv_exact, &mut log_msg) {
                return;
            }
            let id_from = u32::from_be_bytes(extra[0..4].try_into().unwrap());
            let lines = i32::from_be_bytes(extra[4..8].try_into().unwrap());
            let mut nr_entries = 0u32;
            let mut enc = WireEntryEncoder::new();
            collect_log(lines, id_from, &mut nr_entries, &mut enc);
            let entries = enc.finish();
            let mut hdr = make_table_header(TABLE_RESPONSE_SIZE, LOG_ENTRY_WIRE_SIZE);
            hdr[12..16].copy_from_slice(&nr_entries.to_be_bytes());
            hdr[16..20].copy_from_slice(&(entries.len() as u32).to_be_bytes());
            let _ = send_all(&hdr);
            if !entries.is_empty() {
                let _ = send_all(&entries);
            }
        }
        8 => {
            // rrShutdown: send bool response BEFORE invoking ExitProc!
            send_bool(true, b"Stopping server", &mut send_all);
            let _ = run_action(8, 0, 0, 0, 0, b"", b"", b"", b"", &[], &[]);
        }
        9 => {
            // rrReload: send bool response BEFORE invoking Reload!
            send_bool(true, b"Reloading server", &mut send_all);
            let _ = run_action(9, 0, 0, 0, 0, b"", b"", b"", b"", &[], &[]);
        }
        10 => {
            // rrVersion
            send_bool(true, version, &mut send_all);
        }
        11 => {
            // rrPostQueue
            let mut nr_entries = 0u32;
            let mut enc = WireEntryEncoder::new();
            collect_post(&mut nr_entries, &mut enc);
            let entries = enc.finish();
            let mut hdr = make_table_header(TABLE_RESPONSE_SIZE, POST_ENTRY_WIRE_SIZE);
            hdr[12..16].copy_from_slice(&nr_entries.to_be_bytes());
            hdr[16..20].copy_from_slice(&(entries.len() as u32).to_be_bytes());
            let _ = send_all(&hdr);
            if !entries.is_empty() {
                let _ = send_all(&entries);
            }
        }
        12 => {
            // rrWriteLog (84 bytes)
            let mut extra = [0u8; 8];
            if !recv_extra(&mut extra, &mut recv_exact, &mut log_msg) {
                return;
            }
            let msg_kind = i32::from_be_bytes(extra[0..4].try_into().unwrap());
            let data_len = i32::from_be_bytes(extra[4..8].try_into().unwrap());
            let Some(buf) = recv_trailing(
                i64::from(data_len),
                1024 * 1024,
                &mut recv_exact,
                &mut log_msg,
            ) else {
                return;
            };
            let ok = if (0..=4).contains(&msg_kind) {
                log_msg(msg_kind, c_slice(&buf));
                true
            } else {
                false
            };
            send_bool(
                ok,
                if ok {
                    b"Message added to log"
                } else {
                    b"Invalid message-kind"
                },
                &mut send_all,
            );
        }
        13 => {
            // rrScan (80 bytes)
            let mut extra = [0u8; 4];
            if !recv_extra(&mut extra, &mut recv_exact, &mut log_msg) {
                return;
            }
            let sync_mode = i32::from_be_bytes(extra) != 0;
            let _ = run_action(
                13,
                i32::from(sync_mode),
                0,
                0,
                0,
                b"",
                b"",
                b"",
                b"",
                &[],
                &[],
            );
            send_bool(
                true,
                if sync_mode {
                    b"Scan-Command completed"
                } else {
                    b"Scan-Command scheduled successfully"
                },
                &mut send_all,
            );
        }
        14 => {
            // rrHistory (80 bytes)
            let mut extra = [0u8; 4];
            if !recv_extra(&mut extra, &mut recv_exact, &mut log_msg) {
                return;
            }
            let show_hidden = i32::from_be_bytes(extra) != 0;
            let mut nr_entries = 0u32;
            let mut enc = WireEntryEncoder::new();
            collect_history(show_hidden, &mut nr_entries, &mut enc);
            let entries = enc.finish();
            let mut hdr = make_table_header(TABLE_RESPONSE_SIZE, HISTORY_ENTRY_WIRE_SIZE);
            hdr[12..16].copy_from_slice(&nr_entries.to_be_bytes());
            hdr[16..20].copy_from_slice(&(entries.len() as u32).to_be_bytes());
            let _ = send_all(&hdr);
            if !entries.is_empty() {
                let _ = send_all(&entries);
            }
        }
        _ => {
            let msg = format!("Received unsupported request {}", kind as i32);
            log_msg(2, msg.as_bytes());
        }
    }
}

/// Summary fields returned by `Frontend::RequestFileList` (`op == 101`).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct FrontendSummary {
    pub current_download_speed: i64,
    pub remaining_size: i64,
    pub all_bytes: i64,
    pub pause_download: i32,
    pub download_limit: i32,
    pub thread_count: i32,
    pub post_job_count: i32,
    pub up_time_sec: i32,
    pub dn_time_sec: i32,
    pub stand_by: i32,
}

#[cfg(unix)]
struct PlainBinRpcConn {
    host: Vec<u8>,
    sock: std::sync::atomic::AtomicI32,
    status: std::sync::atomic::AtomicI32,
}

#[cfg(unix)]
impl PlainBinRpcConn {
    fn connect<S>(
        raw_host: &[u8],
        port: i32,
        retry_attempts: usize,
        mut is_stopped: S,
    ) -> Option<Self>
    where
        S: FnMut() -> bool,
    {
        use std::sync::atomic::{AtomicI32, Ordering};
        let host: &[u8] = if raw_host == b"0.0.0.0" {
            b"127.0.0.1"
        } else {
            raw_host
        };
        for _ in 0..retry_attempts {
            let sock = AtomicI32::new(crate::socket::INVALID_SOCKET);
            let status = AtomicI32::new(crate::socket::CS_DISCONNECTED);
            let mut tls_err = false;
            let ok = crate::socket::connect_stream(
                host,
                port,
                0,
                60,
                &[],
                false,
                |_, _, _| false,
                &mut is_stopped,
                &sock,
                &status,
                &mut tls_err,
                |_| {},
                |_| {},
            );
            if ok && !is_stopped() {
                status.store(crate::socket::CS_CONNECTED, Ordering::SeqCst);
                return Some(Self {
                    host: host.to_vec(),
                    sock,
                    status,
                });
            }
            let mut is_listener = false;
            crate::socket::disconnect_socket(&host, false, &mut is_listener, &sock, &status);
            if retry_attempts > 1 {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        None
    }

    fn send(&self, data: &[u8]) -> bool {
        use std::sync::atomic::Ordering;
        crate::socket::send_all(&self.status, data, |chunk| unsafe {
            let fd = self.sock.load(Ordering::SeqCst);
            libc::send(fd, chunk.as_ptr().cast(), chunk.len(), 0) as std::ffi::c_int
        })
    }

    fn recv(&self, dst: &mut [u8]) -> bool {
        use std::sync::atomic::Ordering;
        let mut buf_offset = 0usize;
        let mut buf_avail = 0usize;
        let mut total_read = 0i32;
        let mut tls_err = false;
        crate::socket::recv_exact(
            &self.host,
            &[],
            &mut buf_offset,
            &mut buf_avail,
            &mut total_read,
            dst,
            &mut tls_err,
            |chunk, _| unsafe {
                let fd = self.sock.load(Ordering::SeqCst);
                libc::recv(fd, chunk.as_mut_ptr().cast(), chunk.len(), 0) as std::ffi::c_int
            },
            |_| {},
        )
    }

    fn disconnect(&self) {
        let mut is_listener = false;
        crate::socket::disconnect_socket(
            &self.host,
            false,
            &mut is_listener,
            &self.sock,
            &self.status,
        );
    }
}

#[cfg(unix)]
impl Drop for PlainBinRpcConn {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// Parameters for `client_execute` (`RemoteClient`, `Frontend` remote mode, and `RustyGet::ProcessClientRequest`).
pub struct BinRpcClientParams<'a> {
    pub control_ip: &'a [u8],
    pub control_port: i32,
    pub control_username: &'a [u8],
    pub control_password: &'a [u8],
    pub verbose: bool,
    pub match_mode: i32,
    pub set_rate: i32,
    pub edit_action: i32,
    pub edit_offset: i32,
    pub edit_text: Option<&'a [u8]>,
    pub edit_ids: &'a [i32],
    pub edit_names: &'a [&'a [u8]],
    pub log_lines: i32,
    pub log_first_id: u32,
    pub add_nzb_filename: &'a [u8],
    pub arg_filename: &'a [u8],
    pub add_category: &'a [u8],
    pub auto_category: bool,
    pub add_top: bool,
    pub add_paused: bool,
    pub add_priority: i32,
    pub add_dupe_key: &'a [u8],
    pub add_dupe_mode: i32,
    pub add_dupe_score: i32,
    pub write_log_kind: i32,
    pub write_log_text: &'a [u8],
    pub want_file_list: bool,
    pub want_summary: bool,
}

/// Executes a BinRPC client operation (`1..=23` for `CommandLineParser::EClientOperation`,
/// `100` for `Frontend::RequestMessages`, `101` for `Frontend::RequestFileList`).
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
pub fn client_execute<OUT, ERR, STOP, LOGCB, NZBCB, PARAMCB, FILECB>(
    op: i32,
    p: &BinRpcClientParams<'_>,
    mut out_summary: Option<&mut FrontendSummary>,
    mut write_stdout: OUT,
    mut write_perror: ERR,
    mut is_stopped: STOP,
    mut on_log_entry: LOGCB,
    mut on_nzb: NZBCB,
    mut on_param: PARAMCB,
    mut on_file: FILECB,
) -> bool
where
    OUT: FnMut(&[u8]),
    ERR: FnMut(&CStr),
    STOP: FnMut() -> bool,
    LOGCB: FnMut(u32, u32, u32, *const u8),
    NZBCB: FnMut(
        u32,
        u32,
        i64,
        i64,
        i64,
        i32,
        i32,
        i32,
        *const u8,
        *const u8,
        *const u8,
        *const u8,
        *const u8,
    ),
    PARAMCB: FnMut(usize, *const u8, *const u8),
    FILECB: FnMut(usize, u32, i64, i64, i32, i32, i32, *const u8, *const u8),
{
    let effective_ip: &[u8] = if p.control_ip == b"0.0.0.0" {
        b"127.0.0.1"
    } else {
        p.control_ip
    };

    let vprint = |bytes: &[u8], out: &mut OUT| {
        if p.verbose {
            out(bytes);
        }
    };

    let connect_retry = |out: &mut OUT, stop: &mut STOP| -> Option<PlainBinRpcConn> {
        let conn = PlainBinRpcConn::connect(p.control_ip, p.control_port, 10, stop);
        if conn.is_none() && p.verbose {
            let mut msg = Vec::with_capacity(64 + effective_ip.len());
            msg.extend_from_slice(b"Unable to send request to RustyGet server at ");
            msg.extend_from_slice(effective_ip);
            msg.extend_from_slice(b" (port ");
            msg.extend_from_slice(p.control_port.to_string().as_bytes());
            msg.extend_from_slice(b")\n");
            out(&msg);
        }
        conn
    };

    let send_and_recv_bool = |conn: &PlainBinRpcConn,
                              req: &[u8],
                              trailing: Option<&[u8]>,
                              out: &mut OUT,
                              perr: &mut ERR|
     -> bool {
        let ok = if !conn.send(req) {
            if p.verbose {
                perr(c"m_pConnection->Send");
            }
            false
        } else {
            if let Some(t) = trailing {
                if !t.is_empty() {
                    let _ = conn.send(t);
                }
            }
            vprint(b"Request sent\n", out);
            let mut resp = [0u8; 16];
            let read_ok = conn.recv(&mut resp);
            let sig = i32::from_be_bytes(resp[0..4].try_into().unwrap());
            let sz = u32::from_be_bytes(resp[4..8].try_into().unwrap());
            if !read_ok || sig != SIGNATURE || sz != 16 {
                vprint(
                    b"No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n",
                    out,
                );
                false
            } else {
                let success = u32::from_be_bytes(resp[8..12].try_into().unwrap()) != 0;
                let text_len = i32::from_be_bytes(resp[12..16].try_into().unwrap());
                let mut text_buf = vec![0u8; text_len.max(0) as usize];
                if text_len < 0 || (text_len > 0 && !conn.recv(&mut text_buf)) {
                    vprint(
                        b"No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n",
                        out,
                    );
                    false
                } else {
                    if p.verbose {
                        let txt = c_slice(&text_buf);
                        let mut line = Vec::with_capacity(18 + txt.len());
                        line.extend_from_slice(b"server returned: ");
                        line.extend_from_slice(txt);
                        line.push(b'\n');
                        out(&line);
                    }
                    success
                }
            }
        };
        conn.disconnect();
        ok
    };

    let exchange_trailing = |conn: &PlainBinRpcConn,
                             req: &[u8],
                             resp_size: usize,
                             verbose_exchange: bool,
                             out: &mut OUT,
                             perr: &mut ERR|
     -> Option<(Vec<u8>, Vec<u8>)> {
        if !conn.send(req) {
            if verbose_exchange && p.verbose {
                perr(c"m_pConnection->Send");
            }
            return None;
        }
        if verbose_exchange {
            vprint(b"Request sent\n", out);
        }
        let mut resp = vec![0u8; resp_size];
        let read_ok = conn.recv(&mut resp);
        let sig = i32::from_be_bytes(resp[0..4].try_into().unwrap());
        let sz = u32::from_be_bytes(resp[4..8].try_into().unwrap()) as usize;
        if !read_ok || sig != SIGNATURE || sz != resp_size {
            if verbose_exchange {
                vprint(
                    b"No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n",
                    out,
                );
            }
            return None;
        }
        let trailing_len = i32::from_be_bytes(resp[resp_size - 4..resp_size].try_into().unwrap());
        let mut trailing = Vec::new();
        if trailing_len > 0 {
            let n = trailing_len as usize;
            if trailing.try_reserve_exact(n).is_err() {
                return None;
            }
            trailing.resize(n, 0);
            if !conn.recv(&mut trailing) {
                return None;
            }
        }
        conn.disconnect();
        Some((resp, trailing))
    };

    let make_req = |req_type: i32, size: usize| -> Vec<u8> {
        let mut req = vec![0u8; size];
        let base_arr: &mut [u8; BASE_SIZE] = (&mut req[..BASE_SIZE]).try_into().unwrap();
        init_message_base(
            base_arr,
            req_type,
            size as i32,
            p.control_username,
            p.control_password,
        );
        req
    };

    match op {
        1 => {
            // opClientRequestDownload
            let mut arg_c = Vec::with_capacity(p.arg_filename.len() + 1);
            arg_c.extend_from_slice(p.arg_filename);
            arg_c.push(0);
            let is_url_flag = unsafe { is_url_cstr(arg_c.as_ptr()) };
            let payload: Vec<u8>;
            let trailing_slice: &[u8];
            if is_url_flag {
                trailing_slice = &arg_c;
            } else {
                let cstr = match CStr::from_bytes_with_nul(&arg_c) {
                    Ok(c) => c,
                    Err(_) => {
                        if p.verbose {
                            let mut msg = Vec::with_capacity(24 + p.arg_filename.len());
                            msg.extend_from_slice(b"Could not load file ");
                            msg.extend_from_slice(p.arg_filename);
                            msg.push(b'\n');
                            write_stdout(&msg);
                        }
                        return false;
                    }
                };
                let mut loaded = Vec::new();
                let ok = crate::fsops::load_file_into_buffer(
                    cstr,
                    false,
                    &crate::fsops::posix_stat,
                    |sz| {
                        if sz < 0 || loaded.try_reserve_exact(sz as usize).is_err() {
                            return std::ptr::null_mut();
                        }
                        loaded.resize(sz as usize, 0);
                        if sz == 0 {
                            std::ptr::NonNull::<u8>::dangling().as_ptr()
                        } else {
                            loaded.as_mut_ptr()
                        }
                    },
                );
                if !ok {
                    if p.verbose {
                        let mut msg = Vec::with_capacity(24 + p.arg_filename.len());
                        msg.extend_from_slice(b"Could not load file ");
                        msg.extend_from_slice(p.arg_filename);
                        msg.push(b'\n');
                        write_stdout(&msg);
                    }
                    return false;
                }
                payload = loaded;
                trailing_slice = &payload;
            }

            let Some(conn) = connect_retry(&mut write_stdout, &mut is_stopped) else {
                return false;
            };
            let mut req = [0u8; DOWNLOAD_REQUEST_SIZE];
            init_download_request(
                &mut req,
                p.control_username,
                p.control_password,
                p.add_nzb_filename,
                p.arg_filename,
                is_url_flag,
                p.add_category,
                p.add_top,
                p.add_paused,
                p.add_priority,
                p.add_dupe_score,
                p.add_dupe_mode,
                p.add_dupe_key,
                trailing_slice.len() as i32,
                p.auto_category,
            );
            send_and_recv_bool(
                &conn,
                &req,
                Some(trailing_slice),
                &mut write_stdout,
                &mut write_perror,
            )
        }
        2..=4 => {
            // opClientRequestListFiles (2), ListGroups (3), ListStatus (4)
            let files = op == 2;
            let groups = op == 3;
            let pattern_opt: Option<&[u8]> = if (files || groups) && p.match_mode == 3 {
                p.edit_text
            } else {
                None
            };
            let Some(conn) = connect_retry(&mut write_stdout, &mut is_stopped) else {
                return false;
            };
            let mut req = make_req(3, 604);
            req[76..80].copy_from_slice(&1u32.to_be_bytes());
            req[80..84].copy_from_slice(&1u32.to_be_bytes());
            req[84..88]
                .copy_from_slice(&(if pattern_opt.is_some() { 3u32 } else { 1u32 }).to_be_bytes());
            req[88..92].copy_from_slice(&(u32::from(groups)).to_be_bytes());
            if let Some(pat) = pattern_opt {
                let n = pat.len().min(511);
                req[92..92 + n].copy_from_slice(&pat[..n]);
            }
            let Some((resp, trailing)) = exchange_trailing(
                &conn,
                &req,
                LIST_RESPONSE_SIZE,
                true,
                &mut write_stdout,
                &mut write_perror,
            ) else {
                return false;
            };
            let regex_valid = i32::from_ne_bytes(resp[72..76].try_into().unwrap()) != 0;
            if pattern_opt.is_some() && !regex_valid {
                vprint(b"Error in regular expression\n", &mut write_stdout);
                return false;
            }
            let nr_nzb = u32::from_be_bytes(resp[76..80].try_into().unwrap());
            let nr_ppp = u32::from_be_bytes(resp[80..84].try_into().unwrap());
            let nr_files = u32::from_be_bytes(resp[84..88].try_into().unwrap());

            if files {
                if trailing.is_empty() {
                    vprint(
                        b"Server has no files queued for download\n",
                        &mut write_stdout,
                    );
                } else {
                    let formatted = file_listing_from_wire(
                        &trailing,
                        nr_nzb,
                        nr_ppp,
                        nr_files,
                        pattern_opt.is_some(),
                    );
                    write_stdout(&formatted);
                }
            }
            if groups {
                if trailing.is_empty() {
                    vprint(
                        b"Server has no files queued for download\n",
                        &mut write_stdout,
                    );
                } else {
                    let formatted = group_listing_from_wire(
                        &trailing,
                        nr_nzb,
                        nr_ppp,
                        nr_files,
                        pattern_opt.is_some(),
                    );
                    write_stdout(&formatted);
                }
            }

            let rem_lo = u32::from_be_bytes(resp[12..16].try_into().unwrap());
            let rem_hi = u32::from_be_bytes(resp[16..20].try_into().unwrap());
            let dl_lo = u32::from_be_bytes(resp[64..68].try_into().unwrap());
            let dl_hi = u32::from_be_bytes(resp[68..72].try_into().unwrap());
            let spec = StatusSummarySpec {
                include_remaining_size: i32::from(!files && !groups),
                remaining_size: join_i64(rem_hi, rem_lo),
                download_rate: u32::from_be_bytes(resp[20..24].try_into().unwrap()),
                download_paused: u32::from_be_bytes(resp[28..32].try_into().unwrap()),
                download2_paused: u32::from_be_bytes(resp[32..36].try_into().unwrap()),
                download_stand_by: u32::from_be_bytes(resp[36..40].try_into().unwrap()),
                downloaded_bytes: join_i64(dl_hi, dl_lo),
                download_time_sec: u32::from_be_bytes(resp[60..64].try_into().unwrap()),
                download_limit: u32::from_be_bytes(resp[24..28].try_into().unwrap()),
                up_time_sec: u32::from_be_bytes(resp[56..60].try_into().unwrap()),
                thread_count: u32::from_be_bytes(resp[48..52].try_into().unwrap()),
                post_job_count: u32::from_be_bytes(resp[52..56].try_into().unwrap()),
                scan_paused: u32::from_be_bytes(resp[44..48].try_into().unwrap()),
                post_paused: u32::from_be_bytes(resp[40..44].try_into().unwrap()),
            };
            let summary = status_summary(&spec);
            write_stdout(&summary);
            true
        }
        5 => {
            // opClientRequestSetRate
            let Some(conn) = connect_retry(&mut write_stdout, &mut is_stopped) else {
                return false;
            };
            let mut req = make_req(4, 80);
            req[76..80].copy_from_slice(&(p.set_rate as u32).to_be_bytes());
            send_and_recv_bool(&conn, &req, None, &mut write_stdout, &mut write_perror)
        }
        6 | 9 | 10 | 11 => {
            // DumpDebug (6->5), Shutdown (9->8), Reload (10->9), Version (11->10)
            let rr = match op {
                6 => 5,
                9 => 8,
                10 => 9,
                _ => 10,
            };
            let Some(conn) = connect_retry(&mut write_stdout, &mut is_stopped) else {
                return false;
            };
            let req = make_req(rr, BASE_SIZE);
            send_and_recv_bool(&conn, &req, None, &mut write_stdout, &mut write_perror)
        }
        7 => {
            // opClientRequestEditQueue
            if p.edit_ids.is_empty() && p.edit_names.is_empty() {
                vprint(b"File(s) not specified\n", &mut write_stdout);
                return false;
            }
            let Some(conn) = connect_retry(&mut write_stdout, &mut is_stopped) else {
                return false;
            };
            let (trailing, text_len, names_len) =
                encode_edit_queue_trailing(p.edit_text, p.edit_ids, p.edit_names);
            let mut req = make_req(6, 108);
            req[76..80].copy_from_slice(&(p.edit_action as u32).to_be_bytes());
            req[80..84].copy_from_slice(&(p.edit_offset as u32).to_be_bytes());
            req[84..88].copy_from_slice(&(p.match_mode as u32).to_be_bytes());
            req[88..92].copy_from_slice(&(p.edit_ids.len() as u32).to_be_bytes());
            req[92..96].copy_from_slice(&(p.edit_names.len() as u32).to_be_bytes());
            req[96..100].copy_from_slice(&(names_len as u32).to_be_bytes());
            req[100..104].copy_from_slice(&(text_len as u32).to_be_bytes());
            req[104..108].copy_from_slice(&(trailing.len() as u32).to_be_bytes());
            send_and_recv_bool(
                &conn,
                &req,
                Some(&trailing),
                &mut write_stdout,
                &mut write_perror,
            )
        }
        8 => {
            // opClientRequestLog
            let Some(conn) = connect_retry(&mut write_stdout, &mut is_stopped) else {
                return false;
            };
            let mut req = make_req(7, 84);
            req[80..84].copy_from_slice(&(p.log_lines as u32).to_be_bytes());
            let Some((resp, trailing)) = exchange_trailing(
                &conn,
                &req,
                TABLE_RESPONSE_SIZE,
                true,
                &mut write_stdout,
                &mut write_perror,
            ) else {
                return false;
            };
            if trailing.is_empty() {
                vprint(b"Log is empty\n", &mut write_stdout);
            } else {
                let count = u32::from_be_bytes(resp[12..16].try_into().unwrap());
                let hdr = format!(
                    "Log (last {} entries)\n-----------------------------------\n",
                    count as i32
                );
                vprint(hdr.as_bytes(), &mut write_stdout);
                let listing = log_listing(&trailing, count);
                vprint(c_slice(&listing), &mut write_stdout);
                vprint(b"-----------------------------------\n", &mut write_stdout);
            }
            true
        }
        12 => {
            // opClientRequestPostQueue
            let Some(conn) = connect_retry(&mut write_stdout, &mut is_stopped) else {
                return false;
            };
            let req = make_req(11, BASE_SIZE);
            let Some((resp, trailing)) = exchange_trailing(
                &conn,
                &req,
                TABLE_RESPONSE_SIZE,
                true,
                &mut write_stdout,
                &mut write_perror,
            ) else {
                return false;
            };
            if trailing.is_empty() {
                vprint(
                    b"Server has no jobs queued for post-processing\n",
                    &mut write_stdout,
                );
            } else {
                let count = u32::from_be_bytes(resp[12..16].try_into().unwrap());
                vprint(
                    b"Post-Processing List\n-----------------------------------\n",
                    &mut write_stdout,
                );
                let listing = post_queue_listing(&trailing, count);
                vprint(c_slice(&listing), &mut write_stdout);
                vprint(b"-----------------------------------\n", &mut write_stdout);
            }
            true
        }
        13 => {
            // opClientRequestWriteLog
            let Some(conn) = connect_retry(&mut write_stdout, &mut is_stopped) else {
                return false;
            };
            let mut trailing = Vec::with_capacity(p.write_log_text.len() + 1);
            trailing.extend_from_slice(p.write_log_text);
            trailing.push(0);
            let mut req = make_req(12, 84);
            req[76..80].copy_from_slice(&(p.write_log_kind as u32).to_be_bytes());
            req[80..84].copy_from_slice(&(trailing.len() as u32).to_be_bytes());
            send_and_recv_bool(
                &conn,
                &req,
                Some(&trailing),
                &mut write_stdout,
                &mut write_perror,
            )
        }
        14 | 15 => {
            // ScanSync (14), ScanAsync (15)
            let sync_mode = op == 14;
            let Some(conn) = connect_retry(&mut write_stdout, &mut is_stopped) else {
                return false;
            };
            let mut req = make_req(13, 80);
            req[76..80].copy_from_slice(&(u32::from(sync_mode)).to_be_bytes());
            send_and_recv_bool(&conn, &req, None, &mut write_stdout, &mut write_perror)
        }
        16..=21 => {
            // DownloadPause (16), DownloadUnpause (17), PostPause (18), PostUnpause (19), ScanPause (20), ScanUnpause (21)
            let (pause, action) = match op {
                16 => (true, 1u32),
                17 => (false, 1u32),
                18 => (true, 2u32),
                19 => (false, 2u32),
                20 => (true, 3u32),
                _ => (false, 3u32),
            };
            let Some(conn) = connect_retry(&mut write_stdout, &mut is_stopped) else {
                return false;
            };
            let mut req = make_req(2, 84);
            req[76..80].copy_from_slice(&(u32::from(pause)).to_be_bytes());
            req[80..84].copy_from_slice(&action.to_be_bytes());
            send_and_recv_bool(&conn, &req, None, &mut write_stdout, &mut write_perror)
        }
        22 | 23 => {
            // History (22), HistoryAll (23)
            let with_hidden = op == 23;
            let Some(conn) = connect_retry(&mut write_stdout, &mut is_stopped) else {
                return false;
            };
            let mut req = make_req(14, 80);
            req[76..80].copy_from_slice(&(u32::from(with_hidden)).to_be_bytes());
            let Some((resp, trailing)) = exchange_trailing(
                &conn,
                &req,
                TABLE_RESPONSE_SIZE,
                true,
                &mut write_stdout,
                &mut write_perror,
            ) else {
                return false;
            };
            if trailing.is_empty() {
                vprint(b"Server has no files in history\n", &mut write_stdout);
            } else {
                let count = u32::from_be_bytes(resp[12..16].try_into().unwrap());
                vprint(
                    b"History (most recent first)\n-----------------------------------\n",
                    &mut write_stdout,
                );
                let listing = history_listing(&trailing, count);
                vprint(c_slice(&listing), &mut write_stdout);
                let footer = format!(
                    "-----------------------------------\nItems: {}\n",
                    count as i32
                );
                vprint(footer.as_bytes(), &mut write_stdout);
            }
            true
        }
        100 => {
            // Frontend::RequestMessages (single connect attempt)
            let Some(conn) =
                PlainBinRpcConn::connect(p.control_ip, p.control_port, 1, &mut is_stopped)
            else {
                return false;
            };
            let id_from = if p.log_lines == 0 {
                if p.log_first_id > 0 {
                    p.log_first_id
                } else {
                    1
                }
            } else {
                0
            };
            let mut req = make_req(7, 84);
            req[76..80].copy_from_slice(&id_from.to_be_bytes());
            req[80..84].copy_from_slice(&(p.log_lines as u32).to_be_bytes());
            let Some((resp, trailing)) = exchange_trailing(
                &conn,
                &req,
                TABLE_RESPONSE_SIZE,
                false,
                &mut write_stdout,
                &mut write_perror,
            ) else {
                return false;
            };
            if !trailing.is_empty() {
                let nr_entries = u32::from_be_bytes(resp[12..16].try_into().unwrap());
                decode_log_response_entries(&trailing, nr_entries, |id, kind, time, ptr| {
                    on_log_entry(id, kind, time, ptr);
                });
            }
            true
        }
        101 => {
            // Frontend::RequestFileList (single connect attempt)
            let Some(conn) =
                PlainBinRpcConn::connect(p.control_ip, p.control_port, 1, &mut is_stopped)
            else {
                return false;
            };
            let mut req = make_req(3, 604);
            req[76..80].copy_from_slice(&(u32::from(p.want_file_list)).to_be_bytes());
            req[80..84].copy_from_slice(&(u32::from(p.want_summary)).to_be_bytes());
            let Some((resp, trailing)) = exchange_trailing(
                &conn,
                &req,
                LIST_RESPONSE_SIZE,
                false,
                &mut write_stdout,
                &mut write_perror,
            ) else {
                return false;
            };
            if p.want_summary {
                if let Some(ref mut sum) = out_summary {
                    let rem_lo = u32::from_be_bytes(resp[12..16].try_into().unwrap());
                    let rem_hi = u32::from_be_bytes(resp[16..20].try_into().unwrap());
                    let dl_lo = u32::from_be_bytes(resp[64..68].try_into().unwrap());
                    let dl_hi = u32::from_be_bytes(resp[68..72].try_into().unwrap());
                    sum.pause_download =
                        i32::from(u32::from_be_bytes(resp[28..32].try_into().unwrap()) != 0);
                    sum.remaining_size = join_i64(rem_hi, rem_lo);
                    sum.current_download_speed =
                        u32::from_be_bytes(resp[20..24].try_into().unwrap()) as i64;
                    sum.download_limit = i32::from_be_bytes(resp[24..28].try_into().unwrap());
                    sum.thread_count = i32::from_be_bytes(resp[48..52].try_into().unwrap());
                    sum.post_job_count = i32::from_be_bytes(resp[52..56].try_into().unwrap());
                    sum.up_time_sec = i32::from_be_bytes(resp[56..60].try_into().unwrap());
                    sum.dn_time_sec = i32::from_be_bytes(resp[60..64].try_into().unwrap());
                    sum.stand_by =
                        i32::from(u32::from_be_bytes(resp[36..40].try_into().unwrap()) != 0);
                    sum.all_bytes = join_i64(dl_hi, dl_lo);
                }
            }
            if p.want_file_list && !trailing.is_empty() {
                let nr_nzb = u32::from_be_bytes(resp[76..80].try_into().unwrap());
                let nr_ppp = u32::from_be_bytes(resp[80..84].try_into().unwrap());
                let nr_files = u32::from_be_bytes(resp[84..88].try_into().unwrap());
                let mut at = 0usize;
                let mut added_nzb = 0usize;
                for _ in 0..nr_nzb {
                    let Some((e, next)) = read::<NzbEntry, false>(&trailing, at) else {
                        return true;
                    };
                    at = next;
                    on_nzb(
                        e.id,
                        e.kind,
                        join_i64(e.size_hi, e.size_lo),
                        join_i64(e.remaining_size_hi, e.remaining_size_lo),
                        join_i64(e.paused_size_hi, e.paused_size_lo),
                        e.paused_count as i32,
                        e.remaining_par_count as i32,
                        e.priority as i32,
                        e.texts[0].data,
                        e.texts[1].data,
                        e.texts[2].data,
                        e.texts[3].data,
                        e.texts[4].data,
                    );
                    added_nzb += 1;
                }
                for _ in 0..nr_ppp {
                    let Some((e, next)) = read::<ParamEntry, false>(&trailing, at) else {
                        return true;
                    };
                    at = next;
                    let idx = (e.nzb_index as usize).wrapping_sub(1);
                    if idx < added_nzb {
                        on_param(idx, e.texts[0].data, e.texts[1].data);
                    }
                }
                for _ in 0..nr_files {
                    let Some((e, next)) = read::<FileEntry, false>(&trailing, at) else {
                        return true;
                    };
                    at = next;
                    let idx = (e.nzb_index as usize).wrapping_sub(1);
                    if idx < added_nzb {
                        on_file(
                            idx,
                            e.id,
                            join_i64(e.size_hi, e.size_lo),
                            join_i64(e.remaining_size_hi, e.remaining_size_lo),
                            i32::from(e.paused != 0),
                            i32::from(e.filename_confirmed != 0),
                            e.active_downloads as i32,
                            e.texts[0].data,
                            e.texts[1].data,
                        );
                    }
                }
            }
            true
        }
        _ => false,
    }
}

#[cfg(all(test, unix))]
mod loopback_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn binrpc_client_and_server_roundtrip_over_loopback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port() as i32;

        let server = std::thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut wstream = stream.try_clone().unwrap();
                let mut sig = [0u8; 4];
                stream.read_exact(&mut sig).unwrap();
                assert_eq!(i32::from_be_bytes(sig), SIGNATURE);
                serve_binrpc(
                    b"127.0.0.1",
                    b"admin",
                    b"secret",
                    b"26.0-test",
                    port,
                    |dst| stream.read_exact(dst).is_ok(),
                    |src| wstream.write_all(src).is_ok(),
                    |_, _| {},
                    |kind, i1, _, _, _, _, _, _, _, _, _| {
                        assert!(matches!(kind, 4 | 13));
                        if kind == 4 {
                            assert_eq!(i1, 512_000);
                        }
                        true
                    },
                    |want_files, want_state, b| {
                        if want_files {
                            b.push_nzb(
                                7,
                                0,
                                4096,
                                2048,
                                0,
                                0,
                                0,
                                10,
                                b"ubuntu.nzb",
                                b"ubuntu",
                                b"/dl/ubuntu",
                                b"iso",
                                b"/q/ubuntu.nzb",
                            );
                            b.push_param(1, b"*unpack:", b"yes");
                            b.push_file(
                                11,
                                1,
                                4096,
                                2048,
                                0,
                                1,
                                2,
                                b"ubuntu",
                                b"ubuntu.rar",
                                b"ubuntu.rar",
                            );
                        }
                        if want_state {
                            b.remaining_size = 2048;
                            b.download_rate = 1024;
                            b.download_limit = 512_000;
                            b.thread_count = 4;
                            b.up_time_sec = 65;
                            b.dn_time_sec = 30;
                            b.downloaded_bytes = 2048;
                            b.state_populated = true;
                        }
                    },
                    |_, _, _, _| {},
                    |_, _| {},
                    |_, _, _| {},
                );
            }
        });

        let mut base_params = BinRpcClientParams {
            control_ip: b"127.0.0.1",
            control_port: port,
            control_username: b"admin",
            control_password: b"secret",
            verbose: true,
            match_mode: 1,
            set_rate: 512_000,
            edit_action: 0,
            edit_offset: 0,
            edit_text: None,
            edit_ids: &[],
            edit_names: &[],
            log_lines: 0,
            log_first_id: 0,
            add_nzb_filename: b"",
            arg_filename: b"",
            add_category: b"",
            auto_category: true,
            add_top: false,
            add_paused: false,
            add_priority: 0,
            add_dupe_key: b"",
            add_dupe_mode: 0,
            add_dupe_score: 0,
            write_log_kind: 0,
            write_log_text: b"",
            want_file_list: true,
            want_summary: true,
        };

        // 1. Version request (op = 11)
        let mut out = Vec::new();
        assert!(client_execute(
            11,
            &base_params,
            None,
            |b| out.extend_from_slice(b),
            |_| {},
            || false,
            |_, _, _, _| {},
            |_, _, _, _, _, _, _, _, _, _, _, _, _| {},
            |_, _, _| {},
            |_, _, _, _, _, _, _, _, _| {},
        ));
        assert_eq!(out, b"Request sent\nserver returned: 26.0-test\n");

        // 2. SetRate request (op = 5)
        out.clear();
        assert!(client_execute(
            5,
            &base_params,
            None,
            |b| out.extend_from_slice(b),
            |_| {},
            || false,
            |_, _, _, _| {},
            |_, _, _, _, _, _, _, _, _, _, _, _, _| {},
            |_, _, _| {},
            |_, _, _, _, _, _, _, _, _| {},
        ));
        assert!(out.ends_with(b"server returned: Rate-Command completed successfully\n"));

        // 3. Frontend RequestFileList (op = 101)
        base_params.verbose = false;
        let mut summary = FrontendSummary::default();
        let mut nzb_ids = Vec::new();
        let mut file_ids = Vec::new();
        assert!(client_execute(
            101,
            &base_params,
            Some(&mut summary),
            |_| {},
            |_| {},
            || false,
            |_, _, _, _| {},
            |id, _, _, _, _, _, _, _, _, _, _, _, _| nzb_ids.push(id),
            |_, _, _| {},
            |_, fid, _, _, _, _, _, _, _| file_ids.push(fid),
        ));
        assert_eq!(summary.remaining_size, 2048);
        assert_eq!(summary.download_limit, 512_000);
        assert_eq!(nzb_ids, vec![7]);
        assert_eq!(file_ids, vec![11]);

        server.join().unwrap();
    }
}
