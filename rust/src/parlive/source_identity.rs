//! Descriptor identity for cooperative source writers. All filesystem work is
//! outside the queue. This does not claim protection from hostile external edits.
use super::{ControlLease, WriteBlocked};
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RootIdentity {
    device: u64,
    inode: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    device: u64,
    inode: u64,
}
impl FileIdentity {
    pub fn of(file: &File) -> io::Result<Self> {
        let meta = file.metadata()?;
        if !meta.is_file() || meta.nlink() != 1 {
            return Err(refused());
        }
        Ok(Self {
            device: meta.dev(),
            inode: meta.ino(),
        })
    }
}

pub(crate) struct SourceRoot {
    directory: File,
    pub identity: RootIdentity,
    pub spelling: PathBuf,
    // The directory FD, owned spelling and retained object remain charged.
    _charge: ControlLease,
}
impl SourceRoot {
    pub fn open(root: &Path, job: i32) -> io::Result<Self> {
        if !root.is_absolute() || root.as_os_str().len() > 4096 {
            return Err(refused());
        }
        let bytes = root
            .as_os_str()
            .len()
            .checked_add(1024)
            .ok_or_else(refused)?;
        let charge = super::reserve_root_control(
            job,
            bytes as u64,
            crate::parchecker::effective_live_pool_bytes(),
        )
        .map_err(|_| refused())?;
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(root)?;
        let meta = directory.metadata()?;
        if !meta.is_dir() {
            return Err(refused());
        }
        Ok(Self {
            directory,
            identity: RootIdentity {
                device: meta.dev(),
                inode: meta.ino(),
            },
            spelling: root.to_owned(),
            _charge: charge,
        })
    }
    pub fn is_current(&self) -> io::Result<()> {
        let other = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(&self.spelling)?;
        let meta = other.metadata()?;
        if self.identity
            != (RootIdentity {
                device: meta.dev(),
                inode: meta.ino(),
            })
        {
            return Err(refused());
        }
        Ok(())
    }
    pub fn open_leaf(&self, leaf: &std::ffi::OsStr, write: bool, create: bool) -> io::Result<File> {
        let bytes = leaf.as_encoded_bytes();
        if bytes.is_empty()
            || bytes.len() > 255
            || bytes == b"."
            || bytes == b".."
            || bytes.iter().any(|&b| matches!(b, 0 | b'/' | b'\\'))
        {
            return Err(refused());
        }
        self.is_current()?;
        let leaf = CString::new(bytes).map_err(|_| refused())?;
        let flags = libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if write { libc::O_RDWR } else { libc::O_RDONLY }
            | if create { libc::O_CREAT } else { 0 };
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                leaf.as_ptr(),
                flags,
                0o666 as libc::c_uint,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        FileIdentity::of(&file)?;
        Ok(file)
    }
}

impl SourceRoot {
    /// Creates `leaf` in the root directory, failing if anything already has that name
    /// (`O_CREAT | O_EXCL` through the directory handle, never following a link).
    pub fn create_leaf_exclusive(&self, leaf: &std::ffi::OsStr) -> io::Result<File> {
        let leaf = checked_leaf(leaf)?;
        self.is_current()?;
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                leaf.as_ptr(),
                libc::O_CLOEXEC
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_EXCL,
                0o666 as libc::c_uint,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        FileIdentity::of(&file)?;
        Ok(file)
    }

    /// Removes `leaf` from the root directory only while that name still is `file` (same
    /// device and inode, by `fstatat` without following links), through the directory
    /// handle (`unlinkat`). Anything else under the name is left alone. True when removed.
    pub fn unlink_leaf_if_same(&self, leaf: &std::ffi::OsStr, file: &File) -> io::Result<bool> {
        use std::os::unix::fs::MetadataExt;
        let leaf = checked_leaf(leaf)?;
        let ours = file.metadata()?;
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        let rc = unsafe {
            libc::fstatat(
                self.directory.as_raw_fd(),
                leaf.as_ptr(),
                &mut stat,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        if stat.st_dev as u64 != ours.dev() || stat.st_ino as u64 != ours.ino() {
            return Ok(false);
        }
        let rc = unsafe { libc::unlinkat(self.directory.as_raw_fd(), leaf.as_ptr(), 0) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(true)
    }
}

/// A plain file name, as `open_leaf` accepts one.
fn checked_leaf(leaf: &std::ffi::OsStr) -> io::Result<CString> {
    let bytes = leaf.as_encoded_bytes();
    if bytes.is_empty()
        || bytes.len() > 255
        || bytes == b"."
        || bytes == b".."
        || bytes.iter().any(|&b| matches!(b, 0 | b'/' | b'\\'))
    {
        return Err(refused());
    }
    CString::new(bytes).map_err(|_| refused())
}

pub(crate) fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "source physical ownership unavailable",
    )
}
pub(crate) fn blocked(_: WriteBlocked) -> io::Error {
    refused()
}
