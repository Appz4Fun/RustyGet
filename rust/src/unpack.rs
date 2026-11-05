//! `DirectUnpack` (`daemon/postprocess/DirectUnpack.cpp`) and `UnpackController`
//! (`daemon/postprocess/UnpackController.cpp`) end-to-end Rust implementation.

pub use crate::postrepair::NzbgetRsUnpackMessageInfo;

#[inline]
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[inline]
fn c_atoi_bytes(bytes: &[u8]) -> i32 {
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut neg = false;
    if i < bytes.len() {
        if bytes[i] == b'-' {
            neg = true;
            i += 1;
        } else if bytes[i] == b'+' {
            i += 1;
        }
    }
    let mut val: i32 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        val = val
            .wrapping_mul(10)
            .wrapping_add(i32::from(bytes[i] - b'0'));
        i += 1;
    }
    if neg {
        val.wrapping_neg()
    } else {
        val
    }
}

/// `RarVolume::DetectRarVersion` and `UnpackController::FileHasRarSignature`:
/// Given the 8-byte file signature read from disk, returns `3` (RAR3/4), `5` (RAR5), or `0` (not RAR).
pub fn detect_rar_signature(sig: &[u8]) -> i32 {
    if sig.len() != 8 {
        return 0;
    }
    const RAR3: &[u8] = b"Rar!\x1a\x07\x00";
    const RAR5: &[u8] = b"Rar!\x1a\x07\x01\x00";
    let strcmp_eq = |s: &[u8]| s.iter().zip(sig.iter()).all(|(a, b)| a == b);
    let rar5 = strcmp_eq(RAR5);
    let rar3 = !rar5 && strcmp_eq(RAR3);
    if rar3 {
        3
    } else if rar5 {
        5
    } else {
        0
    }
}

/// `UnpackController::JoinFile` fragment continuity and size validation:
/// Returns `true` when the split fragments can be joined, or `false` if missing fragments are detected.
pub fn check_joinable_fragments(
    count: i32,
    min_seg: i32,
    max_seg: i32,
    dif_size_min: i32,
    par_success: bool,
) -> bool {
    let corrected_count = count - if min_seg == 0 { 1 } else { 0 };
    if min_seg > 1
        || corrected_count != max_seg
        || ((dif_size_min != corrected_count || dif_size_min > max_seg) && !par_success)
    {
        return false;
    }
    true
}

/// Parses an unpacker output line for both `DirectUnpack::AddMessage` (`is_seven_zip == false`)
/// and `UnpackController::AddMessage` (`is_seven_zip == false` for Unrar, `true` for 7-Zip).
pub fn parse_unpack_message(
    text: &[u8],
    unpack_extended_dir: &[u8],
    is_seven_zip: bool,
) -> NzbgetRsUnpackMessageInfo {
    let mut info = NzbgetRsUnpackMessageInfo::default();
    let len = text.len();

    if !is_seven_zip
        && text.starts_with(b"Unrar: Extracting  ")
        && text[19..].starts_with(unpack_extended_dir)
    {
        info.strip_extracting_dir = 1;
        info.stripped_suffix_offset = (19 + unpack_extended_dir.len() + 1).min(len);
    }

    if !is_seven_zip && text.starts_with(b"Unrar: Insert disk with") {
        if let Some(pos) = find_subslice(text, b" [C]ontinue, [Q]uit") {
            info.is_insert_disk = 1;
            if pos >= 24 {
                info.insert_disk_name_offset = 24;
                info.insert_disk_name_len = pos - 24;
            }
        }
    }

    if !is_seven_zip
        && info.strip_extracting_dir == 0
        && text.starts_with(b"Unrar: UNRAR ")
        && find_subslice(text, b" Copyright ").is_some()
        && find_subslice(text, b" Alexander Roshal").is_some()
    {
        info.reset_stage_time = 1;
    }

    if !is_seven_zip && (info.strip_extracting_dir != 0 || text.starts_with(b"Unrar: Extracting ")) {
        info.has_extracting_progress = 1;
    }

    if !is_seven_zip && text.starts_with(b"Unrar: Extracting from ") {
        info.has_extracting_from = 1;
    }

    if !is_seven_zip {
        if text.starts_with(&b"Unrar: Checksum error in the encrypted file"[..42])
            || text.starts_with(b"Unrar: CRC failed in the encrypted file")
        {
            info.decrypt_error = 1;
        }
        if text.starts_with(b"Unrar: The specified password is incorrect.") {
            info.password_error = 1;
            info.direct_password_error = 1;
        } else if text.starts_with(b"Unrar: Incorrect password for") {
            info.password_error = 1;
        }
    } else if find_subslice(text, b"Cannot open encrypted archive. Wrong password?").is_some()
        || find_subslice(text, b"Data Error in encrypted file. Wrong password?").is_some()
        || find_subslice(text, b"CRC Failed in encrypted file. Wrong password?").is_some()
    {
        info.decrypt_error = 1;
    }

    if find_subslice(text, b" : packed data CRC failed in volume").is_some()
        || find_subslice(text, b" : packed data checksum error in volume").is_some()
        || (len > 13 && text.ends_with(b" - CRC failed"))
        || (len > 18 && text.ends_with(b" - checksum failed"))
        || text.starts_with(b"Unrar: WARNING: You need to start extraction from a previous volume")
    {
        info.crc_or_volume_error = 1;
    }

    if (!is_seven_zip && text.starts_with(b"Unrar: All OK"))
        || (is_seven_zip && text.starts_with(b"7-Zip: Everything is Ok"))
    {
        info.all_ok = 1;
    }

    info
}

