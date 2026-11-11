//! Test NNTP server (`daemon/nserv/`) subsystems:
//! - `YEncoder` (`daemon/nserv/YEncoder.cpp`): yEnc segment validation, file reading,
//!   line wrapping, escaping, dot-stuffing, and CRC32 calculation.
//! - `NzbGenerator` (`daemon/nserv/NzbGenerator.cpp`): directory scanning and `.nzb`
//!   XML generation for test data directories.
//! - `NntpCache` & `NntpProcessor` (`daemon/nserv/NntpServer.cpp`): in-memory segment
//!   cache, NNTP command dispatch, message-id parsing, disk/memory segment caching,
//!   yEnc encoding, and rate-limited chunk streaming.
//! - `NServFrontend` (`daemon/nserv/NServFrontend.cpp`): log cursor tracking and terminal output.
//! - `NServOpts` (`daemon/nserv/NServMain.cpp`): CLI option parsing and option-list generation.

use std::collections::HashMap;
use std::ffi::CStr;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::crc::Crc32;

const MAX_SEGMENT_BUFFER: usize = 16 * 1024 * 1024;

/// Extract the filename component after the last `/` or `\`.
pub fn base_file_name(path: &[u8]) -> &[u8] {
    path.iter()
        .rposition(|&b| b == b'/' || b == b'\\')
        .map_or(path, |pos| &path[pos + 1..])
}

/// Retained file handle opened and validated by `yencoder_open_file`.
pub struct YEncoderFile {
    file: Option<File>,
}

/// Validates and opens a file segment for `YEncoder::OpenFile`.
/// Updates `size` (setting `size = file_size - offset + 1` when `size == 0`)
/// and `file_size`, seeks the file to `offset`, and returns the open handle
/// (or the exact static error string expected by callers).
pub fn yencoder_open_file(
    path: &Path,
    offset: i64,
    size: &mut i32,
    file_size: &mut i64,
) -> Result<YEncoderFile, &'static CStr> {
    if *size < 0 {
        return Err(c"Invalid segment size");
    }

    let mut file = File::open(path).map_err(|_| c"File not found")?;
    let end_pos = file.seek(SeekFrom::End(0)).map_err(|_| c"File not found")? as i64;
    *file_size = end_pos;

    if *size == 0 {
        *size = (end_pos.wrapping_sub(offset).wrapping_add(1)) as i32;
    }
    if end_pos < offset.saturating_add(i64::from(*size)) {
        return Err(c"Invalid segment size");
    }
    if offset < 0 || file.seek(SeekFrom::Start(offset as u64)).is_err() {
        return Err(c"Invalid segment offset");
    }

    Ok(YEncoderFile { file: Some(file) })
}

/// Formats the `=ybegin` and `=ypart` headers for a yEnc segment.
pub fn format_yenc_begin_headers(
    part: i32,
    file_size: i64,
    base_filename: &[u8],
    offset: i64,
    size: i32,
) -> Vec<u8> {
    let part_s = part.to_string();
    let fsize_s = file_size.to_string();
    let begin_s = offset.saturating_add(1).to_string();
    let end_s = offset.saturating_add(i64::from(size)).to_string();
    let mut out = Vec::with_capacity(
        48 + part_s.len() + fsize_s.len() + base_filename.len() + begin_s.len() + end_s.len(),
    );
    out.extend_from_slice(b"=ybegin part=");
    out.extend_from_slice(part_s.as_bytes());
    out.extend_from_slice(b" line=128 size=");
    out.extend_from_slice(fsize_s.as_bytes());
    out.extend_from_slice(b" name=");
    out.extend_from_slice(base_filename);
    out.extend_from_slice(b"\r\n=ypart begin=");
    out.extend_from_slice(begin_s.as_bytes());
    out.extend_from_slice(b" end=");
    out.extend_from_slice(end_s.as_bytes());
    out.extend_from_slice(b"\r\n");
    out
}

/// Formats the `=yend` trailer line for a yEnc segment.
pub fn format_yenc_end_trailer(size: i32, pcrc32: u32) -> Vec<u8> {
    format!("=yend size={} part=0 pcrc32={:08x}\r\n", size, pcrc32).into_bytes()
}

fn flush_nul_terminated<F: FnMut(&[u8])>(outbuf: &mut Vec<u8>, write_fn: &mut F) {
    let payload_len = outbuf.len();
    outbuf.push(0);
    write_fn(&outbuf[..payload_len]);
    outbuf.clear();
}

/// Reads and yEnc-encodes a segment from the already-opened `handle` (`offset..offset + size`),
/// invoking `write_fn` with NUL-terminated output chunks matching `YEncoder::WriteSegment`.
pub fn yencoder_write_segment<F>(
    handle: &mut YEncoderFile,
    filename_bytes: &[u8],
    part: i32,
    offset: i64,
    size: i32,
    file_size: i64,
    mut write_fn: F,
) where
    F: FnMut(&[u8]),
{
    let Some(mut file) = handle.file.take() else {
        return;
    };
    if size <= 0 || offset < 0 {
        return;
    }

    let scaled = ((size as f64) * 1.1) as usize;
    let reserve_size = 2048.max(scaled.min(MAX_SEGMENT_BUFFER));
    let inbuf_size = (size as usize).min(MAX_SEGMENT_BUFFER);

    let mut outbuf = Vec::with_capacity(reserve_size + 1);
    outbuf.extend_from_slice(&format_yenc_begin_headers(
        part,
        file_size,
        base_file_name(filename_bytes),
        offset,
        size,
    ));

    let mut crc = Crc32::new();
    let mut inbuf = vec![0u8; inbuf_size.max(1)];
    let mut line_buf = [0u8; 260];
    let mut lnsz = 0usize;
    let mut pos = offset;
    let target_end = offset.saturating_add(i64::from(size));

    while pos < target_end {
        let need_bytes = (inbuf.len() as i64).min(target_end - pos) as usize;
        let read_bytes = match file.read(&mut inbuf[..need_bytes]) {
            Ok(0) | Err(_) => {
                handle.file = Some(file);
                return;
            }
            Ok(n) => n,
        };
        pos += read_bytes as i64;
        let last_block = pos == target_end;

        let chunk = &inbuf[..read_bytes];
        crc.append(chunk);

        for (idx, &raw) in chunk.iter().enumerate() {
            let mut ch = raw.wrapping_add(42);
            if matches!(ch, b'\0' | b'\n' | b'\r' | b'=' | b' ' | b'\t') {
                line_buf[lnsz] = b'=';
                lnsz += 1;
                ch = ch.wrapping_add(64);
            }
            if ch == b'.' && lnsz == 0 {
                line_buf[lnsz] = b'.';
                lnsz += 1;
            }
            line_buf[lnsz] = ch;
            lnsz += 1;

            if lnsz >= 128 || (idx + 1 == read_bytes && last_block) {
                line_buf[lnsz] = b'\r';
                line_buf[lnsz + 1] = b'\n';
                lnsz += 2;
                outbuf.extend_from_slice(&line_buf[..lnsz]);
                lnsz = 0;

                if outbuf.len() > reserve_size.saturating_sub(200) {
                    flush_nul_terminated(&mut outbuf, &mut write_fn);
                }
            }
        }
    }

    drop(file);

    outbuf.extend_from_slice(&format_yenc_end_trailer(size, crc.finish()));
    flush_nul_terminated(&mut outbuf, &mut write_fn);
}

/// Returns `true` if `filename` has length > 4 and ends with `.nzb` (ASCII case-insensitive).
pub fn is_nzb_filename(filename: &[u8]) -> bool {
    const EXT: &[u8] = b".nzb";
    filename.len() > EXT.len() && filename[filename.len() - EXT.len()..].eq_ignore_ascii_case(EXT)
}

/// Formats an `<file>...</file>` XML block for `NzbGenerator`.
pub fn format_nzb_file_xml(
    file_size: i64,
    segment_size: i32,
    timestamp: i64,
    base_name: &[u8],
    relative_path: Option<&[u8]>,
) -> Vec<u8> {
    let seg_size_64 = i64::from(segment_size);
    let seg_count = if seg_size_64 > 0 {
        ((file_size + seg_size_64 - 1) / seg_size_64) as i32
    } else {
        0
    };
    let mut out = Vec::with_capacity((seg_count.max(1) as usize) * 80 + 200);
    out.extend_from_slice(b"<file poster=\"nserv\" date=\"");
    out.extend_from_slice((timestamp as i32).to_string().as_bytes());
    out.extend_from_slice(b"\" subject=\"&quot;");
    out.extend_from_slice(base_name);
    out.extend_from_slice(b"&quot; yEnc (1/");
    out.extend_from_slice(seg_count.to_string().as_bytes());
    out.extend_from_slice(
        b")\">\n\
<groups>\n\
<group>alt.binaries.test</group>\n\
</groups>\n\
<segments>\n",
    );
    let mut seg_offset: i64 = 0;
    for segno in 1..=seg_count {
        let seg_size = (if seg_offset + seg_size_64 < file_size {
            seg_size_64
        } else {
            file_size - seg_offset
        }) as i32;
        out.extend_from_slice(b"<segment bytes=\"");
        out.extend_from_slice(seg_size.to_string().as_bytes());
        out.extend_from_slice(b"\" number=\"");
        out.extend_from_slice(segno.to_string().as_bytes());
        out.extend_from_slice(b"\">");
        if let Some(rel) = relative_path {
            out.extend_from_slice(rel);
            out.push(b'/');
        }
        out.extend_from_slice(base_name);
        out.push(b'?');
        out.extend_from_slice(segno.to_string().as_bytes());
        out.push(b'=');
        out.extend_from_slice(seg_offset.to_string().as_bytes());
        out.push(b':');
        out.extend_from_slice(seg_size.to_string().as_bytes());
        out.extend_from_slice(b"</segment>\n");
        seg_offset = seg_offset.wrapping_add(i64::from(seg_size));
    }
    out.extend_from_slice(b"</segments>\n</file>\n");
    out
}

