//! End-to-end Rust implementation of `daemon/extension/` (`ScriptConfig`,
//! `ExtensionManager`, `NzbScriptController`, `ScanScriptController`,
//! `PostScriptController`, and `QueueScriptController`).

use std::ffi::{c_char, c_int, CStr, CString};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

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

fn io_err_msg(err: &std::io::Error) -> String {
    if let Some(code) = err.raw_os_error() {
        extern "C" {
            fn strerror(errnum: c_int) -> *const c_char;
        }
        let ptr = unsafe { strerror(code) };
        if !ptr.is_null() {
            return unsafe { CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned();
        }
    }
    err.to_string()
}

fn fs_remove_all(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
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
        if let Some((raw_name, raw_val)) =
            crate::options::options_split_option_string(line)
        {
            let name_c = cstring_lossy(raw_name);
            let val_c = cstring_lossy(raw_val);
            let (conv_name, conv_val) = crate::options::convert_old_option(&name_c, &val_c);
            entries.push((conv_name, conv_val));
        }
    }
    crate::options::convert_old_options(&mut entries);
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
            if let Some((raw_name, raw_val)) =
                crate::options::options_split_option_string(trimmed)
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
        fn fcopyfile(
            from: c_int,
            to: c_int,
            state: *mut std::ffi::c_void,
            flags: u32,
        ) -> c_int;
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

    pub fn read_cipher_descriptor(
        file: &std::fs::File,
    ) -> Option<(Vec<u8>, std::fs::Metadata)> {
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
            if got != bytes
                || set_fn(target, name_cstr, value.as_ptr(), value.len(), 0).is_err()
            {
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
            .map_or(&b"."[..], |p| crate::cleanup::os_str_to_bytes(p.as_os_str()));
        let parent_c = cstring_lossy(parent_bytes);
        let leaf_c = cstring_lossy(leaf_bytes);

        let parent_fd = unsafe {
            open(
                parent_c.as_ptr(),
                O_RDONLY | O_DIRECTORY | O_CLOEXEC,
                0,
            )
        };
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

/// Checks whether `target_name` is in the `,;`-separated `extensions_list` (exact byte match),
/// matching `ExtensionManager::Manager::CreateTasks`.
pub fn ext_is_in_list(extensions_list: &[u8], target_name: &[u8]) -> bool {
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

    for raw_tok in extensions_list.split(|&b| b == b',' || b == b';') {
        let tok = trim_tokenizer_ws(raw_tok);
        if !tok.is_empty() && tok == target_name {
            return true;
        }
    }
    false
}

/// Computes the sorted index permutation for `ExtensionManager::Manager::Sort`.
pub fn ext_sort_indices(names: &[&[u8]], order_str: &[u8]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..names.len()).collect();
    fn trim_tok(mut s: &[u8]) -> &[u8] {
        while let Some(&b) = s.first() {
            if b <= b' ' {
                s = &s[1..];
            } else {
                break;
            }
        }
        while let Some(&b) = s.last() {
            if b <= b' ' {
                s = &s[..s.len() - 1];
            } else {
                break;
            }
        }
        s
    }

    let mut order: Vec<&[u8]> = Vec::new();
    for raw in order_str.split(|&b| b == b',' || b == b';') {
        let tok = trim_tok(raw);
        if !tok.is_empty() && !order.contains(&tok) {
            order.push(tok);
        }
    }

    if order.is_empty() {
        indices.sort_by(|&a, &b| names[a].cmp(names[b]));
        return indices;
    }

    let mut count = 0usize;
    for target in &order {
        if let Some(pos) = indices.iter().position(|&idx| names[idx] == *target) {
            indices.swap(count, pos);
            count += 1;
        }
    }

    indices[count..].sort_by(|&a, &b| names[a].cmp(names[b]));
    indices
}

/// Extracts the string value for `"<key>"` from `package-info.json` in `Maintenance::ReadPackageInfoStr`.
/// Returns `Some((value_start, value_len))` if found and `value_len < max_len`.
pub fn maintenance_extract_package_info_str(
    json: &[u8],
    key: &[u8],
    max_len: usize,
) -> Option<(usize, usize)> {
    let mut quoted_key = Vec::with_capacity(key.len() + 2);
    quoted_key.push(b'"');
    let key_trunc = if key.len() > 97 { &key[..97] } else { key };
    quoted_key.extend_from_slice(key_trunc);
    if quoted_key.len() < 99 {
        quoted_key.push(b'"');
    }

    let key_pos = json
        .windows(quoted_key.len())
        .position(|w| w == quoted_key.as_slice())?;
    let after_key = key_pos + quoted_key.len();
    let quote_open_rel = json[after_key..].iter().position(|&b| b == b'"')?;
    let val_start = after_key + quote_open_rel + 1;
    let quote_close_rel = json[val_start..].iter().position(|&b| b == b'"')?;
    if quote_close_rel >= max_len {
        return None;
    }
    Some((val_start, quote_close_rel))
}

/// Classifies a script output line (after stripping `m_prefixLen`) in `UpdateScriptController::AddMessage`:
/// - `0`: regular log line (does not start with `"[NZB] "`)
/// - `1`: `"[NZB] QUIT"` command
/// - `-1`: invalid `"[NZB] ..."` command
pub fn maintenance_classify_update_message(text: &[u8]) -> i32 {
    if !text.starts_with(b"[NZB] ") {
        0
    } else if &text[6..] == b"QUIT" {
        1
    } else {
        -1
    }
}

/// Classifies a script output line (after stripping `m_prefixLen`) in `UpdateInfoScriptController::AddMessage`:
/// - `0`: regular log line (does not start with `"[NZB] "`)
/// - `1`: `"[NZB] [UPDATEINFO]..."` command (payload starts at `text + 18`)
/// - `-1`: invalid `"[NZB] ..."` command
pub fn maintenance_classify_update_info_message(text: &[u8]) -> i32 {
    if !text.starts_with(b"[NZB] ") {
        0
    } else if text[6..].starts_with(b"[UPDATEINFO]") {
        1
    } else {
        -1
    }
}

/// Decodes a hex-encoded RSA-SHA256 signature line tail in `Signature::ReadSignature`
/// (starting after `sigTitle`, trimming 2 trailing bytes when `sig_len > 2`, and decoding
/// up to 256 bytes into `out_sig`). Returns `true` iff exactly 256 bytes were decoded.
pub fn maintenance_decode_hex_signature(hex_sig: &[u8], out_sig: &mut [u8; 256]) -> bool {
    let eff_len = if hex_sig.len() > 2 {
        hex_sig.len() - 2
    } else {
        hex_sig.len()
    };
    let slice = &hex_sig[..eff_len];
    let mut in_idx = 0;
    let mut out_idx = 0;
    while in_idx + 1 < slice.len() && out_idx < 256 {
        let c1 = slice[in_idx];
        let c2 = slice[in_idx + 1];
        in_idx += 2;
        let n1 = match c1 {
            b'0'..=b'9' => c1 - b'0',
            b'A'..=b'F' => c1 - b'A' + 10,
            b'a'..=b'f' => c1 - b'a' + 10,
            _ => 0,
        };
        let n2 = match c2 {
            b'0'..=b'9' => c2 - b'0',
            b'A'..=b'F' => c2 - b'A' + 10,
            b'a'..=b'f' => c2 - b'a' + 10,
            _ => 0,
        };
        out_sig[out_idx] = (n1 << 4).wrapping_add(n2);
        out_idx += 1;
    }
    out_idx == 256
}

/// Phase 1 of `ExtensionManager::Manager::UpdateExtension`: validates lookup and
/// `use_count`, moves `target_path` aside to `<target_path>.update-backup`, and
/// cleans up `archive_path` if moving aside fails.
/// Returns `(backup_path, None)` on success or `(backup_path, Some(error))` on failure.
pub fn ext_mgr_prepare_update(
    ext_name: &[u8],
    found: bool,
    use_count: i64,
    target_path: &[u8],
    archive_path: &[u8],
) -> (Vec<u8>, Option<Vec<u8>>) {
    let ext_str = String::from_utf8_lossy(ext_name);
    if !found {
        return (
            Vec::new(),
            Some(format!("Update failed: Extension '{ext_str}' not found").into_bytes()),
        );
    }
    if use_count > 1 {
        return (
            Vec::new(),
            Some(format!("Update failed: Extension '{ext_str}' is currently in use").into_bytes()),
        );
    }

    let mut backup_bytes = Vec::with_capacity(target_path.len() + 14);
    backup_bytes.extend_from_slice(target_path);
    backup_bytes.extend_from_slice(b".update-backup");

    let target = bytes_to_path(target_path);
    let backup = bytes_to_path(&backup_bytes);
    let _ = fs_remove_all(&backup);

    if let Err(ec) = std::fs::rename(&target, &backup) {
        let target_str = String::from_utf8_lossy(target_path);
        let move_err = format!(
            "Cannot move old extension from '{}' ({})",
            target_str,
            io_err_msg(&ec)
        );
        let archive = bytes_to_path(archive_path);
        if let Err(ec2) = std::fs::remove_file(&archive) {
            let archive_str = String::from_utf8_lossy(archive_path);
            let compound = format!(
                "{move_err} and cannot delete downloaded archive '{archive_str}': {}",
                io_err_msg(&ec2)
            );
            return (backup_bytes, Some(compound.into_bytes()));
        }
        return (backup_bytes, Some(move_err.into_bytes()));
    }

    (backup_bytes, None)
}

/// Phase 2 of `ExtensionManager::Manager::UpdateExtension`: either cleans up
/// `backup_path` after a successful install, or restores `backup_path` back to
/// `target_path` when `install_err` is `Some`.
pub fn ext_mgr_finish_update(
    target_path: &[u8],
    backup_path: &[u8],
    archive_path: &[u8],
    install_err: Option<&[u8]>,
) -> Option<Vec<u8>> {
    let target = bytes_to_path(target_path);
    let backup = bytes_to_path(backup_path);
    if let Some(err_bytes) = install_err {
        let archive = bytes_to_path(archive_path);
        let _ = std::fs::remove_file(&archive);
        let _ = fs_remove_all(&target);
        if let Err(ec) = std::fs::rename(&backup, &target) {
            let err_str = String::from_utf8_lossy(err_bytes);
            let target_str = String::from_utf8_lossy(target_path);
            let backup_str = String::from_utf8_lossy(backup_path);
            return Some(
                format!(
                    "{err_str} and cannot restore previous extension to '{target_str}' from '{backup_str}': {}",
                    io_err_msg(&ec)
                )
                .into_bytes(),
            );
        }
        return Some(err_bytes.to_vec());
    }

    let _ = fs_remove_all(&backup);
    None
}

/// End-to-end validation and removal for `ExtensionManager::Manager::DeleteExtension`.
pub fn ext_mgr_delete_extension(
    ext_name: &[u8],
    found: bool,
    use_count: i64,
    target_path: &[u8],
) -> Option<Vec<u8>> {
    let ext_str = String::from_utf8_lossy(ext_name);
    if !found {
        return Some(format!("Deletion failed: Extension '{ext_str}' not found").into_bytes());
    }
    if use_count > 1 {
        return Some(
            format!("Deletion failed: Extension '{ext_str}' is currently in use").into_bytes(),
        );
    }

    let target = bytes_to_path(target_path);
    if let Err(ec) = fs_remove_all(&target) {
        let target_str = String::from_utf8_lossy(target_path);
        return Some(
            format!("Cannot delete directory '{target_str}': {}", io_err_msg(&ec)).into_bytes(),
        );
    }
    None
}

/// Formats error messages for `ExtensionManager::Manager::InstallExtension`.
pub fn ext_mgr_install_error(file_path: &[u8], os_err: Option<&[u8]>, kind: i32) -> Vec<u8> {
    let file_str = String::from_utf8_lossy(file_path);
    match kind {
        0 => format!("Unsupported archive type for file: {file_str}").into_bytes(),
        1 => format!("Failed to extract archive: {file_str}").into_bytes(),
        _ => {
            let err_str = String::from_utf8_lossy(os_err.unwrap_or(b""));
            format!("Cannot delete archive '{file_str}': {err_str}").into_bytes()
        }
    }
}

/// End-to-end implementation of `NzbScriptController::PrepareEnvScript` and
/// `NzbScriptController::PrepareEnvParameters`.
pub fn nzb_script_prepare_env(
    script_name: &[u8],
    has_parameters: bool,
    param_names: &[&CStr],
    param_values: &[&CStr],
    mut set_env_special: impl FnMut(&CStr, &CStr, &CStr),
    mut prepare_env_options: impl FnMut(&CStr),
) {
    if has_parameters {
        for (&name, &val) in param_names.iter().zip(param_values.iter()) {
            set_env_special(c"NZBPR", name, val);
        }
    }

    let mut prefix_bytes = Vec::with_capacity(script_name.len() + 2);
    if let Some(nul) = script_name.iter().position(|&b| b == 0) {
        prefix_bytes.extend_from_slice(&script_name[..nul]);
    } else {
        prefix_bytes.extend_from_slice(script_name);
    }
    prefix_bytes.push(b':');
    prefix_bytes.push(0);
    let prefix_cstr = CStr::from_bytes_with_nul(&prefix_bytes).expect("NUL-terminated");
    let prefix_slice = prefix_cstr.to_bytes();
    let prefix_len = prefix_slice.len();

    if has_parameters && prefix_len > 0 {
        for (&name, &val) in param_names.iter().zip(param_values.iter()) {
            let name_bytes = name.to_bytes();
            if name_bytes.len() > prefix_len && name_bytes.starts_with(prefix_slice) {
                let stripped = CStr::from_bytes_with_nul(&name.to_bytes_with_nul()[prefix_len..])
                    .expect("NUL-terminated suffix");
                set_env_special(c"NZBPR", stripped, val);
            }
        }
    }

    prepare_env_options(prefix_cstr);
}

fn dupe_mode_cstr(dupe_mode: i32) -> &'static CStr {
    match dupe_mode {
        1 => c"ALL",
        2 => c"FORCE",
        _ => c"SCORE",
    }
}

/// End-to-end implementation of `ScanScriptController::PrepareParams`.
#[allow(clippy::too_many_arguments)]
pub fn scan_script_prepare_env(
    nzb_filename: &CStr,
    base_filename: &CStr,
    url: &CStr,
    nzb_name: &CStr,
    category: &CStr,
    priority: i32,
    add_top: bool,
    add_paused: bool,
    dupe_key: &CStr,
    dupe_score: i32,
    dupe_mode: i32,
    directory: &[u8],
    path_sep: u8,
    mut set_env_var: impl FnMut(&CStr, &CStr),
    mut set_int_env_var: impl FnMut(&CStr, i32),
) {
    set_env_var(c"NZBNP_FILENAME", nzb_filename);
    set_env_var(c"NZBNP_URL", url);
    let effective_name = if !nzb_name.to_bytes().is_empty() {
        nzb_name
    } else {
        base_filename
    };
    set_env_var(c"NZBNP_NZBNAME", effective_name);
    set_env_var(c"NZBNP_CATEGORY", category);
    set_int_env_var(c"NZBNP_PRIORITY", priority);
    set_int_env_var(c"NZBNP_TOP", i32::from(add_top));
    set_int_env_var(c"NZBNP_PAUSED", i32::from(add_paused));
    set_env_var(c"NZBNP_DUPEKEY", dupe_key);
    set_int_env_var(c"NZBNP_DUPESCORE", dupe_score);
    set_env_var(c"NZBNP_DUPEMODE", dupe_mode_cstr(dupe_mode));

    let trimmed_dir = if directory.last() == Some(&path_sep) {
        &directory[..directory.len() - 1]
    } else {
        directory
    };
    let dir_c = cstring_lossy(trimmed_dir);
    set_env_var(c"NZBNP_DIRECTORY", &dir_c);
}

pub struct PostScriptEnvInput<'a> {
    pub nzb_id: i32,
    pub nzb_name: &'a CStr,
    pub dest_dir: &'a CStr,
    pub filename: &'a CStr,
    pub queued_filename: &'a CStr,
    pub url: &'a CStr,
    pub final_dir: &'a CStr,
    pub category: &'a CStr,
    pub health: i32,
    pub critical_health: i32,
    pub dupe_key: &'a CStr,
    pub dupe_score: i32,
    pub dupe_mode: i32,
    pub status: &'a CStr,
    pub script_status: i32,
    pub par_status: i32,
    pub unpack_status: i32,
    pub delete_status: i32,
    pub mark_status: i32,
    pub total_articles: i32,
    pub success_articles: i32,
    pub failed_articles: i32,
    pub server_ids: &'a [i32],
    pub server_success_articles: &'a [i32],
    pub server_failed_articles: &'a [i32],
}

