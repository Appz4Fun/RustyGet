//! `DirectUnpack` (`daemon/postprocess/DirectUnpack.cpp`) and `UnpackController`
//! (`daemon/postprocess/UnpackController.cpp`) end-to-end Rust implementation.

use std::ffi::{c_char, c_int, CStr};

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

/// Matches the overwrite mode switch in `Unrar::MakeArgs()`:
/// `0 (Skip) -> "-o-"`, `1 (Overwrite) -> "-o+"`, `2 (AutoRename) -> "-or"`.
pub fn unrar_overwrite_switch(mode: i32) -> &'static std::ffi::CStr {
    match mode {
        0 => c"-o-",
        1 => c"-o+",
        2 => c"-or",
        _ => c"-o-",
    }
}

/// Matches the `.part` / `\\.part0*1\\.rar$` check in `Unrar::IsSupported` after
/// `FileTypes::IsRarExt` has already returned `true`.
pub fn unrar_is_part1_or_unsplit_rar(filename: &[u8]) -> bool {
    let has_part = filename
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b".part"));
    if !has_part {
        return true;
    }
    if filename.len() < 10 {
        return false;
    }
    let suffix_start = filename.len() - 4;
    if !filename[suffix_start..].eq_ignore_ascii_case(b".rar") {
        return false;
    }
    let before_rar = &filename[..suffix_start];
    if !before_rar.ends_with(b"1") {
        return false;
    }
    let mut i = before_rar.len() - 1;
    while i > 0 && before_rar[i - 1] == b'0' {
        i -= 1;
    }
    if i < 5 {
        return false;
    }
    before_rar[i - 5..i].eq_ignore_ascii_case(b".part")
}

/// Matches `Unrar::DecodeExitCode(ec)`:
/// Returns `(is_success, known_error_msg)`.
pub fn unrar_decode_exit_code(ec: i32) -> (bool, &'static std::ffi::CStr) {
    match ec {
        0 => (true, c""),
        1 => (
            false,
            c"Extraction finished, but some files might be missing or incomplete",
        ),
        2 => (
            false,
            c"The process could not start or was interrupted unexpectedly",
        ),
        3 => (
            false,
            c"The archive is damaged. The extracted files are likely corrupt",
        ),
        4 => (
            false,
            c"This archive is locked and cannot be modified or unpacked",
        ),
        5 => (
            false,
            c"Could not write files to the destination. Please check your permissions and disk space",
        ),
        6 => (
            false,
            c"Couldn't open the archive. The file may be missing or you don't have permission to read it",
        ),
        7 => (false, c"An internal program error occurred"),
        8 => (
            false,
            c"Your computer ran out of memory. Please try closing other applications first",
        ),
        9 => (
            false,
            c"Couldn't create files in the destination folder. Please check your permissions",
        ),
        10 => (
            false,
            c"There were no files inside the archive to extract",
        ),
        11 => (false, c"The password you entered was incorrect"),
        _ => (false, c""),
    }
}

/// Matches the overwrite mode switch in `SevenZip::MakeArgs()`:
/// `0 (Skip) -> "-aos"`, `1 (Overwrite) -> "-aoa"`, `2 (AutoRename) -> "-aou"`.
pub fn sevenzip_overwrite_switch(mode: i32) -> &'static std::ffi::CStr {
    match mode {
        0 => c"-aos",
        1 => c"-aoa",
        2 => c"-aou",
        _ => c"-aos",
    }
}

/// Matches `SevenZip::DecodeExitCode(ec)`:
/// Returns `(is_success, known_error_msg)`.
pub fn sevenzip_decode_exit_code(ec: i32) -> (bool, &'static std::ffi::CStr) {
    match ec {
        0 => (true, c""),
        1 => (
            false,
            c"Completed with warnings. Some files may have been skipped because they were in use",
        ),
        2 => (
            false,
            c"A fatal error occurred. Check for permission issues, a corrupt archive, password, disk space",
        ),
        7 => (false, c"Command line error"),
        8 => (false, c"Not enough memory for operation"),
        255 => (false, c"User stopped the process"),
        _ => (false, c""),
    }
}

/// Format `"<prefix> for <name>"` (e.g. `"unpack for MyNzb"`, `"Unpack for MyNzb"`)
/// for post-processing controllers (`DirectUnpack.cpp`, `UnpackController.cpp`,
/// `Cleanup.cpp`, `Rename.cpp`).
pub fn post_format_info_name(prefix: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + 5 + name.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(b" for ");
    out.extend_from_slice(name);
    out
}

/// Strips the last `.ext` from `frag_base_name` for `UnpackController::JoinFile`.
pub fn unpack_ctrl_strip_last_extension(frag_base_name: &[u8]) -> Vec<u8> {
    match frag_base_name.iter().rposition(|&b| b == b'.') {
        Some(idx) => frag_base_name[..idx].to_vec(),
        None => frag_base_name.to_vec(),
    }
}

/// Uppercases the first byte of `buf` in-place if it is an ASCII lowercase letter (`a..=z`).
pub fn post_capitalize_first_ascii(buf: &mut [u8]) {
    if let Some(first) = buf.first_mut() {
        *first = first.to_ascii_uppercase();
    }
}

fn path_filename(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/' || b == b'\\') {
        Some(pos) => &path[pos + 1..],
        None => path,
    }
}

/// `Unpack::SevenZip::IsSupported(const fs::path& path)`.
pub fn sevenzip_is_supported(path: &[u8]) -> bool {
    let filename = path_filename(path);
    if filename.is_empty() || filename == b"." || filename == b".." || !filename[1..].contains(&b'.') {
        return false;
    }
    crate::filetypes::is_seven_zip_file(filename)
}

/// `Unpack::Unrar::IsSupported(const fs::path& path)`.
pub fn unrar_is_supported(path: &[u8]) -> bool {
    let filename = path_filename(path);
    if filename.is_empty() || filename == b"." || filename == b".." {
        return false;
    }
    let ext = match filename.iter().rposition(|&b| b == b'.') {
        Some(pos) if pos > 0 => &filename[pos..],
        _ => b"",
    };
    if !crate::filetypes::is_rar_ext(ext) {
        return false;
    }
    unrar_is_part1_or_unsplit_rar(filename)
}

/// `Unpack::ExtractorBase::MakePassword()`.
pub fn extractor_make_password(password: &[u8]) -> Vec<u8> {
    if password.is_empty() {
        b"-p-".to_vec()
    } else {
        let mut out = Vec::with_capacity(2 + password.len());
        out.extend_from_slice(b"-p");
        out.extend_from_slice(password);
        out
    }
}

// ============================================================================
// ZIP Content Identification (`zipview`) & Native Archive Extraction (`zipfast`)
// ============================================================================

/// High-level classification of what is inside a ZIP container, determined by [`inspect_zip_bytes`].
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZipContentKind {
    /// Archive contains no file entries.
    Empty = 0,
    /// Archive contains one or more `.nzb` files (for `Incoming::ArchiveProcessor` / `Scanner`).
    NzbArchive = 1,
    /// Archive contains an NZBGet extension package (`manifest.json`).
    ExtensionPackage = 2,
    /// Archive contains video, audio, or disc-image media files.
    MediaArchive = 3,
    /// Archive contains nested archives or parity files (`.rar`, `.7z`, `.zip`, `.par2`, etc.).
    NestedArchive = 4,
    /// ZIP-based document or application container (`.epub`, `.docx`/`.xlsx`/`.pptx`, `.odt`/`.ods`/`.odp`, `.cbz`, `.jar`/`.apk`).
    DocumentContainer = 5,
    /// Archive contains other regular files.
    GeneralFiles = 6,
}

