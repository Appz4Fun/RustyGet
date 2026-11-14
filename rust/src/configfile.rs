//! The main configuration file reader/writer (`ScriptConfig`): load, merge-save
//! with the TLS cipher baseline check, and atomic cipher patching.

use std::collections::BTreeMap;
use std::ffi::{c_char, c_int, CStr, CString};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

fn cstring_lossy(bytes: &[u8]) -> CString {
    if let Some(nul) = bytes.iter().position(|&b| b == 0) {
        CString::new(&bytes[..nul]).unwrap_or_default()
    } else {
        CString::new(bytes).unwrap_or_default()
    }
}

static CONFIG_WRITE_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Computes `ScriptConfig::CipherBaseline` from `(name, value)` pairs.
pub fn script_config_cipher_baseline(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut names = Vec::with_capacity(entries.len());
    let mut values = Vec::with_capacity(entries.len());
    for &(k, v) in entries {
        names.push(k);
        values.push(v);
    }
    build_cipher_baseline(&names, &values)
}

fn script_config_cipher_baseline_cstrings(entries: &[(CString, CString)]) -> Vec<u8> {
    let mut names = Vec::with_capacity(entries.len());
    let mut values = Vec::with_capacity(entries.len());
    for (k, v) in entries {
        names.push(k.to_bytes());
        values.push(v.to_bytes());
    }
    build_cipher_baseline(&names, &values)
}

/// Parses config file bytes into `(name, value)` entries, applying `ConvertOldOption`
/// and `ConvertOldOptions` (`ScriptConfig::LoadConfigUnlocked`).
pub fn script_config_parse_bytes(raw: &[u8]) -> Vec<(CString, CString)> {
    let mut entries = Vec::new();
    for raw_line in raw.split_inclusive(|&b| b == b'\n') {
        let nul_trimmed = match raw_line.iter().position(|&b| b == 0) {
            Some(pos) => &raw_line[..pos],
            None => raw_line,
        };
        let trimmed_len = crate::util::trim_right_line(nul_trimmed);
        let line = &nul_trimmed[..trimmed_len];
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        if let Some((raw_name, raw_val)) = crate::options::options_split_option_string(line) {
            let name_c = cstring_lossy(raw_name);
            let val_c = cstring_lossy(raw_val);
            let (conv_name, conv_val) = crate::options::convert_old_option(&name_c, &val_c);
            entries.push((conv_name, conv_val));
        }
    }
    entries
}

fn script_config_load_unlocked(filename: &[u8]) -> Option<Vec<(CString, CString)>> {
    let path = bytes_to_path(filename);
    let mut file = std::fs::File::open(&path).ok()?;
    let meta = file.metadata().ok()?;
    if meta.is_dir() {
        return None;
    }
    if meta.len() == 0 {
        return Some(Vec::new());
    }
    let mut raw = Vec::with_capacity(meta.len() as usize);
    file.read_to_end(&mut raw).ok()?;
    Some(script_config_parse_bytes(&raw))
}

/// End-to-end implementation of `ScriptConfig::LoadConfig`.
pub fn script_config_load(filename: &[u8]) -> Option<Vec<(CString, CString)>> {
    let _guard = CONFIG_WRITE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    script_config_load_unlocked(filename)
}

/// Merges `entries` into existing config file content `raw`, replacing existing options
/// in-place (using the first occurrence's option name casing in `entries` and the last
/// occurrence's value), dropping orphaned/duplicate option lines, preserving comments
/// with guaranteed trailing `\n`, and appending new options at the end.
pub fn script_config_merge_bytes(raw: &[u8], entries: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut values: Vec<(&[u8], &[u8])> = Vec::with_capacity(entries.len());
    for &(name, val) in entries {
        if let Some(existing) = values
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
        {
            existing.1 = val;
        } else {
            values.push((name, val));
        }
    }

    let mut written_options: Vec<Vec<u8>> = Vec::with_capacity(values.len());
    let mut out = Vec::with_capacity(raw.len() + 256);

    for raw_line in raw.split_inclusive(|&b| b == b'\n') {
        let line = match raw_line.iter().position(|&b| b == 0) {
            Some(pos) => &raw_line[..pos],
            None => raw_line,
        };
        if !line.is_empty() && line[0] != b'#' && line.contains(&b'=') {
            let mut trimmed = line;
            while trimmed
                .last()
                .is_some_and(|&b| matches!(b, b'\n' | b'\r' | b' '))
            {
                trimmed = &trimmed[..trimmed.len() - 1];
            }
            if let Some((raw_name, raw_val)) = crate::options::options_split_option_string(trimmed)
            {
                let name_c = cstring_lossy(raw_name);
                let val_c = cstring_lossy(raw_val);
                let (conv_name, _) = crate::options::convert_old_option(&name_c, &val_c);
                let opt_bytes = conv_name.to_bytes();
                if let Some(&(canon_name, latest_val)) = values
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(opt_bytes))
                {
                    if !written_options
                        .iter()
                        .any(|w| w.eq_ignore_ascii_case(canon_name))
                    {
                        written_options.push(canon_name.to_vec());
                        out.extend_from_slice(canon_name);
                        out.push(b'=');
                        out.extend_from_slice(latest_val);
                        out.push(b'\n');
                    }
                }
            }
        } else {
            out.extend_from_slice(line);
            if !line.ends_with(b"\n") {
                out.push(b'\n');
            }
        }
    }

    for &(name, val) in entries {
        if !written_options.iter().any(|w| w.eq_ignore_ascii_case(name)) {
            written_options.push(name.to_vec());
            let final_val = values
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map_or(val, |&(_, v)| v);
            out.extend_from_slice(name);
            out.push(b'=');
            out.extend_from_slice(final_val);
            out.push(b'\n');
        }
    }

    out
}