fn post_script_deprecated_status_code(status: i32) -> i32 {
    match status {
        0 | 1 => 0,
        2 => 1,
        3 => 2,
        4 => 3,
        5 => 4,
        _ => 0,
    }
}

/// End-to-end implementation of `PostScriptController::PrepareParams`.
pub fn post_script_prepare_env(
    inp: &PostScriptEnvInput<'_>,
    mut set_env_var: impl FnMut(&CStr, &CStr),
    mut set_int_env_var: impl FnMut(&CStr, i32),
) {
    set_int_env_var(c"NZBPP_NZBID", inp.nzb_id);
    set_env_var(c"NZBPP_NZBNAME", inp.nzb_name);
    set_env_var(c"NZBPP_DIRECTORY", inp.dest_dir);
    set_env_var(c"NZBPP_NZBFILENAME", inp.filename);
    set_env_var(c"NZBPP_QUEUEDFILE", inp.queued_filename);
    set_env_var(c"NZBPP_URL", inp.url);
    set_env_var(c"NZBPP_FINALDIR", inp.final_dir);
    set_env_var(c"NZBPP_CATEGORY", inp.category);
    set_int_env_var(c"NZBPP_HEALTH", inp.health);
    set_int_env_var(c"NZBPP_CRITICALHEALTH", inp.critical_health);
    set_env_var(c"NZBPP_DUPEKEY", inp.dupe_key);
    set_int_env_var(c"NZBPP_DUPESCORE", inp.dupe_score);
    set_env_var(c"NZBPP_DUPEMODE", dupe_mode_cstr(inp.dupe_mode));
    set_env_var(c"NZBPP_STATUS", inp.status);

    let status_bytes = inp.status.to_bytes();
    let total_len = status_bytes
        .iter()
        .position(|&b| b == b'/')
        .unwrap_or(status_bytes.len());
    let total_status = cstring_lossy(&status_bytes[..total_len]);
    set_env_var(c"NZBPP_TOTALSTATUS", &total_status);

    let script_status_name = match inp.script_status {
        1 => c"FAILURE",
        2 => c"SUCCESS",
        _ => c"NONE",
    };
    set_env_var(c"NZBPP_SCRIPTSTATUS", script_status_name);

    // psFailure = 2; dsNone = 0; ksBad = 1
    let compat_par_status = if inp.delete_status != 0 || inp.mark_status == 1 {
        2
    } else {
        inp.par_status
    };
    set_int_env_var(
        c"NZBPP_PARSTATUS",
        post_script_deprecated_status_code(compat_par_status),
    );
    set_int_env_var(
        c"NZBPP_UNPACKSTATUS",
        post_script_deprecated_status_code(inp.unpack_status),
    );
    // dsHealth = 2
    set_int_env_var(c"NZBPP_HEALTHDELETED", i32::from(inp.delete_status == 2));

    set_int_env_var(c"NZBPP_TOTALARTICLES", inp.total_articles);
    set_int_env_var(c"NZBPP_SUCCESSARTICLES", inp.success_articles);
    set_int_env_var(c"NZBPP_FAILEDARTICLES", inp.failed_articles);

    let count = inp
        .server_ids
        .len()
        .min(inp.server_success_articles.len())
        .min(inp.server_failed_articles.len());
    for i in 0..count {
        let srv_id = inp.server_ids[i];
        let succ_name =
            CString::new(format!("NZBPP_SERVER{srv_id}_SUCCESSARTICLES")).expect("no NUL");
        set_int_env_var(&succ_name, inp.server_success_articles[i]);
        let fail_name =
            CString::new(format!("NZBPP_SERVER{srv_id}_FAILEDARTICLES")).expect("no NUL");
        set_int_env_var(&fail_name, inp.server_failed_articles[i]);
    }
}

pub struct QueueScriptEnvInput<'a> {
    pub nzb_name: &'a CStr,
    pub nzb_id: i32,
    pub filename: &'a CStr,
    pub dest_dir: &'a CStr,
    pub queued_filename: &'a CStr,
    pub url: &'a CStr,
    pub category: &'a CStr,
    pub priority: i32,
    pub dupe_key: &'a CStr,
    pub dupe_score: i32,
    pub dupe_mode: i32,
    pub event: i32,
    pub delete_status: i32,
    pub url_status: i32,
    pub mark_status: i32,
}