/// `DirectUnpack::CreateUnpackDir` and `UnpackController::CreateUnpackDir`:
/// `!inter_dir.is_empty() && dest_dir.starts_with(inter_dir) && dest_dir.get(inter_dir.len()) == Some(&sep)`.
pub fn use_inter_dir(dest_dir: &[u8], inter_dir: &[u8], sep: u8) -> bool {
    if inter_dir.is_empty() || dest_dir.len() <= inter_dir.len() {
        return false;
    }
    dest_dir.starts_with(inter_dir) && dest_dir[inter_dir.len()] == sep
}

/// `DirectUnpack::IsMainArchive`:
/// Matches `RegEx regExRarPart(".*\\.part([0-9]+)\\.rar$"); return !regExRarPart.Match(filename) || atoi(...) == 1;`.
pub fn is_main_archive(filename: &[u8]) -> bool {
    if filename.len() < 10 {
        return true;
    }
    let rar_pos = filename.len() - 4;
    if !filename[rar_pos..].eq_ignore_ascii_case(b".rar") {
        return true;
    }
    let before_rar = &filename[..rar_pos];
    let mut d_start = before_rar.len();
    while d_start > 0 && before_rar[d_start - 1].is_ascii_digit() {
        d_start -= 1;
    }
    if d_start == before_rar.len() || d_start < 5 {
        return true;
    }
    if !before_rar[d_start - 5..d_start].eq_ignore_ascii_case(b".part") {
        return true;
    }
    let digits = &before_rar[d_start..];
    let mut val: i64 = 0;
    for &b in digits {
        val = val.saturating_mul(10).saturating_add(i64::from(b - b'0'));
    }
    val == 1
}

/// `DirectUnpack::ReadLine` prompt check.
pub fn is_insert_disk_prompt(buf: &[u8], i: usize, ch: i32) -> bool {
    if i <= 35 || ch != i32::from(b' ') {
        return false;
    }
    if !buf.starts_with(b"Insert disk with") {
        return false;
    }
    let needle = b" [C]ontinue, [Q]uit ";
    buf.windows(needle.len()).any(|w| w == needle)
}

/// Executes one character step of `DirectUnpack::ReadLine`:
/// updates `buf` and `backspace`, and returns `(final_i_if_break, should_break)`.
pub fn direct_unpack_readline_step(
    buf: &mut [u8],
    i: usize,
    ch: i32,
    backspace: &mut bool,
) -> (usize, bool) {
    if i + 1 >= buf.len() {
        return (i, true);
    }
    if ch == i32::from(b'\x08') {
        *backspace = true;
        buf[i] = 0;
    }
    if !*backspace {
        buf[i] = ch as u8;
        buf[i + 1] = 0;
    }
    if ch == -1 {
        return (i, true);
    }
    if ch == i32::from(b'\n') {
        return (i + 1, true);
    }
    if i > 35 && ch == i32::from(b' ') {
        let nul_end = buf[..=i + 1]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(i + 1);
        if is_insert_disk_prompt(&buf[..nul_end], i, ch) {
            return (i + 1, true);
        }
    }
    (i, false)
}

