//! FileSystem's file operations (FileSystem.cpp, POSIX): FileExists,
//! DirectoryExists, ForceDirectories, MakeUniqueFilename and ExpandHomePath,
//! and the error text of GetLastErrorMessage.

use std::ffi::{c_char, c_int, CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

extern "C" {
    // These libcs export the GNU ABI as strerror_r and the XSI ABI under
    // __xpg_strerror_r. In particular, Cygwin has an empty target_env.
    // Other POSIX libcs (including musl and the BSDs) use strerror_r for XSI.
    #[cfg_attr(
        any(target_env = "gnu", target_env = "uclibc", target_os = "cygwin"),
        link_name = "__xpg_strerror_r"
    )]
    fn strerror_r(errnum: c_int, buf: *mut c_char, len: usize) -> c_int;
}

/// BString<1024>'s capacity
const BSTRING_MAX: usize = 1023;

pub(crate) fn path(p: &[u8]) -> &Path {
    let nul = p.iter().position(|&b| b == 0).unwrap_or(p.len());
    Path::new(OsStr::from_bytes(&p[..nul]))
}

pub(crate) fn posix_stat(p: &[u8]) -> i32 {
    match std::fs::metadata(path(p)) {
        Ok(m) if m.is_file() => 1,
        Ok(m) if m.is_dir() => 2,
        Ok(_) => 0,
        Err(e) => -e.raw_os_error().unwrap_or(1),
    }
}

pub(crate) fn list_dir_entries(dir: &[u8]) -> Vec<Vec<u8>> {
    let mut entries = Vec::new();
    let Ok(rd) = std::fs::read_dir(path(dir)) else { return entries };
    for entry in rd.flatten() {
        entries.push(entry.file_name().as_bytes().to_vec());
    }
    entries
}

/// The text of an errno value, cut to BString<1024> (GetLastErrorMessage).
pub fn error_message(errnum: i32) -> Vec<u8> {
    let mut buf = [0u8; BSTRING_MAX + 1];
    // The C++ passes msg.Capacity(), 1023 including the terminator, rather
    // than sizeof(msg). Keep that cut, including on ERANGE or unknown errors.
    unsafe { strerror_r(errnum, buf.as_mut_ptr().cast(), BSTRING_MAX) };
    CStr::from_bytes_until_nul(&buf).map(|c| c.to_bytes().to_vec()).unwrap_or_default()
}

/// A native stat callback returns -errno on failure, 1 for a regular file,
/// 2 for a directory, and 0 for another file type. Use the caller's stat ABI:
/// std::fs::metadata may use statx, changing errno and 32-bit overflow behavior.
pub type Stat<'a> = dyn Fn(&[u8]) -> i32 + 'a;

/// FileSystem::FileExists: a regular file (following links).
pub fn file_exists(p: &[u8], stat: &Stat<'_>) -> bool {
    stat(p) == 1
}

/// FileSystem::DirectoryExists: a directory (following links).
pub fn directory_exists(p: &[u8], stat: &Stat<'_>) -> bool {
    stat(p) == 2
}