/// Metadata for a single entry inside a ZIP archive inspected by `zipview`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntryInfo {
    pub name: String,
    pub uncompressed_size: u64,
    pub compressed_size: u64,
    pub compression_method: u16,
    pub effective_compression: u16,
    pub compression_name: &'static str,
    pub crc32: u32,
    pub local_header_offset: u64,
    pub is_dir: bool,
    pub is_encrypted: bool,
    pub is_symlink: bool,
}

/// Summary of a ZIP archive's contents produced by [`inspect_zip_bytes`] / [`inspect_zip_file`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipArchiveInspection {
    pub kind: ZipContentKind,
    pub entries: Vec<ZipEntryInfo>,
    pub file_count: usize,
    pub dir_count: usize,
    pub total_uncompressed_size: u64,
    pub total_compressed_size: u64,
    pub has_encrypted_entries: bool,
    pub has_compressed_entries: bool,
    pub has_symlinks: bool,
    pub has_nzb_files: bool,
    /// `(entry_name, uncompressed_size, is_compressed)` of the largest regular file entry.
    pub largest_file: Option<(String, u64, bool)>,
}

fn is_image_ext(ext: &[u8]) -> bool {
    matches!(
        ext,
        b".jpg" | b".jpeg" | b".png" | b".webp" | b".gif" | b".bmp" | b".avif" | b".jxl"
    )
}

fn entry_base_and_lower_ext(name: &str) -> (&str, Vec<u8>) {
    let trimmed = name.trim_end_matches(['/', '\\']);
    let base = trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed);
    let ext = match base.rfind('.') {
        Some(pos) if pos > 0 => base[pos..].bytes().map(|b| b.to_ascii_lowercase()).collect(),
        _ => Vec::new(),
    };
    (base, ext)
}

fn inspect_zip_entries<F>(
    raw_entries: &[zipview::ZipEntry],
    mut read_entry_str: F,
) -> ZipArchiveInspection
where
    F: FnMut(&zipview::ZipEntry) -> Option<String>,
{
    let mut entries = Vec::with_capacity(raw_entries.len());
    let mut file_count = 0usize;
    let mut dir_count = 0usize;
    let mut total_uncompressed_size = 0u64;
    let mut total_compressed_size = 0u64;
    let mut has_encrypted_entries = false;
    let mut has_compressed_entries = false;
    let mut has_symlinks = false;
    let mut has_nzb_files = false;
    let mut has_extension_manifest = false;
    let mut has_media_files = false;
    let mut has_nested_archives = false;
    let mut has_document_marker = false;
    let mut image_file_count = 0usize;
    let mut largest_file: Option<(String, u64, bool)> = None;

    if raw_entries.iter().any(|e| {
        matches!(
            e.name(),
            "[Content_Types].xml"
                | "META-INF/container.xml"
                | "META-INF/manifest.xml"
                | "META-INF/MANIFEST.MF"
                | "AndroidManifest.xml"
        )
    }) {
        has_document_marker = true;
    } else if let Some(mime_entry) = raw_entries.iter().find(|e| e.name() == "mimetype") {
        if !mime_entry.is_encrypted() && mime_entry.size() <= 4096 {
            if let Some(mime) = read_entry_str(mime_entry) {
                let trimmed = mime.trim();
                if trimmed.starts_with("application/epub+zip")
                    || trimmed.starts_with("application/vnd.oasis.opendocument")
                {
                    has_document_marker = true;
                }
            }
        }
    }

    for entry in raw_entries {
        let is_dir = entry.is_dir()
            || entry
                .unix_mode()
                .is_some_and(|m| (m & 0o170000) == 0o040000);
        let is_symlink = entry
            .unix_mode()
            .is_some_and(|m| (m & 0o170000) == 0o120000);
        let is_encrypted = entry.is_encrypted();
        let eff_comp = entry.effective_compression();
        let is_compressed = !is_dir && eff_comp != 0;

        has_symlinks |= is_symlink;
        has_encrypted_entries |= is_encrypted;

        if is_dir {
            dir_count += 1;
        } else {
            file_count += 1;
            total_uncompressed_size = total_uncompressed_size.saturating_add(entry.size());
            total_compressed_size = total_compressed_size.saturating_add(entry.compressed_size());
            has_compressed_entries |= is_compressed;

            let (base, ext) = entry_base_and_lower_ext(entry.name());
            if base.eq_ignore_ascii_case("manifest.json") {
                if is_encrypted {
                    has_extension_manifest = true;
                } else if entry.size() <= 64 * 1024 {
                    if let Some(manifest_text) = read_entry_str(entry) {
                        if manifest_text.contains("\"name\"") && manifest_text.contains("\"main\"")
                        {
                            has_extension_manifest = true;
                        }
                    }
                }
            }
            if ext == b".nzb" {
                has_nzb_files = true;
            } else if crate::filetypes::is_video_ext(&ext)
                || crate::filetypes::is_audio_ext(&ext)
                || crate::filetypes::is_disc_structure_ext(&ext)
                || crate::filetypes::is_disc_descriptor_ext(&ext)
                || ext == b".iso"
                || ext == b".img"
            {
                has_media_files = true;
            } else if crate::filetypes::is_rar_ext(&ext)
                || crate::filetypes::is_seven_zip_ext(&ext)
                || ext == b".par2"
            {
                has_nested_archives = true;
            } else if is_image_ext(&ext) {
                image_file_count += 1;
            }

            if !is_symlink
                && largest_file
                    .as_ref()
                    .is_none_or(|(_, max_sz, _)| entry.size() > *max_sz)
            {
                largest_file = Some((base.to_string(), entry.size(), is_compressed));
            }
        }

        entries.push(ZipEntryInfo {
            name: entry.name().to_string(),
            uncompressed_size: entry.size(),
            compressed_size: entry.compressed_size(),
            compression_method: entry.compression(),
            effective_compression: eff_comp,
            compression_name: entry.compression_name(),
            crc32: entry.crc32(),
            local_header_offset: entry.local_header_offset(),
            is_dir,
            is_encrypted,
            is_symlink,
        });
    }

    let kind = if file_count == 0 {
        ZipContentKind::Empty
    } else if has_extension_manifest {
        ZipContentKind::ExtensionPackage
    } else if has_nzb_files {
        ZipContentKind::NzbArchive
    } else if has_document_marker || (image_file_count == file_count && file_count >= 2) {
        ZipContentKind::DocumentContainer
    } else if has_media_files {
        ZipContentKind::MediaArchive
    } else if has_nested_archives {
        ZipContentKind::NestedArchive
    } else {
        ZipContentKind::GeneralFiles
    };

    ZipArchiveInspection {
        kind,
        entries,
        file_count,
        dir_count,
        total_uncompressed_size,
        total_compressed_size,
        has_encrypted_entries,
        has_compressed_entries,
        has_symlinks,
        has_nzb_files,
        largest_file,
    }
}

#[cfg(unix)]
fn read_file_range_at(
    file: &mut std::fs::File,
    offset: u64,
    buf: &mut [u8],
) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buf, offset)
}

#[cfg(not(unix))]
fn read_file_range_at(
    file: &mut std::fs::File,
    offset: u64,
    buf: &mut [u8],
) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(buf)
}

/// Inspects an in-memory ZIP archive using `zipview::ZipReader`, enumerating all entries
/// and classifying the archive's payload (`ZipContentKind`). Small metadata payloads
/// (`mimetype` `<= 4 KiB` and `manifest.json` `<= 64 KiB`) are decoded lossily to identify
/// document containers and NZBGet extension packages.
pub fn inspect_zip_bytes(bytes: &[u8]) -> Result<ZipArchiveInspection, zipview::ZipReadError> {
    let reader = zipview::ZipReader::new(bytes)?;
    Ok(inspect_zip_entries(reader.entries(), |entry| {
        reader
            .read_entry(entry)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }))
}

