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
    /// Creates a new empty file under a unique temporary name in the root directory
    /// (`O_CREAT | O_EXCL | O_NOFOLLOW` through the directory handle) and returns it with
    /// that name. A file that turns out not to be a plain single-link file is unlinked by
    /// its temporary name again.
    pub fn create_temp_exclusive(&self) -> io::Result<(File, CString)> {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        self.is_current()?;
        for _ in 0..16 {
            let n = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let temp = CString::new(format!(".rustyget-repair-{}-{n}.tmp", std::process::id()))
                .map_err(|_| refused())?;
            let fd = unsafe {
                libc::openat(
                    self.directory.as_raw_fd(),
                    temp.as_ptr(),
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
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(error);
            }
            let file = unsafe { File::from_raw_fd(fd) };
            if let Err(error) = FileIdentity::of(&file) {
                drop(file);
                let _ = self.unlink_temp(&temp);
                return Err(error);
            }
            return Ok((file, temp));
        }
        Err(io::Error::from(io::ErrorKind::AlreadyExists))
    }

    /// Gives the file at temporary name `temp` the name `leaf` too (`linkat`), failing when
    /// anything already has that name: nothing is ever replaced.
    pub fn link_temp(&self, temp: &std::ffi::CStr, leaf: &std::ffi::OsStr) -> io::Result<()> {
        let leaf = checked_leaf(leaf)?;
        let rc = unsafe {
            libc::linkat(
                self.directory.as_raw_fd(),
                temp.as_ptr(),
                self.directory.as_raw_fd(),
                leaf.as_ptr(),
                0,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Removes the temporary name `temp` (`unlinkat`); only names `create_temp_exclusive`
    /// made are ever passed here.
    pub fn unlink_temp(&self, temp: &std::ffi::CStr) -> io::Result<()> {
        if !temp.to_bytes().starts_with(b".rustyget-repair-") {
            return Err(refused());
        }
        let rc = unsafe { libc::unlinkat(self.directory.as_raw_fd(), temp.as_ptr(), 0) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
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