/// End-to-end implementation of `ScriptConfig::SaveConfig`.
/// Returns `(saved, conflict)`.
pub fn script_config_save(
    filename: &[u8],
    entries: &[(&[u8], &[u8])],
    cipher_baseline: Option<&[u8]>,
) -> (bool, bool) {
    let _guard = CONFIG_WRITE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(expected) = cipher_baseline {
        let Some(current) = script_config_load_unlocked(filename) else {
            return (false, false);
        };
        let actual = script_config_cipher_baseline_cstrings(&current);
        if actual.as_slice() != expected {
            return (false, true);
        }
    }

    let path = bytes_to_path(filename);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
    else {
        return (false, false);
    };
    if file.metadata().map_or(true, |m| m.is_dir()) {
        return (false, false);
    }

    let mut raw = Vec::new();
    if file.read_to_end(&mut raw).is_err() {
        return (false, false);
    }

    let merged = script_config_merge_bytes(&raw, entries);
    if file.seek(SeekFrom::Start(0)).is_err() {
        return (false, false);
    }
    if file.write_all(&merged).is_err() || file.flush().is_err() {
        return (false, false);
    }
    let _ = file.set_len(merged.len() as u64);
    (true, false)
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsCipherCommit {
    Saved = 0,
    Unchanged = 1,
    Conflict = 2,
    Cancelled = 3,
    Failed = 4,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod cipher_atomic {
    use super::*;
    use std::os::unix::fs::{FileExt, MetadataExt};
    use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd};
    use std::sync::atomic::{AtomicU64, Ordering};

    pub const O_RDONLY: c_int = 0;
    pub const O_WRONLY: c_int = 1;
    #[cfg(test)]
    pub const O_ACCMODE: c_int = 3;

    #[cfg(target_os = "macos")]
    pub const O_NONBLOCK: c_int = 0x0004;
    #[cfg(target_os = "macos")]
    pub const O_NOFOLLOW: c_int = 0x0100;
    #[cfg(target_os = "macos")]
    pub const O_CREAT: c_int = 0x0200;
    #[cfg(target_os = "macos")]
    pub const O_EXCL: c_int = 0x0800;
    #[cfg(target_os = "macos")]
    pub const O_DIRECTORY: c_int = 0x100000;
    #[cfg(target_os = "macos")]
    pub const O_CLOEXEC: c_int = 0x1000000;
    #[cfg(all(target_os = "macos", test))]
    pub const ENOTSUP: i32 = 45;
    #[cfg(all(target_os = "macos", test))]
    pub const EOPNOTSUPP: i32 = 102;

    #[cfg(target_os = "linux")]
    pub const O_CREAT: c_int = 0o100;
    #[cfg(target_os = "linux")]
    pub const O_EXCL: c_int = 0o200;
    #[cfg(target_os = "linux")]
    pub const O_NONBLOCK: c_int = 0o4000;
    #[cfg(all(
        target_os = "linux",
        any(
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "powerpc",
            target_arch = "powerpc64",
            target_arch = "m68k"
        )
    ))]
    pub const O_DIRECTORY: c_int = 0o40000;
    #[cfg(all(
        target_os = "linux",
        any(
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "powerpc",
            target_arch = "powerpc64",
            target_arch = "m68k"
        )
    ))]
    pub const O_NOFOLLOW: c_int = 0o100000;
    #[cfg(all(
        target_os = "linux",
        not(any(
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "powerpc",
            target_arch = "powerpc64",
            target_arch = "m68k"
        ))
    ))]
    pub const O_DIRECTORY: c_int = 0o200000;
    #[cfg(all(
        target_os = "linux",
        not(any(
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "powerpc",
            target_arch = "powerpc64",
            target_arch = "m68k"
        ))
    ))]
    pub const O_NOFOLLOW: c_int = 0o400000;
    #[cfg(target_os = "linux")]
    pub const O_CLOEXEC: c_int = 0o2000000;
    #[cfg(target_os = "linux")]
    pub const ENOTSUP: i32 = 95;
    #[cfg(target_os = "linux")]
    pub const EOPNOTSUPP: i32 = 95;

    pub const EEXIST: i32 = 17;

    extern "C" {
        pub fn open(path: *const c_char, oflag: c_int, ...) -> c_int;
        pub fn openat(dirfd: c_int, pathname: *const c_char, flags: c_int, ...) -> c_int;
        pub fn close(fd: c_int) -> c_int;
        pub fn fchown(fd: c_int, owner: u32, group: u32) -> c_int;
        pub fn fchmod(fd: c_int, mode: u32) -> c_int;
        pub fn renameat(
            olddirfd: c_int,
            oldpath: *const c_char,
            newdirfd: c_int,
            newpath: *const c_char,
        ) -> c_int;
        pub fn unlinkat(dirfd: c_int, pathname: *const c_char, flags: c_int) -> c_int;
        #[cfg(target_os = "macos")]
        fn fcopyfile(from: c_int, to: c_int, state: *mut std::ffi::c_void, flags: u32) -> c_int;
        #[cfg(target_os = "linux")]
        fn flistxattr(fd: c_int, list: *mut c_char, size: usize) -> isize;
        #[cfg(target_os = "linux")]
        fn fgetxattr(
            fd: c_int,
            name: *const c_char,
            value: *mut std::ffi::c_void,
            size: usize,
        ) -> isize;
        #[cfg(target_os = "linux")]
        fn fsetxattr(
            fd: c_int,
            name: *const c_char,
            value: *const std::ffi::c_void,
            size: usize,
            flags: c_int,
        ) -> c_int;
    }

    pub fn same_cipher_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
        a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.size() == b.size()
            && a.mode() == b.mode()
            && a.uid() == b.uid()
            && a.gid() == b.gid()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }

    pub fn read_cipher_descriptor(file: &std::fs::File) -> Option<(Vec<u8>, std::fs::Metadata)> {
        let before = file.metadata().ok()?;
        if !before.file_type().is_file() || before.size() > 16 * 1024 * 1024 {
            return None;
        }
        let mut output = vec![0u8; before.size() as usize];
        let mut used = 0usize;
        while used < output.len() {
            match file.read_at(&mut output[used..], used as u64) {
                Ok(0) => return None,
                Ok(n) => used += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return None,
            }
        }
        let mut extra = [0u8; 1];
        match file.read_at(&mut extra, before.size()) {
            Ok(0) => {}
            _ => return None,
        }
        let after = file.metadata().ok()?;
        if !same_cipher_file(&before, &after) {
            return None;
        }
        Some((output, before))
    }

    #[cfg(any(target_os = "linux", test))]
    pub fn copy_cipher_xattrs_with(
        source: c_int,
        target: c_int,
        mut list_fn: impl FnMut(c_int, *mut u8, usize) -> Result<usize, i32>,
        mut get_fn: impl FnMut(c_int, &CStr, *mut u8, usize) -> Result<usize, i32>,
        mut set_fn: impl FnMut(c_int, &CStr, *const u8, usize, c_int) -> Result<(), i32>,
    ) -> bool {
        let size = match list_fn(source, std::ptr::null_mut(), 0) {
            Ok(s) => s,
            Err(err) => return err == ENOTSUP || err == EOPNOTSUPP,
        };
        if size > 65536 {
            return false;
        }
        let mut names = vec![0u8; size];
        if size > 0 {
            match list_fn(source, names.as_mut_ptr(), names.len()) {
                Ok(s) if s == size => {}
                _ => return false,
            }
        }
        let mut cursor = 0usize;
        let mut total = 0usize;
        while cursor < names.len() {
            let Some(length) = names[cursor..].iter().position(|&b| b == 0) else {
                return false;
            };
            let Ok(name_cstr) = CStr::from_bytes_with_nul(&names[cursor..cursor + length + 1])
            else {
                return false;
            };
            let Ok(bytes) = get_fn(source, name_cstr, std::ptr::null_mut(), 0) else {
                return false;
            };
            if bytes > 1024 * 1024 - total {
                return false;
            }
            let mut value = vec![0u8; bytes];
            let Ok(got) = get_fn(source, name_cstr, value.as_mut_ptr(), value.len()) else {
                return false;
            };
            if got != bytes || set_fn(target, name_cstr, value.as_ptr(), value.len(), 0).is_err() {
                return false;
            }
            total += value.len();
            cursor += length + 1;
        }
        true
    }

    pub fn copy_cipher_attributes(source: c_int, target: c_int) -> bool {
        #[cfg(target_os = "macos")]
        {
            const COPYFILE_SECURITY_AND_XATTR: u32 = (1 << 0) | (1 << 1) | (1 << 2);
            unsafe {
                fcopyfile(
                    source,
                    target,
                    std::ptr::null_mut(),
                    COPYFILE_SECURITY_AND_XATTR,
                ) == 0
            }
        }
        #[cfg(target_os = "linux")]
        {
            copy_cipher_xattrs_with(
                source,
                target,
                |fd, buf, size| {
                    let res = unsafe { flistxattr(fd, buf.cast::<c_char>(), size) };
                    if res < 0 {
                        Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
                    } else {
                        Ok(res as usize)
                    }
                },
                |fd, name, buf, size| {
                    let res = unsafe {
                        fgetxattr(fd, name.as_ptr(), buf.cast::<std::ffi::c_void>(), size)
                    };
                    if res < 0 {
                        Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
                    } else {
                        Ok(res as usize)
                    }
                },
                |fd, name, buf, size, flags| {
                    let res = unsafe {
                        fsetxattr(
                            fd,
                            name.as_ptr(),
                            buf.cast::<std::ffi::c_void>(),
                            size,
                            flags,
                        )
                    };
                    if res != 0 {
                        Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
                    } else {
                        Ok(())
                    }
                },
            )
        }
    }

    pub fn read_snapshot_unlocked(filename: &[u8]) -> Option<Vec<u8>> {
        let path_c = cstring_lossy(filename);
        let fd = unsafe {
            open(
                path_c.as_ptr(),
                O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            return None;
        }
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        read_cipher_descriptor(&file).map(|(bytes, _)| bytes)
    }

    pub fn commit_patch_unlocked(
        filename: &[u8],
        expected: &[u8],
        replacement: &[u8],
        mut cancel: impl FnMut() -> bool,
        mut copy_attrs: impl FnMut(c_int, c_int) -> bool,
        on_dir_sync_fail: impl FnOnce(),
    ) -> RsCipherCommit {
        if expected.len() > 16 * 1024 * 1024 || replacement.len() > 16 * 1024 * 1024 {
            return RsCipherCommit::Failed;
        }
        if cancel() {
            return RsCipherCommit::Cancelled;
        }
        let path = bytes_to_path(filename);
        let Some(leaf_os) = path.file_name() else {
            return RsCipherCommit::Failed;
        };
        let leaf_bytes = crate::cleanup::os_str_to_bytes(leaf_os);
        if leaf_bytes.is_empty() {
            return RsCipherCommit::Failed;
        }
        let parent_bytes = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or(&b"."[..], |p| {
                crate::cleanup::os_str_to_bytes(p.as_os_str())
            });
        let parent_c = cstring_lossy(parent_bytes);
        let leaf_c = cstring_lossy(leaf_bytes);

        let parent_fd = unsafe { open(parent_c.as_ptr(), O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0) };
        if parent_fd < 0 {
            return RsCipherCommit::Failed;
        }
        let parent_file = unsafe { std::fs::File::from_raw_fd(parent_fd) };

        let orig_fd = unsafe {
            openat(
                parent_file.as_raw_fd(),
                leaf_c.as_ptr(),
                O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC,
                0,
            )
        };
        if orig_fd < 0 {
            return RsCipherCommit::Failed;
        }
        let orig_file = unsafe { std::fs::File::from_raw_fd(orig_fd) };
        let Some((current, before)) = read_cipher_descriptor(&orig_file) else {
            return RsCipherCommit::Failed;
        };
        if current.as_slice() != expected {
            return RsCipherCommit::Conflict;
        }
        if current.as_slice() == replacement {
            return RsCipherCommit::Unchanged;
        }

        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let pid = std::process::id();
        let mut temp_c = CString::default();
        let mut out_fd: c_int = -1;
        for _ in 0..16 {
            let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let mut tmp_name = Vec::with_capacity(leaf_bytes.len() + 32);
            tmp_name.push(b'.');
            tmp_name.extend_from_slice(leaf_bytes);
            tmp_name.extend_from_slice(format!(".tls-{pid}-{seq}").as_bytes());
            temp_c = cstring_lossy(&tmp_name);
            out_fd = unsafe {
                openat(
                    parent_file.as_raw_fd(),
                    temp_c.as_ptr(),
                    O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                    0o600 as c_int,
                )
            };
            if out_fd >= 0 {
                break;
            }
            let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if err != EEXIST {
                break;
            }
        }
        if out_fd < 0 {
            return RsCipherCommit::Failed;
        }
        let output_file = unsafe { std::fs::File::from_raw_fd(out_fd) };

        struct RemoveTemporary<'a> {
            parent_fd: c_int,
            name: &'a CStr,
        }
        impl Drop for RemoveTemporary<'_> {
            fn drop(&mut self) {
                unsafe {
                    unlinkat(self.parent_fd, self.name.as_ptr(), 0);
                }
            }
        }
        let _cleanup = RemoveTemporary {
            parent_fd: parent_file.as_raw_fd(),
            name: &temp_c,
        };

        let mut used = 0usize;
        while used < replacement.len() {
            if cancel() {
                return RsCipherCommit::Cancelled;
            }
            match (&output_file).write(&replacement[used..]) {
                Ok(0) => return RsCipherCommit::Failed,
                Ok(n) => used += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return RsCipherCommit::Failed,
            }
        }

        let raw_orig_fd = orig_file.as_raw_fd();
        let raw_out_fd = output_file.as_raw_fd();
        if !copy_attrs(raw_orig_fd, raw_out_fd)
            || unsafe { fchown(raw_out_fd, before.uid(), before.gid()) } != 0
            || unsafe { fchmod(raw_out_fd, before.mode() & 0o7777) } != 0
            || output_file.sync_all().is_err()
        {
            return RsCipherCommit::Failed;
        }

        let fd_to_close = output_file.into_raw_fd();
        if unsafe { close(fd_to_close) } != 0 {
            return RsCipherCommit::Failed;
        }

        let Some((rechecked, after)) = read_cipher_descriptor(&orig_file) else {
            return RsCipherCommit::Conflict;
        };
        if !same_cipher_file(&before, &after) || rechecked.as_slice() != expected {
            return RsCipherCommit::Conflict;
        }

        let entry_fd = unsafe {
            openat(
                parent_file.as_raw_fd(),
                leaf_c.as_ptr(),
                O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC,
                0,
            )
        };
        if entry_fd < 0 {
            return RsCipherCommit::Conflict;
        }
        let entry_file = unsafe { std::fs::File::from_raw_fd(entry_fd) };
        let Ok(entry_meta) = entry_file.metadata() else {
            return RsCipherCommit::Conflict;
        };
        drop(entry_file);
        if !same_cipher_file(&before, &entry_meta) {
            return RsCipherCommit::Conflict;
        }

        if cancel() {
            return RsCipherCommit::Cancelled;
        }
        if unsafe {
            renameat(
                parent_file.as_raw_fd(),
                temp_c.as_ptr(),
                parent_file.as_raw_fd(),
                leaf_c.as_ptr(),
            )
        } != 0
        {
            return RsCipherCommit::Failed;
        }
        if parent_file.sync_all().is_err() {
            on_dir_sync_fail();
        }
        RsCipherCommit::Saved
    }
}