/// Inspects an already-opened `zipview::ZipIndex`, fetching only small metadata entries
/// (`mimetype` `<= 4 KiB` or `manifest.json` `<= 64 KiB`) on demand via `read` and decoding
/// them lossily.
pub fn inspect_zip_index<E: From<zipview::ZipReadError>>(
    index: &zipview::ZipIndex,
    mut read: impl FnMut(u64, &mut [u8]) -> Result<(), E>,
) -> ZipArchiveInspection {
    inspect_zip_entries(index.entries(), |entry| {
        index
            .fetch_entry(entry, None, &mut read)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    })
}

/// Opens and inspects a ZIP archive of `total_len` bytes a byte range at a time using
/// `zipview::ZipIndex::open`, without loading the whole archive into memory.
pub fn inspect_zip_range<E: From<zipview::ZipReadError>>(
    total_len: u64,
    mut read: impl FnMut(u64, &mut [u8]) -> Result<(), E>,
) -> Result<ZipArchiveInspection, E> {
    let index = zipview::ZipIndex::open(total_len, &mut read)?;
    Ok(inspect_zip_index(&index, &mut read))
}

/// Opens a `zipview::ZipIndex` step-by-step using `zipview::ZipIndexOpener` (`next_read` /
/// `supply` / `finish`), suitable for callers that fetch byte ranges on demand.
/// Returns `ZipReadError::Truncated` if `fetch_range` returns an empty slice for a requested range.
pub fn open_zip_index_staged<E: From<zipview::ZipReadError>>(
    total_len: u64,
    mut fetch_range: impl FnMut(std::ops::Range<u64>) -> Result<Vec<u8>, E>,
) -> Result<zipview::ZipIndex, E> {
    let mut opener = zipview::ZipIndexOpener::new(total_len);
    while let Some(range) = opener.next_read()? {
        let bytes = fetch_range(range.clone())?;
        if bytes.is_empty() {
            return Err(zipview::ZipReadError::Truncated.into());
        }
        opener.supply(range.start, &bytes)?;
    }
    Ok(opener.finish()?)
}

/// Inspects a ZIP file's central directory via bounded range reads (`zipview::ZipIndex`)
/// without loading the whole archive into memory.
pub fn inspect_zip_file_via_central_directory(
    path: &std::path::Path,
) -> Result<ZipArchiveInspection, String> {
    inspect_zip_file(path)
}

/// Inspects a ZIP file on disk of any size using `zipview::ZipIndex` range reads.
pub fn inspect_zip_file(path: &std::path::Path) -> Result<ZipArchiveInspection, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let total_len = file.metadata().map_err(|e| e.to_string())?.len();
    inspect_zip_range(total_len, |offset, buf| {
        read_file_range_at(&mut file, offset, buf)
    })
    .map_err(|e| e.to_string())
}

