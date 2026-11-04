//
//  This file is part of nzbget. See <https://nzbget.com>.
//
//  Copyright (C) 2026 Denis <denis@nzbget.com>
//
//  This program is free software; you can redistribute it and/or modify
//  it under the terms of the GNU General Public License as published by
//  the Free Software Foundation; either version 2 of the License, or
//  (at your option) any later version.
//
//  This program is distributed in the hope that it will be useful,
//  but WITHOUT ANY WARRANTY; without even the implied warranty of
//  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//  GNU General Public License for more details.
//
//  You should have received a copy of the GNU General Public License
//  along with this program.  If not, see <https://www.gnu.org/licenses/>.
//

/// Joins two 32-bit unsigned integers (`hi`, `lo`) into a 64-bit signed integer
/// (`Util::JoinInt64` in `daemon/util/Util.cpp`).
pub fn util_join_int64(hi: u32, lo: u32) -> i64 {
    ((hi as i64) << 32) | (lo as i64)
}

/// Splits a 64-bit signed integer into `(hi, lo)` 32-bit unsigned integers
/// (`Util::SplitInt64` in `daemon/util/Util.cpp`).
pub fn util_split_int64(int64_val: i64) -> (u32, u32) {
    let u = int64_val as u64;
    ((u >> 32) as u32, (u & 0xFFFF_FFFF) as u32)
}

/// Matches a `Tokenizer` token `shellover` (of the form `ext=cmd`) against
/// `file_ext`, returning `Some(cmd_offset)` when `file_ext` equals the prefix
/// before `'='` (`Util::FindShellOverriddenExecutor` in `daemon/util/Util.cpp`).
pub fn util_match_shell_overridden_token(shellover: &[u8], file_ext: &[u8]) -> Option<usize> {
    let eq_pos = shellover.iter().position(|&b| b == b'=')?;
    if &shellover[..eq_pos] == file_ext {
        Some(eq_pos + 1)
    } else {
        None
    }
}

/// Resolves `path` against colon-separated directories in `env_path`, returning the
/// first directory entry where `dir/path` exists and is a regular file
/// (`Util::ResolvePathFromEnv` in `daemon/util/Util.cpp`).
pub fn util_resolve_path_from_env(path: &[u8], env_path: &[u8]) -> Option<Vec<u8>> {
    if path.is_empty() || env_path.is_empty() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let rel = std::path::Path::new(std::ffi::OsStr::from_bytes(path));
        for dir in env_path.split(|&b| b == b':') {
            if dir.is_empty() {
                continue;
            }
            let base = std::path::Path::new(std::ffi::OsStr::from_bytes(dir));
            let candidate = base.join(rel);
            if candidate.exists() && candidate.is_file() {
                return Some(candidate.as_os_str().as_bytes().to_vec());
            }
        }
    }
    None
}

#[inline]
fn is_leap(year: i32) -> usize {
    if year % 400 == 0 {
        1
    } else if year % 100 == 0 {
        0
    } else if year % 4 == 0 {
        1
    } else {
        0
    }
}

#[inline]
fn days_from_0(mut year: i32) -> i32 {
    year = year.wrapping_sub(1);
    365_i32
        .wrapping_mul(year)
        .wrapping_add(year / 400)
        .wrapping_sub(year / 100)
        .wrapping_add(year / 4)
}

#[inline]
fn days_from_1970(year: i32) -> i32 {
    const DAYS_FROM_0_TO_1970: i32 = 719_162;
    days_from_0(year).wrapping_sub(DAYS_FROM_0_TO_1970)
}

#[inline]
fn days_from_1jan(year: i32, month: i32, day: i32) -> i32 {
    const DAYS: [[i32; 12]; 2] = [
        [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334],
        [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335],
    ];
    let idx = ((month - 1).clamp(0, 11)) as usize;
    DAYS[is_leap(year)][idx]
        .wrapping_add(day)
        .wrapping_sub(1)
}

/// Converts a broken-down UTC `tm` tuple into seconds since the Unix epoch
/// (`internal_timegm` / `Util::Timegm` in `daemon/util/Util.cpp`).
pub fn util_internal_timegm(
    tm_year: i32,
    tm_mon: i32,
    tm_mday: i32,
    tm_hour: i32,
    tm_min: i32,
    tm_sec: i32,
) -> i64 {
    let mut year = tm_year.wrapping_add(1900);
    let mut month = tm_mon;
    if month > 11 {
        year = year.wrapping_add(month / 12);
        month %= 12;
    } else if month < 0 {
        let years_diff = month.wrapping_neg().wrapping_add(11) / 12;
        year = year.wrapping_sub(years_diff);
        month = month.wrapping_add(12_i32.wrapping_mul(years_diff));
    }
    month = month.wrapping_add(1);
    let day_of_year = days_from_1jan(year, month, tm_mday);
    let days_since_epoch = days_from_1970(year).wrapping_add(day_of_year);
    let seconds_in_day: i64 = 3600 * 24;
    seconds_in_day
        .wrapping_mul(days_since_epoch as i64)
        .wrapping_add(3600_i64.wrapping_mul(tm_hour as i64))
        .wrapping_add(60_i64.wrapping_mul(tm_min as i64))
        .wrapping_add(tm_sec as i64)
}