/// Log event kind emitted by `execute_nzb_generator`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NzbGenLogKind {
    StartDir = 0,
    GeneratingFor = 1,
    CreateError = 2,
    ProcessingFile = 3,
    Finished = 4,
}

fn append_nzb_file<L>(
    outfile: &mut File,
    file_path: &Path,
    base_name: &[u8],
    relative_path: Option<&[u8]>,
    segment_size: i32,
    log_fn: &mut L,
) where
    L: FnMut(NzbGenLogKind, &[u8]),
{
    log_fn(NzbGenLogKind::ProcessingFile, base_name);
    let file_size = std::fs::metadata(file_path)
        .ok()
        .filter(|m| !m.is_dir())
        .map(|m| m.len() as i64)
        .unwrap_or(-1);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let xml = format_nzb_file_xml(file_size, segment_size, timestamp, base_name, relative_path);
    let _ = outfile.write_all(&xml);
}

#[cfg(unix)]
fn os_str_bytes(s: &std::ffi::OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes()
}

#[cfg(not(unix))]
fn os_str_bytes(s: &std::ffi::OsStr) -> &[u8] {
    s.to_str().map(str::as_bytes).unwrap_or(b"")
}

/// Runs `NzbGenerator::Execute` end-to-end on `data_dir` with `segment_size`.
pub fn execute_nzb_generator<L>(data_dir: &Path, data_dir_bytes: &[u8], segment_size: i32, mut log_fn: L)
where
    L: FnMut(NzbGenLogKind, &[u8]),
{
    log_fn(NzbGenLogKind::StartDir, data_dir_bytes);

    if let Ok(entries) = std::fs::read_dir(data_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name_bytes = os_str_bytes(&file_name);
            if is_nzb_filename(name_bytes) {
                continue;
            }

            let entry_path = entry.path();
            let nzb_filename = {
                let mut s = file_name.clone();
                s.push(".nzb");
                s
            };
            let nzb_path = data_dir.join(&nzb_filename);
            if std::fs::metadata(&nzb_path)
                .map(|m| !m.is_dir())
                .unwrap_or(false)
            {
                continue;
            }

            log_fn(NzbGenLogKind::GeneratingFor, name_bytes);

            let mut outfile = match File::create(&nzb_path) {
                Ok(f) => f,
                Err(_) => {
                    log_fn(NzbGenLogKind::CreateError, os_str_bytes(nzb_path.as_os_str()));
                    continue;
                }
            };

            let _ = outfile.write_all(
                b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE nzb PUBLIC \"-//newzBin//DTD NZB 1.0//EN\" \"http://www.newzbin.com/DTD/nzb/nzb-1.0.dtd\">\n\
<nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n",
            );

            let is_dir = std::fs::metadata(&entry_path)
                .map(|m| m.is_dir())
                .unwrap_or(false);
            if is_dir {
                if let Ok(sub_entries) = std::fs::read_dir(&entry_path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        let sub_is_dir = std::fs::metadata(&sub_path)
                            .map(|m| m.is_dir())
                            .unwrap_or(false);
                        if !sub_is_dir {
                            let sub_name = sub_entry.file_name();
                            append_nzb_file(
                                &mut outfile,
                                &sub_path,
                                os_str_bytes(&sub_name),
                                Some(name_bytes),
                                segment_size,
                                &mut log_fn,
                            );
                        }
                    }
                }
            } else {
                append_nzb_file(
                    &mut outfile,
                    &entry_path,
                    name_bytes,
                    None,
                    segment_size,
                    &mut log_fn,
                );
            }

            let _ = outfile.write_all(b"</nzb>\n");
        }
    }

    log_fn(NzbGenLogKind::Finished, b"");
}

// ============================================================================
// NntpCache & NntpProcessor (`daemon/nserv/NntpServer.cpp`)
// ============================================================================

#[cfg(unix)]
fn bytes_to_path_buf(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path_buf(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

fn format_io_error(err: &std::io::Error) -> Vec<u8> {
    if let Some(code) = err.raw_os_error() {
        unsafe extern "C" {
            fn strerror(errnum: std::ffi::c_int) -> *const std::ffi::c_char;
        }
        let ptr = unsafe { strerror(code) };
        if !ptr.is_null() {
            return unsafe { CStr::from_ptr(ptr) }.to_bytes().to_vec();
        }
    }
    err.to_string().into_bytes()
}

fn join_slash_1023(dir: &[u8], file: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dir.len() + 1 + file.len());
    out.extend_from_slice(dir);
    out.push(b'/');
    out.extend_from_slice(file);
    if out.len() > 1023 {
        out.truncate(1023);
    }
    out
}

/// Thread-safe in-memory yEnc segment cache for `NntpServer` (`NntpCache` in `NntpServer.h`).
/// Entries are never evicted or mutated once inserted (`emplace` semantics), so cached pointers
/// remain valid for the lifetime of `NntpCache`.
#[derive(Default)]
pub struct NntpCache {
    items: Mutex<HashMap<Vec<u8>, Box<[u8]>>>,
}

impl NntpCache {
    pub fn new() -> Self {
        Self {
            items: Mutex::new(HashMap::new()),
        }
    }

    /// Inserts `(key, data)` if `key` is not already present (matching `std::unordered_map::emplace`),
    /// appending a trailing NUL byte so C callers can also treat the buffer as a C-string.
    pub fn append(&self, key: &[u8], data: &[u8]) {
        if let Ok(mut guard) = self.items.lock() {
            guard.entry(key.to_vec()).or_insert_with(|| {
                let mut buf = Vec::with_capacity(data.len() + 1);
                buf.extend_from_slice(data);
                buf.push(0);
                buf.into_boxed_slice()
            });
        }
    }

    /// Looks up `key` and returns a slice (excluding the trailing NUL) whose backing allocation
    /// lives as long as `self` (since entries are never removed or reallocated).
    pub fn find(&self, key: &[u8]) -> Option<&[u8]> {
        let guard = self.items.lock().ok()?;
        let boxed = guard.get(key)?;
        let len = boxed.len().saturating_sub(1);
        let ptr = boxed.as_ptr();
        // Safety: `boxed` is heap-allocated inside `Box<[u8]>` and never removed or mutated
        // during `&self`'s lifetime.
        Some(unsafe { std::slice::from_raw_parts(ptr, len) })
    }
}

fn starts_with_ignore_ascii_case(s: &[u8], prefix: &[u8]) -> bool {
    s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn trim_tokenizer_ws(mut s: &[u8]) -> &[u8] {
    while let Some(&b) = s.first() {
        if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
            s = &s[1..];
        } else {
            break;
        }
    }
    while let Some(&b) = s.last() {
        if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
            s = &s[..s.len() - 1];
        } else {
            break;
        }
    }
    s
}

fn trim_right_cstring(mut s: &[u8]) -> &[u8] {
    while let Some(&b) = s.last() {
        if (b as i8) <= (b' ' as i8) {
            s = &s[..s.len() - 1];
        } else {
            break;
        }
    }
    s
}

/// Classifies an incoming NNTP line in `NntpProcessor::Run`, returning `(cmd_kind, arg_offset)`:
/// - `(1, 8)`: `"ARTICLE "`
/// - `(2, 5)`: `"BODY "`
/// - `(3, 5)`: `"STAT "`
/// - `(4, 6)`: `"GROUP "`
/// - `(5, 13)`: `"AUTHINFO USER"`
/// - `(6, 13)`: `"AUTHINFO PASS"`
/// - `(7, 4)`: `"QUIT"`
/// - `(0, 0)`: unknown command
pub fn nntp_classify_command(line: &[u8]) -> (i32, usize) {
    if starts_with_ignore_ascii_case(line, b"ARTICLE ") {
        (1, b"ARTICLE ".len())
    } else if starts_with_ignore_ascii_case(line, b"BODY ") {
        (2, b"BODY ".len())
    } else if starts_with_ignore_ascii_case(line, b"STAT ") {
        (3, b"STAT ".len())
    } else if starts_with_ignore_ascii_case(line, b"GROUP ") {
        (4, b"GROUP ".len())
    } else if starts_with_ignore_ascii_case(line, b"AUTHINFO USER") {
        (5, b"AUTHINFO USER".len())
    } else if starts_with_ignore_ascii_case(line, b"AUTHINFO PASS") {
        (6, b"AUTHINFO PASS".len())
    } else if line.eq_ignore_ascii_case(b"QUIT") {
        (7, b"QUIT".len())
    } else {
        (0, 0)
    }
}