/// End-to-end implementation of `QueueScriptController::PrepareParams`.
pub fn queue_script_prepare_env(
    inp: &QueueScriptEnvInput<'_>,
    mut set_env_var: impl FnMut(&CStr, &CStr),
    mut set_int_env_var: impl FnMut(&CStr, i32),
) {
    set_env_var(c"NZBNA_NZBNAME", inp.nzb_name);
    set_int_env_var(c"NZBNA_NZBID", inp.nzb_id);
    set_env_var(c"NZBNA_FILENAME", inp.filename);
    set_env_var(c"NZBNA_DIRECTORY", inp.dest_dir);
    set_env_var(c"NZBNA_QUEUEDFILE", inp.queued_filename);
    set_env_var(c"NZBNA_URL", inp.url);
    set_env_var(c"NZBNA_CATEGORY", inp.category);
    set_int_env_var(c"NZBNA_PRIORITY", inp.priority);
    set_int_env_var(c"NZBNA_LASTID", inp.nzb_id);
    set_env_var(c"NZBNA_DUPEKEY", inp.dupe_key);
    set_int_env_var(c"NZBNA_DUPESCORE", inp.dupe_score);
    set_env_var(c"NZBNA_DUPEMODE", dupe_mode_cstr(inp.dupe_mode));

    let event_name = match inp.event {
        0 => c"FILE_DOWNLOADED",
        1 => c"URL_COMPLETED",
        2 => c"NZB_MARKED",
        3 => c"NZB_ADDED",
        4 => c"NZB_NAMED",
        5 => c"NZB_DOWNLOADED",
        6 => c"NZB_DELETED",
        _ => c"NONE",
    };
    set_env_var(c"NZBNA_EVENT", event_name);

    let delete_status_name = match inp.delete_status {
        0 => c"NONE",
        1 => c"MANUAL",
        2 => c"HEALTH",
        3 => c"DUPE",
        4 => c"BAD",
        5 => c"GOOD",
        6 => c"COPY",
        7 => c"SCAN",
        _ => c"NONE",
    };
    set_env_var(c"NZBNA_DELETESTATUS", delete_status_name);

    let url_status_name = match inp.url_status {
        0 => c"NONE",
        1 => c"UNKNOWN",
        2 => c"SUCCESS",
        3 => c"FAILURE",
        4 => c"UNKNOWN",
        5 => c"SCAN_SKIPPED",
        6 => c"SCAN_FAILURE",
        _ => c"NONE",
    };
    set_env_var(c"NZBNA_URLSTATUS", url_status_name);

    let mark_status_name = match inp.mark_status {
        0 => c"NONE",
        1 => c"BAD",
        2 => c"GOOD",
        3 => c"SUCCESS",
        _ => c"NONE",
    };
    set_env_var(c"NZBNA_MARKSTATUS", mark_status_name);
}

use std::collections::BTreeMap;

pub const QUEUE_EVENT_NAMES: [&[u8]; 7] = [
    b"FILE_DOWNLOADED",
    b"URL_COMPLETED",
    b"NZB_MARKED",
    b"NZB_ADDED",
    b"NZB_NAMED",
    b"NZB_DOWNLOADED",
    b"NZB_DELETED",
];

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsQueueScriptCmd {
    /// - `0`: not an `[NZB] ` command (log normally)
    /// - `1`: `[NZB] NZBPR_<key>=<val>`
    /// - `2`: `[NZB] DIRECTORY=<dir>` (only when `event == qe_nzb_downloaded`)
    /// - `3`: `[NZB] MARK=BAD`
    /// - `-1`: invalid `[NZB] ` command
    pub kind: i32,
    pub key_offset: usize,
    pub key_len: usize,
    pub val_offset: usize,
    pub val_len: usize,
}

/// Parses a `[NZB] ...` line in `QueueScriptController::AddMessage`.
pub fn parse_queue_script_cmd(
    msg_text: &[u8],
    event: i32,
    qe_nzb_downloaded: i32,
) -> RsQueueScriptCmd {
    if !msg_text.starts_with(b"[NZB] ") {
        return RsQueueScriptCmd::default();
    }
    let body = &msg_text[6..];
    if body.starts_with(b"NZBPR_") {
        let kv = &body[6..];
        if let Some(eq) = kv.iter().position(|&b| b == b'=') {
            RsQueueScriptCmd {
                kind: 1,
                key_offset: 12,
                key_len: eq,
                val_offset: 12 + eq + 1,
                val_len: kv.len() - eq - 1,
            }
        } else {
            RsQueueScriptCmd {
                kind: -1,
                ..Default::default()
            }
        }
    } else if body.starts_with(b"DIRECTORY=") && event == qe_nzb_downloaded {
        RsQueueScriptCmd {
            kind: 2,
            key_offset: 6,
            key_len: 9,
            val_offset: 16,
            val_len: body.len() - 10,
        }
    } else if body.starts_with(b"MARK=BAD") {
        RsQueueScriptCmd {
            kind: 3,
            ..Default::default()
        }
    } else {
        RsQueueScriptCmd {
            kind: -1,
            ..Default::default()
        }
    }
}

/// Evaluates the `EventInterval` suppression check in `QueueScriptCoordinator::EnqueueScript`.
pub fn should_skip_queue_event_interval(
    event: i32,
    qe_file_downloaded: i32,
    event_interval: i32,
    cur_time: i64,
    queue_script_time: i64,
) -> bool {
    event == qe_file_downloaded
        && (event_interval == -1
            || (event_interval > 0
                && cur_time - queue_script_time > 0
                && ((cur_time - queue_script_time) as i32) < event_interval))
}

/// Checks if a parameter value is truthy (`"yes"`, `"on"`, `"1"`, case-insensitive).
fn is_param_enabled_value(val: &[u8]) -> bool {
    val.eq_ignore_ascii_case(b"yes") || val.eq_ignore_ascii_case(b"on") || val == b"1"
}

/// Evaluates `QueueScriptCoordinator::UsableScript`.
pub fn is_usable_queue_script(
    is_queue_script: bool,
    queue_events: &[u8],
    script_name: &[u8],
    event: i32,
    qe_url_completed: i32,
    param_names: &[&[u8]],
    param_values: &[&[u8]],
    url_extensions: &[u8],
) -> bool {
    if !is_queue_script {
        return false;
    }
    let event_name = if event >= 0 && (event as usize) < QUEUE_EVENT_NAMES.len() {
        QUEUE_EVENT_NAMES[event as usize]
    } else {
        b""
    };
    if !queue_events.is_empty()
        && !event_name.is_empty()
        && !queue_events
            .windows(event_name.len())
            .any(|w| w == event_name)
    {
        return false;
    }

    let param_count = param_names.len().min(param_values.len());
    for i in 0..param_count {
        let varname = param_names[i];
        if !varname.is_empty()
            && varname[0] != b'*'
            && varname[varname.len() - 1] == b':'
            && is_param_enabled_value(param_values[i])
        {
            let stripped = &varname[..varname.len() - 1];
            let truncated = if stripped.len() > 1023 {
                &stripped[..1023]
            } else {
                stripped
            };
            if truncated == script_name {
                return true;
            }
        }
    }

    if event == qe_url_completed && !url_extensions.is_empty() {
        for tok in url_extensions.split(|&b| b == b',' || b == b';') {
            let trimmed = trim_ascii_spaces(tok);
            if !trimmed.is_empty() && trimmed == script_name {
                return true;
            }
        }
    }

    false
}