/// End-to-end implementation of `ScriptConfig::ReadCipherSnapshot`.
pub fn script_config_read_cipher_snapshot(filename: &[u8]) -> Option<Vec<u8>> {
    let _guard = CONFIG_WRITE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        cipher_atomic::read_snapshot_unlocked(filename)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = filename;
        None
    }
}

/// End-to-end implementation of `ScriptConfig::CommitCipherPatch`.
pub fn script_config_commit_cipher_patch(
    filename: &[u8],
    expected: &[u8],
    replacement: &[u8],
    cancel: impl FnMut() -> bool,
    on_dir_sync_fail: impl FnOnce(),
) -> RsCipherCommit {
    let _guard = CONFIG_WRITE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        cipher_atomic::commit_patch_unlocked(
            filename,
            expected,
            replacement,
            cancel,
            cipher_atomic::copy_cipher_attributes,
            on_dir_sync_fail,
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (filename, on_dir_sync_fail);
        let mut cancel = cancel;
        if expected.len() > 16 * 1024 * 1024 || replacement.len() > 16 * 1024 * 1024 {
            return RsCipherCommit::Failed;
        }
        if cancel() {
            return RsCipherCommit::Cancelled;
        }
        RsCipherCommit::Failed
    }
}

/// Builds the canonical versioned `v1\n` cipher baseline token in `ScriptConfig::CipherBaseline`.
pub fn build_cipher_baseline(names: &[&[u8]], values: &[&[u8]]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let count = names.len().min(values.len());
    let mut fields: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

    for i in 0..count {
        let raw_name = names[i];
        let mut name: Vec<u8> = raw_name
            .iter()
            .map(|&b| {
                if b.is_ascii_uppercase() {
                    b + (b'a' - b'A')
                } else {
                    b
                }
            })
            .collect();
        if !name.starts_with(b"server") {
            continue;
        }
        let dot = match name[6..].iter().position(|&b| b == b'.') {
            Some(pos) => 6 + pos,
            None => continue,
        };
        if dot == 6 || name[6] == b'0' || !name[6..dot].iter().all(|&c| c.is_ascii_digit()) {
            continue;
        }
        let suffix = &name[dot + 1..];
        if suffix != b"host" && suffix != b"port" && suffix != b"cipher" {
            continue;
        }
        let raw_val = values[i];
        let mut hex_val = Vec::with_capacity(raw_val.len() * 2);
        for &c in raw_val {
            hex_val.push(HEX[(c >> 4) as usize]);
            hex_val.push(HEX[(c & 15) as usize]);
        }
        fields.insert(std::mem::take(&mut name), hex_val);
    }

    let mut token = Vec::from(&b"v1\n"[..]);
    for (name, val) in fields {
        token.extend_from_slice(&name);
        token.push(b'=');
        token.extend_from_slice(&val);
        token.push(b'\n');
    }
    token
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_config_merge_and_parse() {
        let existing = b"# comment line\n\nServer1.Active=yes\nserver1.active=yes\nOrphan=1\n";
        let entries: &[(&[u8], &[u8])] = &[
            (b"Server1.Active", b"first"),
            (b"server1.active", b"no"),
            (b"NewOpt", b"val"),
        ];
        let merged = script_config_merge_bytes(existing, entries);
        assert_eq!(
            std::str::from_utf8(&merged).unwrap(),
            "# comment line\n\nServer1.Active=no\nNewOpt=val\n"
        );

        let parsed = script_config_parse_bytes(&merged);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0.as_bytes(), b"Server1.Active");
        assert_eq!(parsed[0].1.as_bytes(), b"no");
        assert_eq!(parsed[1].0.as_bytes(), b"NewOpt");
        assert_eq!(parsed[1].1.as_bytes(), b"val");
    }

    #[test]
    fn save_config_duplicate_and_orphaned_option_tests() {
        // SaveConfigDoesNotAppendDuplicateOptionNames
        let entries: &[(&[u8], &[u8])] = &[
            (b"Extension.Option", b"first"),
            (b"extension.option", b"second"),
        ];
        let first = script_config_merge_bytes(b"# existing config\n", entries);
        assert_eq!(first, b"# existing config\nExtension.Option=second\n");
        let second = script_config_merge_bytes(&first, entries);
        assert_eq!(second, first);

        // SaveConfigRemovesDuplicateLinesFromDamagedConfig
        let damaged = b"# existing config\n\
            Server1.Active=yes\n\
            Server2.Active=yes\n\
            Server2.Active=yes\n\
            server2.active=yes\n\
            Server2.Host=news.example.com\n";
        let entries2: &[(&[u8], &[u8])] = &[
            (b"Server1.Active", b"yes"),
            (b"Server2.Active", b"no"),
            (b"Server2.Host", b"news.example.com"),
        ];
        let merged2 = script_config_merge_bytes(damaged, entries2);
        assert_eq!(
            merged2,
            b"# existing config\n\
            Server1.Active=yes\n\
            Server2.Active=no\n\
            Server2.Host=news.example.com\n"
        );

        // SaveConfigKeepsLastValueWhenConfigHasDuplicateLines
        let dup_cfg = b"# existing config\n\
            Server2.Active=yes\n\
            Server2.Name=xsusenet\n\
            server2.active=no\n";
        let entries3: &[(&[u8], &[u8])] = &[
            (b"Server2.Active", b"yes"),
            (b"Server2.Name", b"xsusenet"),
            (b"server2.active", b"no"),
        ];
        let merged3 = script_config_merge_bytes(dup_cfg, entries3);
        assert_eq!(
            merged3,
            b"# existing config\n\
            Server2.Active=no\n\
            Server2.Name=xsusenet\n"
        );

        // SaveConfigDropsOrphanedOptionLines
        let orphan_cfg = b"# existing config\n\
            ActiveOption=yes\n\
            RemovedExtension.Option=old_value\n";
        let entries4: &[(&[u8], &[u8])] = &[(b"ActiveOption", b"yes")];
        let merged4 = script_config_merge_bytes(orphan_cfg, entries4);
        assert_eq!(merged4, b"# existing config\nActiveOption=yes\n");

        // SaveConfigNormalizesOptionNameCasing
        let case_cfg = b"# existing config\nserver2.active=yes\n";
        let entries5: &[(&[u8], &[u8])] = &[(b"Server2.Active", b"no")];
        let merged5 = script_config_merge_bytes(case_cfg, entries5);
        assert_eq!(merged5, b"# existing config\nServer2.Active=no\n");

        // SaveConfigLastLineWithoutLineEnd
        let no_nl_cfg = b"# existing config";
        let entries6: &[(&[u8], &[u8])] = &[(b"Extension.Option", b"value")];
        let merged6 = script_config_merge_bytes(no_nl_cfg, entries6);
        assert_eq!(merged6, b"# existing config\nExtension.Option=value\n");
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn script_config_cipher_patch_full_suite_port_from_cpp() {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        use std::sync::atomic::{AtomicU64, Ordering};

        static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

        struct TempConfigFile {
            path: PathBuf,
        }
        impl TempConfigFile {
            fn new() -> Self {
                let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "nzbget_rs_cipher_patch_{}_{}",
                    std::process::id(),
                    id
                ));
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_dir_all(&path);
                Self { path }
            }
            fn bytes(&self) -> &[u8] {
                self.path.as_os_str().as_bytes()
            }
        }
        impl Drop for TempConfigFile {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.path);
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum CipherAttributeFailure {
            None,
            InitialList,
            NamesList,
            GetSize,
            GetValue,
            Set,
        }

        const CIPHER_ATTR_NAME: &CStr = c"user.nzbget-cipher-fixture";
        const CIPHER_ATTR_VALUE: &[u8] = b"fixture-metadata";
        const F_GETFL: c_int = 3;
        const EIO: i32 = 5;
        const EBADF: i32 = 9;
        const ENOMEM: i32 = 12;
        const EACCES: i32 = 13;
        const EINVAL: i32 = 22;
        const ERANGE: i32 = 34;
        const ENODATA: i32 = 61;

        extern "C" {
            fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
        }

        fn check_cipher_attribute_transaction(
            stage: CipherAttributeFailure,
            error: i32,
            attributes: bool,
            saved: bool,
        ) {
            let file = TempConfigFile::new();
            let original = b"# original\r\nExtension.Option=original\r\n";
            let replacement = b"# original\r\nExtension.Option=replacement\r\n";
            std::fs::write(&file.path, original).unwrap();
            std::fs::set_permissions(&file.path, std::fs::Permissions::from_mode(0o640)).unwrap();

            let snapshot = script_config_read_cipher_snapshot(file.bytes()).unwrap();
            let before = std::fs::metadata(&file.path).unwrap();

            let source_fd = std::cell::Cell::new(-1 as c_int);
            let mut target_fd: c_int = -1;
            let mut lists = 0u32;
            let mut gets = 0u32;
            let mut sets = 0u32;
            let valid_arguments = std::cell::Cell::new(true);
            let mut observed_value = Vec::new();

            let result = {
                let _guard = CONFIG_WRITE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
                cipher_atomic::commit_patch_unlocked(
                    file.bytes(),
                    &snapshot,
                    replacement,
                    || false,
                    |src, dst| {
                        cipher_atomic::copy_cipher_xattrs_with(
                            src,
                            dst,
                            |fd, names, size| {
                                lists += 1;
                                if lists == 1 {
                                    source_fd.set(fd);
                                    let fl = unsafe { fcntl(fd, F_GETFL) };
                                    valid_arguments.set(
                                        valid_arguments.get()
                                            && names.is_null()
                                            && size == 0
                                            && (fl & cipher_atomic::O_ACCMODE)
                                                == cipher_atomic::O_RDONLY,
                                    );
                                    if stage == CipherAttributeFailure::InitialList {
                                        return Err(error);
                                    }
                                    if !attributes {
                                        return Ok(0);
                                    }
                                    return Ok(CIPHER_ATTR_NAME.to_bytes_with_nul().len());
                                }
                                let expected_len = CIPHER_ATTR_NAME.to_bytes_with_nul().len();
                                valid_arguments.set(
                                    valid_arguments.get()
                                        && fd == source_fd.get()
                                        && !names.is_null()
                                        && size == expected_len,
                                );
                                if stage == CipherAttributeFailure::NamesList {
                                    return Err(error);
                                }
                                if names.is_null() || size < expected_len {
                                    return Err(ERANGE);
                                }
                                unsafe {
                                    std::ptr::copy_nonoverlapping(
                                        CIPHER_ATTR_NAME.as_ptr().cast::<u8>(),
                                        names,
                                        expected_len,
                                    );
                                }
                                Ok(expected_len)
                            },
                            |fd, name, value, size| {
                                gets += 1;
                                valid_arguments.set(
                                    valid_arguments.get()
                                        && fd == source_fd.get()
                                        && name == CIPHER_ATTR_NAME,
                                );
                                if gets == 1 {
                                    valid_arguments
                                        .set(valid_arguments.get() && value.is_null() && size == 0);
                                    if stage == CipherAttributeFailure::GetSize {
                                        return Err(error);
                                    }
                                    return Ok(CIPHER_ATTR_VALUE.len());
                                }
                                valid_arguments.set(
                                    valid_arguments.get()
                                        && !value.is_null()
                                        && size == CIPHER_ATTR_VALUE.len(),
                                );
                                if stage == CipherAttributeFailure::GetValue {
                                    return Err(error);
                                }
                                if value.is_null() || size < CIPHER_ATTR_VALUE.len() {
                                    return Err(ERANGE);
                                }
                                unsafe {
                                    std::ptr::copy_nonoverlapping(
                                        CIPHER_ATTR_VALUE.as_ptr(),
                                        value,
                                        CIPHER_ATTR_VALUE.len(),
                                    );
                                }
                                Ok(CIPHER_ATTR_VALUE.len())
                            },
                            |fd, name, value, size, flags| {
                                sets += 1;
                                target_fd = fd;
                                let fl = unsafe { fcntl(fd, F_GETFL) };
                                valid_arguments.set(
                                    valid_arguments.get()
                                        && fd != source_fd.get()
                                        && name == CIPHER_ATTR_NAME
                                        && !value.is_null()
                                        && size == CIPHER_ATTR_VALUE.len()
                                        && flags == 0
                                        && (fl & cipher_atomic::O_ACCMODE)
                                            == cipher_atomic::O_WRONLY,
                                );
                                if value.is_null() || size != CIPHER_ATTR_VALUE.len() {
                                    return Err(EINVAL);
                                }
                                observed_value =
                                    unsafe { std::slice::from_raw_parts(value, size) }.to_vec();
                                if stage == CipherAttributeFailure::Set {
                                    return Err(error);
                                }
                                Ok(())
                            },
                        )
                    },
                    || {},
                )
            };

            assert_eq!(
                result,
                if saved {
                    RsCipherCommit::Saved
                } else {
                    RsCipherCommit::Failed
                }
            );
            assert_eq!(
                std::fs::read(&file.path).unwrap().as_slice(),
                if saved {
                    &replacement[..]
                } else {
                    &original[..]
                }
            );
            let after = std::fs::metadata(&file.path).unwrap();
            assert_eq!(after.mode(), before.mode());
            assert_eq!(after.uid(), before.uid());
            assert_eq!(after.gid(), before.gid());
            if saved {
                assert_ne!(after.ino(), before.ino());
            } else {
                assert!(cipher_atomic::same_cipher_file(&before, &after));
            }
            assert!(valid_arguments.get());
            assert!(source_fd.get() >= 0);
            assert_eq!(
                lists,
                if stage == CipherAttributeFailure::InitialList || !attributes {
                    1
                } else {
                    2
                }
            );
            assert_eq!(
                gets,
                if !attributes || stage == CipherAttributeFailure::NamesList {
                    0
                } else if stage == CipherAttributeFailure::GetSize {
                    1
                } else {
                    2
                }
            );
            assert_eq!(
                sets,
                if attributes
                    && (stage == CipherAttributeFailure::Set
                        || stage == CipherAttributeFailure::None)
                {
                    1
                } else {
                    0
                }
            );
            if sets > 0 {
                assert!(target_fd >= 0 && target_fd != source_fd.get());
                assert_eq!(observed_value, CIPHER_ATTR_VALUE);
            }
            let prefix = format!(".{}.tls-", file.path.file_name().unwrap().to_string_lossy());
            for entry in std::fs::read_dir(file.path.parent().unwrap()).unwrap() {
                let entry = entry.unwrap();
                assert!(!entry.file_name().to_string_lossy().starts_with(&prefix));
            }
        }

        // 1. CipherPatchUnsupportedSourceAttributes
        for error in [cipher_atomic::ENOTSUP, cipher_atomic::EOPNOTSUPP] {
            check_cipher_attribute_transaction(
                CipherAttributeFailure::InitialList,
                error,
                false,
                true,
            );
        }
        for error in [EACCES, EIO, EBADF, ENOMEM, ERANGE, EINVAL] {
            check_cipher_attribute_transaction(
                CipherAttributeFailure::InitialList,
                error,
                false,
                false,
            );
        }

        // 2. CipherPatchAttributeCopyFailuresPreserveOriginal
        for stage in [
            CipherAttributeFailure::NamesList,
            CipherAttributeFailure::GetSize,
            CipherAttributeFailure::GetValue,
            CipherAttributeFailure::Set,
        ] {
            for error in [
                cipher_atomic::ENOTSUP,
                cipher_atomic::EOPNOTSUPP,
                EACCES,
                EIO,
                ENODATA,
            ] {
                check_cipher_attribute_transaction(stage, error, true, false);
            }
        }

        // 3. CipherPatchAttributeCopyControls
        check_cipher_attribute_transaction(CipherAttributeFailure::None, EACCES, false, true);
        check_cipher_attribute_transaction(CipherAttributeFailure::None, 0, true, true);

        // 4. CipherPatchConflictPreservesOriginal
        {
            let file = TempConfigFile::new();
            std::fs::write(&file.path, b"# unchanged\nExtension.Option=original\n").unwrap();
            let snapshot = script_config_read_cipher_snapshot(file.bytes()).unwrap();
            let concurrent = b"# concurrent change\nExtension.Option=new\n";
            std::fs::write(&file.path, concurrent).unwrap();
            assert_eq!(
                script_config_commit_cipher_patch(
                    file.bytes(),
                    &snapshot,
                    b"replacement",
                    || false,
                    || {}
                ),
                RsCipherCommit::Conflict
            );
            assert_eq!(std::fs::read(&file.path).unwrap(), concurrent);
        }

        // 5. CipherPatchPreservesModeAndExactBytes
        {
            let file = TempConfigFile::new();
            std::fs::write(&file.path, b"# original\r\nExtension.Option=private\r\n").unwrap();
            std::fs::set_permissions(&file.path, std::fs::Permissions::from_mode(0o640)).unwrap();
            let snapshot = script_config_read_cipher_snapshot(file.bytes()).unwrap();
            let mut replacement = snapshot.clone();
            replacement.extend_from_slice(b"# appended\r\n");
            assert_eq!(
                script_config_commit_cipher_patch(
                    file.bytes(),
                    &snapshot,
                    &replacement,
                    || false,
                    || {}
                ),
                RsCipherCommit::Saved
            );
            assert_eq!(std::fs::read(&file.path).unwrap(), replacement);
            let meta = std::fs::metadata(&file.path).unwrap();
            assert_eq!(meta.mode() & 0o7777, 0o640);
        }

        // 6. CipherPatchRejectsSymlink
        {
            let file = TempConfigFile::new();
            let link = TempConfigFile::new();
            std::fs::write(&file.path, b"Extension.Option=original\n").unwrap();
            std::os::unix::fs::symlink(&file.path, &link.path).unwrap();
            assert!(script_config_read_cipher_snapshot(link.bytes()).is_none());
            assert_eq!(
                script_config_commit_cipher_patch(
                    link.bytes(),
                    b"",
                    b"replacement",
                    || false,
                    || {}
                ),
                RsCipherCommit::Failed
            );
            assert_eq!(
                std::fs::read(&file.path).unwrap(),
                b"Extension.Option=original\n"
            );
        }

        // 7. CipherPatchRejectsDirectory
        {
            let dir = TempConfigFile::new();
            std::fs::create_dir(&dir.path).unwrap();
            assert!(script_config_read_cipher_snapshot(dir.bytes()).is_none());
            assert_eq!(
                script_config_commit_cipher_patch(
                    dir.bytes(),
                    b"",
                    b"replacement",
                    || false,
                    || {}
                ),
                RsCipherCommit::Failed
            );
            assert!(dir.path.is_dir());
        }

        // 8. CipherPatchSerializesOrdinarySave
        {
            let file = TempConfigFile::new();
            std::fs::write(
                &file.path,
                b"Server1.Host=example.invalid\nServer1.Cipher=old\n",
            )
            .unwrap();
            let snapshot = script_config_read_cipher_snapshot(file.bytes()).unwrap();
            let saved = script_config_load(file.bytes()).unwrap();
            let baseline = script_config_cipher_baseline_cstrings(&saved);

            let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
            let (entered_tx, entered_rx) = std::sync::mpsc::channel::<()>();
            let (done_tx, done_rx) = std::sync::mpsc::channel::<bool>();
            let file_path_clone = file.path.clone();
            let baseline_clone = baseline.clone();

            let writer = std::thread::spawn(move || {
                go_rx.recv().unwrap();
                entered_tx.send(()).unwrap();
                let entries: &[(&[u8], &[u8])] = &[
                    (b"Server1.Host", b"example.invalid"),
                    (b"Server1.Cipher", b"ordinary"),
                ];
                let (ok, _) = script_config_save(
                    file_path_clone.as_os_str().as_bytes(),
                    entries,
                    Some(&baseline_clone),
                );
                done_tx.send(ok).unwrap();
            });

            let mut triggered = false;
            let mut blocked = false;
            let committed = script_config_commit_cipher_patch(
                file.bytes(),
                &snapshot,
                b"Server1.Host=example.invalid\nServer1.Cipher=optimized\n",
                || {
                    if !triggered {
                        triggered = true;
                        let _ = go_tx.send(());
                        if entered_rx
                            .recv_timeout(std::time::Duration::from_secs(1))
                            .is_ok()
                        {
                            blocked = done_rx
                                .recv_timeout(std::time::Duration::from_millis(25))
                                .is_err();
                        }
                    }
                    false
                },
                || {},
            );
            if !triggered {
                let _ = go_tx.send(());
            }
            writer.join().unwrap();
            assert!(blocked);
            assert_eq!(committed, RsCipherCommit::Saved);
            assert!(!done_rx.recv().unwrap());
            let final_snap = script_config_read_cipher_snapshot(file.bytes()).unwrap();
            assert_eq!(
                final_snap,
                b"Server1.Host=example.invalid\nServer1.Cipher=optimized\n"
            );
        }

        // 9. CipherBaselineCanonicalValuesAndOrdinarySaveFirst
        {
            let parity: &[(&[u8], &[u8])] = &[
                (b"Server1.Host", b"h\xc3\xa9"),
                (b"Server1.Cipher", b"old"),
                (b"server1.cipher", b""),
                (b"Server1.Port", b"563"),
                (b"Server01.Host", b"ignored"),
            ];
            assert_eq!(
                script_config_cipher_baseline(parity),
                b"v1\nserver1.cipher=\nserver1.host=68c3a9\nserver1.port=353633\n"
            );
            let absent: &[(&[u8], &[u8])] =
                &[(b"Server1.Host", b"h\xc3\xa9"), (b"Server1.Port", b"563")];
            assert_ne!(
                script_config_cipher_baseline(parity),
                script_config_cipher_baseline(absent)
            );

            let file = TempConfigFile::new();
            std::fs::write(
                &file.path,
                b"Server1.Host=example.invalid\nServer1.Cipher=old\n",
            )
            .unwrap();
            let original = script_config_read_cipher_snapshot(file.bytes()).unwrap();
            let loaded = script_config_load(file.bytes()).unwrap();
            let token = script_config_cipher_baseline_cstrings(&loaded);
            let mut entry_pairs: Vec<(&[u8], &[u8])> = loaded
                .iter()
                .map(|(k, v)| (k.as_bytes(), v.as_bytes()))
                .collect();
            entry_pairs.push((b"Server1.Cipher", b"ordinary"));
            let (saved_ok, conflict) = script_config_save(file.bytes(), &entry_pairs, Some(&token));
            assert!(saved_ok);
            assert!(!conflict);
            assert_eq!(
                script_config_commit_cipher_patch(
                    file.bytes(),
                    &original,
                    b"Server1.Host=example.invalid\nServer1.Cipher=optimized\n",
                    || false,
                    || {}
                ),
                RsCipherCommit::Conflict
            );
            let changed = std::fs::read(&file.path).unwrap();
            let (saved_ok2, conflict2) =
                script_config_save(file.bytes(), &entry_pairs, Some(&token));
            assert!(!saved_ok2);
            assert!(conflict2);
            assert_eq!(std::fs::read(&file.path).unwrap(), changed);
        }

        // 10. CipherPatchCancelledDuringWritePreservesOriginal
        {
            let file = TempConfigFile::new();
            std::fs::write(&file.path, b"Extension.Option=original\n").unwrap();
            let snapshot = script_config_read_cipher_snapshot(file.bytes()).unwrap();
            let mut calls = 0;
            assert_eq!(
                script_config_commit_cipher_patch(
                    file.bytes(),
                    &snapshot,
                    b"replacement",
                    || {
                        calls += 1;
                        calls >= 2
                    },
                    || {}
                ),
                RsCipherCommit::Cancelled
            );
            assert_eq!(std::fs::read(&file.path).unwrap(), snapshot);
        }

        // 11. CancelAfterCommitReportsSaved
        {
            let file = TempConfigFile::new();
            std::fs::write(&file.path, b"Extension.Option=original\n").unwrap();
            let snapshot = script_config_read_cipher_snapshot(file.bytes()).unwrap();
            let mut calls = 0;
            assert_eq!(
                script_config_commit_cipher_patch(
                    file.bytes(),
                    &snapshot,
                    b"replacement",
                    || {
                        calls += 1;
                        calls > 3
                    },
                    || {}
                ),
                RsCipherCommit::Saved
            );
            assert_eq!(std::fs::read(&file.path).unwrap(), b"replacement");
        }

        // 12. CipherPatchCreationFailurePreservesOriginal
        {
            let mut file = TempConfigFile::new();
            file.path = file.path.parent().unwrap().join("x".repeat(250));
            std::fs::write(&file.path, b"Extension.Option=original\n").unwrap();
            let snapshot = script_config_read_cipher_snapshot(file.bytes()).unwrap();
            assert_eq!(
                script_config_commit_cipher_patch(
                    file.bytes(),
                    &snapshot,
                    b"replacement",
                    || false,
                    || {}
                ),
                RsCipherCommit::Failed
            );
            assert_eq!(std::fs::read(&file.path).unwrap(), snapshot);
        }
    }

}