/// Computes the bounded copy length for an XML tag value into a buffer of size
/// `value_buf_size` (`WebUtil::XmlParseTagValue` in `daemon/util/Util.cpp`).
pub fn webutil_xml_calc_copy_len(value_len: i32, value_buf_size: i32) -> i32 {
    if value_len < value_buf_size {
        value_len
    } else {
        value_buf_size.wrapping_sub(1)
    }
}

/// Computes the match length `rm_eo - rm_so` for a regex capture group
/// (`RegEx::GetMatchLen` in `daemon/util/Util.cpp`).
pub fn regex_calc_match_len(rm_so: i32, rm_eo: i32) -> i32 {
    rm_eo.wrapping_sub(rm_so)
}

/// Returns `1` when a tokenizer input string of length `len` fits in the inline
/// short-string buffer of capacity `capacity` (`Tokenizer::Tokenizer` in `daemon/util/Util.cpp`).
pub fn tokenizer_fits_short_string(len: i32, capacity: i32) -> i32 {
    i32::from(len < capacity)
}

/// Validates a file size for `FileSystem::LoadFileIntoBuffer` (`0 <= file_size <= INT_MAX - 1`)
/// and returns `Some((size, buf_size))` (`daemon/util/FileSystem.cpp`).
pub fn fs_validate_load_file_size(file_size: i64, add_trailing_null: i32) -> Option<(i32, i32)> {
    if file_size < 0 || file_size > (i32::MAX as i64) - 1 {
        return None;
    }
    let size = file_size as i32;
    let buf_size = size + if add_trailing_null != 0 { 1 } else { 0 };
    Some((size, buf_size))
}

/// Computes the next write chunk size (`min(remaining, zeros_size)`) when
/// pre-allocating a file with zeroes (`FileSystem::AllocateFile` in `daemon/util/FileSystem.cpp`).
pub fn fs_calc_allocate_chunk_size(remaining: i64, zeros_size: i64) -> i64 {
    remaining.min(zeros_size)
}

/// Classifies whether `filename` requires relative-to-cwd expansion (`1` if non-empty
/// and not starting with `'/'`, `0` otherwise) and returns the leading `"./"` skip offset
/// (`2` when starting with `"./"`, `0` otherwise)
/// (`FileSystem::ExpandFileName` in `daemon/util/FileSystem.cpp`).
pub fn fs_classify_expand_file_name(filename: &[u8]) -> (i32, usize) {
    if filename.is_empty() || filename[0] == b'/' {
        return (0, 0);
    }
    let skip = if filename.len() >= 2 && filename[0] == b'.' && filename[1] == b'/' {
        2
    } else {
        0
    };
    (1, skip)
}

/// Returns `1` when `filename` is `"."` or `".."`, `0` otherwise
/// (`DirBrowser::Next` in `daemon/util/FileSystem.cpp`).
pub fn dir_browser_is_dot_or_dotdot(filename: &[u8]) -> i32 {
    i32::from(filename == b"." || filename == b"..")
}

/// Maps `DiskFile::ESeekOrigin` to the C stdio `SEEK_*` origin constant
/// (`DiskFile::Seek` in `daemon/util/FileSystem.cpp`).
pub fn disk_file_select_seek_origin(
    origin: i32,
    so_cur: i32,
    so_end: i32,
    seek_set: i32,
    seek_cur: i32,
    seek_end: i32,
) -> i32 {
    if origin == so_cur {
        seek_cur
    } else if origin == so_end {
        seek_end
    } else {
        seek_set
    }
}

/// Returns `1` when `opt_name` starts with `strip_prefix` (of length `prefix_len`)
/// and has at least one character after the prefix
/// (`ScriptController::PrepareEnvOptions` in `daemon/util/ScriptController.cpp`).
pub fn script_ctrl_matches_env_prefix(
    opt_name: &[u8],
    strip_prefix: Option<&[u8]>,
    prefix_len: i32,
) -> i32 {
    let Some(prefix) = strip_prefix else {
        return 0;
    };
    if prefix_len < 0 {
        return 0;
    }
    let plen = prefix_len as usize;
    let cmp_prefix = if plen <= prefix.len() {
        &prefix[..plen]
    } else {
        prefix
    };
    i32::from(
        opt_name.len() > plen
            && opt_name.len() >= cmp_prefix.len()
            && &opt_name[..cmp_prefix.len()] == cmp_prefix,
    )
}