fn c_atoll_bytes(s: &[u8]) -> i64 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    if i >= s.len() {
        return 0;
    }
    let mut neg = false;
    if s[i] == b'-' {
        neg = true;
        i += 1;
    } else if s[i] == b'+' {
        i += 1;
    }
    let mut val: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        let d = (s[i] - b'0') as i64;
        val = val.wrapping_mul(10).wrapping_add(d);
        i += 1;
    }
    if neg {
        val.wrapping_neg()
    } else {
        val
    }
}

fn c_atoi_bytes(s: &[u8]) -> i32 {
    c_atoll_bytes(s) as i32
}

/// Parses an NServ `<file?part=offset:size!servers>` message-id in `NntpProcessor::ServArticle`.
/// Returns `Some((name_len, part, offset, size, serv_offset))` where `name_len = from_idx - 1`
/// and `serv_offset` is `-1` when `'!'` is absent or the index after `'!'` when present.
pub fn nntp_parse_message_id(msg_id: &[u8]) -> Option<(i32, i32, i64, i32, i32)> {
    let from_idx = msg_id.iter().position(|&b| b == b'?')?;
    let off_idx = msg_id.iter().position(|&b| b == b'=')?;
    let to_idx = msg_id.iter().position(|&b| b == b':')?;
    let _end_idx = msg_id.iter().position(|&b| b == b'>')?;
    let serv_offset = match msg_id.iter().position(|&b| b == b'!') {
        Some(idx) => (idx + 1) as i32,
        None => -1,
    };
    let name_len = (from_idx as i32) - 1;
    let part = c_atoi_bytes(&msg_id[from_idx + 1..]);
    let offset = c_atoll_bytes(&msg_id[off_idx + 1..]);
    let size = c_atoi_bytes(&msg_id[to_idx + 1..]);
    Some((name_len, part, offset, size, serv_offset))
}

/// Checks whether `server_id` is in `serv_list` (comma-separated) for `NntpProcessor::ServerInList`.
pub fn nntp_server_in_list(serv_list: &[u8], server_id: i32) -> bool {
    for raw_tok in serv_list.split(|&b| b == b',') {
        let tok = trim_tokenizer_ws(raw_tok);
        if !tok.is_empty() && c_atoi_bytes(tok) == server_id {
            return true;
        }
    }
    false
}

/// Calculates `(expected_time, chunk_num, chunk_size, pause)` for `NntpProcessor::SendData`.
pub fn nntp_calc_send_chunks(
    size: i32,
    speed: i32,
    elapsed_ticks: i64,
) -> (i64, i32, i32, i32) {
    let expected_time =
        1000_i64 * (size as i64) / ((speed as i64) * 1024) - elapsed_ticks / 1000;
    let mut chunk_num = 21;
    let mut chunk_size = size;
    let mut pause = 0;
    while pause < 1 && chunk_num > 1 {
        chunk_num -= 1;
        chunk_size = size / chunk_num;
        pause = (expected_time / (chunk_num as i64)) as i32;
    }
    (expected_time, chunk_num, chunk_size, pause)
}

/// Formats `"<part>=<offset>-<size>"` for `NntpProcessor::SendSegment`.
pub fn nntp_format_cache_file_name(part: i32, offset: i64, size: i32) -> Vec<u8> {
    format!("{part}={offset}-{size}").into_bytes()
}

/// Formats NNTP server reply lines for `NntpProcessor`:
/// `kind`:
/// - `0`: `"211 0 0 0 <arg>\r\n"`
/// - `1`: `"403 <arg>\r\n"`
/// - `2`: `"223 0 <arg>\r\n"`
/// - `3`: `"220 0 <arg>\r\n"`
/// - `4`: `"222 0 <arg>\r\n"`
/// - `5`: `"Message-ID: <arg>\r\n"`
/// - `6`: `"Subject: \"<arg>\"\r\n"`
pub fn nntp_format_reply_line(kind: i32, arg: &[u8]) -> Vec<u8> {
    let (prefix, suffix): (&[u8], &[u8]) = match kind {
        0 => (b"211 0 0 0 ", b"\r\n"),
        1 => (b"403 ", b"\r\n"),
        2 => (b"223 0 ", b"\r\n"),
        3 => (b"220 0 ", b"\r\n"),
        4 => (b"222 0 ", b"\r\n"),
        5 => (b"Message-ID: ", b"\r\n"),
        6 => (b"Subject: \"", b"\"\r\n"),
        _ => (b"", b"\r\n"),
    };
    let mut out = Vec::with_capacity(prefix.len() + arg.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(arg);
    out.extend_from_slice(suffix);
    out
}

fn send_data_rate_limited(
    buffer: &[u8],
    speed: i32,
    start_instant: Option<Instant>,
    send_fn: &mut dyn FnMut(&[u8]),
) {
    if speed == 0 {
        send_fn(buffer);
        return;
    }

    let size = buffer.len() as i32;
    let elapsed_ticks = start_instant
        .map(|s| s.elapsed().as_micros() as i64)
        .unwrap_or(0);
    let (expected_time, chunk_num, chunk_size, pause) =
        nntp_calc_send_chunks(size, speed, elapsed_ticks);

    let mut sent: i32 = 0;
    for _ in 0..chunk_num {
        if sent >= size {
            break;
        }
        let mut len = if sent + chunk_size < size {
            chunk_size
        } else {
            size - sent
        };
        while sent + len < size && buffer[(sent + len) as usize] != b'\r' {
            len += 1;
        }
        send_fn(&buffer[sent as usize..(sent + len) as usize]);
        let now_ticks = start_instant
            .map(|s| s.elapsed().as_micros() as i64)
            .unwrap_or(0);
        if now_ticks + i64::from(pause) * 1000 < expected_time * 1000 && pause > 0 {
            std::thread::sleep(Duration::from_millis(pause as u64));
        }
        sent += len;
    }
}

#[allow(clippy::too_many_arguments)]
fn send_segment(
    message_id: &[u8],
    filename: &[u8],
    part: i32,
    offset: i64,
    size: i32,
    send_headers: bool,
    send_stat: bool,
    data_dir: Option<&[u8]>,
    cache_dir: Option<&[u8]>,
    speed: i32,
    cache: Option<&NntpCache>,
    write_line_fn: &mut dyn FnMut(&[u8]),
    send_fn: &mut dyn FnMut(&[u8]),
    log_fn: &mut dyn FnMut(i32, &[u8]),
) {
    let mut seg_log = Vec::with_capacity(32 + filename.len());
    seg_log.extend_from_slice(b"Sending segment ");
    seg_log.extend_from_slice(filename);
    seg_log.extend_from_slice(format!(" ({part}={offset}:{size})").as_bytes());
    log_fn(1, &seg_log);

    let start_instant = (speed > 0).then(Instant::now);

    let full_filename = join_slash_1023(data_dir.unwrap_or(b""), filename);
    let cache_file_dir = join_slash_1023(cache_dir.unwrap_or(b""), filename);
    let cache_file_name = nntp_format_cache_file_name(part, offset, size);
    let cache_full_filename = join_slash_1023(&cache_file_dir, &cache_file_name);
    let cache_key = join_slash_1023(filename, &cache_file_name);

    let cached_data = cache.and_then(|c| c.find(&cache_key));

    let has_cache_dir = cache_dir.is_some_and(|d| !d.is_empty());
    let mut read_cache_file: Option<File> = None;
    if cached_data.is_none() && has_cache_dir {
        read_cache_file = File::open(bytes_to_path_buf(&cache_full_filename)).ok();
    }
    let write_cache = cached_data.is_none() && has_cache_dir && read_cache_file.is_none();

    let mut cache_mem: Vec<u8> = Vec::new();
    if cache.is_some() && cached_data.is_none() && size > 0 {
        cache_mem.reserve(((size as f64) * 1.1) as usize);
    }

    let full_path = bytes_to_path_buf(&full_filename);
    if cached_data.is_none()
        && read_cache_file.is_none()
        && !std::fs::metadata(&full_path)
            .map(|m| m.is_file())
            .unwrap_or(false)
    {
        write_line_fn(b"430 Article not found\r\n");
        return;
    }

    let mut yenc_handle: Option<YEncoderFile> = None;
    let mut enc_size = size;
    let mut file_size = 0i64;
    if cached_data.is_none() && read_cache_file.is_none() {
        match yencoder_open_file(&full_path, offset, &mut enc_size, &mut file_size) {
            Ok(h) => yenc_handle = Some(h),
            Err(errmsg) => {
                let reply = nntp_format_reply_line(1, errmsg.to_bytes());
                write_line_fn(&reply);
                return;
            }
        }
    }

    if send_stat {
        let reply = nntp_format_reply_line(2, message_id);
        write_line_fn(&reply);
        return;
    }

    let mut write_cache_file: Option<File> = None;
    if write_cache {
        let dir_path = bytes_to_path_buf(&cache_file_dir);
        if let Err(e) = std::fs::create_dir_all(&dir_path) {
            let err_str = format_io_error(&e);
            let mut msg = b"Could not create directory ".to_vec();
            msg.extend_from_slice(&cache_file_dir);
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(&err_str);
            log_fn(3, &msg);
        }
        let file_path = bytes_to_path_buf(&cache_full_filename);
        match File::create(&file_path) {
            Ok(f) => write_cache_file = Some(f),
            Err(e) => {
                let err_str = format_io_error(&e);
                let mut msg = b"Could not create file ".to_vec();
                msg.extend_from_slice(&cache_full_filename);
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&err_str);
                log_fn(3, &msg);
            }
        }
    }

    let hdr_reply = nntp_format_reply_line(if send_headers { 3 } else { 4 }, message_id);
    write_line_fn(&hdr_reply);
    if send_headers {
        let msg_id_reply = nntp_format_reply_line(5, message_id);
        write_line_fn(&msg_id_reply);
        let subj_reply = nntp_format_reply_line(6, base_file_name(filename));
        write_line_fn(&subj_reply);
        write_line_fn(b"\r\n");
    }

    if let Some(data) = cached_data {
        send_data_rate_limited(data, speed, start_instant, send_fn);
    } else if let Some(mut cfile) = read_cache_file {
        let end_pos = cfile.seek(SeekFrom::End(0)).unwrap_or(0) as usize;
        let _ = cfile.seek(SeekFrom::Start(0));
        let mut buf = vec![0u8; end_pos];
        if let Err(e) = cfile.read_exact(&mut buf) {
            let err_str = format_io_error(&e);
            let mut msg = b"Could not read file ".to_vec();
            msg.extend_from_slice(&cache_full_filename);
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(&err_str);
            log_fn(3, &msg);
        }
        if cache.is_some() {
            cache_mem.extend_from_slice(&buf);
        }
        send_data_rate_limited(&buf, speed, start_instant, send_fn);
    } else if let Some(mut handle) = yenc_handle {
        yencoder_write_segment(
            &mut handle,
            &full_filename,
            part,
            offset,
            enc_size,
            file_size,
            |chunk| {
                if cache.is_some() {
                    cache_mem.extend_from_slice(chunk);
                }
                if let Some(ref mut wf) = write_cache_file {
                    let _ = wf.write_all(chunk);
                }
                send_data_rate_limited(chunk, speed, start_instant, send_fn);
            },
        );
    }

    if cached_data.is_none() && !cache_mem.is_empty() {
        if let Some(c) = cache {
            c.append(&cache_key, &cache_mem);
        }
    }

    write_line_fn(b".\r\n");
}