fn bstring(s: &[u8]) -> Vec<u8> {
    s[..s.len().min(BSTRING_MAX)].to_vec()
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// FileSystem::ForceDirectories (POSIX): creates `path` and its missing
/// parents; on failure the message is in `errmsg`.
pub fn force_directories(orig: &[u8], errmsg: &mut Vec<u8>, stat: &Stat<'_>) -> bool {
    errmsg.clear();
    let mut norm = bstring(orig);
    crate::paths::normalize_path_separators(&mut norm);
    if norm.last() == Some(&b'/') {
        norm.pop();
    }
    let st = stat(&norm);
    match st {
        e if e < 0 && e != -ENOENT => {
            let e = -e;
            *errmsg = cat(&[b"could not read information for directory ", &norm, b": errno ", e.to_string().as_bytes(), b", ", &error_message(e)]);
            return false;
        }
        kind if kind >= 0 && kind != 2 => {
            *errmsg = cat(&[b"path ", &norm, b" is not a directory"]);
            return false;
        }
        _ => {}
    }
    if st < 0 {
        if let Some(slash) = norm.iter().rposition(|&b| b == b'/') {
            let parent = &norm[..slash];
            if parent.len() != orig.len() && !force_directories(parent, errmsg, stat) {
                return false;
            }
        }
        if let Err(e) = std::fs::DirBuilder::new().mode(0o777).create(path(&norm)) {
            let e = e.raw_os_error().unwrap_or(0);
            if e != EEXIST {
                *errmsg = cat(&[b"could not create directory ", &norm, b": ", &error_message(e)]);
                return false;
            }
        }
        match stat(&norm) {
            e if e < 0 => {
                let e = -e;
                *errmsg = cat(&[b"could not read information for directory ", &norm, b": ", &error_message(e)]);
                return false;
            }
            kind if kind >= 0 && kind != 2 => {
                *errmsg = cat(&[b"path ", &norm, b" is not a directory"]);
                return false;
            }
            _ => {}
        }
    }
    true
}

// ENOENT and EEXIST: the same on Linux, the BSDs and macOS
const ENOENT: i32 = 2;
const EEXIST: i32 = 17;

/// FileSystem::MakeUniqueFilename: `dir/basename`, or the first free
/// `dir/name.duplicateN.ext` (a par2 volume keeps its `.volNN+MM.par2`).
pub fn make_unique_filename(dir: &CStr, basename: &CStr, stat: &Stat<'_>) -> Vec<u8> {
    let base = basename.to_bytes();
    let mut result = cat(&[dir.to_bytes(), b"/", base]);
    let mut n = 0;
    while file_exists(&result, stat) {
        n += 1;
        match base.iter().rposition(|&b| b == b'.').filter(|&e| e > 0) {
            Some(ext_at) => {
                let mut without = bstring(base);
                without.truncate(ext_at.min(BSTRING_MAX));
                let mut ext = &base[ext_at..];
                if ext.eq_ignore_ascii_case(b".par2") {
                    if let Some(vol) = without.iter().rposition(|&b| b == b'.').filter(|&v| v > 0) {
                        let tail = &without[vol..];
                        if tail.len() >= 4 && tail[..4].eq_ignore_ascii_case(b".vol") {
                            without.truncate(vol);
                            ext = &base[vol..];
                        }
                    }
                }
                result = cat(&[dir.to_bytes(), b"/", &without, b".duplicate", n.to_string().as_bytes(), ext]);
            }
            None => {
                result = cat(&[dir.to_bytes(), b"/", base, b".duplicate", n.to_string().as_bytes()]);
            }
        }
    }
    result
}

/// FileSystem::ExpandHomePath: `~/` replaced by $HOME (or the user's home
/// directory), else the name as it is (empty for none).
pub fn expand_home_path(filename: Option<&CStr>, home: Option<&[u8]>) -> Vec<u8> {
    let Some(f) = filename else { return Vec::new() };
    let b = f.to_bytes();
    if b.starts_with(b"~/") {
        let Some(home) = home else { return b.to_vec() };
        let sep: &[u8] = if home.last() == Some(&b'/') { b"" } else { b"/" };
        return cat(&[home, sep, &b[2..]]);
    }
    b.to_vec()
}

pub(crate) fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

#[allow(dead_code)]
pub(crate) mod sys {
    use std::ffi::{c_char, c_int, c_void};

    pub const O_RDONLY: c_int = 0;
    pub const O_WRONLY: c_int = 1;
    pub const O_RDWR: c_int = 2;

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    pub const O_APPEND: c_int = 0x0008;
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    pub const O_APPEND: c_int = 0o2000;

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    pub const O_CREAT: c_int = 0x0200;
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    pub const O_CREAT: c_int = 0o100;

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    pub const O_TRUNC: c_int = 0x0400;
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    pub const O_TRUNC: c_int = 0o1000;

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub const O_CLOEXEC: c_int = 0x1000000;
    #[cfg(any(target_os = "freebsd"))]
    pub const O_CLOEXEC: c_int = 0x00100000;
    #[cfg(any(target_os = "openbsd"))]
    pub const O_CLOEXEC: c_int = 0x10000;
    #[cfg(any(target_os = "netbsd"))]
    pub const O_CLOEXEC: c_int = 0x00400000;
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    pub const O_CLOEXEC: c_int = 0o2000000;

    pub const F_GETFD: c_int = 1;
    pub const FD_CLOEXEC: c_int = 1;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub const F_FULLFSYNC: c_int = 51;

    pub const EIO: c_int = 5;
    pub const EEXIST: c_int = 17;
    pub const EXDEV: c_int = 18;
    pub const EINVAL: c_int = 22;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    pub const ENAMETOOLONG: c_int = 63;
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    pub const ENAMETOOLONG: c_int = 36;

    pub const RLIMIT_FSIZE: c_int = 1;
    pub const SIGXFSZ: c_int = 25;
    pub const SIG_IGN: usize = 1;

    #[repr(C)]
    pub struct rlimit {
        pub rlim_cur: u64,
        pub rlim_max: u64,
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[repr(C)]
    pub struct statvfs {
        pub f_bsize: u64,
        pub f_frsize: u64,
        pub f_blocks: u32,
        pub f_bfree: u32,
        pub f_bavail: u32,
        pub f_files: u32,
        pub f_ffree: u32,
        pub f_favail: u32,
        pub f_fsid: u64,
        pub f_flag: u64,
        pub f_namemax: u64,
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    #[repr(C)]
    pub struct statvfs {
        pub f_bsize: usize,
        pub f_frsize: usize,
        pub f_blocks: u64,
        pub f_bfree: u64,
        pub f_bavail: u64,
        pub f_files: u64,
        pub f_ffree: u64,
        pub f_favail: u64,
        pub f_fsid: usize,
        pub f_flag: usize,
        pub f_namemax: usize,
        pub __f_spare: [c_int; 6],
    }

    extern "C" {
        pub fn remove(filename: *const c_char) -> c_int;
        pub fn mkdir(path: *const c_char, mode: u32) -> c_int;
        pub fn rmdir(path: *const c_char) -> c_int;
        #[cfg_attr(
            all(target_os = "linux", target_env = "gnu", target_pointer_width = "32"),
            link_name = "open64"
        )]
        pub fn open(path: *const c_char, oflag: c_int, ...) -> c_int;
        pub fn close(fd: c_int) -> c_int;
        pub fn dup(fd: c_int) -> c_int;
        pub fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
        pub fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
        #[cfg_attr(
            all(
                any(all(target_os = "linux", target_env = "gnu"), target_os = "android"),
                target_pointer_width = "32"
            ),
            link_name = "truncate64"
        )]
        pub fn truncate(path: *const c_char, length: i64) -> c_int;
        pub fn rename(old: *const c_char, new: *const c_char) -> c_int;
        pub fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        pub fn fdatasync(fd: c_int) -> c_int;
        #[cfg(not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "linux",
            target_os = "android"
        )))]
        pub fn fsync(fd: c_int) -> c_int;
        #[cfg_attr(
            all(target_os = "linux", target_env = "gnu", target_pointer_width = "32"),
            link_name = "statvfs64"
        )]
        pub fn statvfs(path: *const c_char, buf: *mut statvfs) -> c_int;
        pub fn fork() -> c_int;
        pub fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
        pub fn execl(path: *const c_char, arg0: *const c_char, ...) -> c_int;
        pub fn _exit(status: c_int) -> !;
        pub fn getrlimit(resource: c_int, rlp: *mut rlimit) -> c_int;
        pub fn setrlimit(resource: c_int, rlp: *const rlimit) -> c_int;
        pub fn signal(sig: c_int, func: usize) -> usize;

        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd"
        ))]
        pub fn __error() -> *mut c_int;
        #[cfg(not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd"
        )))]
        pub fn __errno_location() -> *mut c_int;
    }

    pub fn wifexited(status: c_int) -> bool {
        (status & 0x7f) == 0
    }

    pub fn wexitstatus(status: c_int) -> c_int {
        (status >> 8) & 0xff
    }
}