/// `UnpackController::CheckArchiveFiles` archive filename classification:
/// - `1`: `.*\\.rar$` (case-insensitive)
/// - `2`: `.*\\.7z$` (case-insensitive)
/// - `3`: `.*\\.7z\\.[0-9]+$` (case-insensitive)
/// - `4`: `.*\\.[a-z,0-9]{3}\\.[0-9]{3}$` with `extNum == 0 || extNum == 1`
/// - `5`: `.*\\.[r-z][0-9][0-9]$`
/// - `6`: `.*\\.[a-z,0-9]{3}\\.[0-9]{3}$` with `extNum > 1`
/// - `0`: other file
pub fn classify_archive_file(filename: &[u8]) -> i32 {
    if filename.len() >= 4 && filename[filename.len() - 4..].eq_ignore_ascii_case(b".rar") {
        return 1;
    }
    if filename.len() >= 3 && filename[filename.len() - 3..].eq_ignore_ascii_case(b".7z") {
        return 2;
    }
    if let Some(dot_pos) = filename.iter().rposition(|&b| b == b'.') {
        let ext = &filename[dot_pos + 1..];
        let before = &filename[..dot_pos];
        if !ext.is_empty()
            && ext.iter().all(|b| b.is_ascii_digit())
            && before.len() >= 3
            && before[before.len() - 3..].eq_ignore_ascii_case(b".7z")
        {
            return 3;
        }
        if ext.len() == 3 && ext.iter().all(|b| b.is_ascii_digit()) && before.len() >= 4 {
            let prev_ext = &before[before.len() - 4..];
            if prev_ext[0] == b'.'
                && prev_ext[1..].iter().all(|&b| {
                    b.is_ascii_lowercase()
                        || b.is_ascii_uppercase()
                        || b.is_ascii_digit()
                        || b == b','
                })
            {
                let ext_num = (i32::from(ext[0] - b'0') * 100)
                    + (i32::from(ext[1] - b'0') * 10)
                    + i32::from(ext[2] - b'0');
                return if ext_num == 0 || ext_num == 1 { 4 } else { 6 };
            }
        }
        if ext.len() == 3 {
            let c0 = ext[0].to_ascii_lowercase();
            if (b'r'..=b'z').contains(&c0) && ext[1].is_ascii_digit() && ext[2].is_ascii_digit() {
                return 5;
            }
        }
    }
    0
}

/// `UnpackController::HasCompletedArchiveFiles` filename check:
/// `.*\\.rar$`, `.*\\.7z$`, or `.*\\.7z\\.[0-9]+$`.
pub fn has_completed_archive_ext(filename: &[u8]) -> bool {
    matches!(classify_archive_file(filename), 1..=3)
}

/// `UnpackController::JoinSplittedFiles`:
/// Matches `.*\.[a-z,0-9]{3}\.001$`.
pub fn is_split_first_piece(filename: &[u8]) -> bool {
    if filename.len() < 8 {
        return false;
    }
    let tail = &filename[filename.len() - 8..];
    if tail[0] != b'.' || &tail[4..8] != b".001" {
        return false;
    }
    tail[1..4]
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b',')
}

/// Updates `(count, min, max, dif_size_min)` when scanning a split file piece in `UnpackController::JoinFile`.
/// Returns `true` if `filename` belongs to `dest_base_name` (case-insensitive prefix + `.NNN` where `N` are digits).
#[allow(clippy::too_many_arguments)]
pub fn accumulate_split_piece(
    filename: &[u8],
    dest_base_name: &[u8],
    segment_size: i64,
    first_segment_size: i64,
    count: &mut i32,
    min: &mut i32,
    max: &mut i32,
    dif_size_min: &mut i32,
) -> bool {
    let base_len = dest_base_name.len();
    if filename.len() != base_len + 4 {
        return false;
    }
    if !filename[..base_len].eq_ignore_ascii_case(dest_base_name) {
        return false;
    }
    if filename[base_len] != b'.' {
        return false;
    }
    let ext = &filename[base_len + 1..];
    if !ext.iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if base_len < 4 || dest_base_name[base_len - 4] != b'.' {
        return false;
    }
    let prev_ext = &dest_base_name[base_len - 3..];
    if !prev_ext
        .iter()
        .all(|b| b.is_ascii_alphabetic() || b.is_ascii_digit() || *b == b',')
    {
        return false;
    }

    let seg_num =
        ((ext[0] - b'0') as i32) * 100 + ((ext[1] - b'0') as i32) * 10 + ((ext[2] - b'0') as i32);
    *count = count.wrapping_add(1);
    if *min == -1 || seg_num < *min {
        *min = seg_num;
    }
    if seg_num > *max {
        *max = seg_num;
    }
    if segment_size != first_segment_size && seg_num < *dif_size_min {
        *dif_size_min = seg_num;
    }
    true
}

