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
        || find_subslice(text, b"Archive is encrypted and requires a password").is_some()
        || find_subslice(text, b"Archive password rejected by zipview verifier").is_some()
        || text.starts_with(b"Unrar: The specified password is incorrect.")
    {
        info.decrypt_error = 1;
        info.password_error = 1;
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
    matches!(classify_archive_file(filename), 1..=3) || is_zipfast_supported_extension(filename)
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
    if is_zipfast_supported_extension(filename) {
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

/// Returns `true` if `path` can be extracted natively in-process by RustyGet's built-in
/// archive engine (`zipfast` + `zipview` + native Rust RAR 1.5–5.0 and 7z decoders).
pub fn can_extract_with_zipfast(path: &std::path::Path) -> bool {
    let bytes = path_to_bytes(path);
    if unrar_is_supported(bytes)
        || sevenzip_is_supported(bytes)
        || is_zipfast_supported_extension(bytes)
    {
        return true;
    }
    if let Ok(mut file) = std::fs::File::open(path) {
        use std::io::Read;
        let mut head = [0u8; 512];
        let n = file.read(&mut head).unwrap_or(0);
        if n >= 6 && head[..6] == [b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c] {
            return true;
        }
        if n >= 7 && &head[..6] == b"Rar!\x1a\x07" {
            return detect_rar_signature(&head[..n.min(8)]) != 0;
        }
        let slice = &head[..n];
        return zipfast::zip::sniff(slice)
            || zipfast::stream::Codec::sniff(slice).is_some()
            || zipfast::tar::sniff_tar(slice);
    }
    false
}

/// Lists archive entries using `zipfast::extract::list_with_password`.
pub fn list_archive_with_zipfast(
    path: &std::path::Path,
    password: Option<&str>,
) -> Result<Vec<zipfast::extract::Entry>, String> {
    let pw = password.filter(|s| !s.is_empty());
    zipfast::extract::list_with_password(path, pw).map_err(|e| e.to_string())
}

fn record_extracted_path_from_msg(
    output_dir: &std::path::Path,
    msg: &str,
    extracted_paths: &mut Vec<std::path::PathBuf>,
) {
    if let Some(entry_name) = msg
        .strip_prefix("Unrar: Extracting  ")
        .or_else(|| msg.strip_prefix("7-Zip: Extracting  "))
    {
        for win in [true, false] {
            if let Ok(rel) = zipfast::path::sanitize(entry_name, win) {
                extracted_paths.push(output_dir.join(rel));
            }
        }
    }
}

fn extract_single_archive_in_process<F>(
    archive_path: &std::path::Path,
    output_dir: &std::path::Path,
    password: &str,
    overwrite_mode: i32,
    log_cb: &mut F,
    is_cancelled: &mut dyn FnMut() -> bool,
    extracted_paths: &mut Vec<std::path::PathBuf>,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    if is_cancelled() {
        return Err("Aborted".to_string());
    }

    if let Err(e) = std::fs::create_dir_all(output_dir) {
        let msg = format!("Could not create output directory {}: {e}", output_dir.display());
        log_cb(1, &msg);
        return Err(msg);
    }

    // Check magic header on disk for native 7z or RAR 1.5–4.x
    if let Ok(mut f) = std::fs::File::open(archive_path) {
        use std::io::Read;
        let mut sig = [0u8; 8];
        let n = f.read(&mut sig).unwrap_or(0);
        if n >= 6 && sig[..6] == [b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c] {
            let mut tracking_cb = |kind: i32, msg: &str| {
                if overwrite_mode != 0 && kind == 4 {
                    record_extracted_path_from_msg(output_dir, msg, extracted_paths);
                }
                if msg != "Unrar: All OK" && msg != "7-Zip: Everything is Ok" {
                    log_cb(kind, msg);
                }
            };
            return crate::archive_native::extract_7z_cancellable(
                archive_path,
                output_dir,
                password,
                overwrite_mode,
                &mut tracking_cb,
                is_cancelled,
            );
        }
        if n >= 7 && &sig[..7] == b"Rar!\x1a\x07\x00" {
            let mut tracking_cb = |kind: i32, msg: &str| {
                if overwrite_mode != 0 && kind == 4 {
                    record_extracted_path_from_msg(output_dir, msg, extracted_paths);
                }
                if msg != "Unrar: All OK" && msg != "7-Zip: Everything is Ok" {
                    log_cb(kind, msg);
                }
            };
            return crate::archive_native::extract_rar4_cancellable(
                archive_path,
                output_dir,
                password,
                overwrite_mode,
                &mut tracking_cb,
                is_cancelled,
            );
        }
    }

    let path_bytes = path_to_bytes(archive_path);
    let fname = path_filename(path_bytes);
    if matches!(classify_archive_file(fname), 2 | 3) {
        let mut tracking_cb = |kind: i32, msg: &str| {
            if overwrite_mode != 0 && kind == 4 {
                record_extracted_path_from_msg(output_dir, msg, extracted_paths);
            }
            if msg != "Unrar: All OK" && msg != "7-Zip: Everything is Ok" {
                log_cb(kind, msg);
            }
        };
        return crate::archive_native::extract_7z_cancellable(
            archive_path,
            output_dir,
            password,
            overwrite_mode,
            &mut tracking_cb,
            is_cancelled,
        );
    }

    // Pre-inspect ZIP archives with `zipview::ZipIndex` via bounded range reads
    // to identify inner contents, reject symlinks before writing any file, and verify passwords.
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

    let mut tracking_cb = |kind: i32, msg: &str| {
        if overwrite_mode != 0 && kind == 4 {
            record_extracted_path_from_msg(output_dir, msg, extracted_paths);
        }
        if msg != "Unrar: All OK" && msg != "7-Zip: Everything is Ok" {
            log_cb(kind, msg);
        }
    };
    let summary = crate::archive_native::extract_zipfast_cancellable(
        archive_path,
        output_dir,
        password,
        overwrite_mode,
        &mut tracking_cb,
        is_cancelled,
    )?;

    log_cb(
        4,
        &format!(
            "Extracted {} file(s) ({} bytes) with zipfast",
            summary.files, summary.bytes
        ),
    );
    Ok(summary)
}

/// Collects all volume files belonging to a multi-volume nested archive so they can be cleaned up
/// after Extended Unpacker extracts the nested archive.
fn collect_nested_archive_volumes(main_archive: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut vols = vec![main_archive.to_path_buf()];
    let name = main_archive
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if name.to_ascii_lowercase().ends_with(".7z.001") {
        let prefix = &name[..name.len() - 3];
        let parent = main_archive
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        for n in 2..1000u32 {
            let p = parent.join(format!("{prefix}{n:03}"));
            if p.exists() {
                vols.push(p);
            } else {
                break;
            }
        }
        return vols;
    }
    let mut cur = main_archive.to_path_buf();
    for _ in 0..4096 {
        let Some(next) = zipfast::volume::next_volume(&cur) else {
            break;
        };
        if next.exists() {
            vols.push(next.clone());
            cur = next;
        } else {
            break;
        }
    }
    vols
}

/// Checks whether a regular file inside `output_dir` is a nested archive that Extended Unpacker
/// / Extended Unrar should recursively unpack.
fn is_nested_extractable_archive(path: &std::path::Path) -> bool {
    let bytes = path_to_bytes(path);
    let fname = path_filename(bytes);
    if fname.is_empty() {
        return false;
    }
    // Skip non-first volumes of multi-part RAR or 7z sets
    if classify_archive_file(fname) == 5 {
        return false;
    }
    if fname.len() >= 4 && fname[fname.len() - 4..].eq_ignore_ascii_case(b".rar") {
        return unrar_is_part1_or_unsplit_rar(fname);
    }
    if let Some(dot_pos) = fname.iter().rposition(|&b| b == b'.') {
        let ext = &fname[dot_pos + 1..];
        let before = &fname[..dot_pos];
        if !ext.is_empty()
            && ext.iter().all(|b| b.is_ascii_digit())
            && before.len() >= 3
            && before[before.len() - 3..].eq_ignore_ascii_case(b".7z")
        {
            return ext == b"001" || ext == b"01" || ext == b"1";
        }
    }
    if classify_archive_file(fname) == 2 {
        return true;
    }
    if has_suffix_icase(fname, b".zip")
        || has_suffix_icase(fname, b".tar")
        || has_suffix_icase(fname, b".tgz")
        || has_suffix_icase(fname, b".tar.gz")
        || has_suffix_icase(fname, b".tbz2")
        || has_suffix_icase(fname, b".tar.bz2")
        || has_suffix_icase(fname, b".txz")
        || has_suffix_icase(fname, b".tar.xz")
        || has_suffix_icase(fname, b".tzst")
        || has_suffix_icase(fname, b".tar.zst")
    {
        return true;
    }
    // Obfuscated nested archive detection via magic header bytes (RAR4/5 or 7z)
    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut sig = [0u8; 8];
        let n = f.read(&mut sig).unwrap_or(0);
        if n >= 6 && sig[..6] == [b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c] {
            return true;
        }
        if n >= 7 && &sig[..6] == b"Rar!\x1a\x07" && detect_rar_signature(&sig[..n.min(8)]) != 0 {
            // Only extract if it's not a continuation volume
            if sig.starts_with(b"Rar!\x1a\x07\x00") {
                return true;
            }
            if sig.starts_with(b"Rar!\x1a\x07\x01\x00") {
                return true;
            }
        }
    }
    false
}

fn find_nested_archives_in_dir(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<std::path::PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            find_nested_archives_in_dir(&path, out);
        } else if meta.is_file() && is_nested_extractable_archive(&path) {
            out.push(path);
        }
    }
}

/// First-class built-in Permission Fixer: normalizes POSIX permissions on all extracted
/// files (`0o644`, or `0o755` if executable) and directories (`0o755`) inside `dir`.
#[cfg(unix)]
pub fn normalize_unpacked_permissions(dir: &std::path::Path) {
    normalize_unpacked_permissions_excluding(dir, &std::collections::BTreeSet::new(), None);
}