/// `FileSystem::FileSize` (`daemon/util/FileSystem.cpp`):
/// Returns `stat(filename).st_size` on success, or `-1` on failure.
pub fn file_size(filename: &CStr) -> i64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path(filename.to_bytes())).map_or(-1, |m| m.size() as i64)
}

/// `FileSystem::DeleteFile` (`daemon/util/FileSystem.cpp`):
/// Removes `filename` via `remove`.
pub fn delete_file(filename: &CStr) -> bool {
    unsafe { sys::remove(filename.as_ptr()) == 0 }
}

/// `FileSystem::CreateDirectoryExclusive` (`daemon/util/FileSystem.cpp`):
/// Creates `dir_filename` via `mkdir(..., 0o777)`, failing if it already exists or is a symlink.
pub fn create_directory_exclusive(dir_filename: &CStr) -> bool {
    unsafe { sys::mkdir(dir_filename.as_ptr(), 0o777) == 0 }
}

/// `FileSystem::RemoveDirectory` (`daemon/util/FileSystem.cpp`):
/// Removes an empty directory via `rmdir`.
pub fn remove_directory(dir_filename: &CStr) -> bool {
    unsafe { sys::rmdir(dir_filename.as_ptr()) == 0 }
}

/// `DiskFile::Open` (`daemon/util/FileSystem.cpp`):
/// Opens `filename` with `O_CLOEXEC` and the mode-specific flags (`omRead`, `omReadWrite`, `omWrite`, `omAppend`).
pub fn open_cloexec_fd(
    filename: &CStr,
    mode: i32,
    om_read: i32,
    om_read_write: i32,
    om_write: i32,
) -> i32 {
    let flags = if mode == om_read {
        sys::O_RDONLY
    } else if mode == om_read_write {
        sys::O_RDWR
    } else if mode == om_write {
        sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC
    } else {
        sys::O_WRONLY | sys::O_CREAT | sys::O_APPEND
    };
    unsafe { sys::open(filename.as_ptr(), flags | sys::O_CLOEXEC, 0o666u32) }
}

/// `FileSystem::FlushFileBuffers` (`daemon/util/FileSystem.cpp`):
/// Flushes `file_descriptor` to disk (`F_FULLFSYNC` on macOS/iOS, `fdatasync` on Linux/Android, `fsync` elsewhere).
pub fn flush_file_buffers(file_descriptor: i32, errmsg: &mut Vec<u8>) -> bool {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let ret = unsafe {
        if sys::fcntl(file_descriptor, sys::F_FULLFSYNC) == -1 {
            1
        } else {
            0
        }
    };
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let ret = unsafe { sys::fdatasync(file_descriptor) };
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android"
    )))]
    let ret = unsafe { sys::fsync(file_descriptor) };

    if ret != 0 {
        *errmsg = error_message(last_errno());
        false
    } else {
        true
    }
}

/// `FileSystem::FlushDirBuffers` (`daemon/util/FileSystem.cpp`):
/// Opens the parent directory of `filename` with `O_RDONLY | O_CLOEXEC` and flushes its buffers.
pub fn flush_dir_buffers(filename: &CStr, path_sep: u8, errmsg: &mut Vec<u8>) -> bool {
    let mut parent = crate::util::fs_extract_parent_dir_path(filename.to_bytes(), path_sep);
    parent.push(0);
    let Ok(parent_cstr) = CStr::from_bytes_with_nul(&parent) else {
        *errmsg = error_message(sys::EINVAL);
        return false;
    };
    let fd = open_cloexec_fd(parent_cstr, 0, 0, 1, 2);
    if fd == -1 {
        *errmsg = error_message(last_errno());
        return false;
    }
    let ok = flush_file_buffers(fd, errmsg);
    unsafe { sys::close(fd) };
    ok
}

/// `FileSystem::AllocateFile` (`daemon/util/FileSystem.cpp`):
/// Preallocates `filename` to `size` bytes via `truncate`, falling back to 16KB zero chunks.
pub fn allocate_file(filename: &CStr, size: i64, errmsg: &mut Vec<u8>) -> bool {
    errmsg.clear();
    let fd = open_cloexec_fd(filename, 3, 0, 1, 2);
    if fd == -1 {
        *errmsg = error_message(last_errno());
        return false;
    }
    unsafe { sys::close(fd) };

    unsafe { sys::truncate(filename.as_ptr(), size) };
    let mut ok = file_size(filename) == size;
    if !ok {
        unsafe { sys::truncate(filename.as_ptr(), 0) };
        let fd = open_cloexec_fd(filename, 3, 0, 1, 2);
        if fd == -1 {
            *errmsg = error_message(last_errno());
            return false;
        }
        let zeros = [0u8; 16 * 1024];
        let mut remaining = size;
        while remaining > 0 {
            let need = crate::util::fs_calc_allocate_chunk_size(remaining, zeros.len() as i64);
            let written = unsafe { sys::write(fd, zeros.as_ptr().cast(), need as usize) as i64 };
            if written != need {
                *errmsg = error_message(last_errno());
                unsafe { sys::close(fd) };
                return false;
            }
            remaining -= written;
        }
        unsafe { sys::close(fd) };
        ok = file_size(filename) == size;
        if !ok {
            *errmsg = b"created file has wrong size".to_vec();
        }
    }
    ok
}