/// Processes a single incoming NNTP client line in `NntpProcessor::Run` (`NntpServer.cpp`).
/// Returns `true` to continue reading commands, or `false` on `QUIT`.
#[allow(clippy::too_many_arguments)]
pub fn nntp_processor_handle_line(
    server_id: i32,
    data_dir: Option<&[u8]>,
    cache_dir: Option<&[u8]>,
    latency: i32,
    speed: i32,
    cache: Option<&NntpCache>,
    raw_line: &[u8],
    write_line_fn: &mut dyn FnMut(&[u8]),
    send_fn: &mut dyn FnMut(&[u8]),
    log_fn: &mut dyn FnMut(i32, &[u8]),
) -> bool {
    let line = trim_right_cstring(raw_line);
    let mut recv_msg = Vec::with_capacity(10 + line.len());
    recv_msg.extend_from_slice(b"Received: ");
    recv_msg.extend_from_slice(line);
    log_fn(0, &recv_msg);

    let (cmd_kind, arg_offset) = nntp_classify_command(line);
    match cmd_kind {
        1..=3 => {
            let message_id = &line[arg_offset..];
            let send_headers = cmd_kind == 1;
            let send_stat = cmd_kind == 3;

            let mut srv_msg = Vec::with_capacity(9 + message_id.len());
            srv_msg.extend_from_slice(b"Serving: ");
            srv_msg.extend_from_slice(message_id);
            log_fn(0, &srv_msg);

            if latency > 0 {
                std::thread::sleep(Duration::from_millis(latency as u64));
            }

            if let Some((name_len, part, offset, size, serv_off)) =
                nntp_parse_message_id(message_id)
            {
                let filename = if name_len > 0 && (1 + name_len as usize) <= message_id.len() {
                    &message_id[1..1 + name_len as usize]
                } else {
                    b""
                };
                let ok = serv_off < 0
                    || ((serv_off as usize) <= message_id.len()
                        && nntp_server_in_list(&message_id[serv_off as usize..], server_id));
                if ok {
                    send_segment(
                        message_id,
                        filename,
                        part,
                        offset,
                        size,
                        send_headers,
                        send_stat,
                        data_dir,
                        cache_dir,
                        speed,
                        cache,
                        write_line_fn,
                        send_fn,
                        log_fn,
                    );
                } else {
                    write_line_fn(b"430 No Such Article Found\r\n");
                }
            } else {
                write_line_fn(b"430 No Such Article Found (invalid message id format)\r\n");
            }
            true
        }
        4 => {
            let reply = nntp_format_reply_line(0, &line[arg_offset..]);
            write_line_fn(&reply);
            true
        }
        5 => {
            write_line_fn(b"381 Enter passphrase\r\n");
            true
        }
        6 => {
            write_line_fn(b"281 Authentication accepted\r\n");
            true
        }
        7 => {
            log_fn(0, b"Closing connection");
            write_line_fn(b"205 Connection closing\r\n");
            false
        }
        _ => {
            let mut warn_msg = Vec::with_capacity(17 + line.len());
            warn_msg.extend_from_slice(b"Unknown command: ");
            warn_msg.extend_from_slice(line);
            log_fn(2, &warn_msg);
            write_line_fn(b"500 Unknown command\r\n");
            true
        }
    }
}

// ============================================================================
// NServFrontend & NServMain (`daemon/nserv/NServFrontend.cpp`, `NServMain.cpp`)
// ============================================================================

/// Matches the start index and skip check in `NServFrontend::Update()`:
/// `int start = needed - first + 1; if (start < 0) { need_skip = true; start = 0; }`.
pub fn nserv_frontend_calc_start_index(needed_first_id: i32, first_msg_id: i32) -> (i32, bool) {
    let start = needed_first_id.wrapping_sub(first_msg_id).wrapping_add(1);
    if start < 0 {
        (0, true)
    } else {
        (start, false)
    }
}

/// Port of `NServOpts` verbosity target flag evaluation.
/// Returns `(mute_info_warn_err, mute_detail, enable_debug)`.
pub fn nserv_verbosity_flags(verbosity: i32) -> (bool, bool, bool) {
    (verbosity < 1, verbosity < 2, verbosity > 2)
}

/// Formats `"LogFile=<log_file>"` for `NServMain`.
pub fn nserv_format_logfile_opt(log_file: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + log_file.len());
    out.extend_from_slice(b"LogFile=");
    out.extend_from_slice(log_file);
    out
}

/// Finalizes `NServOpts` option strings for `Options::CmdOptList` given `log_file` and `verbosity`.
pub fn nserv_build_cmd_opts(log_file: Option<&[u8]>, verbosity: i32) -> Vec<Vec<u8>> {
    let mut opts = Vec::new();
    match log_file.filter(|f| !f.is_empty()) {
        None => {
            opts.push(b"WriteLog=none".to_vec());
        }
        Some(lf) => {
            opts.push(b"WriteLog=append".to_vec());
            let mut log_opt = nserv_format_logfile_opt(lf);
            if log_opt.len() > 1023 {
                log_opt.truncate(1023);
            }
            opts.push(log_opt);
        }
    }
    let (mute_info_warn_err, mute_detail, enable_debug) = nserv_verbosity_flags(verbosity);
    if mute_info_warn_err {
        opts.push(b"InfoTarget=none".to_vec());
        opts.push(b"WarningTarget=none".to_vec());
        opts.push(b"ErrorTarget=none".to_vec());
    }
    if mute_detail {
        opts.push(b"DetailTarget=none".to_vec());
    }
    if enable_debug {
        opts.push(b"DebugTarget=both".to_vec());
    }
    opts
}