#[cfg(unix)]
fn normalize_unpacked_permissions_excluding(
    dir: &std::path::Path,
    excluded: &std::collections::BTreeSet<std::path::PathBuf>,
    inherited_mask: Option<u32>,
) {
    use std::os::unix::fs::PermissionsExt;
    let allowed_mask = inherited_mask.unwrap_or_else(|| {
        std::fs::symlink_metadata(dir)
            .ok()
            .map(|m| 0o700 | (m.permissions().mode() & 0o077))
            .unwrap_or(0o755)
    });
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let cur_mode = meta.permissions().mode() & 0o7777;
        if meta.is_dir() {
            if !excluded.contains(&path) {
                let new_mode = (cur_mode & 0o2000) | (0o755 & allowed_mask);
                if cur_mode != new_mode {
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(new_mode));
                }
            }
            normalize_unpacked_permissions_excluding(&path, excluded, Some(allowed_mask));
        } else if meta.is_file() && !excluded.contains(&path) {
            let base_mode = if (cur_mode & 0o111) != 0 { 0o755 } else { 0o644 };
            let new_mode = base_mode & allowed_mask;
            if (cur_mode & 0o777) != new_mode {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(new_mode));
            }
        }
    }
}

#[cfg(not(unix))]
pub fn normalize_unpacked_permissions(_dir: &std::path::Path) {}

#[cfg(not(unix))]
fn normalize_unpacked_permissions_excluding(
    _dir: &std::path::Path,
    _excluded: &std::collections::BTreeSet<std::path::PathBuf>,
    _inherited_mask: Option<u32>,
) {
}

fn collect_existing_paths_in_dir(
    dir: &std::path::Path,
    out: &mut std::collections::BTreeSet<std::path::PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        out.insert(p.clone());
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.is_dir() && !meta.file_type().is_symlink() {
            collect_existing_paths_in_dir(&p, out);
        }
    }
}

/// Extracts `archive_path` into `output_dir` natively in-process using `zipfast`, `zipview`, and
/// RustyGet's pure-Rust RAR 1.5–5.0 & 7z decoders, followed by built-in Extended Unpacker / Extended Unrar
/// (recursive nested archive extraction up to 5 layers deep) and Permission Fixer.
///
/// `overwrite_mode`: `0 = Skip`, `1 = Overwrite`, `2 = AutoRename`.
/// `log_cb(kind, message)` uses `Message::EKind` values (`0 = mkInfo`, `1 = mkError`, `2 = mkWarning`, `4 = mkDetail`).
pub fn extract_archive_with_zipfast<F>(
    archive_path: &std::path::Path,
    output_dir: &std::path::Path,
    password: &str,
    overwrite_mode: i32,
    log_cb: F,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    let mut never_cancel = || false;
    extract_archive_with_zipfast_cancellable(
        archive_path,
        output_dir,
        password,
        overwrite_mode,
        true,
        log_cb,
        &mut never_cancel,
    )
}

/// Cancellable variant of [`extract_archive_with_zipfast`] that polls `is_cancelled` during
/// decoding and output writes so callers can abort in-process extraction immediately.
pub fn extract_archive_with_zipfast_cancellable<F>(
    archive_path: &std::path::Path,
    output_dir: &std::path::Path,
    password: &str,
    overwrite_mode: i32,
    cleanup_disk: bool,
    mut log_cb: F,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    if is_cancelled() {
        return Err("Aborted".to_string());
    }
    let mut pre_existing: std::collections::BTreeSet<std::path::PathBuf> =
        std::collections::BTreeSet::new();
    collect_existing_paths_in_dir(output_dir, &mut pre_existing);
    let outer_volumes: std::collections::BTreeSet<std::path::PathBuf> =
        collect_nested_archive_volumes(archive_path).into_iter().collect();
    let mut processed_nested = pre_existing.clone();
    processed_nested.extend(outer_volumes.iter().cloned());

    let mark_extracted_files =
        |paths: &[std::path::PathBuf],
         pre_existing: &mut std::collections::BTreeSet<std::path::PathBuf>,
         processed_nested: &mut std::collections::BTreeSet<std::path::PathBuf>| {
            if overwrite_mode != 0 {
                for p in paths {
                    let is_dir = std::fs::symlink_metadata(p)
                        .map(|m| m.is_dir())
                        .unwrap_or(false);
                    if !is_dir {
                        pre_existing.remove(p);
                        if !outer_volumes.contains(p) {
                            processed_nested.remove(p);
                        }
                    }
                }
            }
        };

    let mut extracted_paths = Vec::new();
    let mut summary = extract_single_archive_in_process(
        archive_path,
        output_dir,
        password,
        overwrite_mode,
        &mut log_cb,
        is_cancelled,
        &mut extracted_paths,
    )?;
    mark_extracted_files(&extracted_paths, &mut pre_existing, &mut processed_nested);

    // First-class Extended Unpacker & Extended Unrar: recursively unpack nested archives up to 5 levels
    for depth in 1..=5usize {
        if is_cancelled() {
            return Err("Aborted".to_string());
        }
        let mut nested = Vec::new();
        find_nested_archives_in_dir(output_dir, &mut nested);
        nested.retain(|p| !processed_nested.contains(p));
        if nested.is_empty() {
            break;
        }
        for nested_arch in nested {
            if is_cancelled() {
                return Err("Aborted".to_string());
            }
            processed_nested.insert(nested_arch.clone());
            let target_dir = nested_arch
                .parent()
                .unwrap_or(output_dir)
                .to_path_buf();
            let nested_name = nested_arch
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("nested");
            log_cb(
                0,
                &format!("Extended Unpacker (depth {depth}): unpacking nested archive {nested_name}"),
            );
            let volumes = collect_nested_archive_volumes(&nested_arch);
            let mut sub_extracted = Vec::new();
            let sub_summary = extract_single_archive_in_process(
                &nested_arch,
                &target_dir,
                password,
                overwrite_mode,
                &mut log_cb,
                is_cancelled,
                &mut sub_extracted,
            )?;
            let consumed_count = (sub_summary.volumes.max(1) as usize).min(volumes.len());
            let consumed_volumes = &volumes[..consumed_count];
            for v in consumed_volumes {
                processed_nested.insert(v.clone());
            }
            mark_extracted_files(&sub_extracted, &mut pre_existing, &mut processed_nested);
            summary.files += sub_summary.files;
            summary.dirs += sub_summary.dirs;
            summary.bytes += sub_summary.bytes;
            if cleanup_disk {
                for v in consumed_volumes {
                    let _ = std::fs::remove_file(v);
                }
            }
        }
    }

    if is_cancelled() {
        return Err("Aborted".to_string());
    }

    // First-class Permission Fixer: normalize POSIX file & directory permissions
    normalize_unpacked_permissions_excluding(output_dir, &pre_existing, None);

    log_cb(4, "Unrar: All OK");
    log_cb(4, "7-Zip: Everything is Ok");
    Ok(summary)
}