/// `FileSystem::CopyFile` (`daemon/util/FileSystem.cpp`):
/// Copies `src_filename` to `dst_filename` in 50KB chunks with `O_CLOEXEC`, verifying size and
/// removing `dst_filename` on any read/write/close/size error.
pub fn copy_file(src_filename: &CStr, dst_filename: &CStr) -> bool {
    let in_fd = open_cloexec_fd(src_filename, 0, 0, 1, 2);
    if in_fd == -1 {
        return false;
    }
    let out_fd = open_cloexec_fd(dst_filename, 2, 0, 1, 2);
    if out_fd == -1 {
        let err = last_errno();
        unsafe { sys::close(in_fd) };
        set_last_errno(err);
        return false;
    }

    let mut buffer = vec![0u8; 1024 * 50];
    let mut ok = true;
    let mut copied: i64 = 0;
    let mut write_errno: i32 = 0;
    loop {
        let cnt = unsafe { sys::read(in_fd, buffer.as_mut_ptr().cast(), buffer.len()) as i64 };
        if cnt <= 0 {
            break;
        }
        let mut off = 0usize;
        let mut write_failed = false;
        while off < cnt as usize {
            let w = unsafe {
                sys::write(out_fd, buffer[off..].as_ptr().cast(), (cnt as usize) - off)
            };
            if w <= 0 {
                write_errno = last_errno();
                write_failed = true;
                break;
            }
            off += w as usize;
        }
        if write_failed {
            ok = false;
            break;
        }
        copied += cnt;
    }

    unsafe { sys::close(in_fd) };
    let close_ok = unsafe { sys::close(out_fd) == 0 };
    if !close_ok && write_errno == 0 {
        write_errno = last_errno();
    }
    ok = close_ok
        && ok
        && crate::util::fs_is_copied_file_valid(close_ok, ok, copied, file_size(src_filename));
    if !ok {
        delete_file(dst_filename);
        if write_errno != 0 {
            set_last_errno(write_errno);
        }
    }
    ok
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn set_last_errno(err: i32) {
    unsafe { *sys::__error() = err };
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
fn set_last_errno(err: i32) {
    unsafe { *sys::__errno_location() = err };
}

/// `FileSystem::MoveFile` (`daemon/util/FileSystem.cpp`):
/// Renames `src_filename` to `dst_filename`, falling back to `copy_file` + `delete_file` on `EXDEV`.
pub fn move_file(src_filename: &CStr, dst_filename: &CStr) -> bool {
    let mut ok = unsafe { sys::rename(src_filename.as_ptr(), dst_filename.as_ptr()) == 0 };
    if crate::util::fs_should_fallback_move_to_copy(ok, last_errno(), sys::EXDEV) {
        ok = copy_file(src_filename, dst_filename) && delete_file(src_filename);
    }
    ok
}

/// `fs::move_file` (`daemon/util/FileSystem.h`):
/// Moves `src_filename` to `dst_filename` without overwriting an existing destination,
/// falling back to `copy_file` + `delete_file` on `EXDEV` and removing any partial
/// destination if the cross-device copy or source removal fails.
pub fn move_file_no_overwrite(src_filename: &CStr, dst_filename: &CStr) -> Result<(), i32> {
    match std::fs::metadata(path(dst_filename.to_bytes())) {
        Ok(_) => {
            set_last_errno(sys::EEXIST);
            return Err(sys::EEXIST);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            let err = e.raw_os_error().unwrap_or(sys::EIO);
            set_last_errno(err);
            return Err(err);
        }
    }
    if unsafe { sys::rename(src_filename.as_ptr(), dst_filename.as_ptr()) == 0 } {
        return Ok(());
    }
    let err = last_errno();
    if err != sys::EXDEV {
        return Err(err);
    }
    let src_perms = match std::fs::metadata(path(src_filename.to_bytes())) {
        Ok(m) => m.permissions(),
        Err(e) => {
            let meta_err = e.raw_os_error().unwrap_or(sys::EIO);
            set_last_errno(meta_err);
            return Err(meta_err);
        }
    };
    if !copy_file(src_filename, dst_filename) {
        let copy_err = {
            let e = last_errno();
            if e == 0 { sys::EIO } else { e }
        };
        delete_file(dst_filename);
        set_last_errno(copy_err);
        return Err(copy_err);
    }
    if let Err(e) = std::fs::set_permissions(path(dst_filename.to_bytes()), src_perms) {
        let perm_err = e.raw_os_error().unwrap_or(sys::EIO);
        delete_file(dst_filename);
        set_last_errno(perm_err);
        return Err(perm_err);
    }
    if !delete_file(src_filename) {
        let rm_err = {
            let e = last_errno();
            if e == 0 { sys::EIO } else { e }
        };
        delete_file(dst_filename);
        set_last_errno(rm_err);
        return Err(rm_err);
    }
    Ok(())
}

/// Computes `(available_bytes, total_bytes)` from `(f_bavail, f_blocks, f_frsize)`
/// returned by `statvfs_fn`, saturating each product to `i64::MAX`.
pub fn get_disk_state_with(
    path: &CStr,
    statvfs_fn: &dyn Fn(&CStr) -> Option<(u64, u64, u64)>,
) -> Option<(i64, i64)> {
    let (bavail, blocks, frsize) = statvfs_fn(path)?;
    let avail_u64 = bavail.wrapping_mul(frsize);
    let total_u64 = blocks.wrapping_mul(frsize);
    let available = avail_u64.min(i64::MAX as u64) as i64;
    let total = total_u64.min(i64::MAX as u64) as i64;
    Some((available, total))
}

fn default_statvfs(path: &CStr) -> Option<(u64, u64, u64)> {
    unsafe {
        let mut diskdata: sys::statvfs = std::mem::zeroed();
        if sys::statvfs(path.as_ptr(), &mut diskdata) == 0 {
            Some((
                diskdata.f_bavail as u64,
                diskdata.f_blocks as u64,
                diskdata.f_frsize as u64,
            ))
        } else {
            None
        }
    }
}

/// `FileSystem::GetDiskState` (`daemon/util/FileSystem.cpp`):
/// Returns `Some((available_bytes, total_bytes))` from `statvfs`, saturating to `i64::MAX`.
pub fn get_disk_state(path: &CStr) -> Option<(i64, i64)> {
    get_disk_state_with(path, &default_statvfs)
}

/// `FileSystem::FixExecPermission` (`daemon/util/FileSystem.h`):
/// Adds execute bits (`0o111`) to `filename` if `stat` succeeds.
pub fn fix_exec_permission(filename: &[u8]) {
    use std::os::unix::fs::PermissionsExt;
    let p = path(filename);
    if let Ok(meta) = std::fs::metadata(p) {
        let new_mode = meta.permissions().mode() | 0o111;
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(new_mode));
    }
}

/// `FileSystem::RestorePermissions` (`daemon/util/FileSystem.h`):
/// Sets permissions of `filename` to `(mode & !umask) & 0o7777`.
pub fn restore_permissions(filename: &[u8], mode: u32, umask: u32) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let permissions = (mode & !umask) & 0o7777;
    std::fs::set_permissions(path(filename), std::fs::Permissions::from_mode(permissions)).is_ok()
}