fn parse_c_atoi_or_default(bytes: &[u8], default_val: i32) -> i32 {
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    let mut sign: i64 = 1;
    if idx < bytes.len() {
        if bytes[idx] == b'-' {
            sign = -1;
            idx += 1;
        } else if bytes[idx] == b'+' {
            idx += 1;
        }
    }
    let start = idx;
    let mut acc: i64 = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        acc = acc
            .wrapping_mul(10)
            .wrapping_add((bytes[idx] - b'0') as i64);
        idx += 1;
    }
    if idx == start {
        default_val
    } else {
        (acc.wrapping_mul(sign)) as i32
    }
}

/// Parsed command-line options for `NServOpts` (`daemon/nserv/NServMain.h`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NServParsedArgs {
    pub data_dir: Vec<u8>,
    pub cache_dir: Vec<u8>,
    pub bind_address: Vec<u8>,
    pub first_port: i32,
    pub instances: i32,
    pub log_file: Vec<u8>,
    pub secure_cert: Vec<u8>,
    pub secure_key: Vec<u8>,
    pub generate_nzb: bool,
    pub segment_size: i32,
    pub quit: bool,
    pub latency: i32,
    pub speed: i32,
    pub mem_cache: bool,
    pub param_error: bool,
    pub cmd_opts: Vec<Vec<u8>>,
}