/// Returns `1` when the first output line from a child process begins with
/// `"[ERROR] Could not start "` (`ScriptController::Execute` in `daemon/util/ScriptController.cpp`).
pub fn script_ctrl_is_fork_start_error_line(first_line: i32, buf: &[u8]) -> i32 {
    const PREFIX: &[u8] = b"[ERROR] Could not start ";
    i32::from(first_line != 0 && buf.starts_with(PREFIX))
}

/// Normalizes a child exit code to `-1` when `exit_code == fork_error_exit_code` and
/// `start_error != 0` (`ScriptController::Execute` in `daemon/util/ScriptController.cpp`).
pub fn script_ctrl_normalize_fork_exit_code(
    exit_code: i32,
    fork_error_exit_code: i32,
    start_error: i32,
) -> i32 {
    if exit_code == fork_error_exit_code && start_error != 0 {
        -1
    } else {
        exit_code
    }
}

/// Computes the shell-convention exit code `128 + term_sig` for a process terminated
/// by a signal (`ScriptController::WaitProcess` in `daemon/util/ScriptController.cpp`).
pub fn script_ctrl_calc_signal_exit_code(term_sig: i32) -> i32 {
    128_i32.wrapping_add(term_sig)
}

/// Selects the target PID/PGID argument for `kill()` when terminating a child process:
/// `-child_pid` when `child_pid > 0 && child_pgid == child_pid`, else `child_pid`
/// (`ScriptController::Terminate` in `daemon/util/ScriptController.cpp`).
pub fn script_ctrl_select_kill_pid(child_pid: i32, child_pgid: i32) -> i32 {
    if child_pid > 0 && child_pgid == child_pid {
        child_pid.wrapping_neg()
    } else {
        child_pid
    }
}

fn c_atoi_bytes(bytes: &[u8]) -> i32 {
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut sign: i32 = 1;
    if i < bytes.len() {
        if bytes[i] == b'-' {
            sign = -1;
            i += 1;
        } else if bytes[i] == b'+' {
            i += 1;
        }
    }
    let mut acc: i32 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        acc = acc
            .wrapping_mul(10)
            .wrapping_add((bytes[i] - b'0') as i32);
        i += 1;
    }
    acc.wrapping_mul(sign)
}

/// Parses an unrar backspace progress token (`"\b...%"`) and returns `Some(percent * 10)`
/// when a `'%'` character is present (`UnpackController::ReadLine` in `daemon/postprocess/UnpackController.cpp`).
pub fn unpack_ctrl_parse_backspace_progress(backspace: &[u8]) -> Option<i32> {
    if !backspace.contains(&b'%') {
        return None;
    }
    let after_bs = backspace.get(1..).unwrap_or(&[]);
    let percent = c_atoi_bytes(after_bs);
    Some(percent.wrapping_mul(10))
}

/// Compares two `struct stat` snapshots for equality across device, inode, size,
/// mode, uid, gid, mtime, and ctime (`SameCipherFile` in `daemon/extension/ScriptConfig.cpp`).
#[allow(clippy::too_many_arguments)]
pub fn script_config_same_cipher_file(
    a_dev: u64,
    b_dev: u64,
    a_ino: u64,
    b_ino: u64,
    a_size: i64,
    b_size: i64,
    a_mode: u32,
    b_mode: u32,
    a_uid: u32,
    b_uid: u32,
    a_gid: u32,
    b_gid: u32,
    a_msec: i64,
    b_msec: i64,
    a_mnsec: i64,
    b_mnsec: i64,
    a_csec: i64,
    b_csec: i64,
    a_cnsec: i64,
    b_cnsec: i64,
) -> i32 {
    i32::from(
        a_dev == b_dev
            && a_ino == b_ino
            && a_size == b_size
            && a_mode == b_mode
            && a_uid == b_uid
            && a_gid == b_gid
            && a_msec == b_msec
            && a_mnsec == b_mnsec
            && a_csec == b_csec
            && a_cnsec == b_cnsec,
    )
}