fn trim_ascii_spaces(mut s: &[u8]) -> &[u8] {
    while let Some((&last, rest)) = s.split_last() {
        if last == b'\n' || last == b'\r' || last == b' ' || last == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    while let Some((&first, rest)) = s.split_first() {
        if first == b'\n' || first == b'\r' || first == b' ' || first == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

/// Evaluates whether a queued script item should be ignored/erased in `QueueScriptCoordinator::CheckQueue`.
pub fn should_ignore_queued_script_event(
    has_nzb_info: bool,
    delete_status: i32,
    ds_none: i32,
    mark_status: i32,
    ks_bad: i32,
    event: i32,
    qe_nzb_deleted: i32,
    qe_nzb_marked: i32,
) -> bool {
    !has_nzb_info
        || (delete_status != ds_none && event != qe_nzb_deleted && event != qe_nzb_marked)
        || (mark_status == ks_bad && event != qe_nzb_marked)
}

// ============================================================================
// 7. PostScript (`daemon/extension/PostScript.cpp`)
// ============================================================================

/// Builds the comma-separated list of enabled post-script names in `PostScriptController::Run`.
pub fn build_enabled_script_comma_list(
    param_names: &[&[u8]],
    param_values: &[&[u8]],
) -> Vec<u8> {
    let count = param_names.len().min(param_values.len());
    let mut out = Vec::new();
    for i in 0..count {
        let varname = param_names[i];
        if !varname.is_empty()
            && varname[0] != b'*'
            && varname[varname.len() - 1] == b':'
            && is_param_enabled_value(param_values[i])
        {
            out.extend_from_slice(&varname[..varname.len() - 1]);
            out.push(b',');
        }
    }
    out
}

/// Evaluates `PostScriptController::AnalyseExitCode`.
/// Returns `(script_status, message_action, request_par_check)` where `message_action`:
/// - `1`: info `"%s successful"`
/// - `2`: error `"%s failed"`
/// - `3`: info `"%s skipped"`
/// - `4`: error `"%s requested par-check/repair, but the collection was already checked"`
/// - `5`: info `"%s requested par-check/repair"`
/// - `6`: error `"%s failed (terminated with unknown status)"`
pub fn analyse_post_script_exit_code(
    exit_code: i32,
    par_status: i32,
    ps_skipped: i32,
    parcheck_enabled: bool,
    sr_none: i32,
    sr_failure: i32,
    sr_success: i32,
) -> (i32, i32, bool) {
    match exit_code {
        93 => (sr_success, 1, false),
        94 | -1 => (sr_failure, 2, false),
        95 => (sr_none, 3, false),
        92 if parcheck_enabled => {
            if par_status > ps_skipped {
                (sr_failure, 4, false)
            } else {
                (sr_success, 5, true)
            }
        }
        _ => (sr_failure, 6, false),
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsPostScriptCmd {
    /// - `0`: not an `[NZB] ` command
    /// - `1`: `[NZB] FINALDIR=<dir>`
    /// - `2`: `[NZB] DIRECTORY=<dir>`
    /// - `3`: `[NZB] NZBPR_<key>=<val>`
    /// - `4`: `[NZB] MARK=BAD`
    /// - `-1`: invalid `[NZB] ` command
    pub kind: i32,
    pub key_offset: usize,
    pub key_len: usize,
    pub val_offset: usize,
    pub val_len: usize,
}

/// Parses a `[NZB] ...` line in `PostScriptController::AddMessage`.
pub fn parse_post_script_cmd(msg_text: &[u8]) -> RsPostScriptCmd {
    if !msg_text.starts_with(b"[NZB] ") {
        return RsPostScriptCmd::default();
    }
    let body = &msg_text[6..];
    if body.starts_with(b"FINALDIR=") {
        RsPostScriptCmd {
            kind: 1,
            key_offset: 6,
            key_len: 8,
            val_offset: 15,
            val_len: body.len() - 9,
        }
    } else if body.starts_with(b"DIRECTORY=") {
        RsPostScriptCmd {
            kind: 2,
            key_offset: 6,
            key_len: 9,
            val_offset: 16,
            val_len: body.len() - 10,
        }
    } else if body.starts_with(b"NZBPR_") {
        let kv = &body[6..];
        if let Some(eq) = kv.iter().position(|&b| b == b'=') {
            RsPostScriptCmd {
                kind: 3,
                key_offset: 12,
                key_len: eq,
                val_offset: 12 + eq + 1,
                val_len: kv.len() - eq - 1,
            }
        } else {
            RsPostScriptCmd {
                kind: -1,
                ..Default::default()
            }
        }
    } else if body.starts_with(b"MARK=BAD") {
        RsPostScriptCmd {
            kind: 4,
            ..Default::default()
        }
    } else {
        RsPostScriptCmd {
            kind: -1,
            ..Default::default()
        }
    }
}

// ============================================================================
// 8. ScanScript (`daemon/extension/ScanScript.cpp`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsScanScriptCmd {
    /// - `0`: not an `[NZB] ` command
    /// - `1`: `[NZB] NZBNAME=<val>`
    /// - `2`: `[NZB] CATEGORY=<val>`
    /// - `3`: `[NZB] NZBPR_<key>=<val>`
    /// - `4`: `[NZB] PRIORITY=<int>`
    /// - `5`: `[NZB] TOP=<bool>`
    /// - `6`: `[NZB] PAUSED=<bool>`
    /// - `7`: `[NZB] DUPEKEY=<val>`
    /// - `8`: `[NZB] DUPESCORE=<int>`
    /// - `9`: `[NZB] DUPEMODE=<mode>` (`int_val` = 0 score, 1 all, 2 force)
    /// - `-1`: invalid `[NZB] ` command
    /// - `-2`: invalid value for `DUPEMODE` (`val_offset`/`val_len` point to value)
    pub kind: i32,
    pub key_offset: usize,
    pub key_len: usize,
    pub val_offset: usize,
    pub val_len: usize,
    pub int_val: i32,
}

fn c_atoi(bytes: &[u8]) -> i32 {
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
    let mut acc: i64 = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        acc = acc
            .wrapping_mul(10)
            .wrapping_add((bytes[idx] - b'0') as i64);
        idx += 1;
    }
    (acc * sign) as i32
}

/// Parses a `[NZB] ...` line in `ScanScriptController::AddMessage`.
pub fn parse_scan_script_cmd(msg_text: &[u8]) -> RsScanScriptCmd {
    if !msg_text.starts_with(b"[NZB] ") {
        return RsScanScriptCmd::default();
    }
    let body = &msg_text[6..];
    if body.starts_with(b"NZBNAME=") {
        RsScanScriptCmd {
            kind: 1,
            val_offset: 14,
            val_len: body.len() - 8,
            ..Default::default()
        }
    } else if body.starts_with(b"CATEGORY=") {
        RsScanScriptCmd {
            kind: 2,
            val_offset: 15,
            val_len: body.len() - 9,
            ..Default::default()
        }
    } else if body.starts_with(b"NZBPR_") {
        let kv = &body[6..];
        if let Some(eq) = kv.iter().position(|&b| b == b'=') {
            RsScanScriptCmd {
                kind: 3,
                key_offset: 12,
                key_len: eq,
                val_offset: 12 + eq + 1,
                val_len: kv.len() - eq - 1,
                int_val: 0,
            }
        } else {
            RsScanScriptCmd {
                kind: -1,
                ..Default::default()
            }
        }
    } else if body.starts_with(b"PRIORITY=") {
        RsScanScriptCmd {
            kind: 4,
            val_offset: 15,
            val_len: body.len() - 9,
            int_val: c_atoi(&body[9..]),
            ..Default::default()
        }
    } else if body.starts_with(b"TOP=") {
        RsScanScriptCmd {
            kind: 5,
            val_offset: 10,
            val_len: body.len() - 4,
            int_val: if c_atoi(&body[4..]) != 0 { 1 } else { 0 },
            ..Default::default()
        }
    } else if body.starts_with(b"PAUSED=") {
        RsScanScriptCmd {
            kind: 6,
            val_offset: 13,
            val_len: body.len() - 7,
            int_val: if c_atoi(&body[7..]) != 0 { 1 } else { 0 },
            ..Default::default()
        }
    } else if body.starts_with(b"DUPEKEY=") {
        RsScanScriptCmd {
            kind: 7,
            val_offset: 14,
            val_len: body.len() - 8,
            ..Default::default()
        }
    } else if body.starts_with(b"DUPESCORE=") {
        RsScanScriptCmd {
            kind: 8,
            val_offset: 16,
            val_len: body.len() - 10,
            int_val: c_atoi(&body[10..]),
            ..Default::default()
        }
    } else if body.starts_with(b"DUPEMODE=") {
        let val = &body[9..];
        let mode = if val.eq_ignore_ascii_case(b"score") {
            0
        } else if val.eq_ignore_ascii_case(b"all") {
            1
        } else if val.eq_ignore_ascii_case(b"force") {
            2
        } else {
            return RsScanScriptCmd {
                kind: -2,
                val_offset: 15,
                val_len: val.len(),
                ..Default::default()
            };
        };
        RsScanScriptCmd {
            kind: 9,
            val_offset: 15,
            val_len: val.len(),
            int_val: mode,
            ..Default::default()
        }
    } else {
        RsScanScriptCmd {
            kind: -1,
            ..Default::default()
        }
    }
}

// ============================================================================
// 9. ScriptConfig (`daemon/extension/ScriptConfig.cpp`)
// ============================================================================

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
        if dot == 6
            || name[6] == b'0'
            || !name[6..dot].iter().all(|&c| c.is_ascii_digit())
        {
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

// ============================================================================
// 10. ScriptController (`daemon/util/ScriptController.cpp`)
// ============================================================================

/// Checks whether an environment variable string `var` matches the `NZBXX_...` pattern
/// filtered out in `EnvironmentStrings::InitFromCurrentProcess`.
pub fn is_nzbget_internal_env_var(var: &[u8]) -> bool {
    var.len() > 5 && var.starts_with(b"NZB") && var[5] == b'_'
}

/// Normalizes an environment variable name in `ScriptController::SetEnvVarSpecial`:
/// replaces `.:*!"$%&/()=`+~#'{}[]@- ` with `_` and converts ASCII letters to uppercase.
pub fn normalize_special_env_var_name(varname: &[u8]) -> Vec<u8> {
    const SPECIAL: &[u8] = b".:*!\"$%&/()=`+~#'{}[]@- ";
    varname
        .iter()
        .map(|&b| {
            let replaced = if SPECIAL.contains(&b) { b'_' } else { b };
            replaced.to_ascii_uppercase()
        })
        .collect()
}

/// Strips trailing `\n`, `\r`, and `' '` in-place and classifies the log prefix
/// in `ScriptController::ProcessOutput`.
/// Returns `(is_empty, message_kind, text_offset)` where `message_kind`:
/// - `0`: `mkInfo`
/// - `1`: `mkWarning`
/// - `2`: `mkError`
/// - `3`: `mkDetail`
/// - `4`: `mkDebug`
pub fn classify_script_output_line(line: &[u8]) -> ( usize, i32, usize) {
    let mut trimmed_len = line.len();
    while trimmed_len > 0 {
        let last = line[trimmed_len - 1];
        if last == b'\n' || last == b'\r' || last == b' ' {
            trimmed_len -= 1;
        } else {
            break;
        }
    }
    if trimmed_len == 0 {
        return (0, 0, 0);
    }
    let trimmed = &line[..trimmed_len];
    if trimmed.starts_with(b"[INFO] ") {
        (trimmed_len, 0, 7)
    } else if trimmed.starts_with(b"[WARNING] ") {
        (trimmed_len, 1, 10)
    } else if trimmed.starts_with(b"[ERROR] ") {
        (trimmed_len, 2, 8)
    } else if trimmed.starts_with(b"[DETAIL] ") {
        (trimmed_len, 3, 9)
    } else if trimmed.starts_with(b"[DEBUG] ") {
        (trimmed_len, 4, 8)
    } else {
        (trimmed_len, 0, 0)
    }
}

/// Evaluates `ScriptController::CheckEnvSize`.
/// Returns `Some(exceeded_total)` if cumulative environment size exceeds `limit`, or `None`.
pub fn check_env_size_limit(env_lens: &[usize], ptr_size: usize, limit: usize) -> Option<usize> {
    if limit == 0 {
        return None;
    }
    let mut total = 0usize;
    for &len in env_lens {
        total = total.saturating_add(len).saturating_add(1).saturating_add(ptr_size);
        if total > limit {
            return Some(total);
        }
    }
    None
}

/// Resolves whether a single-argument script should use `found_executor` as `m_cmdLine`
/// and whether to prepend `found_executor` to `m_cmdArgs` (`ScriptController::PrepareArgs`).
pub fn script_resolve_single_arg_executor(
    has_found: bool,
    found_executor: &[u8],
    arg0: &[u8],
) -> (bool, bool) {
    (has_found, has_found && found_executor != arg0)
}

/// Formats `"<name>=<value>"` for `ScriptController::SetEnvVar`.
pub fn script_ctrl_format_env_assignment(name: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + 1 + value.len());
    out.extend_from_slice(name);
    out.push(b'=');
    out.extend_from_slice(value);
    out
}

/// Formats `"<prefix>_<name>"` for `ScriptController::SetEnvVarSpecial`.
pub fn script_ctrl_format_prefixed_varname(prefix: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + 1 + name.len());
    out.extend_from_slice(prefix);
    out.push(b'_');
    out.extend_from_slice(name);
    out
}

/// Formats `"<prefix>: <text>"` for `ScriptController::PrintMessage`.
pub fn script_format_prefixed_message(prefix: &[u8], text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + 2 + text.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(b": ");
    out.extend_from_slice(text);
    out
}

/// Evaluates whether `ScriptController::Terminate` succeeded.
pub fn script_ctrl_is_terminate_ok(child_pid: i32, kill_res: i32, completed: bool) -> bool {
    child_pid <= 0 || kill_res == 0 || completed
}

/// Returns true when `ScriptController::Break` has an active uncompleted process to signal.
pub fn script_ctrl_can_send_break(pid: i32, completed: bool) -> bool {
    pid > 0 && !completed
}

// ============================================================================
// 11. Extension Serialization & ExtensionManager (`Extension.cpp`, `ExtensionManager.cpp`)
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum ExtensionSelectVal {
    Str(Vec<u8>),
    Num(f64),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtensionSectionView {
    pub multi: bool,
    pub name: Vec<u8>,
    pub prefix: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionOptionView {
    pub section: ExtensionSectionView,
    pub name: Vec<u8>,
    pub display_name: Vec<u8>,
    pub description: Vec<Vec<u8>>,
    pub value: ExtensionSelectVal,
    pub select: Vec<ExtensionSelectVal>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtensionCommandView {
    pub section: ExtensionSectionView,
    pub name: Vec<u8>,
    pub display_name: Vec<u8>,
    pub action: Vec<u8>,
    pub description: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtensionScriptView {
    pub entry: Vec<u8>,
    pub location: Vec<u8>,
    pub root_dir: Vec<u8>,
    pub name: Vec<u8>,
    pub display_name: Vec<u8>,
    pub about: Vec<u8>,
    pub author: Vec<u8>,
    pub homepage: Vec<u8>,
    pub license: Vec<u8>,
    pub version: Vec<u8>,
    pub nzbget_min_version: Vec<u8>,
    pub post_script: bool,
    pub scan_script: bool,
    pub queue_script: bool,
    pub scheduler_script: bool,
    pub feed_script: bool,
    pub queue_events: Vec<u8>,
    pub task_time: Vec<u8>,
    pub description: Vec<Vec<u8>>,
    pub requirements: Vec<Vec<u8>>,
    pub options: Vec<ExtensionOptionView>,
    pub commands: Vec<ExtensionCommandView>,
}

pub fn json_append_escaped_str(out: &mut Vec<u8>, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(b'"');
    for &b in bytes {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x09 => out.extend_from_slice(b"\\t"),
            0x0A => out.extend_from_slice(b"\\n"),
            0x0C => out.extend_from_slice(b"\\f"),
            0x0D => out.extend_from_slice(b"\\r"),
            0x00..=0x1F => {
                out.extend_from_slice(b"\\u00");
                out.push(HEX[(b >> 4) as usize]);
                out.push(HEX[(b & 0x0F) as usize]);
            }
            _ => out.push(b),
        }
    }
    out.push(b'"');
}

pub fn json_append_f64(out: &mut Vec<u8>, val: f64) {
    if !val.is_finite() {
        out.extend_from_slice(b"null");
        return;
    }
    if val == 0.0 {
        if val.is_sign_negative() {
            out.extend_from_slice(b"-0E0");
        } else {
            out.extend_from_slice(b"0E0");
        }
        return;
    }
    let formatted = format!("{val:E}");
    out.extend_from_slice(formatted.as_bytes());
}

fn json_append_select_val(out: &mut Vec<u8>, val: &ExtensionSelectVal) {
    match val {
        ExtensionSelectVal::Str(s) => json_append_escaped_str(out, s),
        ExtensionSelectVal::Num(n) => json_append_f64(out, *n),
    }
}

fn json_append_str_array(out: &mut Vec<u8>, items: &[Vec<u8>]) {
    out.push(b'[');
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            out.push(b',');
        }
        json_append_escaped_str(out, item);
    }
    out.push(b']');
}

/// End-to-end implementation of `Extension::ToJsonStr` (`daemon/extension/Extension.cpp`).
pub fn extension_to_json(script: &ExtensionScriptView) -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    out.push(b'{');

    let str_fields: &[(&[u8], &[u8])] = &[
        (b"Entry", &script.entry),
        (b"Location", &script.location),
        (b"RootDir", &script.root_dir),
        (b"Name", &script.name),
        (b"DisplayName", &script.display_name),
        (b"About", &script.about),
        (b"Author", &script.author),
        (b"Homepage", &script.homepage),
        (b"License", &script.license),
        (b"Version", &script.version),
        (b"NZBGetMinVersion", &script.nzbget_min_version),
    ];
    for (idx, &(k, v)) in str_fields.iter().enumerate() {
        if idx > 0 {
            out.push(b',');
        }
        json_append_escaped_str(&mut out, k);
        out.push(b':');
        json_append_escaped_str(&mut out, v);
    }

    let bool_fields: &[(&[u8], bool)] = &[
        (b"PostScript", script.post_script),
        (b"ScanScript", script.scan_script),
        (b"QueueScript", script.queue_script),
        (b"SchedulerScript", script.scheduler_script),
        (b"FeedScript", script.feed_script),
    ];
    for &(k, v) in bool_fields {
        out.push(b',');
        json_append_escaped_str(&mut out, k);
        out.extend_from_slice(if v { b":true" } else { b":false" });
    }

    out.extend_from_slice(b",\"QueueEvents\":");
    json_append_escaped_str(&mut out, &script.queue_events);
    out.extend_from_slice(b",\"TaskTime\":");
    json_append_escaped_str(&mut out, &script.task_time);

    out.extend_from_slice(b",\"Description\":");
    json_append_str_array(&mut out, &script.description);
    out.extend_from_slice(b",\"Requirements\":");
    json_append_str_array(&mut out, &script.requirements);

    out.extend_from_slice(b",\"Options\":[");
    for (idx, opt) in script.options.iter().enumerate() {
        if idx > 0 {
            out.push(b',');
        }
        out.extend_from_slice(b"{\"Name\":");
        json_append_escaped_str(&mut out, &opt.name);
        out.extend_from_slice(b",\"DisplayName\":");
        json_append_escaped_str(&mut out, &opt.display_name);
        out.extend_from_slice(b",\"Section\":");
        json_append_escaped_str(&mut out, &opt.section.name);
        out.extend_from_slice(if opt.section.multi {
            b",\"Multi\":true,\"Prefix\":"
        } else {
            b",\"Multi\":false,\"Prefix\":"
        });
        json_append_escaped_str(&mut out, &opt.section.prefix);
        out.extend_from_slice(b",\"Value\":");
        json_append_select_val(&mut out, &opt.value);
        out.extend_from_slice(b",\"Description\":");
        json_append_str_array(&mut out, &opt.description);
        out.extend_from_slice(b",\"Select\":[");
        for (s_idx, sel) in opt.select.iter().enumerate() {
            if s_idx > 0 {
                out.push(b',');
            }
            json_append_select_val(&mut out, sel);
        }
        out.extend_from_slice(b"]}");
    }
    out.push(b']');

    out.extend_from_slice(b",\"Commands\":[");
    for (idx, cmd) in script.commands.iter().enumerate() {
        if idx > 0 {
            out.push(b',');
        }
        out.extend_from_slice(b"{\"Name\":");
        json_append_escaped_str(&mut out, &cmd.name);
        out.extend_from_slice(b",\"DisplayName\":");
        json_append_escaped_str(&mut out, &cmd.display_name);
        out.extend_from_slice(b",\"Action\":");
        json_append_escaped_str(&mut out, &cmd.action);
        out.extend_from_slice(b",\"Section\":");
        json_append_escaped_str(&mut out, &cmd.section.name);
        out.extend_from_slice(if cmd.section.multi {
            b",\"Multi\":true,\"Prefix\":"
        } else {
            b",\"Multi\":false,\"Prefix\":"
        });
        json_append_escaped_str(&mut out, &cmd.section.prefix);
        out.extend_from_slice(b",\"Description\":");
        json_append_str_array(&mut out, &cmd.description);
        out.push(b'}');
    }
    out.extend_from_slice(b"]}");

    out
}

pub fn xml_escape_text(out: &mut Vec<u8>, bytes: &[u8]) {
    for &b in bytes {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'\r' => out.extend_from_slice(b"&#13;"),
            _ => out.push(b),
        }
    }
}

pub fn xml_append_member_node(out: &mut Vec<u8>, name: &[u8], val_type: &[u8], value: &[u8]) {
    out.extend_from_slice(b"<member>");
    if name.is_empty() {
        out.extend_from_slice(b"<name/>");
    } else {
        out.extend_from_slice(b"<name>");
        xml_escape_text(out, name);
        out.extend_from_slice(b"</name>");
    }
    out.extend_from_slice(b"<value>");
    if value.is_empty() {
        out.push(b'<');
        out.extend_from_slice(val_type);
        out.extend_from_slice(b"/>");
    } else {
        out.push(b'<');
        out.extend_from_slice(val_type);
        out.push(b'>');
        xml_escape_text(out, value);
        out.extend_from_slice(b"</");
        out.extend_from_slice(val_type);
        out.push(b'>');
    }
    out.extend_from_slice(b"</value></member>");
}

fn xml_append_string_list_node(out: &mut Vec<u8>, tag: &[u8], items: &[Vec<u8>]) {
    if items.is_empty() {
        out.push(b'<');
        out.extend_from_slice(tag);
        out.extend_from_slice(b"/>");
        return;
    }
    out.push(b'<');
    out.extend_from_slice(tag);
    out.push(b'>');
    for line in items {
        xml_append_member_node(out, b"Value", b"string", line);
    }
    out.extend_from_slice(b"</");
    out.extend_from_slice(tag);
    out.push(b'>');
}

fn xml_append_select_member(out: &mut Vec<u8>, val: &ExtensionSelectVal) {
    match val {
        ExtensionSelectVal::Str(s) => xml_append_member_node(out, b"Value", b"string", s),
        ExtensionSelectVal::Num(n) => {
            let val_str = format!("{n:.6}");
            xml_append_member_node(out, b"Value", b"double", val_str.as_bytes());
        }
    }
}

/// End-to-end implementation of `Extension::ToXmlStr` (`daemon/extension/Extension.cpp`).
pub fn extension_to_xml(script: &ExtensionScriptView) -> Vec<u8> {
    let mut out = Vec::with_capacity(1024);
    out.extend_from_slice(b"<value><struct>");

    let str_fields: &[(&[u8], &[u8])] = &[
        (b"Entry", &script.entry),
        (b"Location", &script.location),
        (b"RootDir", &script.root_dir),
        (b"Name", &script.name),
        (b"DisplayName", &script.display_name),
        (b"About", &script.about),
        (b"Author", &script.author),
        (b"Homepage", &script.homepage),
        (b"License", &script.license),
        (b"Version", &script.version),
        (b"NZBGetMinVersion", &script.nzbget_min_version),
    ];
    for &(k, v) in str_fields {
        xml_append_member_node(&mut out, k, b"string", v);
    }

    let bool_fields: &[(&[u8], bool)] = &[
        (b"PostScript", script.post_script),
        (b"ScanScript", script.scan_script),
        (b"QueueScript", script.queue_script),
        (b"SchedulerScript", script.scheduler_script),
        (b"FeedScript", script.feed_script),
    ];
    for &(k, v) in bool_fields {
        xml_append_member_node(&mut out, k, b"boolean", if v { b"1" } else { b"0" });
    }

    xml_append_member_node(&mut out, b"QueueEvents", b"string", &script.queue_events);
    xml_append_member_node(&mut out, b"TaskTime", b"string", &script.task_time);

    xml_append_string_list_node(&mut out, b"Description", &script.description);
    xml_append_string_list_node(&mut out, b"Requirements", &script.requirements);

    if script.commands.is_empty() {
        out.extend_from_slice(b"<Commands/>");
    } else {
        out.extend_from_slice(b"<Commands>");
        for cmd in &script.commands {
            xml_append_member_node(&mut out, b"Name", b"string", &cmd.name);
            xml_append_member_node(&mut out, b"DisplayName", b"string", &cmd.display_name);
            xml_append_member_node(&mut out, b"Action", b"string", &cmd.action);
            xml_append_member_node(
                &mut out,
                b"Multi",
                b"boolean",
                if cmd.section.multi { b"1" } else { b"0" },
            );
            xml_append_member_node(&mut out, b"Section", b"string", &cmd.section.name);
            xml_append_member_node(&mut out, b"Prefix", b"string", &cmd.section.prefix);
            xml_append_string_list_node(&mut out, b"Description", &cmd.description);
        }
        out.extend_from_slice(b"</Commands>");
    }

    if script.options.is_empty() {
        out.extend_from_slice(b"<Options/>");
    } else {
        out.extend_from_slice(b"<Options>");
        for opt in &script.options {
            xml_append_member_node(&mut out, b"Name", b"string", &opt.name);
            xml_append_member_node(&mut out, b"DisplayName", b"string", &opt.display_name);
            xml_append_member_node(
                &mut out,
                b"Multi",
                b"boolean",
                if opt.section.multi { b"1" } else { b"0" },
            );
            xml_append_member_node(&mut out, b"Section", b"string", &opt.section.name);
            xml_append_member_node(&mut out, b"Prefix", b"string", &opt.section.prefix);
            xml_append_select_member(&mut out, &opt.value);
            xml_append_string_list_node(&mut out, b"Description", &opt.description);
            if opt.select.is_empty() {
                out.extend_from_slice(b"<Select/>");
            } else {
                out.extend_from_slice(b"<Select>");
                for sel in &opt.select {
                    xml_append_select_member(&mut out, sel);
                }
                out.extend_from_slice(b"</Select>");
            }
        }
        out.extend_from_slice(b"</Options>");
    }

    out.extend_from_slice(b"</struct></value>");
    out
}

/// Strips the last `.ext` from `file_name` (`ExtensionManager::Manager::GetExtensionName`).
pub fn ext_mgr_get_extension_name(file_name: &[u8]) -> Vec<u8> {
    match file_name.iter().rposition(|&b| b == b'.') {
        Some(dot_pos) => file_name[..dot_pos].to_vec(),
        None => file_name.to_vec(),
    }
}

/// Resolves the target path for deleting or updating an extension (`ExtensionManager::Manager::GetExtensionPath`).
/// When multiple loaded extensions share `location`, returns `entry`, otherwise `location`.
pub fn ext_mgr_resolve_extension_path<'a>(
    location: &'a [u8],
    entry: &'a [u8],
    all_locations: &[&[u8]],
) -> &'a [u8] {
    let count = all_locations.iter().filter(|&&loc| loc == location).count();
    if count > 1 {
        entry
    } else {
        location
    }
}