/// Evaluates whether a file in `m_destDir` should be deleted during `UnpackController::Cleanup` (`UnpackCleanupDisk`).
pub fn classify_cleanup_archive_file(
    is_dir: bool,
    inter_dir: bool,
    in_extracted_files: bool,
    is_downloaded_file: bool,
    has_splitted_files: bool,
    in_joined_files: bool,
    filename: &[u8],
) -> i32 {
    if is_dir || (!inter_dir && in_extracted_files) || !is_downloaded_file {
        return 0;
    }
    let len = filename.len();
    if len >= 4 && filename[len - 4..].eq_ignore_ascii_case(b".rar") {
        return 1;
    }
    if len >= 3 && filename[len - 3..].eq_ignore_ascii_case(b".7z") {
        return 1;
    }
    if let Some(dot_pos) = filename.iter().rposition(|&b| b == b'.') {
        let digits = &filename[dot_pos + 1..];
        if !digits.is_empty()
            && digits.iter().all(|b| b.is_ascii_digit())
            && dot_pos >= 3
            && filename[dot_pos - 3..dot_pos].eq_ignore_ascii_case(b".7z")
        {
            return 1;
        }
    }
    if len >= 4
        && filename[len - 4] == b'.'
        && (b'r'..=b'z').contains(&filename[len - 3].to_ascii_lowercase())
        && filename[len - 2].is_ascii_digit()
        && filename[len - 1].is_ascii_digit()
    {
        return 2;
    }
    if has_splitted_files
        && in_joined_files
        && len >= 8
        && filename[len - 4] == b'.'
        && filename[len - 8] == b'.'
        && filename[len - 3..].iter().all(|b| b.is_ascii_digit())
        && filename[len - 7..len - 4]
            .iter()
            .all(|b| b.is_ascii_alphabetic() || b.is_ascii_digit() || *b == b',')
    {
        return 1;
    }
    0
}

/// Executes one character step of `UnpackController::ReadLine`.
pub fn unpack_ctrl_readline_step(
    buf: &mut [u8],
    idx: &mut usize,
    ch: i32,
    printed: &mut bool,
    last_bs: &mut isize,
    process_output_len: &mut isize,
    progress: &mut i32,
) -> bool {
    *process_output_len = -1;
    *progress = -1;
    let i = *idx;
    if i + 1 >= buf.len() {
        return true;
    }
    buf[i] = ch as u8;
    buf[i + 1] = 0;
    if ch == -1 {
        return true;
    }
    if ch == i32::from(b'\n') {
        *idx = i + 1;
        return true;
    }
    if buf[i] == b'\x08' {
        *last_bs = i as isize;
    } else if buf[i] == 0 {
        let nul_end = buf[..=i].iter().position(|&b| b == 0).unwrap_or(i + 1);
        *last_bs = buf[..nul_end]
            .iter()
            .rposition(|&b| b == b'\x08')
            .map(|p| p as isize)
            .unwrap_or(-1);
    }
    if *last_bs >= 0 {
        let bs_pos = *last_bs as usize;
        let nul_end = buf[..=i].iter().position(|&b| b == 0).unwrap_or(i + 1);
        if bs_pos < nul_end {
            if !*printed {
                let line = &buf[..nul_end];
                if !line.starts_with(b"...") {
                    *process_output_len = bs_pos.min(1023) as isize;
                }
                *printed = true;
            }
            let bs_slice = &buf[bs_pos..nul_end];
            if bs_slice.contains(&b'%') {
                let after_bs = bs_slice.get(1..).unwrap_or(&[]);
                let percent = c_atoi_bytes(after_bs);
                *progress = percent.wrapping_mul(10);
            }
        }
    }
    *idx = i + 1;
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rar_signature_and_archive_classification() {
        assert_eq!(detect_rar_signature(b"Rar!\x1a\x07\x00\x99"), 3);
        assert_eq!(detect_rar_signature(b"Rar!\x1a\x07\x01\x00"), 5);
        assert_eq!(detect_rar_signature(b"PK\x03\x04\x00\x00\x00\x00"), 0);

        assert!(is_main_archive(b"movie.part001.rar"));
        assert!(!is_main_archive(b"movie.part002.rar"));
        assert!(is_main_archive(b"movie.rar"));

        assert_eq!(classify_archive_file(b"movie.rar"), 1);
        assert_eq!(classify_archive_file(b"movie.7z"), 2);
        assert_eq!(classify_archive_file(b"movie.7z.001"), 3);
        assert_eq!(classify_archive_file(b"movie.mkv.001"), 4);
        assert_eq!(classify_archive_file(b"movie.r00"), 5);
        assert_eq!(classify_archive_file(b"movie.mkv.002"), 6);
    }

    #[test]
    fn test_split_pieces_and_joinable() {
        assert!(is_split_first_piece(b"movie.mkv.001"));
        assert!(!is_split_first_piece(b"movie.mkv.002"));

        let mut count = 0;
        let mut min = -1;
        let mut max = -1;
        let mut dif_size_min = 999999;
        assert!(accumulate_split_piece(
            b"movie.mkv.001",
            b"movie.mkv",
            1000,
            1000,
            &mut count,
            &mut min,
            &mut max,
            &mut dif_size_min
        ));
        assert!(accumulate_split_piece(
            b"movie.mkv.002",
            b"movie.mkv",
            500,
            1000,
            &mut count,
            &mut min,
            &mut max,
            &mut dif_size_min
        ));
        assert!(check_joinable_fragments(count, min, max, dif_size_min, false));
    }
}