/// Validates that a cipher configuration snapshot is a regular file with size in
/// `0..=16 MiB` (`ReadCipherDescriptor` in `daemon/extension/ScriptConfig.cpp`).
pub fn script_config_is_valid_cipher_size(is_reg: i32, size: i64) -> i32 {
    const MAX_CIPHER_CONFIG_SIZE: i64 = 16 * 1024 * 1024;
    i32::from(is_reg != 0 && (0..=MAX_CIPHER_CONFIG_SIZE).contains(&size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_and_split_int64() {
        let val = util_join_int64(0x1234_5678, 0x9ABC_DEF0);
        assert_eq!(val, 0x1234_5678_9ABC_DEF0_u64 as i64);
        assert_eq!(util_split_int64(val), (0x1234_5678, 0x9ABC_DEF0));
    }

    #[test]
    fn test_match_shell_overridden_token() {
        assert_eq!(
            util_match_shell_overridden_token(b".py=/usr/bin/python3", b".py"),
            Some(4)
        );
        assert_eq!(
            util_match_shell_overridden_token(b".sh=/bin/bash", b".py"),
            None
        );
        assert_eq!(util_match_shell_overridden_token(b".py", b".py"), None);
    }

    #[test]
    fn test_internal_timegm_and_webutil_helpers() {
        assert_eq!(util_internal_timegm(70, 0, 1, 0, 0, 0), 0);
        assert_eq!(util_internal_timegm(124, 1, 29, 12, 30, 45), 1_709_209_845);
        assert_eq!(webutil_xml_calc_copy_len(5, 10), 5);
        assert_eq!(webutil_xml_calc_copy_len(10, 10), 9);
        assert_eq!(regex_calc_match_len(3, 11), 8);
        assert_eq!(tokenizer_fits_short_string(10, 256), 1);
        assert_eq!(tokenizer_fits_short_string(256, 256), 0);
    }

    #[test]
    fn test_fs_helpers() {
        assert_eq!(fs_validate_load_file_size(100, 1), Some((100, 101)));
        assert_eq!(fs_validate_load_file_size(-1, 0), None);
        assert_eq!(fs_validate_load_file_size(i32::MAX as i64, 0), None);
        assert_eq!(fs_calc_allocate_chunk_size(5000, 16384), 5000);
        assert_eq!(fs_calc_allocate_chunk_size(30000, 16384), 16384);
        assert_eq!(fs_classify_expand_file_name(b"./foo/bar"), (1, 2));
        assert_eq!(fs_classify_expand_file_name(b"foo/bar"), (1, 0));
        assert_eq!(fs_classify_expand_file_name(b"/abs/path"), (0, 0));
        assert_eq!(dir_browser_is_dot_or_dotdot(b"."), 1);
        assert_eq!(dir_browser_is_dot_or_dotdot(b".."), 1);
        assert_eq!(dir_browser_is_dot_or_dotdot(b".hidden"), 0);
        assert_eq!(disk_file_select_seek_origin(1, 1, 2, 0, 10, 20), 10);
        assert_eq!(disk_file_select_seek_origin(2, 1, 2, 0, 10, 20), 20);
        assert_eq!(disk_file_select_seek_origin(0, 1, 2, 0, 10, 20), 0);
    }

    #[test]
    fn test_script_and_unpack_helpers() {
        assert_eq!(
            script_ctrl_matches_env_prefix(b"Script:Param", Some(b"Script:"), 7),
            1
        );
        assert_eq!(
            script_ctrl_matches_env_prefix(b"Script:", Some(b"Script:"), 7),
            0
        );
        assert_eq!(
            script_ctrl_is_fork_start_error_line(1, b"[ERROR] Could not start foo"),
            1
        );
        assert_eq!(
            script_ctrl_is_fork_start_error_line(0, b"[ERROR] Could not start foo"),
            0
        );
        assert_eq!(script_ctrl_normalize_fork_exit_code(254, 254, 1), -1);
        assert_eq!(script_ctrl_normalize_fork_exit_code(254, 254, 0), 254);
        assert_eq!(script_ctrl_calc_signal_exit_code(9), 137);
        assert_eq!(script_ctrl_select_kill_pid(1234, 1234), -1234);
        assert_eq!(script_ctrl_select_kill_pid(1234, 1000), 1234);
        assert_eq!(unpack_ctrl_parse_backspace_progress(b"\x08 42%"), Some(420));
        assert_eq!(unpack_ctrl_parse_backspace_progress(b"\x08 42"), None);
    }

    #[test]
    fn test_script_config_cipher_helpers() {
        assert_eq!(
            script_config_same_cipher_file(
                1, 1, 2, 2, 100, 100, 0o600, 0o600, 501, 501, 20, 20, 1000, 1000, 50, 50, 1000,
                1000, 50, 50
            ),
            1
        );
        assert_eq!(
            script_config_same_cipher_file(
                1, 1, 2, 3, 100, 100, 0o600, 0o600, 501, 501, 20, 20, 1000, 1000, 50, 50, 1000,
                1000, 50, 50
            ),
            0
        );
        assert_eq!(script_config_is_valid_cipher_size(1, 4096), 1);
        assert_eq!(script_config_is_valid_cipher_size(0, 4096), 0);
        assert_eq!(script_config_is_valid_cipher_size(1, 17 * 1024 * 1024), 0);
    }
}
