//! `Scanner` (`daemon/queue/Scanner.cpp`) end-to-end Rust implementation.

#[inline]
fn eq_ignore_ascii_case(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// `Scanner::ServiceWork` pass state machine (`NzbDirFileAge < NzbDirInterval`).
/// Returns `(next_interval, next_pass)`.
pub fn next_pass(
    nzb_dir_interval: i32,
    nzb_dir_file_age: i32,
    has_scan_script: bool,
    current_pass: i32,
) -> (i32, i32) {
    let mut next_interval = nzb_dir_interval;
    let mut next_pass = current_pass;
    if nzb_dir_interval > 0 && nzb_dir_file_age < nzb_dir_interval {
        let max_pass = if has_scan_script { 3 } else { 1 };
        if current_pass < max_pass {
            next_interval = nzb_dir_file_age;
            next_pass = current_pass + 1;
        } else {
            next_pass = 0;
        }
    }
    (next_interval, next_pass)
}

/// `Scanner::CanProcessFile` extension filter.
pub fn can_process_extension(full_filename: &[u8]) -> bool {
    let dot_pos = match full_filename.iter().rposition(|&b| b == b'.') {
        Some(pos) => pos,
        None => return false,
    };
    let ext = &full_filename[dot_pos..];
    !eq_ignore_ascii_case(ext, b".queued")
        && !eq_ignore_ascii_case(ext, b".error")
        && !eq_ignore_ascii_case(ext, b".processed")
}

/// `Scanner::CanProcessFile` file size & age stabilization check.
/// Returns `(can_process, updated_size, updated_last_change)`.
pub fn eval_file_age(
    old_size: i64,
    new_size: i64,
    last_change: i64,
    current_time: i64,
    file_age: i32,
) -> (bool, i64, i64) {
    if old_size == new_size && current_time - last_change >= file_age as i64 {
        (true, old_size, last_change)
    } else if old_size != new_size {
        (false, new_size, current_time)
    } else {
        (false, old_size, last_change)
    }
}

/// `Scanner::DropOldFiles` stale entry predicate.
pub fn should_drop_scan_file(
    last_change: i64,
    current_time: i64,
    nzb_dir_interval: i32,
    nzb_dir_file_age: i32,
) -> bool {
    let threshold = ((nzb_dir_interval as i64) + (nzb_dir_file_age as i64)) * 2;
    (current_time - last_change >= threshold) || current_time < last_change
}

/// `Scanner::ShouldSkipItem`:
/// Skips dot-files/directories, `processed_dir`, and `broken_dir`.
pub fn should_skip_item(filename: &[u8], processed_dir: &[u8], broken_dir: &[u8]) -> bool {
    if filename.is_empty() {
        return false;
    }
    filename.starts_with(b".") || filename == processed_dir || filename == broken_dir
}

/// `Scanner::ProcessIncomingFile` extension dispatch:
/// Returns `(rename_to_processed_after_script, dispatch_action)`:
/// - `1`: `.nzb_processed` (rename `.nzb` and `AddFileToQueue`)
/// - `2`: `.nzb` when `exists` (`AddFileToQueue`)
/// - `0`: no queue action
pub fn classify_incoming_extension(
    ran_scan_script: bool,
    exists: bool,
    extension: &[u8],
) -> (bool, i32) {
    let is_nzb = extension.eq_ignore_ascii_case(b".nzb");
    let is_nzb_processed = extension.eq_ignore_ascii_case(b".nzb_processed");
    let rename_to_processed = ran_scan_script && exists && !is_nzb;
    let action = if is_nzb_processed {
        1
    } else if exists && is_nzb {
        2
    } else {
        0
    };
    (rename_to_processed, action)
}

/// `Scanner::AddFileToQueue` missed-articles severity classifier.
/// Returns `0` (none), `1` (`Message::mkDetail`), `2` (`Message::mkWarning`).
pub fn classify_missed_articles(
    missed_articles: i32,
    total_articles: i32,
    is_cut_short: bool,
) -> i32 {
    if missed_articles <= 0 {
        return 0;
    }
    let large = missed_articles >= 100 || missed_articles.saturating_mul(10) >= total_articles;
    if is_cut_short || large {
        2
    } else {
        1
    }
}

/// `Scanner::AddArchive` status aggregation across extracted NZB files.
pub fn resolve_archive_add_status(
    has_url_info: bool,
    statuses: &[i32],
    as_success: i32,
    as_skipped: i32,
) -> i32 {
    if statuses.is_empty() {
        return as_skipped;
    }
    if has_url_info {
        return statuses[0];
    }
    if statuses.contains(&as_success) {
        as_success
    } else {
        statuses[0]
    }
}

/// `Scanner::AddExternalFile` XML/NZB header detector (matches `BString<1024>` truncation).
pub fn is_nzb_xml_prefix(buf: &[u8]) -> bool {
    let max_len = buf.len().min(1023);
    let slice = &buf[..max_len];
    let slice = match slice.iter().position(|&b| b == 0) {
        Some(nul) => &slice[..nul],
        None => slice,
    };
    slice.starts_with(b"<?xml") && slice.windows(4).any(|w| w == b"<nzb")
}

/// Checks whether an NZB name or filename is an absolute path (`/`, `\`, or `<drive>:\` / `<drive>:/`).
pub fn is_absolute_nzb_name(nzb_name: &[u8]) -> bool {
    if nzb_name.is_empty() {
        return false;
    }
    if nzb_name[0] == b'/' || nzb_name[0] == b'\\' {
        return true;
    }
    nzb_name.len() >= 3
        && nzb_name[0].is_ascii_alphabetic()
        && nzb_name[1] == b':'
        && (nzb_name[2] == b'\\' || nzb_name[2] == b'/')
}

/// `Scanner::AddExternalFile` filename normalizer (leading `.` -> `_`, UTF-8-aware 200-byte truncation, `.nzb` suffix).
pub fn normalize_external_nzb_name(nzb_name: &[u8], valid_name: &[u8], is_nzb: bool) -> Vec<u8> {
    let mut name = Vec::with_capacity(valid_name.len() + 6);
    if valid_name.first() == Some(&b'.') {
        name.push(b'_');
    }
    name.extend_from_slice(valid_name);

    if name.len() > 200 {
        let ext = match name.iter().rposition(|&b| b == b'.') {
            Some(dot) if name.len() - dot <= 16 => name[dot..].to_vec(),
            _ => Vec::new(),
        };
        let mut cut = 200usize.saturating_sub(ext.len());
        while cut > 0 && (name[cut] & 0xC0) == 0x80 {
            cut -= 1;
        }
        name.truncate(cut);
        name.extend_from_slice(&ext);
    }

    if is_nzb {
        let has_nzb_ext = match nzb_name.iter().rposition(|&b| b == b'.') {
            Some(dot) => eq_ignore_ascii_case(&nzb_name[dot..], b".nzb"),
            None => false,
        };
        if !has_nzb_ext {
            name.extend_from_slice(b".nzb");
        }
    }

    name
}

/// Finds the last occurrence of `ch` in `text` and returns its byte index (`Some(pos)`), or `None`.
pub fn scanner_find_last_char_pos(text: &[u8], ch: u8) -> Option<usize> {
    text.iter().rposition(|&b| b == ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_next_pass_and_extensions() {
        assert_eq!(next_pass(60, 5, true, 0), (5, 1));
        assert_eq!(next_pass(60, 5, true, 3), (60, 0));
        assert!(can_process_extension(b"movie.nzb"));
        assert!(!can_process_extension(b"movie.nzb.queued"));
        assert!(!can_process_extension(b"movie.nzb.error"));
        assert!(!can_process_extension(b"movie.nzb.processed"));
    }

    #[test]
    fn test_normalize_external_name_and_xml_prefix() {
        assert!(is_nzb_xml_prefix(b"<?xml version=\"1.0\"?><nzb></nzb>"));
        assert_eq!(
            normalize_external_nzb_name(b"release", b".release", true),
            b"_.release.nzb"
        );
        assert!(is_absolute_nzb_name(b"/tmp/test.nzb"));
        assert!(is_absolute_nzb_name(b"C:\\temp\\test.nzb"));
        assert!(!is_absolute_nzb_name(b"[1/10] - Title.nzb"));
    }
}