/// Verifies whether `password` satisfies the encryption header check of encrypted entries
/// in an in-memory ZIP archive using `zipview::PasswordCheck`.
/// Returns `Ok(true)` if the archive has no encrypted entries or `password` is accepted by all
/// verifiable encrypted entries; `Ok(false)` if rejected.
pub fn verify_zip_password_bytes(
    bytes: &[u8],
    password: &str,
) -> Result<bool, zipview::ZipReadError> {
    let reader = zipview::ZipReader::new(bytes)?;
    for entry in reader.entries() {
        if entry.is_encrypted() {
            if password.is_empty() {
                return Ok(false);
            }
            if let Some(check) = reader.password_check(entry) {
                if !check.accepts(password) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// Verifies whether `password` satisfies the encryption header check of encrypted entries
/// in a `zipview::ZipIndex` using two small range reads per encrypted entry (`fetch_password_check`).
pub fn verify_zip_index_password<E: From<zipview::ZipReadError>>(
    index: &zipview::ZipIndex,
    password: &str,
    mut read: impl FnMut(u64, &mut [u8]) -> Result<(), E>,
) -> Result<bool, E> {
    for entry in index.entries() {
        if entry.is_encrypted() {
            if password.is_empty() {
                return Ok(false);
            }
            if let Some(check) = index.fetch_password_check(entry, &mut read)? {
                if !check.accepts(password) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// Verifies whether `password` is accepted for a ZIP file on disk of any size using
/// `zipview::ZipIndex::fetch_password_check` (two small reads per encrypted entry).
pub fn verify_zip_password_file(path: &std::path::Path, password: &str) -> Result<bool, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let total_len = file.metadata().map_err(|e| e.to_string())?.len();
    let mut read = |offset: u64, buf: &mut [u8]| -> std::io::Result<()> {
        read_file_range_at(&mut file, offset, buf)
    };
    let index = zipview::ZipIndex::open(total_len, &mut read).map_err(|e| e.to_string())?;
    verify_zip_index_password(&index, password, &mut read).map_err(|e| e.to_string())
}

/// Reads and decompresses a single named entry from an in-memory ZIP archive using `zipview`.
pub fn read_zip_entry_bytes(
    bytes: &[u8],
    name: &str,
    password: Option<&str>,
) -> Result<Vec<u8>, zipview::ZipReadError> {
    let reader = zipview::ZipReader::new(bytes)?;
    reader.read_by_name_with_password(name, password)
}

/// Reads and decompresses a single named entry from a ZIP file on disk using `zipview::ZipIndex`
/// (opening the central directory, then two range reads for the entry's local header and data).
pub fn read_zip_entry_file(
    path: &std::path::Path,
    name: &str,
    password: Option<&str>,
) -> Result<Vec<u8>, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let total_len = file.metadata().map_err(|e| e.to_string())?.len();
    let mut read = |offset: u64, buf: &mut [u8]| -> std::io::Result<()> {
        read_file_range_at(&mut file, offset, buf)
    };
    let index = zipview::ZipIndex::open(total_len, &mut read).map_err(|e| e.to_string())?;
    let entry = index
        .by_name(name)
        .ok_or_else(|| zipview::ZipReadError::EntryNotFound.to_string())?;
    index
        .fetch_entry(entry, password, &mut read)
        .map_err(|e| e.to_string())
}

fn has_suffix_icase(path: &[u8], suffix: &[u8]) -> bool {
    path.len() >= suffix.len() && path[path.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// Returns `true` if `path` has an archive filename extension natively supported by `zipfast`
/// without needing to inspect magic bytes on disk (`.zip`, `.cbz`, `.tar`, `.gz`, `.tgz`,
/// `.bz2`, `.tbz2`, `.tbz`, `.xz`, `.txz`, `.zst`, `.tzst`).
pub fn is_zipfast_supported_extension(path: &[u8]) -> bool {
    has_suffix_icase(path, b".zip")
        || has_suffix_icase(path, b".cbz")
        || has_suffix_icase(path, b".tar")
        || has_suffix_icase(path, b".gz")
        || has_suffix_icase(path, b".tgz")
        || has_suffix_icase(path, b".bz2")
        || has_suffix_icase(path, b".tbz2")
        || has_suffix_icase(path, b".tbz")
        || has_suffix_icase(path, b".xz")
        || has_suffix_icase(path, b".txz")
        || has_suffix_icase(path, b".zst")
        || has_suffix_icase(path, b".tzst")
}

#[cfg(unix)]
fn path_to_bytes(path: &std::path::Path) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes()
}

#[cfg(not(unix))]
fn path_to_bytes(path: &std::path::Path) -> &[u8] {
    path.to_str().map(str::as_bytes).unwrap_or(b"")
}

/// Returns `true` if `path` can be extracted natively in-process by `zipfast`.
///
/// - `.7z` archives (or files with the 7z signature) return `false` so `SevenZip` handles them.
/// - `.rar` archives return `true` when the file on disk has a RAR 5.0 header (`Rar!\x1a\x07\x01\x00`),
///   and `false` for RAR 1.5–4.x (`Rar!\x1a\x07\x00`) so `Unrar` handles legacy RAR volumes.
/// - `.zip`, `.tar`, `.gz`, `.bz2`, `.xz`, `.tgz`, `.tbz2`, `.txz`, `.zst`, `.tzst` return `true`.
pub fn can_extract_with_zipfast(path: &std::path::Path) -> bool {
    let bytes = path_to_bytes(path);
    let fname = path_filename(bytes);
    if matches!(classify_archive_file(fname), 2 | 3) {
        return false;
    }
    if let Ok(mut file) = std::fs::File::open(path) {
        use std::io::Read;
        let mut head = [0u8; 512];
        let n = file.read(&mut head).unwrap_or(0);
        if n >= 6 && head[..6] == [b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c] {
            return false;
        }
        if n >= 6 && &head[..6] == b"Rar!\x1a\x07" {
            return n >= 8 && detect_rar_signature(&head[..8]) == 5;
        }
        if matches!(classify_archive_file(fname), 1 | 5) {
            return false;
        }
        if is_zipfast_supported_extension(bytes) {
            return true;
        }
        let slice = &head[..n];
        return zipfast::zip::sniff(slice)
            || zipfast::stream::Codec::sniff(slice).is_some()
            || zipfast::tar::sniff_tar(slice);
    }
    is_zipfast_supported_extension(bytes)
}

/// Lists archive entries using `zipfast::extract::list_with_password`.
pub fn list_archive_with_zipfast(
    path: &std::path::Path,
    password: Option<&str>,
) -> Result<Vec<zipfast::extract::Entry>, String> {
    let pw = password.filter(|s| !s.is_empty());
    zipfast::extract::list_with_password(path, pw).map_err(|e| e.to_string())
}

/// Extracts `archive_path` into `output_dir` natively in-process using `zipfast` (with pre-inspection
/// via `zipview` for ZIP containers).
///
/// `overwrite_mode`: `0 = Skip`, `1 = Overwrite`, `2 = AutoRename`.
/// `log_cb(kind, message)` uses `Message::EKind` values (`0 = mkInfo`, `1 = mkError`, `2 = mkWarning`, `4 = mkDetail`).
pub fn extract_archive_with_zipfast<F>(
    archive_path: &std::path::Path,
    output_dir: &std::path::Path,
    password: &str,
    overwrite_mode: i32,
    mut log_cb: F,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    let overwrite = match overwrite_mode {
        0 => zipfast::extract::Overwrite::Skip,
        _ => zipfast::extract::Overwrite::Always,
    };

    if let Err(e) = std::fs::create_dir_all(output_dir) {
        let msg = format!("Could not create output directory {}: {e}", output_dir.display());
        log_cb(1, &msg);
        return Err(msg);
    }

    let pw_opt = (!password.is_empty()).then_some(password);

    // Pre-inspect ZIP archives with `zipview::ZipIndex` via bounded range reads
    // to identify inner contents, reject symlinks before writing any file, and verify passwords.
    let path_bytes = path_to_bytes(archive_path);
    if has_suffix_icase(path_bytes, b".zip") || has_suffix_icase(path_bytes, b".cbz") {
        if let Ok(mut file) = std::fs::File::open(archive_path) {
            if let Ok(meta) = file.metadata() {
                let mut read = |offset: u64, buf: &mut [u8]| -> std::io::Result<()> {
                    read_file_range_at(&mut file, offset, buf)
                };
                if let Ok(index) = zipview::ZipIndex::open(meta.len(), &mut read) {
                    let inspection = inspect_zip_index(&index, &mut read);
                    log_cb(
                        4,
                        &format!(
                            "zipview identified {:?} with {} file(s) ({} bytes uncompressed)",
                            inspection.kind,
                            inspection.file_count,
                            inspection.total_uncompressed_size
                        ),
                    );
                    if inspection.has_symlinks {
                        log_cb(1, "ERROR: Dangerous link path was ignored");
                        log_cb(0, "Stream repair: archive contains link");
                        return Err("archive contains link".to_string());
                    }
                    if inspection.has_encrypted_entries {
                        if password.is_empty() {
                            let msg = "Archive is encrypted and requires a password";
                            log_cb(1, msg);
                            return Err(msg.to_string());
                        }
                        if let Ok(false) = verify_zip_index_password(&index, password, &mut read) {
                            let msg = "Archive password rejected by zipview verifier";
                            log_cb(1, msg);
                            return Err(msg.to_string());
                        }
                    }
                }
            }
        }
    }

    // Also check `zipfast` entry list for symlinks before extracting so no partial files are written
    // when an archive contains a symlink entry.
    if let Ok(entries) = zipfast::extract::list_with_password(archive_path, pw_opt) {
        if entries.iter().any(|e| e.link.is_some()) {
            log_cb(1, "ERROR: Dangerous link path was ignored");
            log_cb(0, "Stream repair: archive contains link");
            return Err("archive contains link".to_string());
        }
    }

    let opts = zipfast::extract::Options {
        mode: zipfast::extract::Mode::Extract,
        dest: output_dir.to_path_buf(),
        overwrite,
        flatten: false,
        threads: Some(3),
        parallel_min: None,
    };

    let summary = zipfast::extract::run_with_password(archive_path, &opts, pw_opt, |line| {
        log_cb(4, line);
    })
    .map_err(|e| {
        let msg = e.to_string();
        log_cb(1, &msg);
        msg
    })?;

    if !summary.errors.is_empty() {
        for err in &summary.errors {
            if err.contains("links are not supported") || err.contains("link") {
                log_cb(1, "ERROR: Dangerous link path was ignored");
                log_cb(0, "Stream repair: archive contains link");
            }
            log_cb(1, err);
        }
        return Err(summary.errors.join("; "));
    }

    log_cb(
        4,
        &format!(
            "Extracted {} file(s) ({} bytes) with zipfast",
            summary.files, summary.bytes
        ),
    );
    log_cb(4, "Everything is Ok");
    Ok(summary)
}

#[cfg(unix)]
fn cstr_to_path(ptr: *const c_char) -> Option<std::path::PathBuf> {
    if ptr.is_null() {
        return None;
    }
    use std::os::unix::ffi::OsStrExt;
    let bytes = unsafe { CStr::from_ptr(ptr).to_bytes() };
    if bytes.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn cstr_to_path(ptr: *const c_char) -> Option<std::path::PathBuf> {
    if ptr.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(ptr).to_str().ok()? };
    if s.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(s))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_is_zipfast_supported_extension(
    archive_path: *const c_char,
) -> c_int {
    if archive_path.is_null() {
        return 0;
    }
    let bytes = unsafe { CStr::from_ptr(archive_path).to_bytes() };
    i32::from(is_zipfast_supported_extension(bytes))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_can_extract_with_zipfast(
    archive_path: *const c_char,
) -> c_int {
    let Some(path) = cstr_to_path(archive_path) else {
        return 0;
    };
    i32::from(can_extract_with_zipfast(&path))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_extract_with_zipfast(
    archive_path: *const c_char,
    output_dir: *const c_char,
    password: *const c_char,
    overwrite_mode: c_int,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
    log_ctx: *mut std::ffi::c_void,
) -> c_int {
    let Some(arch) = cstr_to_path(archive_path) else {
        return 0;
    };
    let Some(out) = cstr_to_path(output_dir) else {
        return 0;
    };
    let pw = if password.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(password).to_str().unwrap_or("") }
    };
    let res = extract_archive_with_zipfast(&arch, &out, pw, overwrite_mode, |kind, msg| {
        if let Some(cb) = log_cb {
            if let Ok(cmsg) = std::ffi::CString::new(msg) {
                unsafe { cb(log_ctx, kind, cmsg.as_ptr()) };
            }
        }
    });
    i32::from(res.is_ok())
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_zipview_inspect_file(
    archive_path: *const c_char,
    out_kind: *mut c_int,
    out_file_count: *mut usize,
    out_total_uncompressed: *mut u64,
    out_has_encrypted: *mut c_int,
    out_has_compressed: *mut c_int,
    out_has_symlinks: *mut c_int,
    out_has_nzb: *mut c_int,
) -> c_int {
    let Some(path) = cstr_to_path(archive_path) else {
        return 0;
    };
    let Ok(info) = inspect_zip_file(&path) else {
        return 0;
    };
    unsafe {
        if !out_kind.is_null() {
            *out_kind = info.kind as c_int;
        }
        if !out_file_count.is_null() {
            *out_file_count = info.file_count;
        }
        if !out_total_uncompressed.is_null() {
            *out_total_uncompressed = info.total_uncompressed_size;
        }
        if !out_has_encrypted.is_null() {
            *out_has_encrypted = i32::from(info.has_encrypted_entries);
        }
        if !out_has_compressed.is_null() {
            *out_has_compressed = i32::from(info.has_compressed_entries);
        }
        if !out_has_symlinks.is_null() {
            *out_has_symlinks = i32::from(info.has_symlinks);
        }
        if !out_has_nzb.is_null() {
            *out_has_nzb = i32::from(info.has_nzb_files);
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpack_util_full_suite_port_from_cpp() {
        // IsSupportedTests
        assert!(sevenzip_is_supported(b"Unpack.7z"));
        assert!(sevenzip_is_supported(b"Unpack.zip"));
        assert!(sevenzip_is_supported(b"Unpack.tar"));
        assert!(sevenzip_is_supported(b"Unpack.gz"));
        assert!(sevenzip_is_supported(b"Unpack.bz2"));

        assert!(sevenzip_is_supported(b"Unpack.tar.gz"));
        assert!(sevenzip_is_supported(b"Unpack.tgz"));
        assert!(sevenzip_is_supported(b"Unpack.txz"));

        assert!(sevenzip_is_supported(b"Unpack.7z.001"));

        assert!(sevenzip_is_supported(b"Unpack.ZIP"));
        assert!(sevenzip_is_supported(b"DATA.TXZ"));

        assert!(!sevenzip_is_supported(b"Unpack.7z.002"));
        assert!(!sevenzip_is_supported(b"document.docx"));
        assert!(!sevenzip_is_supported(b"image.jpg"));
        assert!(!sevenzip_is_supported(b"Unpack.zip.bak"));

        assert!(unrar_is_supported(b"Unpack.rar"));
        assert!(unrar_is_supported(b"Unpack.part1.rar"));
        assert!(unrar_is_supported(b"Unpack.part01.rar"));
        assert!(unrar_is_supported(b"Unpack.part001.rar"));

        assert!(unrar_is_supported(b"Unpack.RAR"));
        assert!(unrar_is_supported(b"DATA.Part1.Rar"));

        assert!(!unrar_is_supported(b"Unpack.r01"));
        assert!(!unrar_is_supported(b"Unpack.part2.rar"));
        assert!(!unrar_is_supported(b"Unpack.zip"));
        assert!(!unrar_is_supported(b"not_an_Unpack.txt"));
        assert!(!unrar_is_supported(b"Unpack.tar.gz"));
        assert!(!unrar_is_supported(b"Unpack.rar.bak"));

        assert!(!unrar_is_supported("Архив.r01".as_bytes()));
        assert!(!unrar_is_supported("Архив.part2.rar".as_bytes()));
        assert!(!unrar_is_supported("Архив.zip".as_bytes()));

        assert!(!sevenzip_is_supported(b"noextension"));
        assert!(!sevenzip_is_supported(b"/some/path/"));

        assert!(sevenzip_is_supported(b"archive.nzb.zip"));
        assert!(sevenzip_is_supported(b"archive.tar.gz"));
        assert!(!sevenzip_is_supported(b"archive.tar.gz.bak"));

        assert!(!unrar_is_supported(b"archive.part02.rar"));
        assert!(unrar_is_supported(b"/path/with/spaces/Unpack.part001.RAR"));

        assert!(sevenzip_is_supported(b"/tmp/archive.TGZ"));
        assert!(unrar_is_supported(b"/tmp/Unpack.part1.rar"));

        assert!(unrar_is_supported("Архив.part001.rar".as_bytes()));

        assert!(sevenzip_is_supported(b"archive.tar.bz2"));
        assert!(!sevenzip_is_supported(b"archive.tar.bz2.backup"));

        // MakePasswordTest
        assert_eq!(extractor_make_password(b""), b"-p-");
        assert_eq!(extractor_make_password(b"mysecret"), b"-pmysecret");
        assert_eq!(extractor_make_password(b"pass with space"), b"-ppass with space");
    }

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

    fn test_crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            let mut c = (crc ^ u32::from(b)) & 0xFF;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
            crc = (crc >> 8) ^ c;
        }
        !crc
    }

    /// Builds a valid ZIP archive in memory from `(name, payload, unix_mode, flags, encrypted_payload)`.
    fn build_test_zip(entries: &[(&str, &[u8], Option<u32>, u16, Option<&[u8]>)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();

        for &(name, plain_data, unix_mode, flags, enc_override) in entries {
            let local_offset = out.len() as u32;
            let crc = test_crc32(plain_data);
            let stored_bytes = enc_override.unwrap_or(plain_data);
            let comp_size = stored_bytes.len() as u32;
            let uncomp_size = plain_data.len() as u32;
            let name_bytes = name.as_bytes();

            // Local file header
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&flags.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // Stored (method 0)
            out.extend_from_slice(&0u16.to_le_bytes()); // mod time
            out.extend_from_slice(&0u16.to_le_bytes()); // mod date
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&comp_size.to_le_bytes());
            out.extend_from_slice(&uncomp_size.to_le_bytes());
            out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(stored_bytes);

            // Central directory header
            let version_made_by: u16 = if unix_mode.is_some() { (3 << 8) | 20 } else { 20 };
            let ext_attr: u32 = unix_mode.map_or(0, |m| m << 16);
            cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            cd.extend_from_slice(&version_made_by.to_le_bytes());
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&flags.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes()); // Stored
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&crc.to_le_bytes());
            cd.extend_from_slice(&comp_size.to_le_bytes());
            cd.extend_from_slice(&uncomp_size.to_le_bytes());
            cd.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes()); // extra len
            cd.extend_from_slice(&0u16.to_le_bytes()); // comment len
            cd.extend_from_slice(&0u16.to_le_bytes()); // disk start
            cd.extend_from_slice(&0u16.to_le_bytes()); // internal attr
            cd.extend_from_slice(&ext_attr.to_le_bytes());
            cd.extend_from_slice(&local_offset.to_le_bytes());
            cd.extend_from_slice(name_bytes);
        }

        let cd_offset = out.len() as u32;
        let cd_size = cd.len() as u32;
        let count = entries.len() as u16;
        out.extend_from_slice(&cd);

        // EOCD
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn zipcrypto_encrypt(password: &str, check_byte: u8, plain: &[u8]) -> Vec<u8> {
        fn crc_byte(crc: u32, b: u8) -> u32 {
            let mut c = (crc ^ u32::from(b)) & 0xff;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0xedb8_8320
                } else {
                    c >> 1
                };
            }
            (crc >> 8) ^ c
        }
        fn update(keys: &mut [u32; 3], b: u8) {
            keys[0] = crc_byte(keys[0], b);
            keys[1] = keys[1]
                .wrapping_add(keys[0] & 0xff)
                .wrapping_mul(134_775_813)
                .wrapping_add(1);
            keys[2] = crc_byte(keys[2], (keys[1] >> 24) as u8);
        }
        fn stream_byte(keys: &[u32; 3]) -> u8 {
            let t = keys[2] | 2;
            (t.wrapping_mul(t ^ 1) >> 8) as u8
        }

        let mut keys = [0x1234_5678u32, 0x2345_6789, 0x3456_7890];
        for &b in password.as_bytes() {
            update(&mut keys, b);
        }
        let mut header = [0x42u8; 12];
        header[11] = check_byte;
        let mut out = Vec::with_capacity(12 + plain.len());
        for &p in &header {
            let k = stream_byte(&keys);
            update(&mut keys, p);
            out.push(p ^ k);
        }
        for &p in plain {
            let k = stream_byte(&keys);
            update(&mut keys, p);
            out.push(p ^ k);
        }
        out
    }

    #[test]
    fn test_zipview_content_identification_and_passwords() {
        // 1. Empty ZIP
        let empty_zip = build_test_zip(&[]);
        let info = inspect_zip_bytes(&empty_zip).unwrap();
        assert_eq!(info.kind, ZipContentKind::Empty);
        assert_eq!(info.file_count, 0);

        // 2. NZB Archive
        let nzb_zip = build_test_zip(&[
            ("releases/movie.nzb", b"<nzb></nzb>", None, 0, None),
            ("releases/readme.txt", b"info", None, 0, None),
        ]);
        let info = inspect_zip_bytes(&nzb_zip).unwrap();
        assert_eq!(info.kind, ZipContentKind::NzbArchive);
        assert!(info.has_nzb_files);
        assert_eq!(info.file_count, 2);
        assert_eq!(
            read_zip_entry_bytes(&nzb_zip, "releases/movie.nzb", None).unwrap(),
            b"<nzb></nzb>"
        );

        // 3. Extension Package
        let manifest = br#"{"name": "Email", "main": "main.py"}"#;
        let ext_zip = build_test_zip(&[
            ("Email/manifest.json", manifest, None, 0, None),
            ("Email/main.py", b"print('hi')", None, 0, None),
        ]);
        let info = inspect_zip_bytes(&ext_zip).unwrap();
        assert_eq!(info.kind, ZipContentKind::ExtensionPackage);

        // 4. Media Archive + largest_file
        let media_zip = build_test_zip(&[
            ("release/sample.nfo", b"nfo", None, 0, None),
            ("release/movie.mkv", &[0xAA; 1024], None, 0, None),
        ]);
        let info = inspect_zip_bytes(&media_zip).unwrap();
        assert_eq!(info.kind, ZipContentKind::MediaArchive);
        assert_eq!(
            info.largest_file,
            Some(("movie.mkv".to_string(), 1024, false))
        );

        // 5. Document Container (EPUB & CBZ)
        let epub_zip = build_test_zip(&[
            ("mimetype", b"application/epub+zip", None, 0, None),
            ("OEBPS/content.opf", b"<package/>", None, 0, None),
        ]);
        assert_eq!(
            inspect_zip_bytes(&epub_zip).unwrap().kind,
            ZipContentKind::DocumentContainer
        );

        let cbz_zip = build_test_zip(&[
            ("001.jpg", b"img1", None, 0, None),
            ("002.png", b"img2", None, 0, None),
        ]);
        assert_eq!(
            inspect_zip_bytes(&cbz_zip).unwrap().kind,
            ZipContentKind::DocumentContainer
        );

        // 6. Symlink detection
        let sym_zip = build_test_zip(&[
            ("movie.mkv", b"data", Some(0o100644), 0, None),
            ("link.mkv", b"movie.mkv", Some(0o120777), 0, None),
        ]);
        let sym_info = inspect_zip_bytes(&sym_zip).unwrap();
        assert!(sym_info.has_symlinks);
        assert!(sym_info.entries[1].is_symlink);

        // 7. ZipCrypto password verification and decryption via zipview
        let plain = b"secret movie payload bytes";
        let crc = test_crc32(plain);
        let check_byte = (crc >> 24) as u8;
        let enc_bytes = zipcrypto_encrypt("hunter2", check_byte, plain);
        let enc_zip = build_test_zip(&[("secret.mkv", plain, None, 0x0001, Some(&enc_bytes))]);
        let enc_info = inspect_zip_bytes(&enc_zip).unwrap();
        assert!(enc_info.has_encrypted_entries);
        assert!(!verify_zip_password_bytes(&enc_zip, "").unwrap());
        assert!(!verify_zip_password_bytes(&enc_zip, "wrongpass").unwrap());
        assert!(verify_zip_password_bytes(&enc_zip, "hunter2").unwrap());
        assert_eq!(
            read_zip_entry_bytes(&enc_zip, "secret.mkv", Some("hunter2")).unwrap(),
            plain
        );

        // 8. Range-based ZipIndex & ZipIndexOpener match in-memory ZipReader
        for archive in [
            &empty_zip,
            &nzb_zip,
            &ext_zip,
            &media_zip,
            &epub_zip,
            &cbz_zip,
            &sym_zip,
            &enc_zip,
        ] {
            let mem_info = inspect_zip_bytes(archive).unwrap();
            let range_info = inspect_zip_range(
                archive.len() as u64,
                |offset, buf| -> std::io::Result<()> {
                    let start = offset as usize;
                    buf.copy_from_slice(&archive[start..start + buf.len()]);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(range_info, mem_info);

            let staged_index = open_zip_index_staged(
                archive.len() as u64,
                |range| -> Result<Vec<u8>, zipview::ZipReadError> {
                    Ok(archive[range.start as usize..range.end as usize].to_vec())
                },
            )
            .unwrap();
            assert_eq!(staged_index.len(), mem_info.entries.len());
        }

        // Staged opener returns Truncated (and does not livelock) if fetch_range yields an empty slice
        let empty_fetch_err = open_zip_index_staged(
            empty_zip.len() as u64,
            |_range| -> Result<Vec<u8>, zipview::ZipReadError> { Ok(Vec::new()) },
        );
        assert_eq!(
            empty_fetch_err.unwrap_err(),
            zipview::ZipReadError::Truncated
        );

        // Lossy UTF-8 manifest.json still classifies as ExtensionPackage across both paths
        let non_utf8_manifest = b"{\"name\": \"Email\xFF\", \"main\": \"main.py\"}";
        let lossy_ext_zip =
            build_test_zip(&[("Email/manifest.json", non_utf8_manifest, None, 0, None)]);
        assert_eq!(
            inspect_zip_bytes(&lossy_ext_zip).unwrap().kind,
            ZipContentKind::ExtensionPackage
        );
        assert_eq!(
            inspect_zip_range(
                lossy_ext_zip.len() as u64,
                |offset, buf| -> Result<(), zipview::ZipReadError> {
                    let start = offset as usize;
                    buf.copy_from_slice(&lossy_ext_zip[start..start + buf.len()]);
                    Ok(())
                }
            )
            .unwrap()
            .kind,
            ZipContentKind::ExtensionPackage
        );

        // Staged entry_range / password_check_range / read_entry_with_password on enc_zip
        let enc_index = open_zip_index_staged(
            enc_zip.len() as u64,
            |range| -> Result<Vec<u8>, zipview::ZipReadError> {
                Ok(enc_zip[range.start as usize..range.end as usize].to_vec())
            },
        )
        .unwrap();
        let secret_entry = enc_index.by_name("secret.mkv").unwrap();
        let hdr_range = enc_index.header_range(secret_entry).unwrap();
        let hdr_bytes = &enc_zip[hdr_range.start as usize..hdr_range.end as usize];
        let pw_range = enc_index
            .password_check_range(secret_entry, hdr_bytes)
            .unwrap();
        let pw_bytes = &enc_zip[pw_range.start as usize..pw_range.end as usize];
        let pw_check = enc_index.password_check(secret_entry, pw_bytes).unwrap();
        assert!(!pw_check.accepts("wrongpass"));
        assert!(pw_check.accepts("hunter2"));
        let data_range = enc_index.entry_range(secret_entry, hdr_bytes).unwrap();
        let data_bytes = &enc_zip[data_range.start as usize..data_range.end as usize];
        assert_eq!(
            enc_index
                .read_entry_with_password(secret_entry, data_bytes, Some("hunter2"))
                .unwrap(),
            plain
        );

        // 9. Virtual 50 GiB archive opens with a single ~65 KB read and refuses >256 MiB entries before reading
        const BIG: u64 = 50 << 30;
        let mut big_head = Vec::new();
        big_head.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        big_head.extend_from_slice(&45u16.to_le_bytes());
        big_head.extend_from_slice(&0x0800u16.to_le_bytes());
        big_head.extend_from_slice(&0u16.to_le_bytes());
        big_head.extend_from_slice(&[0u8; 8]);
        big_head.extend_from_slice(&u32::MAX.to_le_bytes());
        big_head.extend_from_slice(&u32::MAX.to_le_bytes());
        big_head.extend_from_slice(&9u16.to_le_bytes());
        big_head.extend_from_slice(&20u16.to_le_bytes());
        big_head.extend_from_slice(b"movie.mkv");
        big_head.extend_from_slice(&1u16.to_le_bytes());
        big_head.extend_from_slice(&16u16.to_le_bytes());
        big_head.extend_from_slice(&BIG.to_le_bytes());
        big_head.extend_from_slice(&BIG.to_le_bytes());

        let readme_at = big_head.len() as u64 + BIG;
        let body = b"release notes";
        let body_crc = test_crc32(body);
        let mut tail = Vec::new();
        tail.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        tail.extend_from_slice(&20u16.to_le_bytes());
        tail.extend_from_slice(&0x0800u16.to_le_bytes());
        tail.extend_from_slice(&0u16.to_le_bytes());
        tail.extend_from_slice(&[0u8; 4]);
        tail.extend_from_slice(&body_crc.to_le_bytes());
        tail.extend_from_slice(&(body.len() as u32).to_le_bytes());
        tail.extend_from_slice(&(body.len() as u32).to_le_bytes());
        tail.extend_from_slice(&10u16.to_le_bytes());
        tail.extend_from_slice(&0u16.to_le_bytes());
        tail.extend_from_slice(b"readme.txt");
        tail.extend_from_slice(body);

        let cd_at = readme_at + tail.len() as u64;
        let make_cd = |name: &[u8], crc: u32, sz: u32, off: u32, extra: &[u8]| {
            let mut c = Vec::new();
            c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            c.extend_from_slice(&0x032du16.to_le_bytes());
            c.extend_from_slice(&45u16.to_le_bytes());
            c.extend_from_slice(&0x0800u16.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&[0u8; 4]);
            c.extend_from_slice(&crc.to_le_bytes());
            c.extend_from_slice(&sz.to_le_bytes());
            c.extend_from_slice(&sz.to_le_bytes());
            c.extend_from_slice(&(name.len() as u16).to_le_bytes());
            c.extend_from_slice(&(extra.len() as u16).to_le_bytes());
            c.extend_from_slice(&[0u8; 6]);
            c.extend_from_slice(&(0o100644u32 << 16).to_le_bytes());
            c.extend_from_slice(&off.to_le_bytes());
            c.extend_from_slice(name);
            c.extend_from_slice(extra);
            c
        };
        let mut big_extra = vec![1, 0, 16, 0];
        big_extra.extend_from_slice(&BIG.to_le_bytes());
        big_extra.extend_from_slice(&BIG.to_le_bytes());
        let mut readme_extra = vec![1, 0, 8, 0];
        readme_extra.extend_from_slice(&readme_at.to_le_bytes());
        let mut cd = make_cd(b"movie.mkv", 0, u32::MAX, 0, &big_extra);
        cd.extend_from_slice(&make_cd(
            b"readme.txt",
            body_crc,
            body.len() as u32,
            u32::MAX,
            &readme_extra,
        ));
        let record_at = cd_at + cd.len() as u64;
        tail.extend_from_slice(&cd);
        tail.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
        tail.extend_from_slice(&44u64.to_le_bytes());
        tail.extend_from_slice(&45u16.to_le_bytes());
        tail.extend_from_slice(&45u16.to_le_bytes());
        tail.extend_from_slice(&[0u8; 8]);
        tail.extend_from_slice(&2u64.to_le_bytes());
        tail.extend_from_slice(&2u64.to_le_bytes());
        tail.extend_from_slice(&(cd.len() as u64).to_le_bytes());
        tail.extend_from_slice(&cd_at.to_le_bytes());
        tail.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
        tail.extend_from_slice(&0u32.to_le_bytes());
        tail.extend_from_slice(&record_at.to_le_bytes());
        tail.extend_from_slice(&1u32.to_le_bytes());
        tail.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        tail.extend_from_slice(&[0u8; 4]);
        tail.extend_from_slice(&u16::MAX.to_le_bytes());
        tail.extend_from_slice(&u16::MAX.to_le_bytes());
        tail.extend_from_slice(&u32::MAX.to_le_bytes());
        tail.extend_from_slice(&u32::MAX.to_le_bytes());
        tail.extend_from_slice(&0u16.to_le_bytes());

        let virtual_len = readme_at + tail.len() as u64;
        let range_reads = std::cell::RefCell::new(Vec::new());
        let mut sparse_read = |offset: u64, buf: &mut [u8]| -> Result<(), zipview::ZipReadError> {
            range_reads.borrow_mut().push((offset, buf.len()));
            buf.fill(0);
            for (p_start, p_bytes) in [(0u64, big_head.as_slice()), (readme_at, tail.as_slice())] {
                let p_end = p_start + p_bytes.len() as u64;
                let r_end = offset + buf.len() as u64;
                let from = offset.max(p_start);
                let to = r_end.min(p_end);
                if from < to {
                    let dst = (from - offset) as usize..(to - offset) as usize;
                    let src = (from - p_start) as usize..(to - p_start) as usize;
                    buf[dst].copy_from_slice(&p_bytes[src]);
                }
            }
            Ok(())
        };
        let huge_index = zipview::ZipIndex::open(virtual_len, &mut sparse_read).unwrap();
        assert_eq!(range_reads.borrow().len(), 1);
        assert!(range_reads.borrow()[0].1 < 70_000);
        let huge_info = inspect_zip_index(&huge_index, &mut sparse_read);
        assert_eq!(huge_info.kind, ZipContentKind::MediaArchive);
        assert_eq!(
            huge_info.largest_file,
            Some(("movie.mkv".to_string(), BIG, false))
        );
        let readme_entry = huge_index.by_name("readme.txt").unwrap();
        assert_eq!(
            huge_index
                .fetch_entry(readme_entry, None, &mut sparse_read)
                .unwrap(),
            body
        );
        let reads_before_big = range_reads.borrow().len();
        let big_entry = huge_index.by_name("movie.mkv").unwrap();
        assert_eq!(
            huge_index.fetch_entry(big_entry, None, &mut sparse_read),
            Err(zipview::ZipReadError::EntryTooLarge(BIG))
        );
        assert_eq!(range_reads.borrow().len(), reads_before_big);
    }

    #[test]
    fn test_zipfast_extraction_and_symlink_rejection() {
        let work_dir =
            std::env::temp_dir().join(format!("rustyget_zipfast_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work_dir);
        std::fs::create_dir_all(&work_dir).unwrap();

        // 1. Plain multi-file ZIP extraction + Overwrite modes
        let zip_path = work_dir.join("bundle.zip");
        let out_dir = work_dir.join("out");
        let zip_bytes = build_test_zip(&[
            ("sub/hello.txt", b"hello world", Some(0o100644), 0, None),
            ("root.nzb", b"<nzb>content</nzb>", Some(0o100644), 0, None),
        ]);
        std::fs::write(&zip_path, &zip_bytes).unwrap();

        assert!(can_extract_with_zipfast(&zip_path));
        let listed = list_archive_with_zipfast(&zip_path, None).unwrap();
        assert_eq!(listed.len(), 2);

        let mut logs = Vec::new();
        let summary = extract_archive_with_zipfast(&zip_path, &out_dir, "", 1, |kind, msg| {
            logs.push((kind, msg.to_string()));
        })
        .unwrap();
        assert_eq!(summary.files, 2);
        assert_eq!(
            std::fs::read(out_dir.join("sub/hello.txt")).unwrap(),
            b"hello world"
        );
        assert_eq!(
            std::fs::read(out_dir.join("root.nzb")).unwrap(),
            b"<nzb>content</nzb>"
        );
        assert!(logs.iter().any(|(_, m)| m.contains("zipview identified")));

        // OverwriteMode::Skip (0) preserves modified file on disk
        std::fs::write(out_dir.join("root.nzb"), b"modified").unwrap();
        let skip_summary =
            extract_archive_with_zipfast(&zip_path, &out_dir, "", 0, |_, _| {}).unwrap();
        assert_eq!(skip_summary.skipped, 2);
        assert_eq!(std::fs::read(out_dir.join("root.nzb")).unwrap(), b"modified");

        // OverwriteMode::Overwrite (1) replaces modified file on disk
        let ov_summary =
            extract_archive_with_zipfast(&zip_path, &out_dir, "", 1, |_, _| {}).unwrap();
        assert_eq!(ov_summary.files, 2);
        assert_eq!(
            std::fs::read(out_dir.join("root.nzb")).unwrap(),
            b"<nzb>content</nzb>"
        );

        // 2. Encrypted ZIP extraction via zipfast + zipview password verifier
        let plain = b"encrypted archive payload for zipfast";
        let crc = test_crc32(plain);
        let enc_payload = zipcrypto_encrypt("correct-pw", (crc >> 24) as u8, plain);
        let enc_zip_path = work_dir.join("enc.zip");
        let enc_out_dir = work_dir.join("enc_out");
        std::fs::write(
            &enc_zip_path,
            build_test_zip(&[("movie.mkv", plain, None, 0x0001, Some(&enc_payload))]),
        )
        .unwrap();

        assert!(!verify_zip_password_file(&enc_zip_path, "").unwrap());
        assert!(!verify_zip_password_file(&enc_zip_path, "wrong-pw").unwrap());
        assert!(verify_zip_password_file(&enc_zip_path, "correct-pw").unwrap());
        assert_eq!(
            read_zip_entry_file(&enc_zip_path, "movie.mkv", Some("correct-pw")).unwrap(),
            plain
        );

        assert!(
            read_zip_entry_file(&enc_zip_path, "missing.mkv", Some("correct-pw")).is_err()
        );

        let mut wrong_pw_logs = Vec::new();
        assert!(extract_archive_with_zipfast(
            &enc_zip_path,
            &enc_out_dir,
            "wrong-pw",
            1,
            |k, m| wrong_pw_logs.push((k, m.to_string()))
        )
        .is_err());
        assert!(wrong_pw_logs
            .iter()
            .any(|(_, m)| m.contains("Archive password rejected by zipview verifier")));
        let enc_summary =
            extract_archive_with_zipfast(&enc_zip_path, &enc_out_dir, "correct-pw", 1, |_, _| {})
                .unwrap();
        assert_eq!(enc_summary.files, 1);
        assert_eq!(std::fs::read(enc_out_dir.join("movie.mkv")).unwrap(), plain);

        // 3. Symlink ZIP is rejected before writing any file
        let sym_zip_path = work_dir.join("sym.zip");
        let sym_out_dir = work_dir.join("sym_out");
        std::fs::write(
            &sym_zip_path,
            build_test_zip(&[
                ("movie.mkv", b"payload", Some(0o100644), 0, None),
                ("movie-link.mkv", b"movie.mkv", Some(0o120777), 0, None),
            ]),
        )
        .unwrap();
        let mut sym_logs = Vec::new();
        let sym_res = extract_archive_with_zipfast(&sym_zip_path, &sym_out_dir, "", 0, |k, m| {
            sym_logs.push((k, m.to_string()));
        });
        assert!(sym_res.is_err());
        assert!(sym_logs
            .iter()
            .any(|(_, m)| m.contains("ERROR: Dangerous link path was ignored")));
        assert!(sym_logs
            .iter()
            .any(|(_, m)| m.contains("archive contains link")));
        assert!(!sym_out_dir.join("movie.mkv").exists());

        // 4. RAR5 vs RAR3 detection in can_extract_with_zipfast
        let rar5_path = work_dir.join("sample5.rar");
        let rar3_path = work_dir.join("sample3.rar");
        std::fs::write(&rar5_path, b"Rar!\x1a\x07\x01\x00\x00\x00\x00\x00").unwrap();
        std::fs::write(&rar3_path, b"Rar!\x1a\x07\x00\x99\x00\x00\x00\x00").unwrap();
        assert!(can_extract_with_zipfast(&rar5_path));
        assert!(!can_extract_with_zipfast(&rar3_path));
        assert!(inspect_zip_file(&rar5_path).is_err());

        // 5. Bounded central-directory inspection matches in-memory zipview inspection
        let cd_info = inspect_zip_file_via_central_directory(&sym_zip_path).unwrap();
        assert!(cd_info.has_symlinks);
        assert_eq!(cd_info.kind, ZipContentKind::MediaArchive);
        assert_eq!(
            cd_info.largest_file,
            Some(("movie.mkv".to_string(), 7, false))
        );

        // 6. DupeMatcher::FindLargestFile keeps larger movie when a supplementary ZIP is present
        let dupe_dir = work_dir.join("dupe_mix");
        std::fs::create_dir_all(&dupe_dir).unwrap();
        std::fs::write(
            dupe_dir.join("00_subs.zip"),
            build_test_zip(&[("subs.srt", b"small subtitle", None, 0, None)]),
        )
        .unwrap();
        std::fs::write(dupe_dir.join("zz_movie.mkv"), &[0x55u8; 4096]).unwrap();
        let mut fname_buf = [0u8; 256];
        let mut max_size = 0i64;
        let mut compressed = false;
        crate::pardamage::find_largest_file(
            &dupe_dir,
            &mut fname_buf,
            &mut max_size,
            &mut compressed,
            None,
            |_, _, _, _| {},
        );
        assert_eq!(max_size, 4096);
        assert!(!compressed);
        assert!(fname_buf.starts_with(b"zz_movie.mkv\0"));

        let _ = std::fs::remove_dir_all(&work_dir);
    }
}