fn join_path_sep(directory: &[u8], path_sep: u8, name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(directory.len() + 1 + name.len());
    out.extend_from_slice(directory);
    out.push(path_sep);
    out.extend_from_slice(name);
    out
}

/// Loads a V2 extension from `directory` (`ExtensionLoader::V2::Load`).
pub fn ext_loader_v2_load_from_dir(
    directory: &[u8],
    root_dir: &[u8],
    path_sep: u8,
) -> Option<ExtensionScriptView> {
    let manifest_file_bytes = join_path_sep(directory, path_sep, b"manifest.json");
    let path = bytes_to_path(&manifest_file_bytes);
    let raw = std::fs::read(&path).ok()?;
    let limits = crate::json::Limits {
        string: usize::MAX,
        array: usize::MAX,
        object: usize::MAX,
    };
    let crate::manifest::Outcome::Loaded(m) = crate::manifest::load(&raw, limits) else {
        return None;
    };
    let get_field = |idx: usize| m.fields[idx].clone().unwrap_or_default();
    let main_file = get_field(1);
    let entry = join_path_sep(directory, path_sep, &main_file);
    let kind_bits = crate::extload::script_kind(&get_field(7));

    let mut options: Vec<ExtensionOptionView> = m
        .options
        .into_iter()
        .map(|opt| ExtensionOptionView {
            section: ExtensionSectionView {
                multi: opt.section.multi,
                name: opt.section.name,
                prefix: opt.section.prefix,
            },
            name: opt.name,
            display_name: opt.display_name,
            description: opt.description,
            value: match opt.value {
                crate::manifest::Select::Number(n) => ExtensionSelectVal::Num(n),
                crate::manifest::Select::String(s) => ExtensionSelectVal::Str(s),
            },
            select: opt
                .select
                .into_iter()
                .map(|s| match s {
                    crate::manifest::Select::Number(n) => ExtensionSelectVal::Num(n),
                    crate::manifest::Select::String(st) => ExtensionSelectVal::Str(st),
                })
                .collect(),
        })
        .collect();

    let mut commands: Vec<ExtensionCommandView> = m
        .commands
        .into_iter()
        .map(|cmd| ExtensionCommandView {
            section: ExtensionSectionView {
                multi: cmd.section.multi,
                name: cmd.section.name,
                prefix: cmd.section.prefix,
            },
            name: cmd.name,
            display_name: cmd.display_name,
            action: cmd.action,
            description: cmd.description,
        })
        .collect();

    for sec in m.sections {
        let sec_view = ExtensionSectionView {
            multi: sec.multi,
            name: sec.name,
            prefix: sec.prefix,
        };
        for opt in &mut options {
            if opt.section.name == sec_view.name {
                opt.section = sec_view.clone();
            }
        }
        for cmd in &mut commands {
            if cmd.section.name == sec_view.name {
                cmd.section = sec_view.clone();
            }
        }
    }

    Some(ExtensionScriptView {
        entry,
        location: directory.to_vec(),
        root_dir: root_dir.to_vec(),
        author: get_field(0),
        homepage: get_field(2),
        about: get_field(3),
        version: get_field(4),
        name: get_field(5),
        display_name: get_field(6),
        license: get_field(8),
        task_time: get_field(9),
        queue_events: get_field(10),
        nzbget_min_version: get_field(11),
        post_script: (kind_bits & 1) != 0,
        scan_script: (kind_bits & 2) != 0,
        queue_script: (kind_bits & 4) != 0,
        scheduler_script: (kind_bits & 8) != 0,
        feed_script: (kind_bits & 16) != 0,
        description: m.description,
        requirements: m.requirements,
        options,
        commands,
    })
}