/// `FileSystem::RestoreFilePermissions` (`daemon/util/FileSystem.h`):
/// Restores `0o666 & !umask` permissions on `filename`.
pub fn restore_file_permissions(filename: &[u8], umask: u32) -> bool {
    restore_permissions(filename, 0o666, umask)
}

/// `FileSystem::RestoreDirPermissions` (`daemon/util/FileSystem.h`):
/// Restores `0o777 & !umask` permissions on `filename`.
pub fn restore_dir_permissions(filename: &[u8], umask: u32) -> bool {
    restore_permissions(filename, 0o777, umask)
}

/// `FileSystem::RestoreFileOrDirPermissions` (`daemon/util/FileSystem.h`):
/// Restores `0o777 & !umask` if `filename` is a directory, or `0o666 & !umask` if it is a file.
pub fn restore_file_or_dir_permissions(filename: &[u8], umask: u32) -> bool {
    let Ok(meta) = std::fs::metadata(path(filename)) else {
        return false;
    };
    if meta.is_dir() {
        restore_dir_permissions(filename, umask)
    } else {
        restore_file_permissions(filename, umask)
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fs_fix_exec_permission(filename: *const c_char) {
    if filename.is_null() {
        return;
    }
    let bytes = unsafe { CStr::from_ptr(filename).to_bytes() };
    fix_exec_permission(bytes);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fs_restore_permissions(
    filename: *const c_char,
    mode: u32,
    umask: u32,
) -> c_int {
    if filename.is_null() {
        return 0;
    }
    let bytes = unsafe { CStr::from_ptr(filename).to_bytes() };
    c_int::from(restore_permissions(bytes, mode, umask))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_fs_restore_file_or_dir_permissions(
    filename: *const c_char,
    umask: u32,
) -> c_int {
    if filename.is_null() {
        return 0;
    }
    let bytes = unsafe { CStr::from_ptr(filename).to_bytes() };
    c_int::from(restore_file_or_dir_permissions(bytes, umask))
}

pub(crate) struct PosixFsOps;

impl crate::fsdelete::Ops for PosixFsOps {
    fn path_type(&mut self, p: &[u8]) -> crate::fsdelete::R<crate::fsdelete::PathType> {
        use crate::fsdelete::PathType;
        match std::fs::symlink_metadata(path(p)) {
            Ok(m) => {
                let ft = m.file_type();
                if ft.is_symlink() {
                    Ok(PathType::Link)
                } else if ft.is_dir() {
                    Ok(PathType::Directory)
                } else {
                    Ok(PathType::File)
                }
            }
            Err(e) => {
                if let Some(err) = e.raw_os_error() {
                    set_last_errno(err);
                }
                Ok(PathType::Error)
            }
        }
    }
    fn remove_dir(&mut self, p: &[u8]) -> crate::fsdelete::R<bool> {
        let Ok(c) = std::ffi::CString::new(p) else {
            return Ok(false);
        };
        Ok(remove_directory(&c))
    }
    fn delete_file(&mut self, p: &[u8]) -> crate::fsdelete::R<bool> {
        let Ok(c) = std::ffi::CString::new(p) else {
            return Ok(false);
        };
        Ok(delete_file(&c))
    }
    fn delete_link(&mut self, p: &[u8], _kind: crate::fsdelete::PathType) -> crate::fsdelete::R<bool> {
        self.delete_file(p)
    }
    fn list(&mut self, dir: &[u8]) -> crate::fsdelete::R<Vec<Vec<u8>>> {
        Ok(list_dir_entries(dir))
    }
    fn file_exists(&mut self, p: &[u8]) -> crate::fsdelete::R<bool> {
        Ok(file_exists(p, &posix_stat))
    }
    fn dir_exists(&mut self, p: &[u8]) -> crate::fsdelete::R<bool> {
        Ok(directory_exists(p, &posix_stat))
    }
    fn move_file(&mut self, from: &[u8], to: &[u8]) -> crate::fsdelete::R<bool> {
        let (Ok(f), Ok(t)) = (std::ffi::CString::new(from), std::ffi::CString::new(to)) else {
            return Ok(false);
        };
        Ok(move_file(&f, &t))
    }
    fn last_error(&mut self) -> crate::fsdelete::R<Vec<u8>> {
        Ok(error_message(last_errno()))
    }
    fn too_long(&mut self) -> crate::fsdelete::R<()> {
        set_last_errno(sys::ENAMETOOLONG);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsdelete::{delete_directory, delete_directory_with_content};
    use crate::paths::{escape_path_for_shell, extract_file_path_from_cmd, split_path_and_filename};
    use std::ffi::CString;

    fn stat(p: &[u8]) -> i32 {
        match std::fs::metadata(path(p)) {
            Ok(m) => if m.is_file() { 1 } else if m.is_dir() { 2 } else { 0 },
            Err(e) => -e.raw_os_error().unwrap(),
        }
    }

    use PosixFsOps as RealPosixFsOps;

    fn child_sees_descriptor(fd: i32) -> i32 {
        let script = CString::new(format!("exec 2>/dev/null; : <&{fd}")).unwrap();
        let sh = c"/bin/sh";
        let sh_arg = c"sh";
        let c_arg = c"-c";
        unsafe {
            let pid = sys::fork();
            if pid == -1 {
                return -1;
            }
            if pid == 0 {
                sys::execl(
                    sh.as_ptr(),
                    sh_arg.as_ptr(),
                    c_arg.as_ptr(),
                    script.as_ptr(),
                    std::ptr::null::<c_char>(),
                );
                sys::_exit(127);
            }
            let mut status: c_int = 0;
            if sys::waitpid(pid, &mut status, 0) != pid
                || !sys::wifexited(status)
                || sys::wexitstatus(status) == 127
            {
                return -1;
            }
            if sys::wexitstatus(status) == 0 {
                1
            } else {
                0
            }
        }
    }

    #[test]
    fn messages() {
        assert_eq!(error_message(2), b"No such file or directory");
    }

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir().join(format!("nzbget-fsops-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let d = std::ffi::CString::new(dir.as_os_str().as_bytes()).unwrap();
        let mut f = dir.as_os_str().as_bytes().to_vec();
        f.extend_from_slice(b"/a.vol01+02.par2");
        std::fs::write(path(&f), b"").unwrap();
        let u = make_unique_filename(&d, c"a.vol01+02.par2", &stat);
        assert!(u.ends_with(b"/a.duplicate1.vol01+02.par2"));
        let mut msg = Vec::new();
        let mut sub = dir.as_os_str().as_bytes().to_vec();
        sub.extend_from_slice(b"/x/y/z");
        assert!(force_directories(&sub, &mut msg, &stat), "{}", String::from_utf8_lossy(&msg));
        assert!(directory_exists(&sub, &stat));
        assert!(!force_directories(&f, &mut msg, &stat));
        assert!(msg.ends_with(b"is not a directory"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(expand_home_path(Some(c"~/x"), Some(b"/home/u")), b"/home/u/x");
        assert_eq!(expand_home_path(Some(c"~/x"), Some(b"/")), b"/x");
    }

    #[test]
    fn filesystem_full_suite_port_from_cpp() {
        let base_dir = std::env::temp_dir().join(format!(
            "nzbget-fs-suite-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base_dir).unwrap();

        // 1. FileSizeRejectsFailedStat
        {
            let p = base_dir.join("missing-stat-target");
            let p_c = CString::new(p.as_os_str().as_bytes()).unwrap();
            let p_sub_c = CString::new(p.join("missing").as_os_str().as_bytes()).unwrap();
            for _ in 0..64 {
                assert_eq!(file_size(&p_c), -1);
                assert_eq!(file_size(&p_sub_c), -1);
            }
            std::fs::write(&p, b"known file size").unwrap();
            assert_eq!(file_size(&p_c), 15);
            let not_dir_c = CString::new(p.join("not-a-directory").as_os_str().as_bytes()).unwrap();
            assert_eq!(file_size(&not_dir_c), -1);
            let _ = std::fs::remove_file(&p);
        }

        // 2. ExtractFilePathCmdTest & 3. EscapePathForShellTest
        {
            assert_eq!(
                extract_file_path_from_cmd(b"/usr/nzbget/unrar"),
                b"/usr/nzbget/unrar"
            );
            assert_eq!(
                extract_file_path_from_cmd(b"/usr/nzbget/unrar -ai"),
                b"/usr/nzbget/unrar"
            );
            assert_eq!(extract_file_path_from_cmd(b""), b"");

            assert_eq!(
                escape_path_for_shell(b"/usr/my dir/nzbget/unrar"),
                b"\"/usr/my dir/nzbget/unrar\""
            );
            assert_eq!(escape_path_for_shell(b""), b"");
        }

        // 4. DeleteDirectoryDoesNotFollowSymlink
        {
            let outside = base_dir.join("deldir-outside");
            let scratch = base_dir.join("deldir-scratch");
            std::fs::create_dir_all(outside.join("empty")).unwrap();
            std::fs::create_dir_all(outside.join("hidden")).unwrap();
            std::fs::write(outside.join("hidden/.dotfile"), b"safe").unwrap();
            std::fs::create_dir_all(&scratch).unwrap();
            std::os::unix::fs::symlink(&outside, scratch.join("escape")).unwrap();

            let mut ops = RealPosixFsOps;
            let _ = delete_directory(&mut ops, scratch.as_os_str().as_bytes(), b'/');
            assert!(outside.join("empty").exists());
            assert!(outside.join("hidden/.dotfile").exists());
            let _ = std::fs::remove_dir_all(&scratch);
            let _ = std::fs::remove_dir_all(&outside);
        }

        // 5. DeleteDirectoryWithContentDoesNotFollowNestedSymlink
        {
            let outside = base_dir.join("delete-outside");
            let scratch = base_dir.join("delete-scratch");
            let sentinel = outside.join("sentinel");
            std::fs::create_dir_all(&outside).unwrap();
            std::fs::create_dir_all(&scratch).unwrap();
            std::fs::write(&sentinel, b"safe").unwrap();
            std::os::unix::fs::symlink(&outside, scratch.join("escape")).unwrap();

            let mut ops = RealPosixFsOps;
            let mut errmsg = Vec::new();
            let deleted = delete_directory_with_content(
                &mut ops,
                scratch.as_os_str().as_bytes(),
                b'/',
                &mut errmsg,
            )
            .unwrap();
            assert!(deleted, "{}", String::from_utf8_lossy(&errmsg));
            assert!(sentinel.exists());
            assert!(!scratch.exists() && std::fs::symlink_metadata(&scratch).is_err());
            let _ = std::fs::remove_dir_all(&outside);
        }

        // 6. DeleteDirectoryWithContentDoesNotFollowRootSymlink
        {
            let outside = base_dir.join("delete-root-outside");
            let root_link = base_dir.join("delete-root-link");
            let sentinel = outside.join("sentinel");
            std::fs::create_dir_all(&outside).unwrap();
            std::fs::write(&sentinel, b"safe").unwrap();
            std::os::unix::fs::symlink(&outside, &root_link).unwrap();

            let mut ops = RealPosixFsOps;
            let mut errmsg = Vec::new();
            let deleted = delete_directory_with_content(
                &mut ops,
                root_link.as_os_str().as_bytes(),
                b'/',
                &mut errmsg,
            )
            .unwrap();
            assert!(deleted, "{}", String::from_utf8_lossy(&errmsg));
            assert!(sentinel.exists());
            assert!(!root_link.exists() && std::fs::symlink_metadata(&root_link).is_err());
            let _ = std::fs::remove_dir_all(&outside);
        }

        // 7. CreateDirectoryExclusiveRejectsSymlink & 11. CreateDirectoryExclusiveCreatesOnce
        {
            let outside = base_dir.join("create-outside");
            let dir_link = base_dir.join("create-link");
            std::fs::create_dir_all(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, &dir_link).unwrap();
            let dir_link_c = CString::new(dir_link.as_os_str().as_bytes()).unwrap();
            assert!(!create_directory_exclusive(&dir_link_c));
            assert!(std::fs::symlink_metadata(&dir_link)
                .unwrap()
                .file_type()
                .is_symlink());
            assert!(outside.is_dir());

            let new_dir = base_dir.join("create-exclusive-once");
            let new_dir_c = CString::new(new_dir.as_os_str().as_bytes()).unwrap();
            assert!(create_directory_exclusive(&new_dir_c));
            assert!(!create_directory_exclusive(&new_dir_c));
            assert!(new_dir.is_dir());
        }

        // 8. DiskFileCloseOnExecTest & 9. ForkExecDoesNotRetainCloseOnExecFilesTest
        {
            let clo_dir = base_dir.join("cloexec-dir");
            std::fs::create_dir_all(&clo_dir).unwrap();
            let temp_file = clo_dir.join("file.tmp");
            let temp_file_c = CString::new(temp_file.as_os_str().as_bytes()).unwrap();
            let clo_dir_c = CString::new(clo_dir.as_os_str().as_bytes()).unwrap();

            for (target, mode) in [
                (&temp_file_c, 2), // omWrite
                (&temp_file_c, 3), // omAppend
                (&temp_file_c, 1), // omReadWrite
                (&temp_file_c, 0), // omRead
                (&clo_dir_c, 0),   // directory omRead
            ] {
                let fd = open_cloexec_fd(target, mode, 0, 1, 2);
                assert!(fd >= 0);
                let flags = unsafe { sys::fcntl(fd, sys::F_GETFD) };
                assert_ne!(flags, -1);
                assert_ne!(flags & sys::FD_CLOEXEC, 0);
                unsafe { sys::close(fd) };
            }

            let fd = open_cloexec_fd(&temp_file_c, 0, 0, 1, 2);
            assert!(fd >= 0);
            let inherited = unsafe { sys::dup(fd) };
            assert!(inherited >= 0);
            assert_eq!(child_sees_descriptor(inherited), 1);
            unsafe { sys::close(inherited) };
            assert_eq!(child_sees_descriptor(fd), 0);
            unsafe { sys::close(fd) };
        }

        // 10. AllocateAndFlushDirBuffersTest
        {
            let alloc_dir = base_dir.join("allocate-dir");
            std::fs::create_dir_all(&alloc_dir).unwrap();
            let alloc_file = alloc_dir.join("allocated.bin");
            let alloc_file_c = CString::new(alloc_file.as_os_str().as_bytes()).unwrap();
            let mut errmsg = Vec::new();
            assert!(
                allocate_file(&alloc_file_c, 100_000, &mut errmsg),
                "{}",
                String::from_utf8_lossy(&errmsg)
            );
            assert_eq!(file_size(&alloc_file_c), 100_000);
            assert!(
                flush_dir_buffers(&alloc_file_c, b'/', &mut errmsg),
                "{}",
                String::from_utf8_lossy(&errmsg)
            );

            let missing_alloc_c =
                CString::new(alloc_dir.join("missing/x.bin").as_os_str().as_bytes()).unwrap();
            assert!(!allocate_file(&missing_alloc_c, 10, &mut errmsg));
            assert!(!errmsg.is_empty());
        }

        // 12. SplitPathAndFilenameTest
        {
            let check_split = |full: &[u8], exp_dir: &[u8], exp_file: &[u8]| {
                let (dir, file) = split_path_and_filename(full);
                assert_eq!((dir, file), (exp_dir, exp_file));
            };
            check_split(b"/path/to/filename.txt", b"/path/to", b"filename.txt");
            check_split(b"/path/to/", b"/path/to", b"");
            check_split(b"C:\\path\\to\\filename.txt", b"C:\\path\\to", b"filename.txt");
            check_split(b"C:\\path\\to\\", b"C:\\path\\to", b"");
            check_split(b"/path\\to/filename.txt", b"/path\\to", b"filename.txt");
            check_split(b"", b"", b"");
            check_split(b"/filename.txt", b"", b"filename.txt");
            check_split(b"\\filename.txt", b"", b"filename.txt");
            check_split(
                b"/path/to\\a/b\\c/filename.txt",
                b"/path/to\\a/b\\c",
                b"filename.txt",
            );
            check_split(b"/", b"", b"");
            check_split(b"\\", b"", b"");
        }

        // 13. GetDiskStateUtf8PathTest, 14. DeleteFileUtf8PathTest, 15. DeleteReadOnlyFileUtf8PathTest
        {
            let utf8_dir = base_dir.join("nzbget_l\u{00e4}_utf8");
            std::fs::create_dir_all(&utf8_dir).unwrap();
            let utf8_dir_c = CString::new(utf8_dir.as_os_str().as_bytes()).unwrap();
            let state = get_disk_state(&utf8_dir_c).expect("get_disk_state should succeed");
            assert!(state.0 > 0);
            assert!(state.1 > 0);
            let sat_state = get_disk_state_with(&utf8_dir_c, &|_| {
                Some((u64::MAX / 2, u64::MAX, 4096))
            })
            .unwrap();
            assert_eq!(sat_state, (i64::MAX, i64::MAX));

            let utf8_file = utf8_dir.join("\u{4f60}\u{597d}_l\u{00e4}.bin");
            std::fs::write(&utf8_file, b"data").unwrap();
            let utf8_file_c = CString::new(utf8_file.as_os_str().as_bytes()).unwrap();
            assert!(delete_file(&utf8_file_c));
            assert!(!utf8_file.exists());

            let ro_file = utf8_dir.join("readonly_l\u{00e4}.bin");
            std::fs::write(&ro_file, b"data").unwrap();
            let mut perms = std::fs::metadata(&ro_file).unwrap().permissions();
            perms.set_readonly(true);
            std::fs::set_permissions(&ro_file, perms).unwrap();
            let ro_file_c = CString::new(ro_file.as_os_str().as_bytes()).unwrap();
            assert!(delete_file(&ro_file_c));
            assert!(!ro_file.exists());
        }

        // 16. CopyFileFailsOnFailedWrite & 17. MoveFileCrossDeviceFailedCopy
        // Run RLIMIT_FSIZE limit inside a forked child so process-wide RLIMIT_FSIZE never races other test threads.
        {
            let copy_dir = base_dir.join("copyfile-test");
            std::fs::create_dir_all(&copy_dir).unwrap();
            let src = copy_dir.join("src.bin");
            let dst = copy_dir.join("dst.bin");
            std::fs::write(&src, vec![b'x'; 300 * 1024]).unwrap();
            let src_c = CString::new(src.as_os_str().as_bytes()).unwrap();
            let dst_c = CString::new(dst.as_os_str().as_bytes()).unwrap();

            unsafe {
                let pid = sys::fork();
                assert!(pid >= 0);
                if pid == 0 {
                    let mut limit: sys::rlimit = std::mem::zeroed();
                    sys::getrlimit(sys::RLIMIT_FSIZE, &mut limit);
                    limit.rlim_cur = 64 * 1024;
                    sys::signal(sys::SIGXFSZ, sys::SIG_IGN);
                    sys::setrlimit(sys::RLIMIT_FSIZE, &limit);
                    let copied = copy_file(&src_c, &dst_c);
                    sys::_exit(if !copied { 0 } else { 1 });
                }
                let mut status: c_int = 0;
                assert_eq!(sys::waitpid(pid, &mut status, 0), pid);
                assert!(sys::wifexited(status) && sys::wexitstatus(status) == 0);
            }
            assert!(!dst.exists());
            assert!(copy_file(&src_c, &dst_c));
            assert_eq!(std::fs::metadata(&dst).unwrap().len(), 300 * 1024);
            assert_eq!(move_file_no_overwrite(&src_c, &dst_c), Err(sys::EEXIST));

            let shm_src_dir = std::path::Path::new("/dev/shm")
                .join(format!("nzbget-move-src-{}", std::process::id()));
            if std::fs::create_dir_all(&shm_src_dir).is_ok() {
                use std::os::unix::fs::MetadataExt;
                let shm_dev = std::fs::metadata(&shm_src_dir).ok().map(|m| m.dev());
                let mut alt_dst_dir = None;
                let target_dst_dir = if shm_dev.is_some()
                    && std::fs::metadata(&copy_dir).ok().map(|m| m.dev()) != shm_dev
                {
                    Some(copy_dir.clone())
                } else if let Ok(cwd) = std::env::current_dir() {
                    let cwd_tmp = cwd.join(format!("nzbget-move-dst-{}", std::process::id()));
                    if std::fs::create_dir_all(&cwd_tmp).is_ok() {
                        if shm_dev.is_some()
                            && std::fs::metadata(&cwd_tmp).ok().map(|m| m.dev()) != shm_dev
                        {
                            alt_dst_dir = Some(cwd_tmp.clone());
                            Some(cwd_tmp)
                        } else {
                            let _ = std::fs::remove_dir_all(&cwd_tmp);
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(dst_dir) = target_dst_dir {
                    let shm_src = shm_src_dir.join("file.bin");
                    let move_dst = dst_dir.join("moved-cross.bin");
                    std::fs::write(&shm_src, vec![b'x'; 300 * 1024]).unwrap();
                    let shm_src_c = CString::new(shm_src.as_os_str().as_bytes()).unwrap();
                    let move_dst_c = CString::new(move_dst.as_os_str().as_bytes()).unwrap();
                    unsafe {
                        let pid = sys::fork();
                        assert!(pid >= 0);
                        if pid == 0 {
                            let mut limit: sys::rlimit = std::mem::zeroed();
                            sys::getrlimit(sys::RLIMIT_FSIZE, &mut limit);
                            limit.rlim_cur = 64 * 1024;
                            sys::signal(sys::SIGXFSZ, sys::SIG_IGN);
                            sys::setrlimit(sys::RLIMIT_FSIZE, &limit);
                            let moved = move_file_no_overwrite(&shm_src_c, &move_dst_c);
                            sys::_exit(if moved.is_err() { 0 } else { 1 });
                        }
                        let mut status: c_int = 0;
                        assert_eq!(sys::waitpid(pid, &mut status, 0), pid);
                        assert!(sys::wifexited(status) && sys::wexitstatus(status) == 0);
                    }
                    assert!(!move_dst.exists());
                    assert!(shm_src.exists());
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&shm_src, std::fs::Permissions::from_mode(0o600))
                        .unwrap();
                    assert_eq!(move_file_no_overwrite(&shm_src_c, &move_dst_c), Ok(()));
                    assert!(!shm_src.exists());
                    assert!(move_dst.exists());
                    assert_eq!(
                        std::fs::metadata(&move_dst).unwrap().permissions().mode() & 0o777,
                        0o600
                    );
                }
                if let Some(d) = alt_dst_dir {
                    let _ = std::fs::remove_dir_all(&d);
                }
                let _ = std::fs::remove_dir_all(&shm_src_dir);
            }
        }

        let _ = std::fs::remove_dir_all(&base_dir);
    }
}