pub fn extract_single_archive_with_zipfast_cancellable<F>(
    archive_path: &std::path::Path,
    output_dir: &std::path::Path,
    password: &str,
    overwrite_mode: i32,
    mut log_cb: F,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<zipfast::extract::Summary, String>
where
    F: FnMut(i32, &str),
{
    if is_cancelled() {
        return Err("Aborted".to_string());
    }
    let mut extracted_paths = Vec::new();
    let summary = extract_single_archive_in_process(
        archive_path,
        output_dir,
        password,
        overwrite_mode,
        &mut log_cb,
        is_cancelled,
        &mut extracted_paths,
    )?;
    log_cb(4, "Unrar: All OK");
    log_cb(4, "7-Zip: Everything is Ok");
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
    cleanup_disk: c_int,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
    log_ctx: *mut std::ffi::c_void,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
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
    let mut is_cancelled = || {
        is_stopped_cb.is_some_and(|cb| unsafe { cb(log_ctx) != 0 })
    };
    let emit_log = |kind: i32, msg: &str| {
        if let Some(cb) = log_cb {
            if let Ok(cmsg) = std::ffi::CString::new(msg) {
                unsafe { cb(log_ctx, kind, cmsg.as_ptr()) };
            }
        }
    };
    let res = if cleanup_disk < 0 {
        extract_single_archive_with_zipfast_cancellable(
            &arch,
            &out,
            pw,
            overwrite_mode,
            emit_log,
            &mut is_cancelled,
        )
    } else {
        extract_archive_with_zipfast_cancellable(
            &arch,
            &out,
            pw,
            overwrite_mode,
            cleanup_disk != 0,
            emit_log,
            &mut is_cancelled,
        )
    };
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

// ============================================================================
// Pure-Rust `UnpackController` and `DirectUnpack` Orchestration
// ============================================================================

/// `UnpackController::FileHasRarSignature`:
/// Reads the first 8 bytes of `path` and returns `true` if they match a RAR3/4 or RAR5 signature.
pub fn file_has_rar_signature(path: &std::path::Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut sig = [0u8; 8];
    let n = file.read(&mut sig).unwrap_or(0);
    n == 8 && detect_rar_signature(&sig) != 0
}

/// `UnpackController::IsDownloadedFile`:
/// Without a temporary unpack folder, extracted files land next to the downloaded ones
/// (and an archive may hold archives): only the download's own files count as its archives.
/// Without a record of them, every file does.
pub fn is_downloaded_file(
    filename: &[u8],
    use_temp_unpack_dir: bool,
    downloaded_files: &[&[u8]],
) -> bool {
    use_temp_unpack_dir || downloaded_files.is_empty() || downloaded_files.contains(&filename)
}

/// Result of [`check_archive_files`] (`UnpackController::CheckArchiveFiles`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UnpackArchiveCheckResult {
    pub has_rar_files: bool,
    pub has_not_unpacked_rar_files: bool,
    pub has_renamed_archive_files: bool,
    pub has_seven_zip_files: bool,
    pub has_seven_zip_multi_files: bool,
    pub has_splitted_files: bool,
}

/// `UnpackController::CheckArchiveFiles`:
/// Scans `dest_dir` and classifies downloaded archive files, multi-volume 7z sets, split `.001`
/// pieces, and obfuscated/renamed RAR archives.
pub fn check_archive_files(
    dest_dir: &[u8],
    use_temp_unpack_dir: bool,
    downloaded_files: &[&[u8]],
    extracted_archives: &[&[u8]],
    unpack_ignore_ext: &[u8],
) -> UnpackArchiveCheckResult {
    let mut res = UnpackArchiveCheckResult::default();
    let sep = crate::paths::PATH_SEPARATOR;
    let ascii_fold = crate::wildmask::Lower::Ascii;

    for filename in crate::fsops::list_dir_entries(dest_dir) {
        let full = [dest_dir, &[sep], &filename].concat();
        if crate::fsops::directory_exists(&full, &crate::fsops::posix_stat)
            || !is_downloaded_file(&filename, use_temp_unpack_dir, downloaded_files)
        {
            continue;
        }
        let archive_kind = classify_archive_file(&filename);
        if archive_kind == 1 {
            res.has_rar_files = true;
            res.has_not_unpacked_rar_files |= !extracted_archives.contains(&filename.as_slice());
        } else if archive_kind == 2
            || (archive_kind == 0 && is_zipfast_supported_extension(&filename))
        {
            res.has_seven_zip_files = true;
        } else if archive_kind == 3 {
            res.has_seven_zip_multi_files = true;
        } else if archive_kind == 4 {
            res.has_splitted_files = true;
        } else if !res.has_renamed_archive_files
            && (archive_kind == 0 || archive_kind == 6)
            && !crate::util::match_file_ext(
                &filename,
                unpack_ignore_ext,
                b",;",
                &ascii_fold,
                &ascii_fold,
            )
            && file_has_rar_signature(crate::fsops::path(&full))
        {
            res.has_renamed_archive_files = true;
        }
    }

    res
}

/// Discovers and lexicographically sorts archives in `dest_dir` for `UnpackController::ExecuteUnrar`
/// (`is_seven_zip == false`) or `UnpackController::ExecuteSevenZip` (`is_seven_zip == true`).
pub fn discover_unpack_archives(
    dest_dir: &[u8],
    is_seven_zip: bool,
    multi_volumes: bool,
    use_temp_unpack_dir: bool,
    downloaded_files: &[&[u8]],
) -> Vec<Vec<u8>> {
    let mut archives = Vec::new();
    let sep = crate::paths::PATH_SEPARATOR;

    for filename in crate::fsops::list_dir_entries(dest_dir) {
        let full = [dest_dir, &[sep], &filename].concat();
        if crate::fsops::directory_exists(&full, &crate::fsops::posix_stat)
            || !is_downloaded_file(&filename, use_temp_unpack_dir, downloaded_files)
        {
            continue;
        }
        let archive_kind = classify_archive_file(&filename);
        if !is_seven_zip {
            if archive_kind == 1 && is_main_archive(&filename) {
                archives.push(filename);
            }
        } else if multi_volumes {
            if archive_kind == 3 && sevenzip_is_supported(&filename) {
                archives.push(filename);
            }
        } else if archive_kind == 2
            || (archive_kind == 0 && is_zipfast_supported_extension(&filename))
        {
            archives.push(filename);
        }
    }

    archives.sort();
    archives
}

/// Discovers, sorts, and extracts all matching archives in `dest_dir` into `unpack_dir`
/// for `UnpackController::ExecuteUnrar` / `UnpackController::ExecuteSevenZip`.
#[allow(clippy::too_many_arguments)]
pub fn extract_discovered_archives(
    dest_dir: &[u8],
    unpack_dir: &std::path::Path,
    password: &str,
    is_seven_zip: bool,
    multi_volumes: bool,
    use_temp_unpack_dir: bool,
    cleanup_disk: bool,
    downloaded_files: &[&[u8]],
    reset_all_ok: &mut dyn FnMut(),
    has_all_ok: &mut dyn FnMut() -> bool,
    log_cb: &mut dyn FnMut(i32, &str),
    is_stopped: &mut dyn FnMut() -> bool,
) -> bool {
    let archives = discover_unpack_archives(
        dest_dir,
        is_seven_zip,
        multi_volumes,
        use_temp_unpack_dir,
        downloaded_files,
    );
    if archives.is_empty() {
        return false;
    }

    let sep = crate::paths::PATH_SEPARATOR;
    for archive in archives {
        if is_stopped() {
            return false;
        }
        let full = [dest_dir, &[sep], &archive].concat();
        reset_all_ok();
        let res = extract_archive_with_zipfast_cancellable(
            crate::fsops::path(&full),
            unpack_dir,
            password,
            1,
            cleanup_disk,
            &mut *log_cb,
            is_stopped,
        );
        if res.is_err() || !has_all_ok() {
            return false;
        }
    }

    true
}

fn read_chunk_until_full_or_eof(file: &mut std::fs::File, buf: &mut [u8]) -> usize {
    use std::io::Read;
    let mut total = 0usize;
    while total < buf.len() {
        match file.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    total
}

fn close_output_file(out_writer: std::io::BufWriter<std::fs::File>) -> Result<(), i32> {
    match out_writer.into_inner() {
        Ok(out_file) => {
            #[cfg(unix)]
            {
                use std::os::unix::io::IntoRawFd;
                let fd = out_file.into_raw_fd();
                // SAFETY: `fd` was owned by `out_file` and is closed exactly once here.
                if unsafe { libc::close(fd) } == 0 {
                    Ok(())
                } else {
                    Err(crate::fsops::last_errno())
                }
            }
            #[cfg(not(unix))]
            {
                drop(out_file);
                Ok(())
            }
        }
        Err(into_err) => {
            let raw_err = into_err.error().raw_os_error().unwrap_or(0);
            Err(raw_err)
        }
    }
}

/// `UnpackController::JoinFile`:
/// Validates and joins a split file (`<base>.000` / `<base>.001` .. `<base>.NNN`) from `dest_dir`
/// into `unpack_dir/<base>`, appending joined piece filenames to `joined_files`.
#[allow(clippy::too_many_arguments)]
pub fn join_file(
    dest_dir: &[u8],
    unpack_dir: &[u8],
    frag_base_name: &[u8],
    par_success: bool,
    write_buffer_kb: i32,
    joined_files: &mut Vec<Vec<u8>>,
    log_cb: &mut dyn FnMut(i32, &[u8]),
    progress_cb: &mut dyn FnMut(Option<&[u8]>, i32),
) -> bool {
    use std::io::Write;

    let sep = crate::paths::PATH_SEPARATOR;
    let dest_base_name = unpack_ctrl_strip_last_extension(frag_base_name);
    let first_full = [dest_dir, &[sep], frag_base_name].concat();
    let Ok(first_cstr) = std::ffi::CString::new(first_full) else {
        return false;
    };
    let first_segment_size = crate::fsops::file_size(&first_cstr);

    let mut count = 0i32;
    let mut min = -1i32;
    let mut max = -1i32;
    let mut dif_size_min = 999_999i32;

    for filename in crate::fsops::list_dir_entries(dest_dir) {
        let full = [dest_dir, &[sep], &filename].concat();
        if !crate::fsops::directory_exists(&full, &crate::fsops::posix_stat) {
            let segment_size = std::ffi::CString::new(full)
                .map(|c| crate::fsops::file_size(&c))
                .unwrap_or(-1);
            accumulate_split_piece(
                &filename,
                &dest_base_name,
                segment_size,
                first_segment_size,
                &mut count,
                &mut min,
                &mut max,
                &mut dif_size_min,
            );
        }
    }

    if !check_joinable_fragments(count, min, max, dif_size_min, par_success) {
        let msg = [
            b"Could not join splitted file ",
            dest_base_name.as_slice(),
            b": missing fragments detected",
        ]
        .concat();
        log_cb(2, &msg);
        return false;
    }

    let joining_header = [b"Joining splitted file ", dest_base_name.as_slice()].concat();
    log_cb(0, &joining_header);
    progress_cb(None, 0);

    let dest_filename = [unpack_dir, &[sep], &dest_base_name].concat();
    let out_file = match std::fs::File::create(crate::fsops::path(&dest_filename)) {
        Ok(f) => f,
        Err(e) => {
            let err_msg = crate::fsops::error_message(e.raw_os_error().unwrap_or(0));
            let msg = [
                b"Could not create file ",
                dest_filename.as_slice(),
                b": ",
                err_msg.as_slice(),
            ]
            .concat();
            log_cb(1, &msg);
            return false;
        }
    };

    let mut total_size = 0i64;
    for i in min..=max {
        let frag = [
            dest_dir,
            &[sep],
            &dest_base_name,
            format!(".{i:03}").as_bytes(),
        ]
        .concat();
        if crate::fsops::file_exists(&frag, &crate::fsops::posix_stat) {
            if let Ok(c) = std::ffi::CString::new(frag) {
                total_size += crate::fsops::file_size(&c);
            }
        }
    }
    if total_size <= 0 {
        let msg = [
            b"Could not join splitted file ",
            dest_base_name.as_slice(),
            b": file segments are empty",
        ]
        .concat();
        log_cb(1, &msg);
        return false;
    }

    let buf_cap = if write_buffer_kb > 0 {
        (write_buffer_kb as usize).saturating_mul(1024)
    } else {
        8 * 1024
    };
    let mut out_writer = std::io::BufWriter::with_capacity(buf_cap, out_file);
    let mut written = 0i64;
    let mut buffer = vec![0u8; 1024 * 50];
    let mut ok = true;

    for i in min..=max {
        let piece_name = [dest_base_name.as_slice(), format!(".{i:03}").as_bytes()].concat();
        let joining_msg = [b"Joining from ", piece_name.as_slice()].concat();
        log_cb(0, &joining_msg);
        progress_cb(Some(&joining_msg), -1);

        let frag_filename = [dest_dir, &[sep], &piece_name].concat();
        if !crate::fsops::file_exists(&frag_filename, &crate::fsops::posix_stat) {
            break;
        }

        match std::fs::File::open(crate::fsops::path(&frag_filename)) {
            Ok(mut in_file) => {
                let mut cnt = buffer.len();
                while cnt == buffer.len() {
                    cnt = read_chunk_until_full_or_eof(&mut in_file, &mut buffer);
                    if cnt > 0 {
                        if let Err(e) = out_writer.write_all(&buffer[..cnt]) {
                            let err_msg =
                                crate::fsops::error_message(e.raw_os_error().unwrap_or(0));
                            let msg = [
                                b"Could not write file ",
                                dest_filename.as_slice(),
                                b": ",
                                err_msg.as_slice(),
                            ]
                            .concat();
                            log_cb(1, &msg);
                            ok = false;
                            break;
                        }
                        written += cnt as i64;
                    }
                    progress_cb(None, ((written * 1000) / total_size) as i32);
                }
                if !ok {
                    break;
                }
                joined_files.push(piece_name);
            }
            Err(_) => {
                let msg = [b"Could not open file ", frag_filename.as_slice()].concat();
                log_cb(1, &msg);
                ok = false;
                break;
            }
        }
    }

    if let Err(errno) = close_output_file(out_writer) {
        if ok {
            let err_msg = crate::fsops::error_message(errno);
            let msg = [
                b"Could not write file ",
                dest_filename.as_slice(),
                b": ",
                err_msg.as_slice(),
            ]
            .concat();
            log_cb(1, &msg);
            ok = false;
        }
    }

    if ok && written != total_size {
        let msg = [
            b"Could not join splitted file ",
            dest_base_name.as_slice(),
            format!(": joined {written} of {total_size} bytes").as_bytes(),
        ]
        .concat();
        log_cb(1, &msg);
        ok = false;
    }

    ok
}

/// `UnpackController::JoinSplittedFiles`:
/// Scans `dest_dir` for split `.001` first pieces (excluding RAR files) and joins each group into `unpack_dir`.
pub fn join_splitted_files(
    dest_dir: &[u8],
    unpack_dir: &[u8],
    par_success: bool,
    write_buffer_kb: i32,
    joined_files: &mut Vec<Vec<u8>>,
    log_cb: &mut dyn FnMut(i32, &[u8]),
    progress_cb: &mut dyn FnMut(Option<&[u8]>, i32),
) -> bool {
    let sep = crate::paths::PATH_SEPARATOR;
    for filename in crate::fsops::list_dir_entries(dest_dir) {
        let full = [dest_dir, &[sep], &filename].concat();
        if !crate::fsops::directory_exists(&full, &crate::fsops::posix_stat)
            && is_split_first_piece(&filename)
            && !file_has_rar_signature(crate::fsops::path(&full))
            && !join_file(
                dest_dir,
                unpack_dir,
                &filename,
                par_success,
                write_buffer_kb,
                joined_files,
                log_cb,
                progress_cb,
            )
        {
            return false;
        }
    }
    true
}

/// `UnpackController::Cleanup`:
/// Moves unpacked files from `unpack_dir` to `final_dir` (or `dest_dir`), restores permissions
/// using `umask`, removes `unpack_dir`, deletes `final_dir` if unpacking failed and `final_dir_created`
/// is true, and deletes source archive files if `unpack_cleanup_disk` is enabled.
/// Returns `(cleanup_ok, cleaned_up_disk)`.
#[allow(clippy::too_many_arguments)]
pub fn unpack_controller_cleanup(
    dest_dir: &[u8],
    final_dir: &[u8],
    unpack_dir: &[u8],
    unpack_ok: bool,
    use_temp_unpack_dir: bool,
    final_dir_created: bool,
    unpack_cleanup_disk: bool,
    inter_dir: bool,
    has_splitted_files: bool,
    umask: u32,
    downloaded_files: &[&[u8]],
    joined_files: &[&[u8]],
    log_cb: &mut dyn FnMut(i32, &[u8]),
) -> (bool, bool) {
    let mut ok = true;
    let mut cleaned_up_disk = false;
    let mut extracted_files: Vec<Vec<u8>> = Vec::new();
    let sep = crate::paths::PATH_SEPARATOR;

    if unpack_ok && use_temp_unpack_dir {
        let target_dest_dir = if !final_dir.is_empty() {
            final_dir
        } else {
            dest_dir
        };
        for filename in crate::fsops::list_dir_entries(unpack_dir) {
            let src_file = [unpack_dir, &[sep], &filename].concat();
            let valid_name = crate::paths::make_valid_filename(&filename, false);
            let dst_file = [target_dest_dir, &[sep], &valid_name].concat();

            if let Ok(dst_cstr) = std::ffi::CString::new(dst_file.clone()) {
                let _ = crate::fsops::delete_file(&dst_cstr);
            }

            let hidden_file = filename.first() == Some(&b'.');
            let moved = match (
                std::ffi::CString::new(src_file.clone()),
                std::ffi::CString::new(dst_file.clone()),
            ) {
                (Ok(s), Ok(d)) => crate::fsops::move_file(&s, &d),
                _ => false,
            };
            if !moved && !hidden_file {
                let err_msg = crate::fsops::error_message(crate::fsops::last_errno());
                let msg = [
                    b"Could not move file ",
                    src_file.as_slice(),
                    b" to ",
                    dst_file.as_slice(),
                    b": ",
                    err_msg.as_slice(),
                ]
                .concat();
                log_cb(1, &msg);
                ok = false;
            }

            let _ = crate::fsops::restore_file_or_dir_permissions(&dst_file, umask);
            extracted_files.push(filename);
        }

        let _ = crate::fsops::restore_dir_permissions(target_dest_dir, umask);
    }

    if ok && use_temp_unpack_dir {
        let mut ops = crate::fsops::PosixFsOps;
        let mut errmsg = Vec::new();
        if !crate::fsdelete::delete_directory_with_content(&mut ops, unpack_dir, sep, &mut errmsg)
            .unwrap_or(false)
        {
            let msg = [
                b"Could not delete temporary directory ",
                unpack_dir,
                b": ",
                errmsg.as_slice(),
            ]
            .concat();
            log_cb(1, &msg);
        }
    }

    if !unpack_ok && final_dir_created && !final_dir.is_empty() {
        let mut ops = crate::fsops::PosixFsOps;
        let _ = crate::fsdelete::delete_directory(&mut ops, final_dir, sep);
    }

    if unpack_ok && ok && unpack_cleanup_disk {
        log_cb(0, b"Deleting archive files");

        for filename in crate::fsops::list_dir_entries(dest_dir) {
            let full = [dest_dir, &[sep], &filename].concat();
            let is_dir = crate::fsops::directory_exists(&full, &crate::fsops::posix_stat);
            let in_extracted = extracted_files.iter().any(|f| f == &filename);
            let is_downloaded = is_downloaded_file(&filename, use_temp_unpack_dir, downloaded_files);
            let in_joined = joined_files.contains(&filename.as_slice());

            let cleanup_kind = classify_cleanup_archive_file(
                is_dir,
                inter_dir,
                in_extracted,
                is_downloaded,
                has_splitted_files,
                in_joined,
                &filename,
            );
            if cleanup_kind == 1
                || (cleanup_kind == 2 && file_has_rar_signature(crate::fsops::path(&full)))
            {
                let del_msg = [b"Deleting file ", filename.as_slice()].concat();
                log_cb(0, &del_msg);
                let deleted = std::ffi::CString::new(full.clone())
                    .map(|c| crate::fsops::delete_file(&c))
                    .unwrap_or(false);
                if !deleted {
                    let err_msg = crate::fsops::error_message(crate::fsops::last_errno());
                    let msg = [
                        b"Could not delete file ",
                        full.as_slice(),
                        b": ",
                        err_msg.as_slice(),
                    ]
                    .concat();
                    log_cb(1, &msg);
                }
            }
        }

        cleaned_up_disk = true;
    }

    (ok, cleaned_up_disk)
}

/// `DirectUnpack::IsMainArchive`:
/// Checks that `filename` has a RAR extension (`FileTypes::IsRarExt`) and is not a non-first `.partN.rar` volume.
pub fn direct_unpack_is_main_archive(filename: &[u8]) -> bool {
    let Some(ext) = crate::fsdelete::file_extension(filename) else {
        return false;
    };
    crate::filetypes::is_rar_ext(ext) && is_main_archive(filename)
}

/// `DirectUnpack::FindArchiveFiles`:
/// Scans `dest_dir` and returns all non-directory main RAR archive filenames.
pub fn direct_unpack_find_archive_files(dest_dir: &[u8]) -> Vec<Vec<u8>> {
    let mut archives = Vec::new();
    let sep = crate::paths::PATH_SEPARATOR;
    for filename in crate::fsops::list_dir_entries(dest_dir) {
        if direct_unpack_is_main_archive(&filename) {
            let full = [dest_dir, &[sep], &filename].concat();
            if !crate::fsops::directory_exists(&full, &crate::fsops::posix_stat) {
                archives.push(filename);
            }
        }
    }
    archives
}

/// `DirectUnpack::Cleanup`:
/// Removes `unpack_dir` (if `use_temp_unpack_dir` is enabled and `unpack_dir` exists) and
/// `final_dir` (if `final_dir_created` is true).
pub fn direct_unpack_cleanup(
    use_temp_unpack_dir: bool,
    unpack_dir: &[u8],
    final_dir_created: bool,
    final_dir: &[u8],
    log_cb: &mut dyn FnMut(i32, &[u8]),
) {
    let sep = crate::paths::PATH_SEPARATOR;
    if use_temp_unpack_dir
        && !unpack_dir.is_empty()
        && crate::fsops::directory_exists(unpack_dir, &crate::fsops::posix_stat)
    {
        let mut ops = crate::fsops::PosixFsOps;
        let mut errmsg = Vec::new();
        if !crate::fsdelete::delete_directory_with_content(&mut ops, unpack_dir, sep, &mut errmsg)
            .unwrap_or(false)
        {
            let msg = [
                b"Could not delete temporary directory ",
                unpack_dir,
                b": ",
                errmsg.as_slice(),
            ]
            .concat();
            log_cb(1, &msg);
        }
    }

    if final_dir_created && !final_dir.is_empty() {
        let mut ops = crate::fsops::PosixFsOps;
        let _ = crate::fsdelete::delete_directory(&mut ops, final_dir, sep);
    }
}

unsafe fn collect_cstr_slice<'a>(
    ptrs: *const *const c_char,
    len: usize,
) -> Vec<&'a [u8]> {
    if ptrs.is_null() || len == 0 {
        return Vec::new();
    }
    let slice = unsafe { std::slice::from_raw_parts(ptrs, len) };
    let mut out = Vec::with_capacity(len);
    for &p in slice {
        if !p.is_null() {
            out.push(unsafe { CStr::from_ptr(p).to_bytes() });
        }
    }
    out
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_controller_check_archive_files(
    dest_dir: *const c_char,
    use_temp_unpack_dir: c_int,
    downloaded_files: *const *const c_char,
    downloaded_len: usize,
    extracted_archives: *const *const c_char,
    extracted_len: usize,
    unpack_ignore_ext: *const c_char,
    out_has_rar_files: *mut c_int,
    out_has_not_unpacked_rar_files: *mut c_int,
    out_has_renamed_archive_files: *mut c_int,
    out_has_seven_zip_files: *mut c_int,
    out_has_seven_zip_multi_files: *mut c_int,
    out_has_splitted_files: *mut c_int,
) {
    let dest = if dest_dir.is_null() {
        &b""[..]
    } else {
        unsafe { CStr::from_ptr(dest_dir).to_bytes() }
    };
    let ignore = if unpack_ignore_ext.is_null() {
        &b""[..]
    } else {
        unsafe { CStr::from_ptr(unpack_ignore_ext).to_bytes() }
    };
    let downloaded = unsafe { collect_cstr_slice(downloaded_files, downloaded_len) };
    let extracted = unsafe { collect_cstr_slice(extracted_archives, extracted_len) };
    let res = check_archive_files(
        dest,
        use_temp_unpack_dir != 0,
        &downloaded,
        &extracted,
        ignore,
    );
    unsafe {
        if !out_has_rar_files.is_null() {
            *out_has_rar_files = c_int::from(res.has_rar_files);
        }
        if !out_has_not_unpacked_rar_files.is_null() {
            *out_has_not_unpacked_rar_files = c_int::from(res.has_not_unpacked_rar_files);
        }
        if !out_has_renamed_archive_files.is_null() {
            *out_has_renamed_archive_files = c_int::from(res.has_renamed_archive_files);
        }
        if !out_has_seven_zip_files.is_null() {
            *out_has_seven_zip_files = c_int::from(res.has_seven_zip_files);
        }
        if !out_has_seven_zip_multi_files.is_null() {
            *out_has_seven_zip_multi_files = c_int::from(res.has_seven_zip_multi_files);
        }
        if !out_has_splitted_files.is_null() {
            *out_has_splitted_files = c_int::from(res.has_splitted_files);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_controller_extract_archives(
    dest_dir: *const c_char,
    unpack_dir: *const c_char,
    password: *const c_char,
    is_seven_zip: c_int,
    multi_volumes: c_int,
    use_temp_unpack_dir: c_int,
    cleanup_disk: c_int,
    downloaded_files: *const *const c_char,
    downloaded_len: usize,
    ctx: *mut std::ffi::c_void,
    reset_all_ok_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    has_all_ok_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
) -> c_int {
    if dest_dir.is_null() {
        return 0;
    }
    let Some(out_dir) = cstr_to_path(unpack_dir) else {
        return 0;
    };
    let dest_bytes = unsafe { CStr::from_ptr(dest_dir).to_bytes() };
    let pw = if password.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(password).to_str().unwrap_or("") }
    };
    let downloaded = unsafe { collect_cstr_slice(downloaded_files, downloaded_len) };

    let mut reset_all_ok = || {
        if let Some(cb) = reset_all_ok_cb {
            unsafe { cb(ctx) };
        }
    };
    let mut has_all_ok = || {
        has_all_ok_cb.is_none_or(|cb| unsafe { cb(ctx) != 0 })
    };
    let mut emit_log = |kind: i32, msg: &str| {
        if let Some(cb) = log_cb {
            if let Ok(cmsg) = std::ffi::CString::new(msg) {
                unsafe { cb(ctx, kind, cmsg.as_ptr()) };
            }
        }
    };
    let mut is_stopped = || {
        is_stopped_cb.is_some_and(|cb| unsafe { cb(ctx) != 0 })
    };

    c_int::from(extract_discovered_archives(
        dest_bytes,
        &out_dir,
        pw,
        is_seven_zip != 0,
        multi_volumes != 0,
        use_temp_unpack_dir != 0,
        cleanup_disk != 0,
        &downloaded,
        &mut reset_all_ok,
        &mut has_all_ok,
        &mut emit_log,
        &mut is_stopped,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_controller_join_splitted_files(
    dest_dir: *const c_char,
    unpack_dir: *const c_char,
    par_success: c_int,
    write_buffer_kb: c_int,
    ctx: *mut std::ffi::c_void,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
    progress_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, c_int)>,
    add_joined_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) -> c_int {
    if dest_dir.is_null() || unpack_dir.is_null() {
        return 0;
    }
    let dest_bytes = unsafe { CStr::from_ptr(dest_dir).to_bytes() };
    let unpack_bytes = unsafe { CStr::from_ptr(unpack_dir).to_bytes() };
    let mut joined_files = Vec::new();

    let mut emit_log = |kind: i32, msg: &[u8]| {
        if let Some(cb) = log_cb {
            if let Ok(cmsg) = std::ffi::CString::new(msg) {
                unsafe { cb(ctx, kind, cmsg.as_ptr()) };
            }
        }
    };
    let mut emit_progress = |label: Option<&[u8]>, stage_progress: i32| {
        if let Some(cb) = progress_cb {
            match label {
                Some(s) => {
                    if let Ok(clabel) = std::ffi::CString::new(s) {
                        unsafe { cb(ctx, clabel.as_ptr(), stage_progress) };
                    }
                }
                None => unsafe { cb(ctx, std::ptr::null(), stage_progress) },
            }
        }
    };

    let ok = join_splitted_files(
        dest_bytes,
        unpack_bytes,
        par_success != 0,
        write_buffer_kb,
        &mut joined_files,
        &mut emit_log,
        &mut emit_progress,
    );

    if let Some(cb) = add_joined_cb {
        for piece in joined_files {
            if let Ok(cpiece) = std::ffi::CString::new(piece) {
                unsafe { cb(ctx, cpiece.as_ptr()) };
            }
        }
    }

    c_int::from(ok)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_controller_cleanup(
    dest_dir: *const c_char,
    final_dir: *const c_char,
    unpack_dir: *const c_char,
    unpack_ok: c_int,
    use_temp_unpack_dir: c_int,
    final_dir_created: c_int,
    unpack_cleanup_disk: c_int,
    inter_dir: c_int,
    has_splitted_files: c_int,
    umask: u32,
    downloaded_files: *const *const c_char,
    downloaded_len: usize,
    joined_files: *const *const c_char,
    joined_len: usize,
    out_cleaned_up_disk: *mut c_int,
    ctx: *mut std::ffi::c_void,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
) -> c_int {
    let dest_bytes = if dest_dir.is_null() {
        &b""[..]
    } else {
        unsafe { CStr::from_ptr(dest_dir).to_bytes() }
    };
    let final_bytes = if final_dir.is_null() {
        &b""[..]
    } else {
        unsafe { CStr::from_ptr(final_dir).to_bytes() }
    };
    let unpack_bytes = if unpack_dir.is_null() {
        &b""[..]
    } else {
        unsafe { CStr::from_ptr(unpack_dir).to_bytes() }
    };
    let downloaded = unsafe { collect_cstr_slice(downloaded_files, downloaded_len) };
    let joined = unsafe { collect_cstr_slice(joined_files, joined_len) };

    let mut emit_log = |kind: i32, msg: &[u8]| {
        if let Some(cb) = log_cb {
            if let Ok(cmsg) = std::ffi::CString::new(msg) {
                unsafe { cb(ctx, kind, cmsg.as_ptr()) };
            }
        }
    };

    let (ok, cleaned_up) = unpack_controller_cleanup(
        dest_bytes,
        final_bytes,
        unpack_bytes,
        unpack_ok != 0,
        use_temp_unpack_dir != 0,
        final_dir_created != 0,
        unpack_cleanup_disk != 0,
        inter_dir != 0,
        has_splitted_files != 0,
        umask,
        &downloaded,
        &joined,
        &mut emit_log,
    );

    if !out_cleaned_up_disk.is_null() && cleaned_up {
        unsafe { *out_cleaned_up_disk = 1 };
    }

    c_int::from(ok)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_direct_unpack_is_main_archive(
    filename: *const c_char,
) -> c_int {
    if filename.is_null() {
        return 0;
    }
    let bytes = unsafe { CStr::from_ptr(filename).to_bytes() };
    c_int::from(direct_unpack_is_main_archive(bytes))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_direct_unpack_find_archive_files(
    dest_dir: *const c_char,
    ctx: *mut std::ffi::c_void,
    add_archive_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) {
    if dest_dir.is_null() {
        return;
    }
    let dest_bytes = unsafe { CStr::from_ptr(dest_dir).to_bytes() };
    let archives = direct_unpack_find_archive_files(dest_bytes);
    if let Some(cb) = add_archive_cb {
        for arch in archives {
            if let Ok(carch) = std::ffi::CString::new(arch) {
                unsafe { cb(ctx, carch.as_ptr()) };
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_direct_unpack_cleanup(
    use_temp_unpack_dir: c_int,
    unpack_dir: *const c_char,
    final_dir_created: c_int,
    final_dir: *const c_char,
    ctx: *mut std::ffi::c_void,
    log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
) {
    let unpack_bytes = if unpack_dir.is_null() {
        &b""[..]
    } else {
        unsafe { CStr::from_ptr(unpack_dir).to_bytes() }
    };
    let final_bytes = if final_dir.is_null() {
        &b""[..]
    } else {
        unsafe { CStr::from_ptr(final_dir).to_bytes() }
    };
    let mut emit_log = |kind: i32, msg: &[u8]| {
        if let Some(cb) = log_cb {
            if let Ok(cmsg) = std::ffi::CString::new(msg) {
                unsafe { cb(ctx, kind, cmsg.as_ptr()) };
            }
        }
    };
    direct_unpack_cleanup(
        use_temp_unpack_dir != 0,
        unpack_bytes,
        final_dir_created != 0,
        final_bytes,
        &mut emit_log,
    );
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack.windows(needle.len()).any(|w| w == needle)
}

/// Checks if an unpack error message reports an out-of-disk-space condition
/// (`UnpackController::AddMessage`).
pub fn is_unpack_space_error(is_error_kind: bool, text: &[u8]) -> bool {
    is_error_kind
        && (contains_subslice(text, b"No space left on device")
            || contains_subslice(text, b"Disk quota exceeded")
            || contains_subslice(text, b"os error 28")
            || contains_subslice(text, b"StorageFull"))
}

/// Reads password lines from `path` in `510`-byte chunks matching `DiskFile::ReadLine(buf, 511)`
/// in `UnpackController::UnpackArchives`, stripping trailing `\r`/`\n` and invoking `try_password`
/// for each non-empty password while `should_continue()` returns `true`.
/// Returns `true` if `path` was opened successfully, `false` otherwise.
pub fn try_passwords_from_file(
    path: &std::path::Path,
    mut should_continue: impl FnMut() -> bool,
    mut try_password: impl FnMut(&[u8]),
) -> bool {
    use std::io::BufRead;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut reader = std::io::BufReader::new(file);
    while should_continue() {
        let Ok(available) = reader.fill_buf() else {
            break;
        };
        if available.is_empty() {
            break;
        }
        let mut chunk = Vec::with_capacity(64);
        while chunk.len() < 510 {
            let Ok(buf) = reader.fill_buf() else {
                break;
            };
            if buf.is_empty() {
                break;
            }
            let max_take = (510 - chunk.len()).min(buf.len());
            if let Some(nl) = buf[..max_take].iter().position(|&b| b == b'\n') {
                chunk.extend_from_slice(&buf[..=nl]);
                reader.consume(nl + 1);
                break;
            } else {
                chunk.extend_from_slice(&buf[..max_take]);
                reader.consume(max_take);
            }
        }
        if let Some(nul_pos) = chunk.iter().position(|&b| b == 0) {
            chunk.truncate(nul_pos);
        }
        while chunk.last().is_some_and(|&b| b == b'\n' || b == b'\r') {
            chunk.pop();
        }
        if !chunk.is_empty() {
            try_password(&chunk);
        }
    }
    true
}

/// Evaluates the failure branch of `UnpackController::Completed`.
/// Returns `(request_par_check, force_repair, unpack_status)`.
/// `unpack_status`: `2 = usFailure`, `4 = usSpace`, `5 = usPassword`.
#[allow(clippy::too_many_arguments)]
pub fn eval_unpack_completed_failure(
    parcheck_enabled: bool,
    unpack_ok: bool,
    par_status: i32,
    ps_skipped: i32,
    par_full: bool,
    unpack_start_error: bool,
    unpack_space_error: bool,
    unpack_password_error: bool,
    unpack_decrypt_error: bool,
    terminated: bool,
    stopped: bool,
    auto_terminated: bool,
    has_par_files: bool,
    has_password: bool,
    has_unpack_pass_file: bool,
    pass_list_tried: bool,
) -> (bool, bool, i32) {
    let unpack_status = if unpack_space_error {
        4 // NzbInfo::usSpace
    } else if unpack_password_error || unpack_decrypt_error {
        5 // NzbInfo::usPassword
    } else {
        2 // NzbInfo::usFailure
    };

    if parcheck_enabled
        && !unpack_ok
        && (par_status <= ps_skipped || !par_full)
        && !unpack_start_error
        && !unpack_space_error
        && !unpack_password_error
        && (!terminated || auto_terminated)
        && (!stopped || auto_terminated)
        && has_par_files
    {
        let force_repair = (has_password
            || !has_unpack_pass_file
            || pass_list_tried
            || (!unpack_decrypt_error && !unpack_password_error))
            && par_status > ps_skipped;
        (true, force_repair, unpack_status)
    } else {
        (false, false, unpack_status)
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_is_space_error(
    is_error_kind: c_int,
    text: *const c_char,
) -> c_int {
    if text.is_null() {
        return 0;
    }
    let bytes = unsafe { CStr::from_ptr(text).to_bytes() };
    c_int::from(is_unpack_space_error(is_error_kind != 0, bytes))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_controller_try_password_file(
    path: *const c_char,
    ctx: *mut std::ffi::c_void,
    should_continue_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    try_password_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) -> c_int {
    let Some(file_path) = cstr_to_path(path) else {
        return 0;
    };
    let mut should_continue = || {
        should_continue_cb.is_some_and(|cb| unsafe { cb(ctx) != 0 })
    };
    let mut try_pw = |pw: &[u8]| {
        if let Some(cb) = try_password_cb {
            if let Ok(cpw) = std::ffi::CString::new(pw) {
                unsafe { cb(ctx, cpw.as_ptr()) };
            }
        }
    };
    c_int::from(try_passwords_from_file(
        &file_path,
        &mut should_continue,
        &mut try_pw,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_unpack_controller_eval_completed_failure(
    parcheck_enabled: c_int,
    unpack_ok: c_int,
    par_status: c_int,
    ps_skipped: c_int,
    par_full: c_int,
    unpack_start_error: c_int,
    unpack_space_error: c_int,
    unpack_password_error: c_int,
    unpack_decrypt_error: c_int,
    terminated: c_int,
    stopped: c_int,
    auto_terminated: c_int,
    has_par_files: c_int,
    has_password: c_int,
    has_unpack_pass_file: c_int,
    pass_list_tried: c_int,
    out_force_repair: *mut c_int,
    out_unpack_status: *mut c_int,
) -> c_int {
    let (req_par, force_repair, status) = eval_unpack_completed_failure(
        parcheck_enabled != 0,
        unpack_ok != 0,
        par_status,
        ps_skipped,
        par_full != 0,
        unpack_start_error != 0,
        unpack_space_error != 0,
        unpack_password_error != 0,
        unpack_decrypt_error != 0,
        terminated != 0,
        stopped != 0,
        auto_terminated != 0,
        has_par_files != 0,
        has_password != 0,
        has_unpack_pass_file != 0,
        pass_list_tried != 0,
    );
    unsafe {
        if !out_force_repair.is_null() {
            *out_force_repair = c_int::from(force_repair);
        }
        if !out_unpack_status.is_null() {
            *out_unpack_status = status;
        }
    }
    c_int::from(req_par)
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

        // 4. RAR5 and RAR3/4 and 7z detection in can_extract_with_zipfast
        let rar5_path = work_dir.join("sample5.rar");
        let rar3_path = work_dir.join("sample3.rar");
        let sevenz_path = work_dir.join("sample.7z");
        std::fs::write(&rar5_path, b"Rar!\x1a\x07\x01\x00\x00\x00\x00\x00").unwrap();
        std::fs::write(&rar3_path, b"Rar!\x1a\x07\x00\x99\x00\x00\x00\x00").unwrap();
        std::fs::write(&sevenz_path, b"7z\xbc\xaf\x27\x1c\x00\x04").unwrap();
        assert!(can_extract_with_zipfast(&rar5_path));
        assert!(can_extract_with_zipfast(&rar3_path));
        assert!(can_extract_with_zipfast(&sevenz_path));
        assert!(inspect_zip_file(&rar5_path).is_err());

        // 4b. Extended Unpacker (nested archive inside ZIP) + Permission Fixer (0o400 -> 0o644)
        let inner_zip = build_test_zip(&[("inner_movie.mkv", b"nested movie data", Some(0o100400), 0, None)]);
        let outer_zip_path = work_dir.join("outer_nested.zip");
        let nested_out_dir = work_dir.join("nested_out");
        std::fs::write(
            &outer_zip_path,
            build_test_zip(&[("subarchive.zip", &inner_zip, Some(0o100644), 0, None)]),
        )
        .unwrap();
        let nested_summary =
            extract_archive_with_zipfast(&outer_zip_path, &nested_out_dir, "", 1, |_, _| {})
                .unwrap();
        assert!(nested_summary.files >= 2);
        assert!(nested_out_dir.join("inner_movie.mkv").exists());
        assert!(!nested_out_dir.join("subarchive.zip").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(nested_out_dir.join("inner_movie.mkv"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o644);

            // Untouched pre-existing files (0o600) and private directories (0o700) in output_dir
            // keep their original permissions even when overwrite_mode == 1.
            let untouched_file = nested_out_dir.join("unrelated_secret.txt");
            let untouched_dir = nested_out_dir.join("private_subdir");
            std::fs::write(&untouched_file, b"secret").unwrap();
            std::fs::set_permissions(&untouched_file, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::fs::create_dir_all(&untouched_dir).unwrap();
            std::fs::set_permissions(&untouched_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            extract_archive_with_zipfast(&outer_zip_path, &nested_out_dir, "", 1, |_, _| {}).unwrap();
            assert_eq!(
                std::fs::metadata(&untouched_file).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(&untouched_dir).unwrap().permissions().mode() & 0o777,
                0o700
            );

            // Pre-existing leftover nested archive is still unpacked when overwritten
            let _ = std::fs::remove_file(nested_out_dir.join("inner_movie.mkv"));
            std::fs::write(nested_out_dir.join("subarchive.zip"), b"stale leftover").unwrap();
            extract_archive_with_zipfast(&outer_zip_path, &nested_out_dir, "", 1, |_, _| {}).unwrap();
            assert!(nested_out_dir.join("inner_movie.mkv").exists());
            assert!(!nested_out_dir.join("subarchive.zip").exists());

            // When cleanup_disk is false, nested archive volumes are retained
            let keep_nested_out = work_dir.join("keep_nested_out");
            let mut never_cancel = || false;
            extract_archive_with_zipfast_cancellable(
                &outer_zip_path,
                &keep_nested_out,
                "",
                1,
                false,
                |_, _| {},
                &mut never_cancel,
            )
            .unwrap();
            assert!(keep_nested_out.join("inner_movie.mkv").exists());
            assert!(keep_nested_out.join("subarchive.zip").exists());

            // Corrupt nested archive propagates failure and does not emit All OK
            let bad_outer_path = work_dir.join("outer_corrupt_nested.zip");
            let bad_nested_out = work_dir.join("bad_nested_out");
            std::fs::write(
                &bad_outer_path,
                build_test_zip(&[("broken_inner.7z", b"7z\xbc\xaf\x27\x1c\x00\x04corrupt", Some(0o100644), 0, None)]),
            )
            .unwrap();
            let mut bad_logs = Vec::new();
            assert!(extract_archive_with_zipfast(&bad_outer_path, &bad_nested_out, "", 1, |k, m| {
                bad_logs.push((k, m.to_string()));
            })
            .is_err());
            assert!(!bad_logs.iter().any(|(_, m)| m.contains("All OK") || m.contains("Everything is Ok")));
        }

        // 4c. Real multi-volume RAR3/4, RAR5, and 7z fixtures from tests/testdata
        let testdata_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/testdata");
        let rar3_fixture = testdata_dir.join("rarrenamer/testfile3.part01.rar");
        if rar3_fixture.exists() {
            let rar3_out = work_dir.join("rar3_out");
            let s = extract_archive_with_zipfast(&rar3_fixture, &rar3_out, "", 1, |_, _| {}).unwrap();
            assert_eq!(s.files, 1);
            assert_eq!(s.volumes, 3);
            assert!(rar3_out.join("testfile3.dat").exists());

            let rar3_old = testdata_dir.join("rarrenamer/testfile3oldnam.rar");
            let rar3_old_out = work_dir.join("rar3_old_out");
            let s_old = extract_archive_with_zipfast(&rar3_old, &rar3_old_out, "", 1, |_, _| {}).unwrap();
            assert_eq!(s_old.files, 1);
            assert!(rar3_old_out.join("testfile3oldnam.dat").exists());

            let rar3_enc = testdata_dir.join("rarrenamer/testfile3encdata.part01.rar");
            let rar3_enc_out = work_dir.join("rar3_enc_out");
            let s_enc = extract_archive_with_zipfast(&rar3_enc, &rar3_enc_out, "123", 1, |_, _| {}).unwrap();
            assert_eq!(s_enc.files, 1);
            assert!(rar3_enc_out.join("testfile3encdata.dat").exists());

            let rar3_hp = testdata_dir.join("rarrenamer/testfile3encnam.part01.rar");
            let rar3_hp_out = work_dir.join("rar3_hp_out");
            let s_hp = extract_archive_with_zipfast(&rar3_hp, &rar3_hp_out, "123", 1, |_, _| {}).unwrap();
            assert_eq!(s_hp.files, 1);
            assert!(rar3_hp_out.join("testfile3encnam.dat").exists());
        }
        let sz_fixture = testdata_dir.join("parchecker2/testfile.7z.001");
        if sz_fixture.exists() {
            // `parchecker2/testfile.7z.001` is intentionally damaged for PAR2 repair tests;
            // verify that our native 7z extractor catches the stream CRC mismatch.
            let sz_out = work_dir.join("sz_out");
            let err = extract_archive_with_zipfast(&sz_fixture, &sz_out, "", 1, |_, _| {})
                .expect_err("damaged 7z fixture must fail CRC check");
            assert!(err.contains("CRC failed"));

            // Also verify clean extraction on an intact multi-volume `.7z.001`..`.003` set.
            let sz_clean_dir = work_dir.join("sz_clean_vols");
            std::fs::create_dir_all(&sz_clean_dir).unwrap();
            let b1 = std::fs::read(testdata_dir.join("parchecker2/testfile.7z.001")).unwrap();
            let b2 = std::fs::read(testdata_dir.join("parchecker2/testfile.7z.002")).unwrap();
            let mut b3 = std::fs::read(testdata_dir.join("parchecker2/testfile.7z.003")).unwrap();
            let mut combined = [b1.as_slice(), b2.as_slice(), b3.as_slice()].concat();
            let actual_crc = crc32fast::hash(&combined[32..32 + 0x1ea7b]).to_le_bytes();
            let b3_len = b3.len();
            // Patch the SubStreamsInfo CRC and EndHeader CRC in volume 3 for the unmodified payload
            let crc_off = b3_len - 98 + 25;
            b3[crc_off..crc_off + 4].copy_from_slice(&actual_crc);
            let new_hdr_crc = crc32fast::hash(&b3[b3_len - 98..]).to_le_bytes();
            let mut b1_fixed = b1.clone();
            b1_fixed[28..32].copy_from_slice(&new_hdr_crc);
            let start_crc = crc32fast::hash(&b1_fixed[12..32]).to_le_bytes();
            b1_fixed[8..12].copy_from_slice(&start_crc);
            std::fs::write(sz_clean_dir.join("testfile.7z.001"), &b1_fixed).unwrap();
            std::fs::write(sz_clean_dir.join("testfile.7z.002"), &b2).unwrap();
            std::fs::write(sz_clean_dir.join("testfile.7z.003"), &b3).unwrap();
            combined.clear();

            let sz_clean_out = work_dir.join("sz_clean_out");
            let s_sz = extract_archive_with_zipfast(
                &sz_clean_dir.join("testfile.7z.001"),
                &sz_clean_out,
                "",
                1,
                |_, _| {},
            )
            .unwrap();
            assert_eq!(s_sz.files, 1);
            assert!(sz_clean_out.join("testfile.dat").exists());

            // Verify cancellation aborts 7z and RAR4 extraction
            let sz_cancel_out = work_dir.join("sz_cancel_out");
            let mut cancel_now = || true;
            assert!(extract_archive_with_zipfast_cancellable(
                &sz_clean_dir.join("testfile.7z.001"),
                &sz_cancel_out,
                "",
                1,
                true,
                |_, _| {},
                &mut cancel_now,
            )
            .is_err());
            assert!(!sz_cancel_out.join("testfile.dat").exists());
        }
        if rar3_fixture.exists() {
            let rar3_cancel_out = work_dir.join("rar3_cancel_out");
            let mut checks = 0usize;
            let mut cancel_after_first = || {
                checks += 1;
                checks > 1
            };
            assert!(extract_archive_with_zipfast_cancellable(
                &rar3_fixture,
                &rar3_cancel_out,
                "",
                1,
                true,
                |_, _| {},
                &mut cancel_after_first,
            )
            .is_err());
        }
        let rar5_fixture = testdata_dir.join("rarrenamer/testfile5.part01.rar");
        if rar5_fixture.exists() {
            let rar5_out = work_dir.join("rar5_out");
            let s5 = extract_archive_with_zipfast(&rar5_fixture, &rar5_out, "", 1, |_, _| {}).unwrap();
            assert_eq!(s5.files, 1);
            assert!(rar5_out.join("testfile5.dat").exists());

            let rar5_cancel_out = work_dir.join("rar5_cancel_out");
            let mut checks = 0usize;
            let mut cancel_mid_rar5 = || {
                checks += 1;
                checks > 4
            };
            assert!(extract_archive_with_zipfast_cancellable(
                &rar5_fixture,
                &rar5_cancel_out,
                "",
                1,
                true,
                |_, _| {},
                &mut cancel_mid_rar5,
            )
            .is_err());
            assert!(!rar5_cancel_out.join("testfile5.dat").exists());
        }
        // Mid-entry ZIP cancellation removes partial output file
        let big_zip_path = work_dir.join("big_cancel.zip");
        let big_zip_out = work_dir.join("big_cancel_out");
        let big_payload = vec![0x42u8; 512 * 1024];
        std::fs::write(
            &big_zip_path,
            build_test_zip(&[("big.bin", &big_payload, Some(0o100644), 0, None)]),
        )
        .unwrap();
        let mut zip_checks = 0usize;
        let mut cancel_mid_zip = || {
            zip_checks += 1;
            zip_checks > 5
        };
        assert!(extract_archive_with_zipfast_cancellable(
            &big_zip_path,
            &big_zip_out,
            "",
            1,
            true,
            |_, _| {},
            &mut cancel_mid_zip,
        )
        .is_err());
        assert!(!big_zip_out.join("big.bin").exists());

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

        // 7. Single-archive extraction (used by ExtensionManager::InstallExtension) preserves bundled nested archives intact
        let ext_dir = work_dir.join("ext_pkg");
        std::fs::create_dir_all(&ext_dir).unwrap();
        let bundled_zip = build_test_zip(&[("inner.txt", b"bundled resource", None, 0, None)]);
        let pkg_zip_path = ext_dir.join("extension_pkg.zip");
        let pkg_out_dir = ext_dir.join("installed");
        std::fs::write(
            &pkg_zip_path,
            build_test_zip(&[
                ("manifest.json", b"{}", None, 0, None),
                ("resources/data.zip", &bundled_zip, None, 0, None),
            ]),
        )
        .unwrap();
        let single_summary = extract_single_archive_with_zipfast_cancellable(
            &pkg_zip_path,
            &pkg_out_dir,
            "",
            1,
            |_, _| {},
            &mut || false,
        )
        .unwrap();
        assert_eq!(single_summary.files, 2);
        assert!(pkg_out_dir.join("manifest.json").exists());
        assert_eq!(
            std::fs::read(pkg_out_dir.join("resources/data.zip")).unwrap(),
            bundled_zip
        );
        assert!(!pkg_out_dir.join("resources/inner.txt").exists());

        // 8. Nested archive cleanup deletes only volumes actually consumed by the nested archive
        let p1 = std::fs::read(testdata_dir.join("rarrenamer/testfile5.part01.rar"));
        let p2 = std::fs::read(testdata_dir.join("rarrenamer/testfile5.part02.rar"));
        let p3 = std::fs::read(testdata_dir.join("rarrenamer/testfile5.part03.rar"));
        if let (Ok(b1), Ok(b2), Ok(b3)) = (p1, p2, p3) {
            let nested_rar_dir = work_dir.join("nested_rar_consumed");
            let nested_rar_out = nested_rar_dir.join("out");
            std::fs::create_dir_all(&nested_rar_out).unwrap();
            let stale_part4 = nested_rar_out.join("inner.part04.rar");
            std::fs::write(&stale_part4, b"unrelated pre-existing part04").unwrap();
            let outer_zip = nested_rar_dir.join("outer.zip");
            std::fs::write(
                &outer_zip,
                build_test_zip(&[
                    ("inner.part01.rar", &b1, None, 0, None),
                    ("inner.part02.rar", &b2, None, 0, None),
                    ("inner.part03.rar", &b3, None, 0, None),
                ]),
            )
            .unwrap();
            extract_archive_with_zipfast_cancellable(
                &outer_zip,
                &nested_rar_out,
                "",
                1,
                true,
                |_, _| {},
                &mut || false,
            )
            .unwrap();
            assert!(!nested_rar_out.join("inner.part01.rar").exists());
            assert!(!nested_rar_out.join("inner.part02.rar").exists());
            assert!(!nested_rar_out.join("inner.part03.rar").exists());
            assert!(
                stale_part4.exists(),
                "unconsumed inner.part04.rar must not be deleted"
            );
            assert!(nested_rar_out.join("testfile5.dat").exists());
        }

        let _ = std::fs::remove_dir_all(&work_dir);
    }

    #[test]
    fn test_unpack_controller_and_direct_unpack_rust_orchestration() {
        let work_dir = std::env::temp_dir().join(format!(
            "rustyget_unpack_ctrl_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dest_dir = work_dir.join("dest");
        let unpack_dir = dest_dir.join("_unpack");
        let final_dir = work_dir.join("final");
        std::fs::create_dir_all(&unpack_dir).unwrap();
        std::fs::create_dir_all(&final_dir).unwrap();

        // 1. Write split pieces, RAR, 7z, and obfuscated RAR into dest_dir
        std::fs::write(dest_dir.join("movie.mkv.001"), b"PART_ONE_DATA!").unwrap();
        std::fs::write(dest_dir.join("movie.mkv.002"), b"PART2").unwrap();
        std::fs::write(dest_dir.join("archive.part01.rar"), b"Rar!\x1a\x07\x00\x00").unwrap();
        std::fs::write(dest_dir.join("archive.part02.rar"), b"Rar!\x1a\x07\x00\x00").unwrap();
        std::fs::write(dest_dir.join("bundle.7z"), b"7z\xbc\xaf\x27\x1c\x00\x04").unwrap();
        std::fs::write(dest_dir.join("multi.7z.001"), b"7z\xbc\xaf\x27\x1c\x00\x04").unwrap();
        std::fs::write(dest_dir.join("obfuscated.bin"), b"Rar!\x1a\x07\x01\x00").unwrap();

        let dest_bytes = path_to_bytes(&dest_dir);
        let unpack_bytes = path_to_bytes(&unpack_dir);
        let final_bytes = path_to_bytes(&final_dir);

        // 2. CheckArchiveFiles
        let chk = check_archive_files(
            dest_bytes,
            true,
            &[],
            &[b"archive.part01.rar"],
            b".nfo;.txt",
        );
        assert!(chk.has_rar_files);
        assert!(
            chk.has_not_unpacked_rar_files,
            "archive.part02.rar was not in extracted_archives"
        );
        assert!(chk.has_renamed_archive_files);
        assert!(chk.has_seven_zip_files);
        assert!(chk.has_seven_zip_multi_files);
        assert!(chk.has_splitted_files);

        // 3. DirectUnpack::FindArchiveFiles finds only main RAR archives
        let direct_main = direct_unpack_find_archive_files(dest_bytes);
        assert_eq!(direct_main, vec![b"archive.part01.rar".to_vec()]);

        // 4. discover_unpack_archives for Unrar, 7z single, and 7z multi
        assert_eq!(
            discover_unpack_archives(dest_bytes, false, false, true, &[]),
            vec![b"archive.part01.rar".to_vec()]
        );
        assert_eq!(
            discover_unpack_archives(dest_bytes, true, false, true, &[]),
            vec![b"bundle.7z".to_vec()]
        );
        assert_eq!(
            discover_unpack_archives(dest_bytes, true, true, true, &[]),
            vec![b"multi.7z.001".to_vec()]
        );

        // 5. JoinSplittedFiles joins movie.mkv.001 + movie.mkv.002 -> _unpack/movie.mkv
        let mut joined_files = Vec::new();
        let mut join_logs = Vec::new();
        let mut last_progress = -1;
        let join_ok = join_splitted_files(
            dest_bytes,
            unpack_bytes,
            false,
            64,
            &mut joined_files,
            &mut |kind, msg| join_logs.push((kind, msg.to_vec())),
            &mut |_label, prog| {
                if prog >= 0 {
                    last_progress = prog;
                }
            },
        );
        assert!(join_ok);
        assert_eq!(last_progress, 1000);
        assert_eq!(
            joined_files,
            vec![b"movie.mkv.001".to_vec(), b"movie.mkv.002".to_vec()]
        );
        assert_eq!(
            std::fs::read(unpack_dir.join("movie.mkv")).unwrap(),
            b"PART_ONE_DATA!PART2"
        );

        // Non-UTF8 filename bytes are preserved verbatim in join diagnostics
        let mut non_utf8_logs = Vec::new();
        let non_utf8_ok = join_file(
            dest_bytes,
            unpack_bytes,
            b"m\xffvie.mkv.001",
            false,
            64,
            &mut Vec::new(),
            &mut |kind, msg| non_utf8_logs.push((kind, msg.to_vec())),
            &mut |_, _| {},
        );
        assert!(!non_utf8_ok);
        assert_eq!(
            non_utf8_logs,
            vec![(
                2,
                b"Could not join splitted file m\xffvie.mkv: missing fragments detected".to_vec()
            )]
        );

        // Output-file close errors are detected and reported by close_output_file
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let probe_path = work_dir.join("close_probe.bin");
            let probe_file = std::fs::File::create(&probe_path).unwrap();
            let raw_fd = probe_file.as_raw_fd();
            let writer = std::io::BufWriter::new(probe_file);
            // Close the underlying descriptor early so close_output_file observes EBADF on close.
            unsafe {
                libc::close(raw_fd);
            }
            assert_eq!(close_output_file(writer), Err(libc::EBADF));
        }

        // 6. UnpackController::Cleanup moves _unpack/movie.mkv to final_dir, removes _unpack, and deletes archives
        let joined_refs: Vec<&[u8]> = joined_files.iter().map(Vec::as_slice).collect();
        let mut cleanup_logs = Vec::new();
        let (cleanup_ok, cleaned_disk) = unpack_controller_cleanup(
            dest_bytes,
            final_bytes,
            unpack_bytes,
            true,
            true,
            true,
            true,
            true,
            true,
            0o022,
            &[],
            &joined_refs,
            &mut |kind, msg| cleanup_logs.push((kind, msg.to_vec())),
        );
        assert!(cleanup_ok);
        assert!(cleaned_disk);
        assert!(!unpack_dir.exists());
        assert_eq!(
            std::fs::read(final_dir.join("movie.mkv")).unwrap(),
            b"PART_ONE_DATA!PART2"
        );
        assert!(!dest_dir.join("movie.mkv.001").exists());
        assert!(!dest_dir.join("movie.mkv.002").exists());
        assert!(!dest_dir.join("archive.part01.rar").exists());
        assert!(!dest_dir.join("bundle.7z").exists());
        assert!(!dest_dir.join("multi.7z.001").exists());
        assert!(
            dest_dir.join("obfuscated.bin").exists(),
            "unrelated non-archive extension stays untouched"
        );

        // 7. DirectUnpack::Cleanup removes temp unpack dir and empty final_dir
        let du_unpack = work_dir.join("du_unpack");
        let du_final = work_dir.join("du_final");
        std::fs::create_dir_all(&du_unpack).unwrap();
        std::fs::write(du_unpack.join("partial.dat"), b"partial").unwrap();
        std::fs::create_dir_all(&du_final).unwrap();
        direct_unpack_cleanup(
            true,
            path_to_bytes(&du_unpack),
            true,
            path_to_bytes(&du_final),
            &mut |_, _| {},
        );
        assert!(!du_unpack.exists());
        assert!(!du_final.exists());

        // 8. Space error classification, password file iteration, and completed failure evaluation
        assert!(is_unpack_space_error(true, b"Unrar: write error (No space left on device)"));
        assert!(is_unpack_space_error(true, b"7-Zip: StorageFull while extracting"));
        assert!(!is_unpack_space_error(false, b"No space left on device"));
        assert!(!is_unpack_space_error(true, b"CRC failed in encrypted file"));

        let pw_file = work_dir.join("passwords.txt");
        std::fs::write(&pw_file, b"\r\nfirst_pass\r\nsecond_pass\n\nthird_pass\n").unwrap();
        let mut tried_pws = Vec::new();
        let count = std::cell::Cell::new(0usize);
        let opened = try_passwords_from_file(
            &pw_file,
            || count.get() < 2,
            |pw| {
                tried_pws.push(pw.to_vec());
                count.set(count.get() + 1);
            },
        );
        assert!(opened);
        assert_eq!(
            tried_pws,
            vec![b"first_pass".to_vec(), b"second_pass".to_vec()]
        );

        let (req_par, force_rep, st) = eval_unpack_completed_failure(
            true, false, 3, 1, false, false, false, false, true, false, false, false, true, true,
            false, false,
        );
        assert!(req_par);
        assert!(force_rep);
        assert_eq!(st, 5);

        let (req_par2, _, st2) = eval_unpack_completed_failure(
            true, false, 1, 1, false, false, true, false, false, false, false, false, true, false,
            false, false,
        );
        assert!(!req_par2);
        assert_eq!(st2, 4);

        let _ = std::fs::remove_dir_all(&work_dir);
    }
}