/// Loads a V1 extension from `entry` (`ExtensionLoader::V1::Load`).
pub fn ext_loader_v1_load_from_file(
    entry: &[u8],
    name: &[u8],
    location: &[u8],
    root_dir: &[u8],
) -> Option<ExtensionScriptView> {
    let path = bytes_to_path(entry);
    let raw = std::fs::read(&path).ok()?;
    let parsed = crate::extload::parse(&raw, &|b| b.is_ascii_whitespace())?;

    let stripped = crate::unpack::unpack_ctrl_strip_last_extension(name);
    let base_off = crate::paths::base_file_name(&stripped);
    let display_name = stripped[base_off..].to_vec();

    let mut about = parsed.about;
    while about.last().is_some_and(|&b| b.is_ascii_whitespace()) {
        about.pop();
    }

    let options = parsed
        .options
        .into_iter()
        .map(|opt| ExtensionOptionView {
            section: ExtensionSectionView {
                multi: opt.section.multi,
                name: opt.section.name,
                prefix: opt.section.prefix,
            },
            display_name: opt.name.clone(),
            name: opt.name,
            description: opt.description,
            value: match opt.value {
                crate::extload::Select::Num(n) => ExtensionSelectVal::Num(n),
                crate::extload::Select::Str(s) => ExtensionSelectVal::Str(s),
            },
            select: opt
                .select
                .into_iter()
                .map(|s| match s {
                    crate::extload::Select::Num(n) => ExtensionSelectVal::Num(n),
                    crate::extload::Select::Str(st) => ExtensionSelectVal::Str(st),
                })
                .collect(),
        })
        .collect();

    let commands = parsed
        .commands
        .into_iter()
        .map(|cmd| ExtensionCommandView {
            section: ExtensionSectionView {
                multi: cmd.section.multi,
                name: cmd.section.name,
                prefix: cmd.section.prefix,
            },
            display_name: cmd.name.clone(),
            name: cmd.name,
            action: cmd.action,
            description: cmd.description,
        })
        .collect();

    Some(ExtensionScriptView {
        entry: entry.to_vec(),
        location: location.to_vec(),
        root_dir: root_dir.to_vec(),
        name: name.to_vec(),
        display_name,
        about,
        author: Vec::new(),
        homepage: Vec::new(),
        license: Vec::new(),
        version: Vec::new(),
        nzbget_min_version: Vec::new(),
        post_script: (parsed.kind & 1) != 0,
        scan_script: (parsed.kind & 2) != 0,
        queue_script: (parsed.kind & 4) != 0,
        scheduler_script: (parsed.kind & 8) != 0,
        feed_script: (parsed.kind & 16) != 0,
        queue_events: parsed.queue_events,
        task_time: parsed.task_time,
        description: parsed.description,
        requirements: parsed.requirements,
        options,
        commands,
    })
}

fn load_extension_dir_recursive(
    directory: &[u8],
    is_sub_dir: bool,
    root_dir: &[u8],
    path_sep: u8,
    extensions: &mut Vec<std::sync::Arc<ExtensionScriptView>>,
) {
    if let Some(v2_ext) = ext_loader_v2_load_from_dir(directory, root_dir, path_sep) {
        if !extensions.iter().any(|e| e.name == v2_ext.name) {
            extensions.push(std::sync::Arc::new(v2_ext));
            return;
        }
    }

    let dir_path = bytes_to_path(directory);
    let Ok(read_dir) = std::fs::read_dir(&dir_path) else {
        return;
    };
    for entry_res in read_dir.flatten() {
        let file_name_os = entry_res.file_name();
        let filename = crate::cleanup::os_str_to_bytes(&file_name_os);
        if filename.is_empty() || filename[0] == b'.' || filename[0] == b'_' {
            continue;
        }
        let entry = join_path_sep(directory, path_sep, filename);
        let is_dir = std::fs::metadata(bytes_to_path(&entry)).is_ok_and(|m| m.is_dir());
        if !is_dir {
            let name = ext_mgr_get_extension_name(filename);
            if extensions.iter().any(|e| e.name == name) {
                continue;
            }
            let location = if is_sub_dir { directory } else { &entry };
            if let Some(v1_ext) = ext_loader_v1_load_from_file(&entry, &name, location, root_dir) {
                extensions.push(std::sync::Arc::new(v1_ext));
            }
        } else if !is_sub_dir {
            load_extension_dir_recursive(&entry, true, root_dir, path_sep, extensions);
        }
    }
}