/// Parses `NServMain` command-line arguments starting at `args[2..]` (matching `optind = 2`
/// with `short_options = "b:c:d:l:p:i:ms:v:w:r:z:q"`).
pub fn nserv_parse_args(args: &[&[u8]]) -> NServParsedArgs {
    let mut out = NServParsedArgs {
        data_dir: Vec::new(),
        cache_dir: Vec::new(),
        bind_address: b"0.0.0.0".to_vec(),
        first_port: 6791,
        instances: 1,
        log_file: Vec::new(),
        secure_cert: Vec::new(),
        secure_key: Vec::new(),
        generate_nzb: false,
        segment_size: 500_000,
        quit: false,
        latency: 0,
        speed: 0,
        mem_cache: false,
        param_error: false,
        cmd_opts: Vec::new(),
    };
    let mut verbosity: i32 = 2;
    let mut idx = 2usize;

    while idx < args.len() {
        let arg = args[idx];
        if arg == b"--" {
            idx += 1;
            break;
        }
        if arg.len() < 2 || arg[0] != b'-' {
            break;
        }
        let mut pos = 1usize;
        while pos < arg.len() {
            let ch = arg[pos];
            pos += 1;
            match ch {
                b'm' => out.mem_cache = true,
                b'q' => out.quit = true,
                b'b' | b'c' | b'd' | b'l' | b'p' | b'i' | b's' | b'v' | b'w' | b'r' | b'z' => {
                    let optarg: Option<&[u8]> = if pos < arg.len() {
                        let rest = &arg[pos..];
                        pos = arg.len();
                        Some(rest)
                    } else {
                        idx += 1;
                        args.get(idx).copied()
                    };
                    let Some(val) = optarg else {
                        break;
                    };
                    match ch {
                        b'd' => out.data_dir = val.to_vec(),
                        b'c' => out.cache_dir = val.to_vec(),
                        b'l' => out.log_file = val.to_vec(),
                        b'b' => out.bind_address = val.to_vec(),
                        b'p' => out.first_port = parse_c_atoi_or_default(val, 6791),
                        b's' => {
                            out.secure_cert = val.to_vec();
                            idx += 1;
                            out.secure_key = args.get(idx).copied().unwrap_or(b"").to_vec();
                        }
                        b'i' => out.instances = parse_c_atoi_or_default(val, 1),
                        b'v' => verbosity = parse_c_atoi_or_default(val, 1),
                        b'w' => out.latency = parse_c_atoi_or_default(val, 0),
                        b'r' => out.speed = parse_c_atoi_or_default(val, 0),
                        b'z' => {
                            out.generate_nzb = true;
                            out.segment_size = parse_c_atoi_or_default(val, 500_000);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        idx += 1;
    }

    if idx < args.len() {
        out.param_error = true;
    }

    out.cmd_opts = nserv_build_cmd_opts(
        if out.log_file.is_empty() {
            None
        } else {
            Some(&out.log_file)
        },
        verbosity,
    );
    out
}

fn nserv_bytes_to_cstring(raw: &[u8]) -> std::ffi::CString {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    unsafe { std::ffi::CString::from_vec_unchecked(raw[..end].to_vec()) }
}

fn nserv_emit_log<L: FnMut(i32, &CStr)>(log_fn: &mut L, level: i32, parts: &[&[u8]]) {
    let joined: Vec<u8> = parts.concat();
    let cstr = nserv_bytes_to_cstring(&joined);
    log_fn(level, &cstr);
}

/// Opaque parsed and validated NServ options handle for `NServMain`.
pub struct NServOptsHandle {
    pub parsed: NServParsedArgs,
    pub c_data_dir: Option<std::ffi::CString>,
    pub c_cache_dir: Option<std::ffi::CString>,
    pub c_bind_address: std::ffi::CString,
    pub c_secure_cert: Option<std::ffi::CString>,
    pub c_secure_key: Option<std::ffi::CString>,
    pub c_cmd_opts: Vec<std::ffi::CString>,
}

impl NServOptsHandle {
    /// Parses `args`; if `data_dir` is empty or `param_error`, writes formatted usage via
    /// `write_stdout` and returns `None`. Otherwise validates `data_dir` and returns `Some`.
    pub fn parse_and_validate<W, L>(
        args: &[&[u8]],
        mut write_stdout: W,
        mut log_fn: L,
    ) -> Option<Self>
    where
        W: FnMut(&[u8]),
        L: FnMut(i32, &CStr),
    {
        let parsed = nserv_parse_args(args);
        if parsed.data_dir.is_empty() || parsed.param_error {
            let argv0 = args.first().copied().unwrap_or(b"rustyget");
            let usage = crate::cmdline::format_nserv_usage(base_file_name(argv0));
            write_stdout(&usage);
            return None;
        }

        let st = crate::fsops::posix_stat(&parsed.data_dir);
        if st != 2 {
            if st == 1 {
                nserv_emit_log(
                    &mut log_fn,
                    4,
                    &[
                        b"Specified data-dir ",
                        &parsed.data_dir,
                        b" is not a directory, but a file",
                    ],
                );
            } else {
                nserv_emit_log(
                    &mut log_fn,
                    4,
                    &[b"Specified data-dir ", &parsed.data_dir, b" does not exist"],
                );
            }
        }

        let opt_cstr = |v: &[u8]| -> Option<std::ffi::CString> {
            if v.is_empty() {
                None
            } else {
                Some(nserv_bytes_to_cstring(v))
            }
        };
        let c_data_dir = opt_cstr(&parsed.data_dir);
        let c_cache_dir = opt_cstr(&parsed.cache_dir);
        let c_bind_address = nserv_bytes_to_cstring(if parsed.bind_address.is_empty() {
            b"0.0.0.0"
        } else {
            &parsed.bind_address
        });
        let c_secure_cert = opt_cstr(&parsed.secure_cert);
        let c_secure_key = opt_cstr(&parsed.secure_key);
        let c_cmd_opts = parsed
            .cmd_opts
            .iter()
            .map(|o| nserv_bytes_to_cstring(o))
            .collect();

        Some(Self {
            parsed,
            c_data_dir,
            c_cache_dir,
            c_bind_address,
            c_secure_cert,
            c_secure_key,
            c_cmd_opts,
        })
    }

    /// Runs post-frontend setup (`NzbGenerator` if `-z` was given, returning `true` when `-q`
    /// requests immediate exit; otherwise creates `cache_dir` when specified and returns `false`).
    pub fn run_post_frontend_setup<L: FnMut(i32, &CStr)>(&self, mut log_fn: L) -> bool {
        if self.parsed.generate_nzb {
            let dir_path = bytes_to_path_buf(&self.parsed.data_dir);
            execute_nzb_generator(
                &dir_path,
                &self.parsed.data_dir,
                self.parsed.segment_size,
                |kind, msg| match kind {
                    NzbGenLogKind::StartDir => {
                        nserv_emit_log(&mut log_fn, 2, &[b"Generating nzbs for ", msg]);
                    }
                    NzbGenLogKind::GeneratingFor => {
                        nserv_emit_log(&mut log_fn, 2, &[b"Generating nzb for ", msg]);
                    }
                    NzbGenLogKind::CreateError => {
                        nserv_emit_log(&mut log_fn, 4, &[b"Could not create file ", msg]);
                    }
                    NzbGenLogKind::ProcessingFile => {
                        nserv_emit_log(&mut log_fn, 1, &[b"Processing ", msg]);
                    }
                    NzbGenLogKind::Finished => {
                        nserv_emit_log(&mut log_fn, 2, &[b"Nzb generation finished"]);
                    }
                },
            );
            if self.parsed.quit {
                return true;
            }
        }

        if !self.parsed.cache_dir.is_empty() {
            let mut errmsg = Vec::new();
            if !crate::fsops::force_directories(
                &self.parsed.cache_dir,
                &mut errmsg,
                &crate::fsops::posix_stat,
            ) {
                nserv_emit_log(
                    &mut log_fn,
                    4,
                    &[
                        b"Could not create directory ",
                        &self.parsed.cache_dir,
                        b": ",
                        &errmsg,
                    ],
                );
            }
        }

        false
    }
}

// ============================================================================
// End-to-End NServ Server & Client Session Runner (`NServMain`, `NntpServer`, `NntpProcessor`)
// ============================================================================

/// Host C++ callback table for `nzbget_rs_nserv_run`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NzbgetRsNservEnv {
    pub ctx: *mut std::ffi::c_void,
    pub version: *const std::ffi::c_char,
    pub write_stdout_cb: Option<unsafe extern "C" fn(*const std::ffi::c_char, usize)>,
    pub log_cb: Option<
        unsafe extern "C" fn(*mut std::ffi::c_void, std::ffi::c_int, *const std::ffi::c_char),
    >,
    pub runtime_cb: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            std::ffi::c_int,
            std::ffi::c_int,
            *const *const std::ffi::c_char,
            usize,
        ),
    >,
    pub conn_cb: Option<
        unsafe extern "C" fn(
            usize,
            std::ffi::c_int,
            std::ffi::c_int,
            std::ffi::c_int,
            *const std::ffi::c_char,
            *const std::ffi::c_char,
            *mut std::ffi::c_char,
        ) -> usize,
    >,
}

#[derive(Clone, Copy)]
struct NservEnvSend(NzbgetRsNservEnv);
unsafe impl Send for NservEnvSend {}
unsafe impl Sync for NservEnvSend {}

fn nserv_now_formatted() -> Vec<u8> {
    #[cfg(unix)]
    let now = unsafe { libc::time(std::ptr::null_mut()) } as i64;
    #[cfg(not(unix))]
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut buf = [0u8; 50];
    crate::util::util_format_time(now, &mut buf);
    let end = buf.iter().position(|&b| b == 0).unwrap_or(0);
    buf[..end].to_vec()
}

fn nserv_env_log(env: &NzbgetRsNservEnv, level: i32, parts: &[&[u8]]) {
    let Some(cb) = env.log_cb else {
        return;
    };
    let joined: Vec<u8> = parts.concat();
    let cstr = nserv_bytes_to_cstring(&joined);
    unsafe { cb(env.ctx, level as std::ffi::c_int, cstr.as_ptr()) };
}

struct NservSharedState {
    env: NservEnvSend,
    bind_address: std::ffi::CString,
    secure_cert: Option<std::ffi::CString>,
    secure_key: Option<std::ffi::CString>,
    data_dir: Option<Vec<u8>>,
    cache_dir: Option<Vec<u8>>,
    latency: i32,
    speed: i32,
    cache: Option<std::sync::Arc<NntpCache>>,
    stopped: std::sync::atomic::AtomicBool,
}

fn run_nntp_processor_session(
    shared: std::sync::Arc<NservSharedState>,
    proc_id: i32,
    server_id: i32,
    conn: usize,
) {
    let env = &shared.env.0;
    let Some(conn_cb) = env.conn_cb else {
        return;
    };
    let cert_ptr = shared
        .secure_cert
        .as_ref()
        .map_or(std::ptr::null(), |s| s.as_ptr());
    let key_ptr = shared
        .secure_key
        .as_ref()
        .map_or(std::ptr::null(), |s| s.as_ptr());

    let mut host_buf = [0 as std::ffi::c_char; 256];
    let started = unsafe {
        conn_cb(
            conn,
            5,
            host_buf.len() as std::ffi::c_int,
            0,
            cert_ptr,
            key_ptr,
            host_buf.as_mut_ptr(),
        )
    };
    if started == 0 {
        nserv_env_log(
            env,
            4,
            &[b"Could not establish secure connection to nntp-client: Start TLS failed"],
        );
        unsafe { conn_cb(conn, 4, 0, 1, std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) };
        return;
    }

    let host_bytes = unsafe { CStr::from_ptr(host_buf.as_ptr()) }.to_bytes();
    let ts = nserv_now_formatted();
    nserv_env_log(
        env,
        2,
        &[
            &ts,
            format!(" [{proc_id}] Incoming connection from: ").as_bytes(),
            host_bytes,
        ],
    );

    unsafe {
        conn_cb(
            conn,
            7,
            0,
            1,
            c"200 Welcome (NServ)\r\n".as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
        );
    }

    let mut line_buf = [0 as std::ffi::c_char; 1024];
    loop {
        let got = unsafe {
            conn_cb(
                conn,
                6,
                1024,
                0,
                std::ptr::null(),
                std::ptr::null(),
                line_buf.as_mut_ptr(),
            )
        };
        if got == 0 {
            break;
        }
        let raw_line = unsafe { CStr::from_ptr(line_buf.as_ptr()) }.to_bytes();
        let mut write_line_fn = |reply: &[u8]| {
            let cstr = nserv_bytes_to_cstring(reply);
            unsafe {
                conn_cb(
                    conn,
                    7,
                    0,
                    1,
                    cstr.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                );
            }
        };
        let mut send_fn = |chunk: &[u8]| unsafe {
            conn_cb(
                conn,
                7,
                chunk.len() as std::ffi::c_int,
                0,
                chunk.as_ptr().cast(),
                std::ptr::null(),
                std::ptr::null_mut(),
            );
        };
        let mut log_fn = |kind: i32, msg: &[u8]| match kind {
            0 => {
                let ts = nserv_now_formatted();
                nserv_env_log(
                    env,
                    1,
                    &[&ts, format!(" [{proc_id}] ").as_bytes(), msg],
                );
            }
            1 => nserv_env_log(env, 1, &[format!("[{proc_id}] ").as_bytes(), msg]),
            2 => nserv_env_log(env, 3, &[format!("[{proc_id}] ").as_bytes(), msg]),
            3 => nserv_env_log(env, 4, &[msg]),
            _ => {}
        };

        let keep_going = nntp_processor_handle_line(
            server_id,
            shared.data_dir.as_deref(),
            shared.cache_dir.as_deref(),
            shared.latency,
            shared.speed,
            shared.cache.as_deref(),
            raw_line,
            &mut write_line_fn,
            &mut send_fn,
            &mut log_fn,
        );
        if !keep_going {
            break;
        }
    }

    unsafe { conn_cb(conn, 4, 1, 1, std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) };
}

fn run_nntp_server_instance(
    shared: std::sync::Arc<NservSharedState>,
    server_id: i32,
    port: i32,
    listener_slot: std::sync::Arc<Mutex<usize>>,
) {
    use std::sync::atomic::Ordering;
    let env = &shared.env.0;
    let Some(conn_cb) = env.conn_cb else {
        return;
    };

    nserv_env_log(env, 0, &[b"Entering NntpServer-loop"]);
    let ts = nserv_now_formatted();
    nserv_env_log(
        env,
        2,
        &[&ts, format!(" Listening on port {port}").as_bytes()],
    );

    let cert_ptr = shared
        .secure_cert
        .as_ref()
        .map_or(std::ptr::null(), |s| s.as_ptr());
    let mut num: i32 = 1;

    while !shared.stopped.load(Ordering::SeqCst) {
        let mut conn = listener_slot.lock().map(|g| *g).unwrap_or(0);
        let bind = if conn == 0 {
            let created = unsafe {
                conn_cb(
                    0,
                    0,
                    port as std::ffi::c_int,
                    0,
                    shared.bind_address.as_ptr(),
                    cert_ptr,
                    std::ptr::null_mut(),
                )
            };
            let bound = created != 0
                && unsafe {
                    conn_cb(
                        created,
                        1,
                        0,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null_mut(),
                    )
                } != 0;
            if let Ok(mut g) = listener_slot.lock() {
                *g = created;
                if shared.stopped.load(Ordering::SeqCst) && created != 0 {
                    unsafe {
                        conn_cb(
                            created,
                            3,
                            0,
                            0,
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null_mut(),
                        );
                    }
                }
            }
            conn = created;
            bound
        } else {
            true
        };

        let accepted = if bind && !shared.stopped.load(Ordering::SeqCst) {
            unsafe {
                conn_cb(
                    conn,
                    2,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                )
            }
        } else {
            0
        };

        if !bind || accepted == 0 {
            if shared.stopped.load(Ordering::SeqCst) {
                break;
            }
            if let Ok(mut g) = listener_slot.lock() {
                let old = std::mem::replace(&mut *g, 0);
                if old != 0 {
                    unsafe {
                        conn_cb(
                            old,
                            4,
                            0,
                            0,
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null_mut(),
                        );
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }

        let proc_id = num;
        num = num.wrapping_add(1);
        let shared_clone = std::sync::Arc::clone(&shared);
        std::thread::spawn(move || {
            run_nntp_processor_session(shared_clone, proc_id, server_id, accepted);
        });
    }

    if let Ok(mut g) = listener_slot.lock() {
        let old = std::mem::replace(&mut *g, 0);
        if old != 0 {
            unsafe {
                conn_cb(
                    old,
                    4,
                    2,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                );
            }
        }
    }

    nserv_env_log(env, 0, &[b"Exiting NntpServer-loop"]);
}

/// Runs `NServMain` (`--nserv`) end-to-end: CLI parsing/validation, NZB generation (`-z`/`-q`),
/// multi-instance `NntpServer` accept loops, and per-client `NntpProcessor` sessions.
///
/// # Safety
/// `argv` (when `argc > 0`) and `env` must be valid for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_nserv_run(
    argc: std::ffi::c_int,
    argv: *const *const std::ffi::c_char,
    env: *const NzbgetRsNservEnv,
) -> std::ffi::c_int {
    let Some(env) = env.as_ref() else {
        return 1;
    };
    let ver_bytes = if env.version.is_null() {
        &b""[..]
    } else {
        CStr::from_ptr(env.version).to_bytes()
    };
    let ts = nserv_now_formatted();
    nserv_env_log(
        env,
        2,
        &[&ts, b" NServ ", ver_bytes, b" (Test NNTP server)"],
    );

    let mut arg_slices: Vec<&[u8]> = Vec::new();
    if argc > 0 && !argv.is_null() {
        for i in 0..argc as usize {
            let p = *argv.add(i);
            arg_slices.push(if p.is_null() { b"" } else { CStr::from_ptr(p).to_bytes() });
        }
    }

    let Some(handle) = NServOptsHandle::parse_and_validate(
        &arg_slices,
        |data| {
            if let Some(cb) = env.write_stdout_cb {
                cb(data.as_ptr().cast(), data.len());
            }
        },
        |level, msg| {
            if let Some(cb) = env.log_cb {
                cb(env.ctx, level as std::ffi::c_int, msg.as_ptr());
            }
        },
    ) else {
        return 1;
    };

    let opt_ptrs: Vec<*const std::ffi::c_char> =
        handle.c_cmd_opts.iter().map(|c| c.as_ptr()).collect();
    if let Some(rt_cb) = env.runtime_cb {
        rt_cb(env.ctx, 0, 0, opt_ptrs.as_ptr(), opt_ptrs.len());
    }

    let quit_now = handle.run_post_frontend_setup(|level, msg| {
        if let Some(cb) = env.log_cb {
            cb(env.ctx, level as std::ffi::c_int, msg.as_ptr());
        }
    });
    if quit_now {
        if let Some(rt_cb) = env.runtime_cb {
            rt_cb(env.ctx, 1, 0, std::ptr::null(), 0);
            rt_cb(env.ctx, 2, 20, std::ptr::null(), 0);
        }
        return 0;
    }

    let shared = std::sync::Arc::new(NservSharedState {
        env: NservEnvSend(*env),
        bind_address: handle.c_bind_address.clone(),
        secure_cert: handle.c_secure_cert.clone(),
        secure_key: handle.c_secure_key.clone(),
        data_dir: (!handle.parsed.data_dir.is_empty()).then(|| handle.parsed.data_dir.clone()),
        cache_dir: (!handle.parsed.cache_dir.is_empty()).then(|| handle.parsed.cache_dir.clone()),
        latency: handle.parsed.latency,
        speed: handle.parsed.speed,
        cache: handle
            .parsed
            .mem_cache
            .then(|| std::sync::Arc::new(NntpCache::new())),
        stopped: std::sync::atomic::AtomicBool::new(false),
    });

    let inst_count = handle.parsed.instances.max(0) as usize;
    let first_port = handle.parsed.first_port;
    let mut slots = Vec::with_capacity(inst_count);
    let mut threads = Vec::with_capacity(inst_count);

    for i in 0..inst_count {
        let slot = std::sync::Arc::new(Mutex::new(0usize));
        slots.push(std::sync::Arc::clone(&slot));
        let shared_clone = std::sync::Arc::clone(&shared);
        let server_id = (i as i32) + 1;
        let port = first_port.wrapping_add(i as i32);
        threads.push(std::thread::spawn(move || {
            run_nntp_server_instance(shared_clone, server_id, port, slot);
        }));
    }

    nserv_env_log(env, 2, &[b"Press Ctrl+C to quit"]);
    if let Some(rt_cb) = env.runtime_cb {
        rt_cb(env.ctx, 3, 0, std::ptr::null(), 0);
    }

    shared
        .stopped
        .store(true, std::sync::atomic::Ordering::SeqCst);
    if let Some(conn_cb) = env.conn_cb {
        for slot in &slots {
            if let Ok(g) = slot.lock() {
                if *g != 0 {
                    conn_cb(
                        *g,
                        3,
                        0,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null_mut(),
                    );
                }
            }
        }
    }

    if let Some(rt_cb) = env.runtime_cb {
        rt_cb(env.ctx, 1, 0, std::ptr::null(), 0);
    }
    for th in threads {
        let _ = th.join();
    }
    if let Some(rt_cb) = env.runtime_cb {
        rt_cb(env.ctx, 2, 50, std::ptr::null(), 0);
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yencoder_end_to_end() {
        let dir = std::env::temp_dir().join(format!("rustyget_yenc_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("sample.dat");
        std::fs::write(&file_path, b"nzbget").unwrap();

        let mut size = 6;
        let mut file_size = 0;
        let mut handle = yencoder_open_file(&file_path, 0, &mut size, &mut file_size).unwrap();
        assert_eq!(size, 6);
        assert_eq!(file_size, 6);

        let mut bad_size = 10;
        assert_eq!(
            yencoder_open_file(&file_path, 0, &mut bad_size, &mut file_size).err(),
            Some(c"Invalid segment size")
        );

        // Even if the file is unlinked after OpenFile, WriteSegment reads the retained handle
        std::fs::remove_file(&file_path).unwrap();

        let mut encoded = Vec::new();
        yencoder_write_segment(
            &mut handle,
            b"/tmp/sample.dat",
            1,
            0,
            size,
            file_size,
            |chunk| {
                // Verify NUL byte sits immediately past the slice for C-string callers
                assert_eq!( unsafe { *chunk.as_ptr().add(chunk.len()) }, 0 );
                encoded.extend_from_slice(chunk);
            },
        );
        let text = String::from_utf8_lossy(&encoded);
        assert!(text.starts_with("=ybegin part=1 line=128 size=6 name=sample.dat\r\n=ypart begin=1 end=6\r\n"));
        assert!(text.ends_with("=yend size=6 part=0 pcrc32=a30bff0d\r\n"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_nzb_generator_end_to_end() {
        let dir = std::env::temp_dir().join(format!("rustyget_nzbgen_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let single_file = dir.join("movie.mkv");
        std::fs::write(&single_file, vec![b'X'; 1000]).unwrap();

        let sub_dir = dir.join("album");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("track01.flac"), vec![b'Y'; 400]).unwrap();

        let mut logs = Vec::new();
        execute_nzb_generator(&dir, b"test_dir", 600, |kind, msg| {
            logs.push((kind, String::from_utf8_lossy(msg).into_owned()));
        });

        assert!(logs.iter().any(|(k, _)| *k == NzbGenLogKind::StartDir));
        assert!(logs.iter().any(|(k, _)| *k == NzbGenLogKind::Finished));

        let movie_nzb = std::fs::read_to_string(dir.join("movie.mkv.nzb")).unwrap();
        assert!(movie_nzb.contains("<segment bytes=\"600\" number=\"1\">movie.mkv?1=0:600</segment>"));
        assert!(movie_nzb.contains("<segment bytes=\"400\" number=\"2\">movie.mkv?2=600:400</segment>"));

        let album_nzb = std::fs::read_to_string(dir.join("album.nzb")).unwrap();
        assert!(album_nzb.contains("<segment bytes=\"400\" number=\"1\">album/track01.flac?1=0:400</segment>"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_nntp_processor_and_cache_end_to_end() {
        let dir = std::env::temp_dir().join(format!("rustyget_nntp_proc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data_dir = dir.join("data");
        let cache_dir = dir.join("cache");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::write(data_dir.join("test.bin"), b"0123456789").unwrap();

        let cache = NntpCache::new();
        let data_bytes = os_str_bytes(data_dir.as_os_str());
        let cache_bytes = os_str_bytes(cache_dir.as_os_str());

        let mut lines = Vec::new();
        let mut payload = Vec::new();
        let mut logs = Vec::new();

        // GROUP command
        assert!(nntp_processor_handle_line(
            1,
            Some(data_bytes),
            Some(cache_bytes),
            0,
            0,
            Some(&cache),
            b"GROUP alt.binaries.test\r\n",
            &mut |l| lines.push(l.to_vec()),
            &mut |p| payload.extend_from_slice(p),
            &mut |k, m| logs.push((k, m.to_vec())),
        ));
        assert_eq!(lines.pop().unwrap(), b"211 0 0 0 alt.binaries.test\r\n");

        // STAT command
        assert!(nntp_processor_handle_line(
            1,
            Some(data_bytes),
            Some(cache_bytes),
            0,
            0,
            Some(&cache),
            b"STAT <test.bin?1=0:5!1,2>\r\n",
            &mut |l| lines.push(l.to_vec()),
            &mut |p| payload.extend_from_slice(p),
            &mut |k, m| logs.push((k, m.to_vec())),
        ));
        assert_eq!(lines.pop().unwrap(), b"223 0 <test.bin?1=0:5!1,2>\r\n");

        // Server not in list
        assert!(nntp_processor_handle_line(
            3,
            Some(data_bytes),
            Some(cache_bytes),
            0,
            0,
            Some(&cache),
            b"STAT <test.bin?1=0:5!1,2>\r\n",
            &mut |l| lines.push(l.to_vec()),
            &mut |p| payload.extend_from_slice(p),
            &mut |k, m| logs.push((k, m.to_vec())),
        ));
        assert_eq!(lines.pop().unwrap(), b"430 No Such Article Found\r\n");

        // ARTICLE command (populates both disk cache and memory cache)
        lines.clear();
        payload.clear();
        assert!(nntp_processor_handle_line(
            1,
            Some(data_bytes),
            Some(cache_bytes),
            0,
            0,
            Some(&cache),
            b"ARTICLE <test.bin?1=0:5>\r\n",
            &mut |l| lines.push(l.to_vec()),
            &mut |p| payload.extend_from_slice(p),
            &mut |k, m| logs.push((k, m.to_vec())),
        ));
        assert_eq!(lines[0], b"220 0 <test.bin?1=0:5>\r\n");
        assert_eq!(lines.last().unwrap(), b".\r\n");
        assert!(!payload.is_empty());
        assert!(cache.find(b"test.bin/1=0-5").is_some());
        assert!(cache_dir.join("test.bin").join("1=0-5").is_file());

        // QUIT command returns false
        assert!(!nntp_processor_handle_line(
            1,
            Some(data_bytes),
            Some(cache_bytes),
            0,
            0,
            Some(&cache),
            b"QUIT\r\n",
            &mut |l| lines.push(l.to_vec()),
            &mut |p| payload.extend_from_slice(p),
            &mut |k, m| logs.push((k, m.to_vec())),
        ));

        assert_eq!(nserv_frontend_calc_start_index(0, 1), (0, false));
        assert_eq!(nserv_frontend_calc_start_index(2, 5), (0, true));
        assert_eq!(
            nserv_build_cmd_opts(None, 0),
            vec![
                b"WriteLog=none".to_vec(),
                b"InfoTarget=none".to_vec(),
                b"WarningTarget=none".to_vec(),
                b"ErrorTarget=none".to_vec(),
                b"DetailTarget=none".to_vec(),
            ]
        );
        assert_eq!(
            nserv_build_cmd_opts(Some(b"/tmp/nserv.log"), 3),
            vec![
                b"WriteLog=append".to_vec(),
                b"LogFile=/tmp/nserv.log".to_vec(),
                b"DebugTarget=both".to_vec(),
            ]
        );

        let parsed = nserv_parse_args(&[
            b"rustyget",
            b"--nserv",
            b"-d",
            b"/tmp/data",
            b"-c",
            b"/tmp/cache",
            b"-m",
            b"-p",
            b"7000",
            b"-s",
            b"/tmp/cert.pem",
            b"/tmp/key.pem",
            b"-z",
            b"250000",
            b"-q",
        ]);
        assert_eq!(parsed.data_dir, b"/tmp/data");
        assert_eq!(parsed.cache_dir, b"/tmp/cache");
        assert!(parsed.mem_cache);
        assert_eq!(parsed.first_port, 7000);
        assert_eq!(parsed.secure_cert, b"/tmp/cert.pem");
        assert_eq!(parsed.secure_key, b"/tmp/key.pem");
        assert!(parsed.generate_nzb);
        assert_eq!(parsed.segment_size, 250000);
        assert!(parsed.quit);
        assert!(!parsed.param_error);

        let trailing = nserv_parse_args(&[b"rustyget", b"--nserv", b"-d", b"/tmp/data", b"extra"]);
        assert!(trailing.param_error);

        let mut usage_out = Vec::new();
        let mut logs = Vec::new();
        assert!(NServOptsHandle::parse_and_validate(
            &[b"rustyget", b"--nserv"],
            |b| usage_out.extend_from_slice(b),
            |lvl, m| logs.push((lvl, m.to_bytes().to_vec())),
        )
        .is_none());
        assert!(!usage_out.is_empty());

        let cache_sub = dir.join("cache_created");
        let d_str = dir.to_str().unwrap().as_bytes();
        let c_str = cache_sub.to_str().unwrap().as_bytes();
        let handle = NServOptsHandle::parse_and_validate(
            &[b"rustyget", b"--nserv", b"-d", d_str, b"-c", c_str, b"-v", b"3"],
            |_| {},
            |lvl, m| logs.push((lvl, m.to_bytes().to_vec())),
        )
        .unwrap();
        assert!(logs.is_empty());
        assert_eq!(handle.c_bind_address.to_bytes(), b"0.0.0.0");
        assert_eq!(handle.c_cmd_opts.len(), 2);
        assert!(!handle.run_post_frontend_setup(|lvl, m| logs.push((lvl, m.to_bytes().to_vec()))));
        assert!(cache_sub.is_dir());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_nserv_run_shutdown_during_bind_does_not_hang_accept() {
        use std::sync::atomic::{AtomicI32, Ordering};
        struct MockState {
            status: AtomicI32, // 0=init, 1=listening, 2=cancelled, 3=freed
            accept_calls: AtomicI32,
            entered_bind: std::sync::Barrier,
            allow_bind_finish: std::sync::Barrier,
        }
        static MOCK: std::sync::OnceLock<MockState> = std::sync::OnceLock::new();
        let mock = MOCK.get_or_init(|| MockState {
            status: AtomicI32::new(0),
            accept_calls: AtomicI32::new(0),
            entered_bind: std::sync::Barrier::new(2),
            allow_bind_finish: std::sync::Barrier::new(2),
        });

        unsafe extern "C" fn mock_rt(_: *mut std::ffi::c_void, op: std::ffi::c_int, _: std::ffi::c_int, _: *const *const std::ffi::c_char, _: usize) {
            if op == 3 {
                let m = MOCK.get().unwrap();
                m.entered_bind.wait();
                // Return from getchar() immediately while Bind() is still in flight!
            }
        }

        unsafe extern "C" fn mock_conn(
            raw: usize,
            op: std::ffi::c_int,
            _: std::ffi::c_int,
            _: std::ffi::c_int,
            _: *const std::ffi::c_char,
            _: *const std::ffi::c_char,
            _: *mut std::ffi::c_char,
        ) -> usize {
            let m = MOCK.get().unwrap();
            match op {
                0 => 0x1234,
                1 => {
                    m.entered_bind.wait();
                    // Give main thread time to set shared.stopped = true and attempt slot cancellation
                    std::thread::sleep(Duration::from_millis(25));
                    m.allow_bind_finish.wait();
                    // Overwrite status with LISTENING just like socket::bind_listener does
                    m.status.store(1, Ordering::SeqCst);
                    1
                }
                2 => {
                    m.accept_calls.fetch_add(1, Ordering::SeqCst);
                    // Would hang forever if called while status == 1 (LISTENING)!
                    while m.status.load(Ordering::SeqCst) == 1 {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    0
                }
                3 => {
                    assert_eq!(raw, 0x1234);
                    m.status.store(2, Ordering::SeqCst);
                    1
                }
                4 => {
                    assert_eq!(raw, 0x1234);
                    m.status.store(3, Ordering::SeqCst);
                    0
                }
                _ => 0,
            }
        }

        let dir = std::env::temp_dir().join(format!("nzbget_nserv_race_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d_cstr = std::ffi::CString::new(dir.to_str().unwrap()).unwrap();
        let a0 = std::ffi::CString::new("rustyget").unwrap();
        let a1 = std::ffi::CString::new("--nserv").unwrap();
        let a2 = std::ffi::CString::new("-d").unwrap();
        let argv = [a0.as_ptr(), a1.as_ptr(), a2.as_ptr(), d_cstr.as_ptr()];
        let ver = std::ffi::CString::new("27.0").unwrap();

        let release_thread = std::thread::spawn(move || {
            MOCK.get().unwrap().allow_bind_finish.wait();
        });

        let env = NzbgetRsNservEnv {
            ctx: std::ptr::null_mut(),
            version: ver.as_ptr(),
            write_stdout_cb: None,
            log_cb: None,
            runtime_cb: Some(mock_rt),
            conn_cb: Some(mock_conn),
        };
        let rc = unsafe { nzbget_rs_nserv_run(argv.len() as std::ffi::c_int, argv.as_ptr(), &env) };
        release_thread.join().unwrap();
        assert_eq!(rc, 0);
        assert_eq!(mock.accept_calls.load(Ordering::SeqCst), 0);
        assert_eq!(mock.status.load(Ordering::SeqCst), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
