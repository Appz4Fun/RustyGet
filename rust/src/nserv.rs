//! Test NNTP server (`daemon/nserv/`) subsystems:
//! - `YEncoder` (`daemon/nserv/YEncoder.cpp`): yEnc segment validation, file reading,
//!   line wrapping, escaping, dot-stuffing, and CRC32 calculation.
//! - `NzbGenerator` (`daemon/nserv/NzbGenerator.cpp`): directory scanning and `.nzb`
//!   XML generation for test data directories.

use std::ffi::CStr;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

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
}