/// Pure-Rust `ExtensionManager` (`daemon/extension/ExtensionManager.cpp`).
pub struct RsExtensionManager {
    extensions: std::sync::RwLock<Vec<std::sync::Arc<ExtensionScriptView>>>,
}

impl Default for RsExtensionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RsExtensionManager {
    pub fn new() -> Self {
        Self {
            extensions: std::sync::RwLock::new(Vec::new()),
        }
    }

    pub fn load_extensions(
        &self,
        script_dir: &[u8],
        script_order: &[u8],
        path_sep: u8,
    ) -> Result<(), String> {
        if script_dir.is_empty() {
            return Err("\"ScriptDir\" is not specified".to_string());
        }
        let mut loaded = Vec::new();
        for raw_dir in script_dir.split(|&b| b == b',' || b == b';') {
            let dir = trim_ascii_spaces(raw_dir);
            if dir.is_empty() {
                continue;
            }
            load_extension_dir_recursive(dir, false, dir, path_sep, &mut loaded);
        }
        if !loaded.is_empty() {
            let names: Vec<&[u8]> = loaded.iter().map(|e| e.name.as_slice()).collect();
            let sorted_indices = ext_sort_indices(&names, script_order);
            let mut sorted = Vec::with_capacity(loaded.len());
            for idx in sorted_indices {
                sorted.push(std::sync::Arc::clone(&loaded[idx]));
            }
            loaded = sorted;
        }
        *self.extensions.write().unwrap() = loaded;
        Ok(())
    }

    pub fn for_each(&self, mut cb: impl FnMut(&std::sync::Arc<ExtensionScriptView>)) {
        let guard = self.extensions.read().unwrap();
        for ext in guard.iter() {
            cb(ext);
        }
    }

    pub fn find_if(
        &self,
        mut pred: impl FnMut(&std::sync::Arc<ExtensionScriptView>) -> bool,
    ) -> Option<std::sync::Arc<ExtensionScriptView>> {
        let guard = self.extensions.read().unwrap();
        guard.iter().find(|ext| pred(ext)).cloned()
    }

    pub fn delete_extension(&self, name: &[u8]) -> Result<(), String> {
        let mut guard = self.extensions.write().unwrap();
        let pos = guard.iter().position(|e| e.name == name);
        let found = pos.is_some();
        let use_count = pos.map_or(0, |i| std::sync::Arc::strong_count(&guard[i]) as i64);
        let target_path = if let Some(i) = pos {
            let all_locs: Vec<&[u8]> = guard.iter().map(|e| e.location.as_slice()).collect();
            ext_mgr_resolve_extension_path(&guard[i].location, &guard[i].entry, &all_locs).to_vec()
        } else {
            Vec::new()
        };
        if let Some(err_bytes) = ext_mgr_delete_extension(name, found, use_count, &target_path) {
            return Err(String::from_utf8_lossy(&err_bytes).into_owned());
        }
        if let Some(i) = pos {
            guard.remove(i);
        }
        Ok(())
    }

    pub fn update_extension(
        &self,
        archive_path: &[u8],
        ext_name: &[u8],
        install_fn: impl FnOnce(&[u8], &[u8]) -> Option<Vec<u8>>,
    ) -> Result<(), String> {
        let mut guard = self.extensions.write().unwrap();
        let pos = guard.iter().position(|e| e.name == ext_name);
        let found = pos.is_some();
        let use_count = pos.map_or(0, |i| std::sync::Arc::strong_count(&guard[i]) as i64);
        let (target_path, root_dir) = if let Some(i) = pos {
            let all_locs: Vec<&[u8]> = guard.iter().map(|e| e.location.as_slice()).collect();
            let tp = ext_mgr_resolve_extension_path(&guard[i].location, &guard[i].entry, &all_locs)
                .to_vec();
            (tp, guard[i].root_dir.clone())
        } else {
            (Vec::new(), Vec::new())
        };
        let (backup_path, prep_err) =
            ext_mgr_prepare_update(ext_name, found, use_count, &target_path, archive_path);
        if let Some(err_bytes) = prep_err {
            return Err(String::from_utf8_lossy(&err_bytes).into_owned());
        }
        let install_err = install_fn(archive_path, &root_dir);
        if let Some(finish_err) = ext_mgr_finish_update(
            &target_path,
            &backup_path,
            archive_path,
            install_err.as_deref(),
        ) {
            return Err(String::from_utf8_lossy(&finish_err).into_owned());
        }
        if let Some(i) = pos {
            guard.remove(i);
        }
        Ok(())
    }
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
    fn test_nzb_script_prepare_env() {
        let mut vars = Vec::new();
        let mut opt_prefix = String::new();
        nzb_script_prepare_env(
            b"Logger.py",
            true,
            &[
                c"*unpack:",
                c"Logger.py:",
                c"logger.py:Ignored",
                c"Logger.py:Verbose",
                c"Other",
            ],
            &[c"yes", c"empty", c"no", c"true", c"1"],
            |prefix, name, val| {
                vars.push((
                    prefix.to_string_lossy().into_owned(),
                    name.to_string_lossy().into_owned(),
                    val.to_string_lossy().into_owned(),
                ));
            },
            |prefix| {
                opt_prefix = prefix.to_string_lossy().into_owned();
            },
        );
        assert_eq!(opt_prefix, "Logger.py:");
        assert_eq!(vars.len(), 6);
        assert_eq!(
            vars[5],
            ("NZBPR".to_string(), "Verbose".to_string(), "true".to_string())
        );
    }

    #[test]
    fn test_post_and_queue_script_prepare_env() {
        let mut env = Vec::new();
        let mut int_env = Vec::new();
        let inp = PostScriptEnvInput {
            nzb_id: 7,
            nzb_name: c"My.Release",
            dest_dir: c"/dst",
            filename: c"/nzb/My.Release.nzb",
            queued_filename: c"/queue/7",
            url: c"",
            final_dir: c"/final",
            category: c"movies",
            health: 1000,
            critical_health: 900,
            dupe_key: c"dk",
            dupe_score: 10,
            dupe_mode: 2,
            status: c"SUCCESS/ALL",
            script_status: 2,
            par_status: 3,
            unpack_status: 3,
            delete_status: 2,
            mark_status: 0,
            total_articles: 100,
            success_articles: 98,
            failed_articles: 2,
            server_ids: &[1],
            server_success_articles: &[98],
            server_failed_articles: &[2],
        };
        post_script_prepare_env(
            &inp,
            |k, v| {
                env.push((
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                ))
            },
            |k, v| int_env.push((k.to_string_lossy().into_owned(), v)),
        );
        assert!(env.contains(&("NZBPP_TOTALSTATUS".to_string(), "SUCCESS".to_string())));
        assert!(env.contains(&("NZBPP_DUPEMODE".to_string(), "FORCE".to_string())));
        assert!(env.contains(&("NZBPP_SCRIPTSTATUS".to_string(), "SUCCESS".to_string())));
        assert!(int_env.contains(&("NZBPP_PARSTATUS".to_string(), 1)));
        assert!(int_env.contains(&("NZBPP_UNPACKSTATUS".to_string(), 2)));
        assert!(int_env.contains(&("NZBPP_HEALTHDELETED".to_string(), 1)));
        assert!(int_env.contains(&("NZBPP_SERVER1_SUCCESSARTICLES".to_string(), 98)));
        assert!(int_env.contains(&("NZBPP_SERVER1_FAILEDARTICLES".to_string(), 2)));

        let mut q_env = Vec::new();
        let q_inp = QueueScriptEnvInput {
            nzb_name: c"My.Release",
            nzb_id: 7,
            filename: c"/nzb/My.Release.nzb",
            dest_dir: c"/dst",
            queued_filename: c"/queue/7",
            url: c"https://example.com/a.nzb",
            category: c"movies",
            priority: 0,
            dupe_key: c"dk",
            dupe_score: 0,
            dupe_mode: 0,
            event: 1,
            delete_status: 0,
            url_status: 5,
            mark_status: 0,
        };
        queue_script_prepare_env(
            &q_inp,
            |k, v| {
                q_env.push((
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                ))
            },
            |_, _| {},
        );
        assert!(q_env.contains(&("NZBNA_URLSTATUS".to_string(), "SCAN_SKIPPED".to_string())));
        assert!(ext_is_in_list(b"ScriptA, ScriptB", b"ScriptB"));
        assert!(!ext_is_in_list(b"ScriptA, ScriptB", b"scriptb"));
    }

    #[test]
    fn scan_script_command_test() {
        let msg = b"[NZB] NZBNAME=my download";
        let cmd = parse_scan_script_cmd(msg);
        assert_eq!(cmd.kind, 1);
        assert_eq!(&msg[cmd.val_offset..cmd.val_offset + cmd.val_len], b"my download");

        let msg = b"[NZB] CATEGORY=my category";
        let cmd = parse_scan_script_cmd(msg);
        assert_eq!(cmd.kind, 2);
        assert_eq!(&msg[cmd.val_offset..cmd.val_offset + cmd.val_len], b"my category");

        let msg = b"[NZB] DUPEKEY=tv show s01e02";
        let cmd = parse_scan_script_cmd(msg);
        assert_eq!(cmd.kind, 7);
        assert_eq!(&msg[cmd.val_offset..cmd.val_offset + cmd.val_len], b"tv show s01e02");

        let msg = b"[NZB] PRIORITY=50";
        let cmd = parse_scan_script_cmd(msg);
        assert_eq!(cmd.kind, 4);
        assert_eq!(cmd.int_val, 50);

        let msg = b"[NZB] DUPESCORE=10";
        let cmd = parse_scan_script_cmd(msg);
        assert_eq!(cmd.kind, 8);
        assert_eq!(cmd.int_val, 10);

        let msg = b"[NZB] TOP=1";
        let cmd = parse_scan_script_cmd(msg);
        assert_eq!(cmd.kind, 5);
        assert_eq!(cmd.int_val, 1);

        let msg = b"[NZB] PAUSED=1";
        let cmd = parse_scan_script_cmd(msg);
        assert_eq!(cmd.kind, 6);
        assert_eq!(cmd.int_val, 1);

        let msg = b"[NZB] DUPEMODE=FORCE";
        let cmd = parse_scan_script_cmd(msg);
        assert_eq!(cmd.kind, 9);
        assert_eq!(cmd.int_val, 2);

        let msg = b"[NZB] NZBPR_myvar=my value";
        let cmd = parse_scan_script_cmd(msg);
        assert_eq!(cmd.kind, 3);
        assert_eq!(&msg[cmd.key_offset..cmd.key_offset + cmd.key_len], b"myvar");
        assert_eq!(&msg[cmd.val_offset..cmd.val_offset + cmd.val_len], b"my value");
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

    #[test]
    fn extension_to_json_and_xml_suite_port_from_cpp() {
        let script = ExtensionScriptView {
            entry: b"/v1/main.py".to_vec(),
            location: b"/v1".to_vec(),
            root_dir: b"/".to_vec(),
            author: b"Author".to_vec(),
            about: b"About".to_vec(),
            homepage: b"Homepage".to_vec(),
            description: vec![b"Description".to_vec()],
            display_name: b"DisplayName".to_vec(),
            post_script: true,
            scan_script: false,
            queue_script: false,
            scheduler_script: false,
            feed_script: false,
            license: b"License".to_vec(),
            name: b"Name".to_vec(),
            requirements: vec![b"Requirements".to_vec()],
            queue_events: b"QueueEvents".to_vec(),
            task_time: b"TaskTime".to_vec(),
            version: b"Version".to_vec(),
            nzbget_min_version: b"23.1".to_vec(),
            options: vec![ExtensionOptionView {
                name: b"name".to_vec(),
                display_name: b"displayName".to_vec(),
                description: vec![b"description".to_vec()],
                value: ExtensionSelectVal::Num(5.0),
                select: vec![
                    ExtensionSelectVal::Num(0.0),
                    ExtensionSelectVal::Num(10.0),
                ],
                section: ExtensionSectionView {
                    multi: true,
                    prefix: b"Prefix".to_vec(),
                    name: b"Section".to_vec(),
                },
            }],
            commands: vec![ExtensionCommandView {
                name: b"name".to_vec(),
                action: b"action".to_vec(),
                display_name: b"displayName".to_vec(),
                description: vec![b"description".to_vec()],
                section: ExtensionSectionView {
                    multi: true,
                    prefix: b"Prefix".to_vec(),
                    name: b"Section".to_vec(),
                },
            }],
        };

        // ToJsonStrTest
        let json_res = extension_to_json(&script);
        let expected_json = concat!(
            "{\"Entry\":\"/v1/main.py\",",
            "\"Location\":\"/v1\",",
            "\"RootDir\":\"/\",",
            "\"Name\":\"Name\",",
            "\"DisplayName\":\"DisplayName\",",
            "\"About\":\"About\",",
            "\"Author\":\"Author\",",
            "\"Homepage\":\"Homepage\",",
            "\"License\":\"License\",",
            "\"Version\":\"Version\",",
            "\"NZBGetMinVersion\":\"23.1\",",
            "\"PostScript\":true,",
            "\"ScanScript\":false,",
            "\"QueueScript\":false,",
            "\"SchedulerScript\":false,",
            "\"FeedScript\":false,",
            "\"QueueEvents\":\"QueueEvents\",",
            "\"TaskTime\":\"TaskTime\",",
            "\"Description\":[\"Description\"],",
            "\"Requirements\":[\"Requirements\"],",
            "\"Options\":[{\"Name\":\"name\",\"DisplayName\":\"displayName\",\"Section\":\"Section\",",
            "\"Multi\":true,\"Prefix\":\"Prefix\",\"Value\":5E0,\"Description\":[\"description\"],\"Select\":[0E0,1E1]}],",
            "\"Commands\":[{\"Name\":\"name\",\"DisplayName\":\"displayName\",\"Action\":\"action\",\"Section\":\"Section\",",
            "\"Multi\":true,\"Prefix\":\"Prefix\",\"Description\":[\"description\"]}]}"
        );
        assert_eq!(std::str::from_utf8(&json_res).unwrap(), expected_json);

        // ToXmlStrTest
        let xml_res = extension_to_xml(&script);
        let expected_xml = concat!(
            "<value><struct>",
            "<member><name>Entry</name><value><string>/v1/main.py</string></value></member>",
            "<member><name>Location</name><value><string>/v1</string></value></member>",
            "<member><name>RootDir</name><value><string>/</string></value></member>",
            "<member><name>Name</name><value><string>Name</string></value></member>",
            "<member><name>DisplayName</name><value><string>DisplayName</string></value></member>",
            "<member><name>About</name><value><string>About</string></value></member>",
            "<member><name>Author</name><value><string>Author</string></value></member>",
            "<member><name>Homepage</name><value><string>Homepage</string></value></member>",
            "<member><name>License</name><value><string>License</string></value></member>",
            "<member><name>Version</name><value><string>Version</string></value></member>",
            "<member><name>NZBGetMinVersion</name><value><string>23.1</string></value></member>",
            "<member><name>PostScript</name><value><boolean>1</boolean></value></member>",
            "<member><name>ScanScript</name><value><boolean>0</boolean></value></member>",
            "<member><name>QueueScript</name><value><boolean>0</boolean></value></member>",
            "<member><name>SchedulerScript</name><value><boolean>0</boolean></value></member>",
            "<member><name>FeedScript</name><value><boolean>0</boolean></value></member>",
            "<member><name>QueueEvents</name><value><string>QueueEvents</string></value></member>",
            "<member><name>TaskTime</name><value><string>TaskTime</string></value></member>",
            "<Description>",
            "<member><name>Value</name><value><string>Description</string></value></member>",
            "</Description>",
            "<Requirements>",
            "<member><name>Value</name><value><string>Requirements</string></value></member>",
            "</Requirements>",
            "<Commands>",
            "<member><name>Name</name><value><string>name</string></value></member>",
            "<member><name>DisplayName</name><value><string>displayName</string></value></member>",
            "<member><name>Action</name><value><string>action</string></value></member>",
            "<member><name>Multi</name><value><boolean>1</boolean></value></member>",
            "<member><name>Section</name><value><string>Section</string></value></member>",
            "<member><name>Prefix</name><value><string>Prefix</string></value></member>",
            "<Description>",
            "<member><name>Value</name><value><string>description</string></value></member>",
            "</Description>",
            "</Commands>",
            "<Options>",
            "<member><name>Name</name><value><string>name</string></value></member>",
            "<member><name>DisplayName</name><value><string>displayName</string></value></member>",
            "<member><name>Multi</name><value><boolean>1</boolean></value></member>",
            "<member><name>Section</name><value><string>Section</string></value></member>",
            "<member><name>Prefix</name><value><string>Prefix</string></value></member>",
            "<member><name>Value</name><value><double>5.000000</double></value></member>",
            "<Description>",
            "<member><name>Value</name><value><string>description</string></value></member>",
            "</Description>",
            "<Select>",
            "<member><name>Value</name><value><double>0.000000</double></value></member>",
            "<member><name>Value</name><value><double>10.000000</double></value></member>",
            "</Select>",
            "</Options>",
            "</struct></value>"
        );
        assert_eq!(std::str::from_utf8(&xml_res).unwrap(), expected_xml);
    }

    #[test]
    fn extension_manager_full_suite_port_from_cpp() {
        use std::os::unix::ffi::OsStrExt;
        use std::path::{Path, PathBuf};

        fn copy_dir_recursive(src: &Path, dst: &Path) {
            std::fs::create_dir_all(dst).unwrap();
            for entry in std::fs::read_dir(src).unwrap() {
                let entry = entry.unwrap();
                let ty = entry.file_type().unwrap();
                let dst_path = dst.join(entry.file_name());
                if ty.is_dir() {
                    copy_dir_recursive(&entry.path(), &dst_path);
                } else {
                    std::fs::copy(entry.path(), dst_path).unwrap();
                }
            }
        }

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let scripts_dir = manifest_dir
            .parent()
            .unwrap()
            .join("tests/testdata/extension/scripts");
        let order = b"Extension2, Extension1, email; Extension1; Extension1";

        // 1. LoadExtensionsTest
        {
            let manager = RsExtensionManager::new();
            assert!(
                manager
                    .load_extensions(scripts_dir.as_os_str().as_bytes(), order, b'/')
                    .is_ok()
            );
            let correct_order = [b"Extension2".as_slice(), b"Extension1", b"email"];
            let mut idx = 0usize;
            manager.for_each(|script| {
                if idx < correct_order.len() {
                    assert_eq!(script.name.as_slice(), correct_order[idx]);
                }
                idx += 1;
            });
            assert_eq!(idx, 4);
        }

        // 2. ShouldNotDeleteExtensionIfExtensionIsBusyTest
        {
            let manager = RsExtensionManager::new();
            assert!(
                manager
                    .load_extensions(scripts_dir.as_os_str().as_bytes(), order, b'/')
                    .is_ok()
            );
            let busy_ext = manager.find_if(|_| true).unwrap();
            let err = manager.delete_extension(&busy_ext.name).unwrap_err();
            let expected_err = format!(
                "Deletion failed: Extension '{}' is currently in use",
                String::from_utf8_lossy(&busy_ext.name)
            );
            assert_eq!(err, expected_err);
        }

        // 3. DeleteExtensionTest
        {
            let working_dir = std::env::temp_dir().join(format!(
                "nzbget_rs_delete_ext_test_{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&working_dir);
            copy_dir_recursive(&scripts_dir, &working_dir);

            let manager = RsExtensionManager::new();
            assert!(
                manager
                    .load_extensions(working_dir.as_os_str().as_bytes(), order, b'/')
                    .is_ok()
            );
            assert!(manager.delete_extension(b"email").is_ok());
            assert!(manager.find_if(|s| s.name == b"email").is_none());
            assert!(!working_dir.join("email").exists());

            let _ = std::fs::remove_dir_all(&working_dir);
        }

        // 4. FailedUpdateKeepsExtensionTest
        {
            let working_dir = std::env::temp_dir().join(format!(
                "nzbget_rs_failed_update_ext_test_{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&working_dir);
            copy_dir_recursive(&scripts_dir, &working_dir);

            let manager = RsExtensionManager::new();
            assert!(
                manager
                    .load_extensions(working_dir.as_os_str().as_bytes(), order, b'/')
                    .is_ok()
            );
            let location = {
                let email = manager.find_if(|s| s.name == b"email").unwrap();
                PathBuf::from(std::ffi::OsStr::from_bytes(&email.location))
            };
            assert!(location.exists());

            let archive = working_dir.join("update.zip");
            std::fs::write(&archive, b"not an archive").unwrap();

            let res = manager.update_extension(
                archive.as_os_str().as_bytes(),
                b"email",
                |_, _| Some(b"Failed to extract archive".to_vec()),
            );
            assert!(res.is_err());
            assert!(location.exists());
            let mut backup = location.as_os_str().to_os_string();
            backup.push(".update-backup");
            assert!(!Path::new(&backup).exists());
            assert!(manager.find_if(|s| s.name == b"email").is_some());

            let _ = std::fs::remove_dir_all(&working_dir);
        }
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
                                    valid_arguments.set(
                                        valid_arguments.get() && value.is_null() && size == 0,
                                    );
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
            let prefix = format!(
                ".{}.tls-",
                file.path.file_name().unwrap().to_string_lossy()
            );
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
            let (saved_ok, conflict) =
                script_config_save(file.bytes(), &entry_pairs, Some(&token));
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
