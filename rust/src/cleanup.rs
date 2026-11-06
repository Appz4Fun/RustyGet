//! End-to-end Rust implementations of:
//! - `MoveController` (`daemon/postprocess/Cleanup.cpp`): recursive post-download
//!   directory move with collision renaming, cross-device fallback, and stale
//!   hardlink cleanup.
//! - `CleanupController` (`daemon/postprocess/Cleanup.cpp`): recursive post-process
//!   extension-based disk cleanup across destination and final directories.
//! - `RenameController` (`daemon/postprocess/Rename.cpp`): PAR/RAR rename stage
//!   configuration and completion state machine.

use std::ffi::{CStr, OsStr};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum CleanupLogKind {
    Info = 0,
    Warning = 1,
    Error = 2,
    Detail = 3,
}

#[cfg(unix)]
pub fn os_str_to_bytes(s: &OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes()
}

#[cfg(not(unix))]
pub fn os_str_to_bytes(s: &OsStr) -> &[u8] {
    s.to_str().map(str::as_bytes).unwrap_or(b"")
}

#[cfg(unix)]
pub fn bytes_to_os_str(b: &[u8]) -> &OsStr {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(b)
}

#[cfg(unix)]
pub fn bytes_to_path(b: &[u8]) -> &Path {
    Path::new(bytes_to_os_str(b))
}

#[cfg(unix)]
pub fn bytes_to_path_buf(b: &[u8]) -> PathBuf {
    PathBuf::from(bytes_to_os_str(b))
}

#[cfg(not(unix))]
pub fn bytes_to_path_buf(b: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(b).into_owned())
}

/// Checks whether two existing paths refer to the same underlying file or directory inode.
pub fn paths_equivalent(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ma.dev() == mb.dev() && ma.ino() == mb.ino()
    }
    #[cfg(not(unix))]
    {
        a == b && ma.len() == mb.len()
    }
}

/// Splits a filename into `(stem, extension_with_dot)` matching `std::filesystem::path::stem()`
/// and `std::filesystem::path::extension()`.
fn split_stem_and_ext(filename: &[u8]) -> (&[u8], &[u8]) {
    if filename == b"." || filename == b".." {
        return (filename, b"");
    }
    if let Some(dot_pos) = filename.iter().rposition(|&b| b == b'.') {
        if dot_pos > 0 {
            return (&filename[..dot_pos], &filename[dot_pos..]);
        }
    }
    (filename, b"")
}

/// Computes a non-colliding path in `target_path.parent()` using `"stem (N).ext"`,
/// matching `fs::make_unique_filename(const fs::path&)` in `daemon/util/FileSystem.h`.
pub fn make_unique_path(target_path: &Path) -> PathBuf {
    if std::fs::metadata(target_path).is_err() {
        return target_path.to_path_buf();
    }

    let base_dir = target_path.parent().unwrap_or_else(|| Path::new(""));
    let file_name = target_path
        .file_name()
        .map(os_str_to_bytes)
        .unwrap_or(b"");
    let (stem, ext) = split_stem_and_ext(file_name);

    let mut counter: i32 = 1;
    loop {
        let mut candidate_name = Vec::with_capacity(file_name.len() + 16);
        candidate_name.extend_from_slice(stem);
        candidate_name.extend_from_slice(b" (");
        candidate_name.extend_from_slice(counter.to_string().as_bytes());
        candidate_name.push(b')');
        candidate_name.extend_from_slice(ext);
        counter = counter.wrapping_add(1);

        #[cfg(unix)]
        let unique_path = base_dir.join(bytes_to_os_str(&candidate_name));
        #[cfg(not(unix))]
        let unique_path = base_dir.join(bytes_to_path_buf(&candidate_name));

        if std::fs::metadata(&unique_path).is_err() {
            return unique_path;
        }
    }
}

#[derive(Debug)]
pub enum MoveFileError {
    FileExists,
    Io(io::Error),
}

fn is_cross_device_error(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::CrossesDevices
}

/// Moves a single file from `src` to `dest`, failing with `MoveFileError::FileExists`
/// if `dest` already exists, and falling back to copy + remove on `EXDEV` (cleaning
/// up any partial destination file on failure), matching `fs::move_file` in `FileSystem.h`.
pub fn move_single_file(src: &Path, dest: &Path) -> Result<(), MoveFileError> {
    match std::fs::metadata(dest) {
        Ok(_) => return Err(MoveFileError::FileExists),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        // A broken symlink at `dest`: `fs::exists` is false without error
        Err(_) if std::fs::symlink_metadata(dest).is_ok() => {}
        Err(e) => return Err(MoveFileError::Io(e)),
    }

    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device_error(&e) => {
            if let Err(copy_err) = std::fs::copy(src, dest) {
                let _ = std::fs::remove_file(dest);
                return Err(MoveFileError::Io(copy_err));
            }
            if let Err(rm_err) = std::fs::remove_file(src) {
                let _ = std::fs::remove_file(dest);
                return Err(MoveFileError::Io(rm_err));
            }
            Ok(())
        }
        Err(e) => Err(MoveFileError::Io(e)),
    }
}

fn format_io_error(err: &io::Error) -> Vec<u8> {
    #[cfg(unix)]
    if let Some(code) = err.raw_os_error() {
        unsafe extern "C" {
            fn strerror(errnum: std::ffi::c_int) -> *const std::ffi::c_char;
        }
        unsafe {
            let ptr = strerror(code);
            if !ptr.is_null() {
                return CStr::from_ptr(ptr).to_bytes().to_vec();
            }
        }
    }
    err.to_string().into_bytes()
}

fn move_files_tree<S, L, R>(
    src: &Path,
    dest: &Path,
    is_stopped: &mut S,
    on_log: &mut L,
    on_rename: &mut R,
) -> bool
where
    S: FnMut() -> bool,
    L: FnMut(CleanupLogKind, &[u8]),
    R: FnMut(&[u8], &[u8]),
{
    let root_iter = match std::fs::read_dir(src) {
        Ok(it) => it,
        Err(e) => {
            let mut msg = b"Could not open directory ".to_vec();
            msg.extend_from_slice(os_str_to_bytes(src.as_os_str()));
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(&format_io_error(&e));
            on_log(CleanupLogKind::Error, &msg);
            return false;
        }
    };

    let mut stack = vec![root_iter];
    while let Some(top) = stack.last_mut() {
        let Some(next_res) = top.next() else {
            stack.pop();
            continue;
        };

        if is_stopped() {
            return false;
        }

        let entry = match next_res {
            Ok(ent) => ent,
            Err(e) => {
                let mut msg = b"Could not read directory ".to_vec();
                msg.extend_from_slice(os_str_to_bytes(src.as_os_str()));
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&format_io_error(&e));
                on_log(CleanupLogKind::Error, &msg);
                return false;
            }
        };

        let entry_path = entry.path();
        let Ok(rel_path) = entry_path.strip_prefix(src) else {
            return false;
        };
        let mut dst_path = dest.join(rel_path);

        let is_dir = std::fs::metadata(&entry_path).is_ok_and(|m| m.is_dir());

        if is_dir {
            if std::fs::create_dir_all(&dst_path).is_err() {
                return false;
            }
            // Match `fs::recursive_directory_iterator` default (do not follow directory symlinks)
            let is_real_dir = entry.file_type().is_ok_and(|ft| ft.is_dir());
            if is_real_dir {
                match std::fs::read_dir(&entry_path) {
                    Ok(sub_iter) => stack.push(sub_iter),
                    Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {}
                    Err(e) => {
                        let mut msg = b"Could not read directory ".to_vec();
                        msg.extend_from_slice(os_str_to_bytes(src.as_os_str()));
                        msg.extend_from_slice(b": ");
                        msg.extend_from_slice(&format_io_error(&e));
                        on_log(CleanupLogKind::Error, &msg);
                        return false;
                    }
                }
            }
        } else {
            let filename_bytes = entry_path
                .file_name()
                .map(os_str_to_bytes)
                .unwrap_or(b"");
            let is_dot_file = filename_bytes.starts_with(b".");

            if !is_dot_file {
                let dst_fname = dst_path.file_name().map(os_str_to_bytes).unwrap_or(b"");
                let mut msg = b"Moving file ".to_vec();
                msg.extend_from_slice(filename_bytes);
                msg.extend_from_slice(b" to ");
                msg.extend_from_slice(dst_fname);
                on_log(CleanupLogKind::Info, &msg);
            }

            match move_single_file(&entry_path, &dst_path) {
                Ok(()) => {}
                Err(MoveFileError::FileExists) => {
                    if entry_path == dst_path {
                        continue;
                    }
                    if paths_equivalent(&entry_path, &dst_path) {
                        if std::fs::remove_file(&entry_path).is_err() {
                            return false;
                        }
                        continue;
                    }

                    dst_path = make_unique_path(&dst_path);
                    if !is_dot_file {
                        let new_fname = dst_path.file_name().map(os_str_to_bytes).unwrap_or(b"");
                        let mut msg = b"File ".to_vec();
                        msg.extend_from_slice(filename_bytes);
                        msg.extend_from_slice(b" already exists, renaming to ");
                        msg.extend_from_slice(new_fname);
                        on_log(CleanupLogKind::Warning, &msg);
                    }

                    if move_single_file(&entry_path, &dst_path).is_err() {
                        return false;
                    }
                }
                Err(MoveFileError::Io(e)) => {
                    let mut msg = b"Could not move file ".to_vec();
                    msg.extend_from_slice(filename_bytes);
                    msg.extend_from_slice(b": ");
                    msg.extend_from_slice(&format_io_error(&e));
                    on_log(CleanupLogKind::Error, &msg);
                    return false;
                }
            }

            if dst_path.file_name() != entry_path.file_name() {
                if let Ok(new_rel) = dst_path.strip_prefix(dest) {
                    on_rename(
                        os_str_to_bytes(rel_path.as_os_str()),
                        os_str_to_bytes(new_rel.as_os_str()),
                    );
                }
            }
        }
    }

    !is_stopped()
}

/// Executes `MoveController::MoveFiles()` end-to-end.
pub fn move_files<S, L, R>(
    inter_dir: &Path,
    dest_dir: &Path,
    mut is_stopped: S,
    mut on_log: L,
    mut on_rename: R,
) -> bool
where
    S: FnMut() -> bool,
    L: FnMut(CleanupLogKind, &[u8]),
    R: FnMut(&[u8], &[u8]),
{
    if inter_dir == dest_dir {
        return true;
    }

    if paths_equivalent(inter_dir, dest_dir) {
        return true;
    }

    if let Err(e) = std::fs::create_dir_all(dest_dir) {
        let mut msg = b"Could not create directory ".to_vec();
        msg.extend_from_slice(os_str_to_bytes(dest_dir.as_os_str()));
        msg.extend_from_slice(b": ");
        msg.extend_from_slice(&format_io_error(&e));
        on_log(CleanupLogKind::Error, &msg);
        return false;
    }

    if !move_files_tree(
        inter_dir,
        dest_dir,
        &mut is_stopped,
        &mut on_log,
        &mut on_rename,
    ) {
        return false;
    }

    if std::fs::metadata(inter_dir).is_ok() {
        if let Err(e) = std::fs::remove_dir_all(inter_dir) {
            let mut msg = b"Could not delete intermediate directory ".to_vec();
            msg.extend_from_slice(os_str_to_bytes(inter_dir.as_os_str()));
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(&format_io_error(&e));
            on_log(CleanupLogKind::Warning, &msg);
        }
    }

    true
}

/// Executes `MoveController::RemoveStaleHardlinks` end-to-end.
pub fn remove_stale_hardlinks<L>(hard_link_path: &Path, dest_dir: &Path, mut on_log: L)
where
    L: FnMut(CleanupLogKind, &[u8]),
{
    if hard_link_path.as_os_str().is_empty() {
        return;
    }
    if hard_link_path == dest_dir {
        return;
    }
    if paths_equivalent(hard_link_path, dest_dir) {
        return;
    }
    if let Err(e) = std::fs::remove_dir_all(hard_link_path) {
        let mut msg = b"Could not remove old hardlink directory: ".to_vec();
        msg.extend_from_slice(&format_io_error(&e));
        on_log(CleanupLogKind::Error, &msg);
    }
}

/// Determines the outcome and `NzbInfo::EMoveStatus` in `MoveController::Run`:
/// - `(1, 2)`: move succeeded -> `msSuccess` (`2`)
/// - `(2, 0)`: interrupted by stop -> keep `msNone` (`0`)
/// - `(3, 1)`: move failed -> `msFailure` (`1`)
#[inline]
pub fn decide_move_outcome(ok: bool, is_stopped: bool) -> (i32, i32) {
    if ok {
        (1, 2)
    } else if is_stopped {
        (2, 0)
    } else {
        (3, 1)
    }
}

// ---- CleanupController (`daemon/postprocess/Cleanup.cpp`) ----

fn cleanup_dir_inner<L>(
    dest_dir: &Path,
    ext_cleanup_disk: &[u8],
    deleted: &mut bool,
    on_log: &mut L,
) -> bool
where
    L: FnMut(CleanupLogKind, &[u8]),
{
    let mut ok = true;

    let Ok(entries) = std::fs::read_dir(dest_dir) else {
        return true;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let filename_bytes = os_str_to_bytes(&file_name);
        if filename_bytes == b"." || filename_bytes == b".." {
            continue;
        }

        let full_path = entry.path();
        // A symlink to a directory is not traversed (`DirectoryExists && !IsLink`).
        let is_link = std::fs::symlink_metadata(&full_path)
            .is_ok_and(|m| m.file_type().is_symlink());
        let dir_exists = std::fs::metadata(&full_path).is_ok_and(|m| m.is_dir());
        let is_dir = dir_exists && !is_link;

        if is_dir {
            ok &= cleanup_dir_inner(&full_path, ext_cleanup_disk, deleted, on_log);
        }

        unsafe extern "C" {
            fn tolower(c: std::ffi::c_int) -> std::ffi::c_int;
        }
        let case_fold = |b: u8| unsafe { tolower(std::ffi::c_int::from(b)) };
        let mask_fold = |b: u8| {
            let ch = b as std::ffi::c_char as std::ffi::c_int;
            if ch < 0 {
                ch
            } else {
                unsafe { tolower(ch) }
            }
        };
        let case_lower = crate::wildmask::Lower::Fold(&case_fold);
        let mask_lower = crate::wildmask::Lower::Fold(&mask_fold);
        let delete_it = !is_dir
            && crate::util::match_file_ext(
                filename_bytes,
                ext_cleanup_disk,
                b",;",
                &case_lower,
                &mask_lower,
            );
        if delete_it {
            let mut info_msg = b"Deleting file ".to_vec();
            info_msg.extend_from_slice(filename_bytes);
            on_log(CleanupLogKind::Info, &info_msg);

            if let Err(e) = std::fs::remove_file(&full_path) {
                let mut err_msg = b"Could not delete file ".to_vec();
                err_msg.extend_from_slice(os_str_to_bytes(full_path.as_os_str()));
                err_msg.extend_from_slice(b": ");
                err_msg.extend_from_slice(&format_io_error(&e));
                on_log(CleanupLogKind::Error, &err_msg);
                ok = false;
            }

            *deleted = true;
        }
    }

    ok
}

/// Determines the outcome and `NzbInfo::ECleanupStatus` in `CleanupController::Run`:
/// - `(1, 2)`: cleanup succeeded and deleted files -> `csSuccess` (`2`)
/// - `(2, 2)`: cleanup succeeded with nothing to delete -> `csSuccess` (`2`)
/// - `(3, 1)`: cleanup failed -> `csFailure` (`1`)
#[inline]
pub fn decide_cleanup_outcome(ok: bool, deleted: bool) -> (i32, i32) {
    if ok && deleted {
        (1, 2)
    } else if ok {
        (2, 2)
    } else {
        (3, 1)
    }
}

/// Executes `CleanupController::Cleanup` across `dest_dir` and (when non-empty) `final_dir`,
/// returning `(outcome, cleanup_status)`.
pub fn run_cleanup<L>(
    dest_dir: &Path,
    final_dir: &Path,
    ext_cleanup_disk: &[u8],
    mut on_log: L,
) -> (i32, i32)
where
    L: FnMut(CleanupLogKind, &[u8]),
{
    let mut deleted = false;
    let mut ok = cleanup_dir_inner(dest_dir, ext_cleanup_disk, &mut deleted, &mut on_log);

    if ok && !final_dir.as_os_str().is_empty() {
        let mut deleted2 = false;
        ok = cleanup_dir_inner(final_dir, ext_cleanup_disk, &mut deleted2, &mut on_log);
        deleted = deleted || deleted2;
    }

    decide_cleanup_outcome(ok, deleted)
}

// ---- RenameController (`daemon/postprocess/Rename.cpp`) ----

/// Returns `"archive "` when `kind == jk_rar`, or `""` otherwise.
#[inline]
pub fn rename_archive_kind_prefix(kind: i32, jk_rar: i32) -> &'static CStr {
    if kind == jk_rar {
        c"archive "
    } else {
        c""
    }
}

/// Classifies the final log outcome of `RenameController::Run`:
/// `1` = stopped (cancelled), `2` = renamed files (`renamed_count > 0`), `3` = no renamed files.
#[inline]
pub fn rename_classify_run_outcome(is_stopped: bool, renamed_count: i32) -> i32 {
    if is_stopped {
        1
    } else if renamed_count > 0 {
        2
    } else {
        3
    }
}

/// Evaluates whether `m_parRenamer` should use `finalDir` instead of `destDir`
/// and whether `DetectMissing` should be enabled in `RenameController::ExecRename`.
#[inline]
pub fn rename_eval_par_config(
    unpack_status: i32,
    us_success: i32,
    us_none: i32,
    final_dir_empty: bool,
) -> (bool, bool) {
    let use_final_dir = unpack_status == us_success && !final_dir_empty;
    let detect_missing = unpack_status == us_none;
    (use_final_dir, detect_missing)
}

/// Computes `(initial_rename_status, request_extra_pars, request_par_check)` for
/// `RenameController::RenameCompleted`.
#[inline]
pub fn rename_complete_action(
    kind: i32,
    renamed_count: i32,
    has_damaged_pars: bool,
    remaining_par_count: i32,
    has_missed_files: bool,
    par_status: i32,
) -> (i32, bool, bool) {
    // NzbInfo::rsNothing = 2, NzbInfo::rsSuccess = 3, NzbInfo::psSkipped = 1
    let initial_status = if renamed_count > 0 { 3 } else { 2 };
    let request_extra_pars =
        kind == 0 && renamed_count == 0 && has_damaged_pars && remaining_par_count > 0;
    let request_par_check = has_missed_files && par_status <= 1;
    (initial_status, request_extra_pars, request_par_check)
}

/// Returns true when `RenameController::RenameCompleted` should reset `ParRenameStatus`
/// to `rsNone` after unpausing extra PAR2 files (`remaining_size > 0`).
#[inline]
pub fn rename_should_reset_par_status(remaining_size: i64) -> bool {
    remaining_size > 0
}

// ---- PostUnpackRenamer & PostDownloadRenamer (`daemon/postprocess/Post*Renamer.cpp`) ----

fn format_move_file_error(err: &MoveFileError) -> Vec<u8> {
    match err {
        MoveFileError::FileExists => format_io_error(&io::Error::from_raw_os_error(17)),
        MoveFileError::Io(e) => format_io_error(e),
    }
}

fn move_error_msg_and_code(err: &MoveFileError) -> (Vec<u8>, i32) {
    match err {
        MoveFileError::FileExists => (format_io_error(&io::Error::from_raw_os_error(17)), 17),
        MoveFileError::Io(e) => (format_io_error(e), e.raw_os_error().unwrap_or(1)),
    }
}

/// Walks `dir` recursively to collect `crate::collection::Entry` items and whether a disc
/// structure directory was encountered, matching `CollectionAnalyzer::Walk`.
pub fn walk_collection_dir(dir: &Path) -> (Vec<crate::collection::Entry>, bool) {
    let mut files = Vec::new();
    let mut disc_found = false;

    if !std::fs::metadata(dir).is_ok_and(|m| m.is_dir()) {
        return (files, disc_found);
    }

    let Ok(root_iter) = std::fs::read_dir(dir) else {
        return (files, disc_found);
    };

    let mut stack = vec![root_iter];
    while let Some(top) = stack.last_mut() {
        let Some(next_res) = top.next() else {
            stack.pop();
            continue;
        };
        let Ok(entry) = next_res else {
            break;
        };

        let entry_path = entry.path();
        let Ok(meta) = std::fs::metadata(&entry_path) else {
            continue;
        };

        if meta.is_dir() {
            let dirname = entry_path.file_name().map(os_str_to_bytes).unwrap_or(b"");
            if crate::filetypes::is_clutter_dir(dirname) {
                continue;
            }
            if crate::filetypes::is_disc_structure_dir(dirname) {
                disc_found = true;
                continue;
            }
            if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                if let Ok(sub_iter) = std::fs::read_dir(&entry_path) {
                    stack.push(sub_iter);
                }
            }
            continue;
        }

        if meta.is_file() {
            let filename = entry_path
                .file_name()
                .map(os_str_to_bytes)
                .unwrap_or(b"")
                .to_vec();
            if crate::filetypes::is_clutter_file(&filename) {
                continue;
            }
            let size = meta.len();
            let (stem_slice, ext_slice) = crate::collection::stem_ext(&filename);
            let stem = stem_slice.to_vec();
            let mut ext = ext_slice.to_vec();

            if !crate::filetypes::is_known_collection_ext(&ext) {
                let sniffed = crate::filetypes::sniff_extension_file(&entry_path).to_bytes();
                if !sniffed.is_empty() {
                    ext = sniffed.to_vec();
                }
            }

            let path_bytes = os_str_to_bytes(entry_path.as_os_str()).to_vec();
            let mut rename_prefix = os_str_to_bytes(
                entry_path
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .join("x")
                    .as_os_str(),
            )
            .to_vec();
            if rename_prefix.ends_with(b"x") {
                rename_prefix.pop();
            }

            files.push(crate::collection::Entry {
                path: path_bytes,
                rename_prefix,
                filename,
                stem,
                ext,
                size,
            });
        }
    }

    (files, disc_found)
}

struct FsCollectionDisk<'a> {
    ignore_ext: &'a [u8],
}

impl crate::collection::Disk for FsCollectionDisk<'_> {
    fn exists(&mut self, path: &[u8]) -> bool {
        std::fs::metadata(bytes_to_path_buf(path)).is_ok()
    }

    fn ignored(&mut self, path: &[u8]) -> bool {
        if self.ignore_ext.is_empty() {
            return false;
        }
        unsafe extern "C" {
            fn tolower(c: std::ffi::c_int) -> std::ffi::c_int;
        }
        let case_fold = |b: u8| unsafe { tolower(std::ffi::c_int::from(b)) };
        let mask_fold = |b: u8| {
            let ch = b as std::ffi::c_char as std::ffi::c_int;
            if ch < 0 {
                ch
            } else {
                unsafe { tolower(ch) }
            }
        };
        let case_lower = crate::wildmask::Lower::Fold(&case_fold);
        let mask_lower = crate::wildmask::Lower::Fold(&mask_fold);
        crate::util::match_file_ext(path, self.ignore_ext, b",", &case_lower, &mask_lower)
    }
}

/// Analyzes a directory recursively (`CollectionAnalyzer::AnalyzeDirectory`), returning the
/// discovered `crate::collection::Entry` list and `crate::collection::Analysis` (with
/// `disc_structure` set if a disc structure subdirectory was encountered).
pub fn analyze_directory(dir: &Path) -> (Vec<crate::collection::Entry>, crate::collection::Analysis) {
    let (entries, disc_found) = walk_collection_dir(dir);
    let mut analysis = crate::collection::analyze(&entries);
    if disc_found {
        analysis.disc_structure = true;
    }
    (entries, analysis)
}

/// Builds a `CollectionAnalyzer::RenamePlan` for `dir` (`CollectionAnalyzer::BuildPlan`).
pub fn build_plan_directory(
    dir: &Path,
    target_name: &[u8],
    ignore_ext: &[u8],
) -> (Vec<crate::collection::Entry>, crate::collection::Plan) {
    let (entries, disc_found) = walk_collection_dir(dir);
    let mut disk = FsCollectionDisk { ignore_ext };
    let plan = crate::collection::build_plan(&entries, disc_found, target_name, &mut disk);
    (entries, plan)
}

/// Classifies a `CollectionAnalyzer::RenamePlan` for `PostUnpackRenamer` and `PostDownloadRenamer`:
/// - `0`: proceed with rename actions
/// - `1`: disc structure detected
/// - `2`: ambiguous multi-file collection detected
/// - `3`: no qualifying media file (`!can_rename`)
/// - `4`: actions empty and target name is obfuscated
/// - `5`: actions empty and target name is clean
#[inline]
pub fn classify_post_rename_plan(
    is_disc: bool,
    is_ambiguous: bool,
    can_rename: bool,
    actions_empty: bool,
    target_obfuscated: bool,
) -> i32 {
    if is_disc {
        1
    } else if is_ambiguous {
        2
    } else if !can_rename {
        3
    } else if actions_empty {
        if target_obfuscated {
            4
        } else {
            5
        }
    } else {
        0
    }
}

/// Executes `PostUnpackRenamer::Controller::Run` (`is_post_unpack = true`) or
/// `PostDownloadRenamer::Controller::Run` (`is_post_unpack = false`) end-to-end.
/// Returns `NzbInfo::PostUnpackRenamingStatus` / `PostDownloadRenamingStatus` discriminant:
/// `1` = `Failure`, `2` = `Success`, `3` = `Skipped`.
pub fn run_post_renamer<I, S, L, R>(
    is_post_unpack: bool,
    meta_name: &[u8],
    nzb_name: &[u8],
    dst_dir: &Path,
    ignore_ext: &[u8],
    mut on_info_name: I,
    mut is_stopped: S,
    mut on_log: L,
    mut on_rename: R,
) -> i32
where
    I: FnMut(&[u8]),
    S: FnMut() -> bool,
    L: FnMut(CleanupLogKind, &[u8]),
    R: FnMut(&[u8], &[u8]),
{
    // Skipped = 3, Failure = 1, Success = 2
    if dst_dir.as_os_str().is_empty() {
        return 3;
    }

    let target_name = crate::collection::resolve_target_name(meta_name, nzb_name);
    let stage_label: &[u8] = if is_post_unpack {
        b"Post-unpack renaming"
    } else {
        b"Post-download renaming"
    };
    let mut info_name = Vec::with_capacity(stage_label.len() + 5 + target_name.len());
    info_name.extend_from_slice(stage_label);
    info_name.extend_from_slice(b" for ");
    info_name.extend_from_slice(&target_name);
    on_info_name(&info_name);

    let (entries, disc_found) = walk_collection_dir(dst_dir);
    let mut disk = FsCollectionDisk { ignore_ext };
    let plan = crate::collection::build_plan(&entries, disc_found, &target_name, &mut disk);

    let plan_class = classify_post_rename_plan(
        plan.disc_structure,
        plan.ambiguous,
        plan.can_rename,
        plan.actions.is_empty(),
        plan.target_name_obfuscated,
    );
    if plan_class != 0 {
        match plan_class {
            1 => {
                let msg: &[u8] = if is_post_unpack {
                    b"Skipping Post-unpack renaming: disc structure detected"
                } else {
                    b"Skipping Post-download renaming: disc structure detected"
                };
                on_log(CleanupLogKind::Info, msg);
            }
            2 => {
                let msg: &[u8] = if is_post_unpack {
                    b"Skipping Post-unpack renaming: ambiguous multi-file collection detected"
                } else {
                    b"Skipping Post-download renaming: ambiguous multi-file collection detected"
                };
                on_log(CleanupLogKind::Info, msg);
            }
            3 => {
                let msg: &[u8] = if is_post_unpack {
                    b"No qualifying media file found for Post-unpack renaming"
                } else {
                    b"No qualifying media file found for Post-download renaming"
                };
                on_log(CleanupLogKind::Info, msg);
            }
            4 => {
                let mut msg = Vec::with_capacity(140 + target_name.len());
                msg.extend_from_slice(b"Skipping ");
                msg.extend_from_slice(stage_label);
                msg.extend_from_slice(b". Obfuscated files found, but NZB filename ");
                msg.extend_from_slice(&target_name);
                msg.extend_from_slice(b" is also obfuscated and no clean metadata was provided.");
                on_log(CleanupLogKind::Warning, &msg);
            }
            _ => {
                let mut msg = Vec::with_capacity(32 + target_name.len());
                msg.extend_from_slice(b"No files needed renaming for ");
                msg.extend_from_slice(&target_name);
                on_log(CleanupLogKind::Info, &msg);
            }
        }
        return 3;
    }

    let mut any_renamed = false;
    let mut any_failed = false;

    for action in &plan.actions {
        if is_stopped() {
            break;
        }
        let entry = &entries[action.src];
        let src_path = bytes_to_path_buf(&entry.path);
        let dst_path = bytes_to_path_buf(&action.dst_path);

        match move_single_file(&src_path, &dst_path) {
            Ok(()) => {
                let mut msg =
                    Vec::with_capacity(12 + entry.filename.len() + action.new_filename.len());
                msg.extend_from_slice(b"Renamed ");
                msg.extend_from_slice(&entry.filename);
                msg.extend_from_slice(b" to ");
                msg.extend_from_slice(&action.new_filename);
                on_log(CleanupLogKind::Info, &msg);
                on_rename(&entry.filename, &action.new_filename);
                any_renamed = true;
            }
            Err(err) => {
                let err_msg = format_move_file_error(&err);
                let mut msg = Vec::with_capacity(
                    30 + entry.filename.len() + action.new_filename.len() + err_msg.len(),
                );
                msg.extend_from_slice(b"Could not rename file ");
                msg.extend_from_slice(&entry.filename);
                msg.extend_from_slice(b" to ");
                msg.extend_from_slice(&action.new_filename);
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&err_msg);
                on_log(CleanupLogKind::Error, &msg);
                any_failed = true;
            }
        }
    }

    if is_stopped() {
        let mut msg = Vec::with_capacity(info_name.len() + 10);
        msg.extend_from_slice(&info_name);
        msg.extend_from_slice(b" cancelled");
        on_log(CleanupLogKind::Warning, &msg);
        3
    } else if any_failed {
        let mut msg = Vec::with_capacity(info_name.len() + 21);
        msg.extend_from_slice(&info_name);
        msg.extend_from_slice(b" finished with errors");
        on_log(CleanupLogKind::Error, &msg);
        1
    } else if any_renamed {
        let mut msg = Vec::with_capacity(info_name.len() + 11);
        msg.extend_from_slice(&info_name);
        msg.extend_from_slice(b" successful");
        on_log(CleanupLogKind::Info, &msg);
        2
    } else {
        let mut msg = Vec::with_capacity(info_name.len() + 29);
        msg.extend_from_slice(b"No files needed renaming for ");
        msg.extend_from_slice(&info_name);
        on_log(CleanupLogKind::Info, &msg);
        3
    }
}

// ---- ArchiveProcessor (`daemon/queue/ArchiveProcessor.cpp`) ----

/// Checks if `ext` is `".nzb"` (case-insensitive) for `ArchiveProcessor::IsNzbFile`.
#[inline]
pub fn archive_is_nzb_extension(ext: &[u8]) -> bool {
    ext.eq_ignore_ascii_case(b".nzb")
}

/// Determines the disposal action for `ArchiveProcessor::DisposeArchive`:
/// - `1`: `Delete` (`0`) configured, but `non_nzb_file_count > 0` -> move to processed dir
/// - `2`: `Delete` (`0`) configured and `non_nzb_file_count == 0` -> delete archive
/// - `3`: `Move` configured -> move archive to processed dir
#[inline]
pub fn archive_decide_dispose_action(config_action: i32, non_nzb_file_count: i32) -> i32 {
    if config_action == 0 {
        if non_nzb_file_count > 0 {
            1
        } else {
            2
        }
    } else {
        3
    }
}

/// Handles extraction failure in `ArchiveProcessor::Extract`: logs the failure, removes
/// `unpack_dir`, creates `broken_dir`, and moves `archive_file` to a unique name in `broken_dir`.
pub fn archive_handle_extract_failure<L>(
    archive_file: &Path,
    unpack_dir: &Path,
    broken_dir: &Path,
    mut on_log: L,
) where
    L: FnMut(CleanupLogKind, &[u8]),
{
    let archive_name = archive_file.file_name().map(os_str_to_bytes).unwrap_or(b"");
    let broken_name = broken_dir.file_name().map(os_str_to_bytes).unwrap_or(b"");
    let mut msg = Vec::with_capacity(56 + archive_name.len() + broken_name.len());
    msg.extend_from_slice(b"ArchiveProcessor: Failed to extract '");
    msg.extend_from_slice(archive_name);
    msg.extend_from_slice(b"'. Moving to '");
    msg.extend_from_slice(broken_name);
    msg.push(b'\'');
    on_log(CleanupLogKind::Error, &msg);

    let _ = std::fs::remove_dir_all(unpack_dir);
    let _ = std::fs::create_dir_all(broken_dir);
    if let Some(fname) = archive_file.file_name() {
        let dst = make_unique_path(&broken_dir.join(fname));
        let _ = move_single_file(archive_file, &dst);
    }
}

fn scan_unpack_dir<L>(unpack_dir: &Path, on_log: &mut L) -> (Vec<PathBuf>, i32)
where
    L: FnMut(CleanupLogKind, &[u8]),
{
    let mut nzb_files = Vec::new();
    let mut non_nzb_files = 0i32;

    let Ok(root_iter) = std::fs::read_dir(unpack_dir) else {
        return (nzb_files, non_nzb_files);
    };

    let mut stack = vec![root_iter];
    while let Some(top) = stack.last_mut() {
        let Some(next_res) = top.next() else {
            stack.pop();
            continue;
        };
        let Ok(entry) = next_res else {
            break;
        };

        let file_path = entry.path();
        let Ok(sym_meta) = std::fs::symlink_metadata(&file_path) else {
            continue;
        };
        if sym_meta.file_type().is_dir() {
            if let Ok(sub_iter) = std::fs::read_dir(&file_path) {
                stack.push(sub_iter);
            }
            continue;
        }
        if !sym_meta.file_type().is_file() {
            continue;
        }

        let filename_bytes = file_path.file_name().map(os_str_to_bytes).unwrap_or(b"");
        if filename_bytes.is_empty() || filename_bytes.starts_with(b".") {
            continue;
        }

        let (_, ext_bytes) = crate::collection::stem_ext(filename_bytes);
        if !archive_is_nzb_extension(ext_bytes) {
            let mut msg = Vec::with_capacity(64 + filename_bytes.len());
            msg.extend_from_slice(b"ArchiveProcessor: Found non-NZB file '");
            msg.extend_from_slice(filename_bytes);
            msg.extend_from_slice(b"' in the archive");
            on_log(CleanupLogKind::Detail, &msg);
            non_nzb_files += 1;
            continue;
        }

        let Ok(rel_path) = file_path.strip_prefix(unpack_dir) else {
            continue;
        };
        let rel_bytes = os_str_to_bytes(rel_path.as_os_str());
        let mut msg = Vec::with_capacity(64 + rel_bytes.len());
        msg.extend_from_slice(b"ArchiveProcessor: Found NZB file '");
        msg.extend_from_slice(rel_bytes);
        msg.extend_from_slice(b"' in the archive");
        on_log(CleanupLogKind::Detail, &msg);

        nzb_files.push(rel_path.to_path_buf());
    }

    (nzb_files, non_nzb_files)
}

fn dispose_archive<L>(
    archive_file: &Path,
    processed_dir: &Path,
    config_action: i32,
    non_nzb_file_count: i32,
    on_log: &mut L,
) where
    L: FnMut(CleanupLogKind, &[u8]),
{
    let filename_bytes = archive_file.file_name().map(os_str_to_bytes).unwrap_or(b"");
    let processed_dir_name = processed_dir.file_name().map(os_str_to_bytes).unwrap_or(b"");
    let dispose_action = archive_decide_dispose_action(config_action, non_nzb_file_count);

    if dispose_action == 1 {
        let mut msg = Vec::with_capacity(140 + filename_bytes.len() + processed_dir_name.len());
        msg.extend_from_slice(b"ArchiveProcessor: Archive '");
        msg.extend_from_slice(filename_bytes);
        msg.extend_from_slice(b"' contains ");
        msg.extend_from_slice(non_nzb_file_count.to_string().as_bytes());
        msg.extend_from_slice(b" non-NZB file(s). Moving to '");
        msg.extend_from_slice(processed_dir_name);
        msg.extend_from_slice(b"' instead of deleting to prevent data loss");
        on_log(CleanupLogKind::Info, &msg);

        let _ = std::fs::create_dir_all(processed_dir);
        let dst = make_unique_path(&processed_dir.join(archive_file.file_name().unwrap_or_default()));
        if let Err(err) = move_single_file(archive_file, &dst) {
            let (err_msg, code) = move_error_msg_and_code(&err);
            let mut warn_msg = Vec::with_capacity(
                96 + filename_bytes.len() + processed_dir_name.len() + err_msg.len(),
            );
            warn_msg.extend_from_slice(b"ArchiveProcessor: Failed to move archive '");
            warn_msg.extend_from_slice(filename_bytes);
            warn_msg.extend_from_slice(b"' to '");
            warn_msg.extend_from_slice(processed_dir_name);
            warn_msg.extend_from_slice(b"': ");
            warn_msg.extend_from_slice(&err_msg);
            warn_msg.extend_from_slice(b" (code: ");
            warn_msg.extend_from_slice(code.to_string().as_bytes());
            warn_msg.push(b')');
            on_log(CleanupLogKind::Warning, &warn_msg);
        }
    } else if dispose_action == 2 {
        match std::fs::remove_file(archive_file) {
            Ok(()) => {
                let mut msg = Vec::with_capacity(48 + filename_bytes.len());
                msg.extend_from_slice(b"ArchiveProcessor: Deleted archive '");
                msg.extend_from_slice(filename_bytes);
                msg.push(b'\'');
                on_log(CleanupLogKind::Info, &msg);
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                let err_msg = format_io_error(&e);
                let code = e.raw_os_error().unwrap_or(1);
                let mut warn_msg =
                    Vec::with_capacity(80 + filename_bytes.len() + err_msg.len());
                warn_msg.extend_from_slice(b"ArchiveProcessor: Failed to delete archive '");
                warn_msg.extend_from_slice(filename_bytes);
                warn_msg.extend_from_slice(b"': ");
                warn_msg.extend_from_slice(&err_msg);
                warn_msg.extend_from_slice(b" (code: ");
                warn_msg.extend_from_slice(code.to_string().as_bytes());
                warn_msg.push(b')');
                on_log(CleanupLogKind::Warning, &warn_msg);
            }
            Err(_) => {}
        }
    } else {
        let mut msg = Vec::with_capacity(64 + filename_bytes.len() + processed_dir_name.len());
        msg.extend_from_slice(b"ArchiveProcessor: Moving archive '");
        msg.extend_from_slice(filename_bytes);
        msg.extend_from_slice(b"' to '");
        msg.extend_from_slice(processed_dir_name);
        msg.push(b'\'');
        on_log(CleanupLogKind::Info, &msg);

        let _ = std::fs::create_dir_all(processed_dir);
        let dst = make_unique_path(&processed_dir.join(archive_file.file_name().unwrap_or_default()));
        if let Err(err) = move_single_file(archive_file, &dst) {
            let (err_msg, code) = move_error_msg_and_code(&err);
            let mut warn_msg = Vec::with_capacity(
                96 + filename_bytes.len() + processed_dir_name.len() + err_msg.len(),
            );
            warn_msg.extend_from_slice(b"ArchiveProcessor: Failed to move archive '");
            warn_msg.extend_from_slice(filename_bytes);
            warn_msg.extend_from_slice(b"' to '");
            warn_msg.extend_from_slice(processed_dir_name);
            warn_msg.extend_from_slice(b"': ");
            warn_msg.extend_from_slice(&err_msg);
            warn_msg.extend_from_slice(b" (code: ");
            warn_msg.extend_from_slice(code.to_string().as_bytes());
            warn_msg.push(b')');
            on_log(CleanupLogKind::Warning, &warn_msg);
        }
    }
}

/// Completes `ArchiveProcessor::Process` after extraction into `unpack_dir` has succeeded:
/// scans `unpack_dir`, moves extracted `.nzb` files into `dest_dir`, cleans up `unpack_dir`,
/// and disposes `archive_file`.
pub fn archive_finish_extracted<L, M>(
    archive_file: &Path,
    unpack_dir: &Path,
    dest_dir: &Path,
    processed_dir: &Path,
    config_action: i32,
    mut on_log: L,
    mut on_moved_nzb: M,
) where
    L: FnMut(CleanupLogKind, &[u8]),
    M: FnMut(&[u8]),
{
    let filename_bytes = archive_file.file_name().map(os_str_to_bytes).unwrap_or(b"");

    {
        let mut msg = Vec::with_capacity(48 + filename_bytes.len());
        msg.extend_from_slice(b"ArchiveProcessor: Extraction of '");
        msg.extend_from_slice(filename_bytes);
        msg.extend_from_slice(b"' completed");
        on_log(CleanupLogKind::Detail, &msg);
    }

    let (nzb_files, non_nzb_files) = scan_unpack_dir(unpack_dir, &mut on_log);

    if nzb_files.is_empty() {
        let mut msg = Vec::with_capacity(72 + filename_bytes.len());
        msg.extend_from_slice(b"ArchiveProcessor: Skipped '");
        msg.extend_from_slice(filename_bytes);
        msg.extend_from_slice(b"': No NZB files were found inside");
        on_log(CleanupLogKind::Info, &msg);
    } else {
        let mut msg = Vec::with_capacity(96 + filename_bytes.len());
        msg.extend_from_slice(b"ArchiveProcessor: Successfully extracted ");
        msg.extend_from_slice(nzb_files.len().to_string().as_bytes());
        msg.extend_from_slice(b" NZB file(s) (and ");
        msg.extend_from_slice(non_nzb_files.to_string().as_bytes());
        msg.extend_from_slice(b" non-NZB file(s)) from '");
        msg.extend_from_slice(filename_bytes);
        msg.push(b'\'');
        on_log(CleanupLogKind::Info, &msg);
    }

    let mut to_paths = Vec::with_capacity(nzb_files.len());
    let mut moved = 0i32;

    for rel_path in &nzb_files {
        let from = unpack_dir.join(rel_path);
        let to = dest_dir.join(rel_path);
        let from_fname = from.file_name().map(os_str_to_bytes).unwrap_or(b"");
        let to_bytes = os_str_to_bytes(to.as_os_str());

        let mut detail_msg = Vec::with_capacity(40 + from_fname.len() + to_bytes.len());
        detail_msg.extend_from_slice(b"ArchiveProcessor: Moving '");
        detail_msg.extend_from_slice(from_fname);
        detail_msg.extend_from_slice(b"' to '");
        detail_msg.extend_from_slice(to_bytes);
        detail_msg.push(b'\'');
        on_log(CleanupLogKind::Detail, &detail_msg);

        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match move_single_file(&from, &to) {
            Ok(()) => {
                moved += 1;
            }
            Err(err) => {
                let err_msg = format_move_file_error(&err);
                let mut err_log =
                    Vec::with_capacity(48 + from_fname.len() + err_msg.len());
                err_log.extend_from_slice(b"ArchiveProcessor: Failed to move NZB '");
                err_log.extend_from_slice(from_fname);
                err_log.extend_from_slice(b"': ");
                err_log.extend_from_slice(&err_msg);
                on_log(CleanupLogKind::Error, &err_log);
            }
        }
        to_paths.push(to);
    }

    let unpack_bytes = os_str_to_bytes(unpack_dir.as_os_str());
    match std::fs::remove_dir_all(unpack_dir) {
        Ok(()) => {
            let mut msg = Vec::with_capacity(56 + unpack_bytes.len());
            msg.extend_from_slice(b"ArchiveProcessor: Cleaned up temporary directory '");
            msg.extend_from_slice(unpack_bytes);
            msg.push(b'\'');
            on_log(CleanupLogKind::Detail, &msg);
        }
        Err(e) => {
            let err_msg = format_io_error(&e);
            let code = e.raw_os_error().unwrap_or(1);
            let mut warn_msg = Vec::with_capacity(72 + unpack_bytes.len() + err_msg.len());
            warn_msg.extend_from_slice(b"ArchiveProcessor: Failed to clean up temp dir '");
            warn_msg.extend_from_slice(unpack_bytes);
            warn_msg.extend_from_slice(b"': ");
            warn_msg.extend_from_slice(&err_msg);
            warn_msg.extend_from_slice(b" (code: ");
            warn_msg.extend_from_slice(code.to_string().as_bytes());
            warn_msg.push(b')');
            on_log(CleanupLogKind::Warning, &warn_msg);
        }
    }

    if moved > 0 {
        let dest_bytes = os_str_to_bytes(dest_dir.as_os_str());
        let mut msg = Vec::with_capacity(56 + dest_bytes.len());
        msg.extend_from_slice(b"ArchiveProcessor: Moved ");
        msg.extend_from_slice(moved.to_string().as_bytes());
        msg.extend_from_slice(b" NZB file(s) to '");
        msg.extend_from_slice(dest_bytes);
        msg.push(b'\'');
        on_log(CleanupLogKind::Info, &msg);
    }

    dispose_archive(
        archive_file,
        processed_dir,
        config_action,
        non_nzb_files,
        &mut on_log,
    );

    if moved > 0 {
        for to_path in &to_paths {
            on_moved_nzb(os_str_to_bytes(to_path.as_os_str()));
        }
    }
}

/// Executes `ArchiveProcessor::Process` (`daemon/queue/ArchiveProcessor.cpp`) end-to-end:
/// logs `"ArchiveProcessor: Extracting '...'"`, calls `extract_fn(archive_file, unpack_dir)`,
/// handles extraction failure via `archive_handle_extract_failure`, or finishes via
/// `archive_finish_extracted`.
#[allow(clippy::too_many_arguments)]
pub fn archive_processor_process<E, L, M>(
    archive_file: &Path,
    unpack_dir: &Path,
    dest_dir: &Path,
    processed_dir: &Path,
    broken_dir: &Path,
    config_action: i32,
    extract_fn: E,
    mut on_log: L,
    on_moved_nzb: M,
) -> bool
where
    E: FnOnce(&Path, &Path) -> (bool, bool),
    L: FnMut(CleanupLogKind, &[u8]),
    M: FnMut(&[u8]),
{
    let filename_bytes = archive_file.file_name().map(os_str_to_bytes).unwrap_or(b"");
    let mut start_msg = Vec::with_capacity(34 + filename_bytes.len());
    start_msg.extend_from_slice(b"ArchiveProcessor: Extracting '");
    start_msg.extend_from_slice(filename_bytes);
    start_msg.push(b'\'');
    on_log(CleanupLogKind::Info, &start_msg);

    if filename_bytes.len() >= 4
        && filename_bytes[filename_bytes.len() - 4..].eq_ignore_ascii_case(b".zip")
    {
        if let Ok(info) = crate::unpack::inspect_zip_file(archive_file) {
            let detail = format!(
                "ArchiveProcessor: zipview identified {:?} with {} file(s) ({} bytes) in '{}'",
                info.kind,
                info.file_count,
                info.total_uncompressed_size,
                String::from_utf8_lossy(filename_bytes)
            );
            on_log(CleanupLogKind::Detail, detail.as_bytes());
        }
    }

    let (supported, extracted) = if crate::unpack::can_extract_with_zipfast(archive_file) {
        let ok = crate::unpack::extract_archive_with_zipfast(
            archive_file,
            unpack_dir,
            "",
            1,
            |kind, msg| {
                let log_kind = match kind {
                    1 => CleanupLogKind::Error,
                    2 => CleanupLogKind::Warning,
                    0 => CleanupLogKind::Info,
                    _ => CleanupLogKind::Detail,
                };
                on_log(log_kind, msg.as_bytes());
            },
        )
        .is_ok();
        (true, ok)
    } else {
        extract_fn(archive_file, unpack_dir)
    };
    if !supported {
        return false;
    }
    if !extracted {
        archive_handle_extract_failure(archive_file, unpack_dir, broken_dir, &mut on_log);
        return false;
    }

    archive_finish_extracted(
        archive_file,
        unpack_dir,
        dest_dir,
        processed_dir,
        config_action,
        &mut on_log,
        on_moved_nzb,
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_files_and_collisions_end_to_end() {
        let work_dir =
            std::env::temp_dir().join(format!("rustyget_cleanup_move_{}", std::process::id()));
        let src = work_dir.join("src");
        let dst = work_dir.join("dst");
        let _ = std::fs::remove_dir_all(&work_dir);
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        std::fs::write(src.join("video.mkv"), b"new data").unwrap();
        std::fs::write(dst.join("video.mkv"), b"old data").unwrap();
        std::fs::write(src.join("sub/nested.txt"), b"nested").unwrap();

        let mut logs = Vec::new();
        let mut renames = Vec::new();
        let ok = move_files(
            &src,
            &dst,
            || false,
            |kind, msg| logs.push((kind, String::from_utf8_lossy(msg).into_owned())),
            |old_rel, new_rel| {
                renames.push((
                    String::from_utf8_lossy(old_rel).into_owned(),
                    String::from_utf8_lossy(new_rel).into_owned(),
                ))
            },
        );
        assert!(ok);
        assert!(dst.join("video.mkv").exists());
        assert!(dst.join("video (1).mkv").exists());
        assert!(dst.join("sub/nested.txt").exists());
        assert!(!src.exists());
        assert_eq!(
            renames,
            vec![("video.mkv".to_string(), "video (1).mkv".to_string())]
        );
        assert_eq!(decide_move_outcome(true, false), (1, 2));
        assert_eq!(decide_move_outcome(false, true), (2, 0));
        assert_eq!(decide_move_outcome(false, false), (3, 1));

        let _ = std::fs::remove_dir_all(&work_dir);
    }

    #[test]
    fn test_cleanup_and_rename_end_to_end() {
        let work_dir =
            std::env::temp_dir().join(format!("rustyget_cleanup_ext_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work_dir);
        std::fs::create_dir_all(work_dir.join("sub")).unwrap();

        std::fs::write(work_dir.join("keep.mkv"), b"video").unwrap();
        std::fs::write(work_dir.join("sub/trash.sfv"), b"sfv").unwrap();
        std::fs::write(work_dir.join("trash.nfo"), b"nfo").unwrap();

        let mut logs = Vec::new();
        let (outcome, status) = run_cleanup(
            &work_dir,
            Path::new(""),
            b".nfo, .sfv",
            |kind, msg| logs.push((kind, String::from_utf8_lossy(msg).into_owned())),
        );
        assert_eq!((outcome, status), (1, 2));
        assert!(work_dir.join("keep.mkv").exists());
        assert!(!work_dir.join("trash.nfo").exists());

        assert_eq!(rename_archive_kind_prefix(1, 1), c"archive ");
        assert_eq!(rename_archive_kind_prefix(0, 1), c"");
        assert_eq!(rename_classify_run_outcome(true, 5), 1);
        assert_eq!(rename_classify_run_outcome(false, 5), 2);
        assert_eq!(rename_classify_run_outcome(false, 0), 3);
        assert_eq!(rename_eval_par_config(3, 3, 0, false), (true, false));
        assert_eq!(
            rename_complete_action(0, 0, true, 2, true, 0),
            (2, true, true)
        );
        assert!(rename_should_reset_par_status(1024));
        assert!(!rename_should_reset_par_status(0));

        let _ = std::fs::remove_dir_all(&work_dir);
    }

    #[test]
    fn test_post_renamer_and_archive_processor_end_to_end() {
        let work_dir =
            std::env::temp_dir().join(format!("rustyget_post_renamer_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work_dir);
        std::fs::create_dir_all(&work_dir).unwrap();

        std::fs::write(work_dir.join("a8f7s6d5f4g3h2j1k0l9.mkv"), b"videodata").unwrap();
        let mut info_names = Vec::new();
        let mut logs = Vec::new();
        let mut renames = Vec::new();
        let status = run_post_renamer(
            true,
            b"",
            b"Movie.2026.1080p",
            &work_dir,
            b"",
            |info| info_names.push(String::from_utf8_lossy(info).into_owned()),
            || false,
            |kind, msg| logs.push((kind, String::from_utf8_lossy(msg).into_owned())),
            |old_name, new_name| {
                renames.push((
                    String::from_utf8_lossy(old_name).into_owned(),
                    String::from_utf8_lossy(new_name).into_owned(),
                ))
            },
        );
        assert_eq!(status, 2); // Success
        assert!(work_dir.join("Movie.2026.1080p.mkv").exists());
        assert_eq!(
            renames,
            vec![(
                "a8f7s6d5f4g3h2j1k0l9.mkv".to_string(),
                "Movie.2026.1080p.mkv".to_string()
            )]
        );

        let unpack_dir = work_dir.join("unpack");
        let nzb_dest = work_dir.join("nzb_dest");
        let processed = work_dir.join("_processed");
        std::fs::create_dir_all(&unpack_dir).unwrap();
        std::fs::create_dir_all(&nzb_dest).unwrap();
        let archive_file = work_dir.join("bundle.zip");
        std::fs::write(&archive_file, b"zip").unwrap();
        std::fs::write(unpack_dir.join("release.nzb"), b"<nzb/>").unwrap();

        let mut moved_nzbs = Vec::new();
        archive_finish_extracted(
            &archive_file,
            &unpack_dir,
            &nzb_dest,
            &processed,
            0, // Delete
            |kind, msg| logs.push((kind, String::from_utf8_lossy(msg).into_owned())),
            |p| moved_nzbs.push(bytes_to_path_buf(p)),
        );
        assert_eq!(moved_nzbs, vec![nzb_dest.join("release.nzb")]);
        assert!(nzb_dest.join("release.nzb").exists());
        assert!(!archive_file.exists());
        assert!(!unpack_dir.exists());

        let _ = std::fs::remove_dir_all(&work_dir);
    }

    #[test]
    fn collection_analyzer_full_suite_port_from_cpp() {
        use crate::collection::{
            analyze, resolve_sample_name, resolve_subtitle_name, resolve_target_name, Entry,
        };

        let make_entry = |path: &str, filename: &str, stem: &str, ext: &str, size: u64| Entry {
            path: path.as_bytes().to_vec(),
            rename_prefix: b"/path/".to_vec(),
            filename: filename.as_bytes().to_vec(),
            stem: stem.as_bytes().to_vec(),
            ext: ext.as_bytes().to_vec(),
            size,
        };

        // 1. CollectionAnalyzerSingleFeatureDominanceTest
        let files = vec![
            make_entry("/path/feature.mkv", "feature.mkv", "feature", ".mkv", 4_000_000_000),
            make_entry("/path/sample.mkv", "sample.mkv", "sample", ".mkv", 50_000_000),
            make_entry("/path/feature.en.srt", "feature.en.srt", "feature.en", ".srt", 100_000),
        ];
        let res = analyze(&files);
        assert!(res.can_rename(&files));
        assert!(!res.ambiguous);
        assert_eq!(files[res.main_video.unwrap()].filename, b"feature.mkv");
        assert_eq!(files[res.sample_video.unwrap()].filename, b"sample.mkv");
        assert_eq!(res.subtitles.len(), 1);
        assert_eq!(files[res.subtitles[0]].filename, b"feature.en.srt");

        // 2. CollectionAnalyzerAmbiguousMultiEpisodeTest
        let files = vec![
            make_entry("/path/ep01.mkv", "ep01.mkv", "ep01", ".mkv", 1_200_000_000),
            make_entry("/path/ep02.mkv", "ep02.mkv", "ep02", ".mkv", 1_100_000_000),
        ];
        let res = analyze(&files);
        assert!(res.ambiguous);
        assert!(!res.can_rename(&files));

        // 3. CollectionAnalyzerDominantRatioTest
        let files = vec![
            make_entry("/path/feature.mkv", "feature.mkv", "feature", ".mkv", 4_000_000_000),
            make_entry("/path/extra.mkv", "extra.mkv", "extra", ".mkv", 1_000_000_000),
        ];
        let res = analyze(&files);
        assert!(res.can_rename(&files));
        assert!(!res.ambiguous);
        assert_eq!(files[res.main_video.unwrap()].filename, b"feature.mkv");

        // 4. CollectionAnalyzerDiscStructureProtectionTest
        let files = vec![
            make_entry("/path/BDMV/STREAM/00001.m2ts", "00001.m2ts", "00001", ".m2ts", 20_000_000_000),
            make_entry("/path/BDMV/PLAYLIST/00001.mpls", "00001.mpls", "00001", ".mpls", 5_000),
        ];
        let res = analyze(&files);
        assert!(res.disc_structure);
        assert!(!res.can_rename(&files));

        // 5. CollectionAnalyzerMusicAlbumProtectionTest
        let files = vec![
            make_entry("/path/01-track.flac", "01-track.flac", "01-track", ".flac", 30_000_000),
            make_entry("/path/02-track.flac", "02-track.flac", "02-track", ".flac", 35_000_000),
        ];
        let res = analyze(&files);
        assert!(res.has_audio);
        assert!(res.ambiguous);
        assert!(!res.can_rename(&files));

        // 6. CollectionAnalyzerSidecarNamingTest
        assert_eq!(
            resolve_subtitle_name(b"Movie.2026", b"12345.en", b".srt"),
            b"Movie.2026.en.srt"
        );
        assert_eq!(
            resolve_subtitle_name(b"Movie.2026", b"sub.fre", b".srt"),
            b"Movie.2026.fre.srt"
        );
        assert_eq!(
            resolve_subtitle_name(b"Movie.2026", b"obfuscated_sub", b".srt"),
            b"Movie.2026.srt"
        );
        assert_eq!(
            resolve_sample_name(b"Movie.2026", b".mkv"),
            b"Movie.2026-sample.mkv"
        );

        // 7. DuplicateCollisionUniqueNameTest
        let base_dir = std::env::temp_dir().join(format!(
            "nzbget_rust_collection_suite_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base_dir);
        let dupe_dir = base_dir.join("dupe_collision");
        std::fs::create_dir_all(&dupe_dir).unwrap();
        std::fs::write(dupe_dir.join("Feature.mkv"), b"existing").unwrap();
        let dupe_dir_cstr = std::ffi::CString::new(os_str_to_bytes(dupe_dir.as_os_str())).unwrap();
        let unique_name = crate::fsops::make_unique_filename(
            &dupe_dir_cstr,
            c"Feature.mkv",
            &|p| {
                if bytes_to_path_buf(p).is_file() {
                    1
                } else {
                    -2
                }
            },
        );
        assert!(String::from_utf8_lossy(&unique_name).contains("Feature.duplicate1.mkv"));

        // 8. CollectionAnalyzerUtf8PathPreservationTest
        let utf8_dir = base_dir.join("nzbget_test_utf8_média");
        std::fs::create_dir_all(&utf8_dir).unwrap();
        std::fs::write(utf8_dir.join("vidéo_12345.mkv"), b"fake video data").unwrap();
        let (entries, res) = analyze_directory(&utf8_dir);
        assert_eq!(
            entries[res.main_video.unwrap()].filename,
            "vidéo_12345.mkv".as_bytes()
        );
        assert!(!entries[res.main_video.unwrap()].filename.contains(&b'?'));

        // 9. CollectionAnalyzerBuildPlanTest
        let plan_dir = base_dir.join("build_plan");
        std::fs::create_dir_all(&plan_dir).unwrap();
        std::fs::write(
            plan_dir.join("1234567890abcdef1234567890abcdef.mkv"),
            b"main video data (large)",
        )
        .unwrap();
        std::fs::write(plan_dir.join("sample.mkv"), b"sample").unwrap();
        std::fs::write(plan_dir.join("random_sub.en.srt"), b"sub").unwrap();
        let (_, plan) = build_plan_directory(&plan_dir, b"Clean.Movie.2026", b".zip, .rar");
        assert!(plan.can_rename);
        assert!(!plan.ambiguous);
        assert!(!plan.disc_structure);
        assert_eq!(plan.actions.len(), 3);
        assert!(plan.actions.iter().any(|a| a.new_filename == b"Clean.Movie.2026.mkv"));
        assert!(plan
            .actions
            .iter()
            .any(|a| a.new_filename == b"Clean.Movie.2026-sample.mkv"));
        assert!(plan
            .actions
            .iter()
            .any(|a| a.new_filename == b"Clean.Movie.2026.en.srt"));

        // 10. CollectionAnalyzerResolveTargetNameTest
        let hashed_meta1 =
            b"9e12c233ddf94502a46e47b1f0246f25 [10_78] _9e12c233ddf94502a46e47b1f0246f25.part09.rar_ yEnc";
        let nzb_name1 = b"Sample.Show.Title.S01E05.Pies.And.Tarts.720p.WEB.h264-GRP";
        assert_eq!(resolve_target_name(hashed_meta1, nzb_name1), nzb_name1);

        let hashed_meta2 =
            b"785d61fbd43d41e8aaa751f0c2481a1c [1_64] _785d61fbd43d41e8aaa751f0c2481a1c.par2_ yEnc";
        let nzb_name2 = b"Secret.Agents.of.A.B.C.D.E.S07E03.PROPER.1080p.WEB.H264-GRP";
        assert_eq!(resolve_target_name(hashed_meta2, nzb_name2), nzb_name2);
        assert_eq!(
            resolve_target_name(b"Some.Show.S01E01.1080p.WEB.x264-GRP", b"renamed-by-user"),
            b"Some.Show.S01E01.1080p.WEB.x264-GRP"
        );
        assert_eq!(resolve_target_name(b"", nzb_name1), nzb_name1);
        assert_eq!(
            resolve_target_name(hashed_meta1, b"9e12c233ddf94502a46e47b1f0246f25"),
            b"9e12c233ddf94502a46e47b1f0246f25"
        );
        assert_eq!(resolve_target_name(b"", b""), b"");

        // 11. CollectionAnalyzerObfuscatedMetaFallbackTest
        let fb_dir = base_dir.join("meta_fallback");
        std::fs::create_dir_all(&fb_dir).unwrap();
        std::fs::write(
            fb_dir.join("1234567890abcdef1234567890abcdef.mkv"),
            b"main video data",
        )
        .unwrap();
        let resolved = resolve_target_name(hashed_meta1, nzb_name1);
        let (entries, plan) = build_plan_directory(&fb_dir, &resolved, b".zip, .rar");
        assert!(plan.can_rename);
        assert!(!plan.target_name_obfuscated);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            entries[plan.actions[0].src].filename,
            b"1234567890abcdef1234567890abcdef.mkv"
        );
        assert_eq!(
            plan.actions[0].new_filename,
            [nzb_name1.as_slice(), b".mkv"].concat()
        );

        // 12. CollectionAnalyzerCueSheetDiscProtectionTest
        let files = vec![
            make_entry("/path/Album.cue", "Album.cue", "Album", ".cue", 1024),
            make_entry("/path/Album.bin", "Album.bin", "Album", ".bin", 650_000_000),
        ];
        let res = analyze(&files);
        assert!(res.disc_structure);
        assert!(!res.can_rename(&files));

        // 13. CollectionAnalyzerClutterSkipTest
        let clutter_dir = base_dir.join("clutter_skip");
        std::fs::create_dir_all(clutter_dir.join("@eaDir")).unwrap();
        std::fs::write(clutter_dir.join("real_video.mkv"), b"video").unwrap();
        std::fs::write(clutter_dir.join("._real_video.mkv"), b"resource fork").unwrap();
        std::fs::write(clutter_dir.join(".DS_Store"), b"desktop").unwrap();
        let (entries, res) = analyze_directory(&clutter_dir);
        assert_eq!(entries[res.main_video.unwrap()].filename, b"real_video.mkv");

        // 14. CollectionAnalyzerCleanVideoSidecarPairingTest
        let sidecar_dir = base_dir.join("clean_video_sidecars");
        std::fs::create_dir_all(&sidecar_dir).unwrap();
        std::fs::write(sidecar_dir.join("Show.S01E01.720p.mkv"), b"clean video data").unwrap();
        std::fs::write(sidecar_dir.join("sub.en.srt"), b"subtitle").unwrap();
        let (_, plan) = build_plan_directory(&sidecar_dir, b"Show.S01E01", b".zip, .rar");
        assert!(plan.can_rename);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].new_filename, b"Show.S01E01.720p.en.srt");

        // 15. CollectionAnalyzerObfuscatedNzbTitleWithCleanVideoTest
        let obf_nzb_dir = base_dir.join("obf_nzb_clean_video");
        std::fs::create_dir_all(&obf_nzb_dir).unwrap();
        std::fs::write(obf_nzb_dir.join("Show.S01E01.720p.mkv"), b"clean video").unwrap();
        std::fs::write(
            obf_nzb_dir.join("1234567890abcdef1234567890abcdef.en.srt"),
            b"sub",
        )
        .unwrap();
        let (_, plan) =
            build_plan_directory(&obf_nzb_dir, b"1a2b3c4d5e6f7g8h9i0j1k2l", b".zip, .rar");
        assert!(plan.can_rename);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].new_filename, b"Show.S01E01.720p.en.srt");

        // 16. CollectionAnalyzerAudioCollectionTest
        let audio_dir = base_dir.join("audio_collection");
        std::fs::create_dir_all(&audio_dir).unwrap();
        std::fs::write(audio_dir.join("track01.flac"), b"audio data").unwrap();
        let (entries, single_res) = analyze_directory(&audio_dir);
        assert!(single_res.has_audio);
        assert!(!single_res.ambiguous);
        assert!(!single_res.can_rename(&entries));
        std::fs::write(audio_dir.join("track02.flac"), b"audio data 2").unwrap();
        let (entries, multi_res) = analyze_directory(&audio_dir);
        assert!(multi_res.has_audio);
        assert!(multi_res.ambiguous);
        assert!(!multi_res.can_rename(&entries));

        // 17. CollectionAnalyzerExtensionlessVideoSniffingTest
        let extless_vid_dir = base_dir.join("extless_video");
        std::fs::create_dir_all(&extless_vid_dir).unwrap();
        let mut mkv_data = vec![
            0x1A, 0x45, 0xDF, 0xA3, 0xA3, 0x42, 0x86, 0x81, 0x01, 0x42, 0xF7, 0x81, 0x01, 0x42,
            0xF2, 0x81, 0x04, 0x42, 0xF3, 0x81, 0x08, 0x42, 0x82, 0x88, b'm', b'a', b't', b'r',
            b'o', b's', b'k', b'a',
        ];
        mkv_data.extend(std::iter::repeat_n(b'X', 1024));
        std::fs::write(
            extless_vid_dir.join("33f5e57280cf9321825f9243daa1dcb0"),
            &mkv_data,
        )
        .unwrap();
        std::fs::write(
            extless_vid_dir.join("33f5e57280cf9321825f9243daa1dcb0.en.srt"),
            b"1\n00:00:01,000 --> 00:00:02,000\nHello\n",
        )
        .unwrap();
        let (entries, plan) =
            build_plan_directory(&extless_vid_dir, b"Release.Title.S01E09.1080p", b".zip, .rar");
        assert!(plan.can_rename);
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(
            entries[plan.actions[0].src].filename,
            b"33f5e57280cf9321825f9243daa1dcb0"
        );
        assert_eq!(
            plan.actions[0].new_filename,
            b"Release.Title.S01E09.1080p.mkv"
        );
        assert_eq!(
            entries[plan.actions[1].src].filename,
            b"33f5e57280cf9321825f9243daa1dcb0.en.srt"
        );
        assert_eq!(
            plan.actions[1].new_filename,
            b"Release.Title.S01E09.1080p.en.srt"
        );

        // 18. CollectionAnalyzerExtensionlessBookSniffingTest
        let extless_book_dir = base_dir.join("extless_book");
        std::fs::create_dir_all(&extless_book_dir).unwrap();
        let mut epub_header = vec![0u8; 80];
        epub_header[..4].copy_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
        let mime = b"mimetypeapplication/epub+zip";
        epub_header[30..30 + mime.len()].copy_from_slice(mime);
        std::fs::write(
            extless_book_dir.join("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"),
            &epub_header,
        )
        .unwrap();
        let (entries, plan) =
            build_plan_directory(&extless_book_dir, b"Author.Novel.Title.2026", b".zip, .rar");
        assert!(plan.can_rename);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            entries[plan.actions[0].src].filename,
            b"a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"
        );
        assert_eq!(
            plan.actions[0].new_filename,
            b"Author.Novel.Title.2026.epub"
        );

        // 19. CollectionAnalyzerMultiBookAmbiguousTest
        let multi_book_dir = base_dir.join("multi_book");
        std::fs::create_dir_all(&multi_book_dir).unwrap();
        std::fs::write(multi_book_dir.join("Volume1.epub"), b"book 1").unwrap();
        std::fs::write(multi_book_dir.join("Volume2.epub"), b"book 2").unwrap();
        let (entries, res) = analyze_directory(&multi_book_dir);
        assert!(res.ambiguous);
        assert!(!res.can_rename(&entries));

        // 20. CollectionAnalyzerSingleBookNotDuplicatedInOtherFilesTest
        let files = vec![make_entry(
            "/path/Novel.epub",
            "Novel.epub",
            "Novel",
            ".epub",
            2_000_000,
        )];
        let res = analyze(&files);
        assert!(res.can_rename(&files));
        assert_eq!(files[res.main_book.unwrap()].filename, b"Novel.epub");
        assert!(res.other_files.is_empty());

        // 21. CollectionAnalyzerDvdVideoTsTest
        let files = vec![
            make_entry("/path/VIDEO_TS/VIDEO_TS.BUP", "VIDEO_TS.BUP", "VIDEO_TS", ".BUP", 1024),
            make_entry("/path/VIDEO_TS/VTS_01_1.VOB", "VTS_01_1.VOB", "VTS_01_1", ".VOB", 1_073_741_824),
            make_entry("/path/VIDEO_TS/VTS_01_0.IFO", "VTS_01_0.IFO", "VTS_01_0", ".IFO", 2048),
        ];
        let res = analyze(&files);
        assert!(res.disc_structure);
        assert!(!res.can_rename(&files));
        assert!(res.main_video.is_none());

        // 22. CollectionAnalyzerDvdAvchdTest
        let avchd_dir = base_dir.join("dvd_avchd");
        std::fs::create_dir_all(avchd_dir.join("AVCHD/BDMV/STREAM")).unwrap();
        std::fs::write(avchd_dir.join("AVCHD/BDMV/STREAM/00001.m2ts"), b"fake m2ts data").unwrap();
        let (entries, res) = analyze_directory(&avchd_dir);
        assert!(res.disc_structure);
        assert!(!res.can_rename(&entries));

        // 23. CollectionAnalyzerMusicAlbumLargerTest & CollectionAnalyzerMixedAudioVideoTest
        let files = vec![
            make_entry("/path/01-intro.flac", "01-intro.flac", "01-intro", ".flac", 8_000_000),
            make_entry("/path/02-main.flac", "02-main.flac", "02-main", ".flac", 45_000_000),
            make_entry("/path/03-solo.flac", "03-solo.flac", "03-solo", ".flac", 38_000_000),
            make_entry("/path/04-outro.flac", "04-outro.flac", "04-outro", ".flac", 12_000_000),
            make_entry("/path/05-bonus.flac", "05-bonus.flac", "05-bonus", ".flac", 25_000_000),
        ];
        let res = analyze(&files);
        assert!(res.has_audio);
        assert!(res.ambiguous);
        assert!(!res.can_rename(&files));
        assert!(res.main_video.is_none());

        let files = vec![
            make_entry("/path/feature.mkv", "feature.mkv", "feature", ".mkv", 4_000_000_000),
            make_entry("/path/commentary.dts", "commentary.dts", "commentary", ".dts", 600_000_000),
            make_entry("/path/score.flac", "score.flac", "score", ".flac", 80_000_000),
            make_entry("/path/extras.flac", "extras.flac", "extras", ".flac", 50_000_000),
        ];
        let res = analyze(&files);
        assert!(res.has_audio);
        assert!(!res.ambiguous);
        assert!(res.can_rename(&files));
        assert_eq!(files[res.main_video.unwrap()].filename, b"feature.mkv");

        // 24. CollectionAnalyzerPdfSniffingTest, BookVsVideo, M4b, CleanDots, Djvu, DiscDescriptor, RepeatedPathSeparators
        let pdf_dir = base_dir.join("pdf_sniff");
        std::fs::create_dir_all(&pdf_dir).unwrap();
        let mut pdf_data = b"%PDF-1.7\0".to_vec();
        pdf_data.extend(std::iter::repeat_n(b'X', 512));
        std::fs::write(pdf_dir.join("deadbeef0102030405060708deadbeef01"), &pdf_data).unwrap();
        let (_, plan) = build_plan_directory(
            &pdf_dir,
            b"Technical.Reference.Guide.2026",
            b".zip, .rar",
        );
        assert!(plan.can_rename);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            plan.actions[0].new_filename,
            b"Technical.Reference.Guide.2026.pdf"
        );

        let files = vec![
            make_entry("/path/feature.mkv", "feature.mkv", "feature", ".mkv", 4_000_000_000),
            make_entry("/path/accompanying.epub", "accompanying.epub", "accompanying", ".epub", 500_000),
        ];
        let res = analyze(&files);
        assert!(!res.ambiguous);
        assert!(res.can_rename(&files));
        assert_eq!(files[res.main_video.unwrap()].filename, b"feature.mkv");
        assert!(res.main_book.is_none());

        let m4b_dir = base_dir.join("extless_m4b");
        std::fs::create_dir_all(&m4b_dir).unwrap();
        let mut m4b_data = vec![
            0x00, 0x00, 0x00, 0x20, b'f', b't', b'y', b'p', b'M', b'4', b'B', b' ', 0x00, 0x00,
            0x00, 0x00, b'M', b'4', b'B', b' ', b'm', b'p', b'4', b'2', b'i', b's', b'o', b'm',
        ];
        m4b_data.extend(std::iter::repeat_n(b'X', 1024));
        std::fs::write(m4b_dir.join("44a5b6c7d8e9f0123456789abcdef012"), &m4b_data).unwrap();
        let (_, plan) = build_plan_directory(
            &m4b_dir,
            b"Famous.Author.Audiobook.Title.2026",
            b".zip, .rar",
        );
        assert!(plan.can_rename);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            plan.actions[0].new_filename,
            b"Famous.Author.Audiobook.Title.2026.m4b"
        );

        let dots_dir = base_dir.join("clean_dots_extless");
        std::fs::create_dir_all(&dots_dir).unwrap();
        std::fs::write(dots_dir.join("Show.S01E01.1080p"), &mkv_data).unwrap();
        let (entries, plan) =
            build_plan_directory(&dots_dir, b"Show.S01E01.1080p", b".zip, .rar");
        assert!(plan.can_rename);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(entries[plan.actions[0].src].filename, b"Show.S01E01.1080p");
        assert_eq!(plan.actions[0].new_filename, b"Show.S01E01.1080p.mkv");

        let djvu_dir = base_dir.join("extless_djvu");
        std::fs::create_dir_all(&djvu_dir).unwrap();
        let mut djvu_data = vec![
            b'A', b'T', b'&', b'T', b'F', b'O', b'R', b'M', 0x00, 0x00, 0x04, 0x00, b'D', b'J',
            b'V', b'M',
        ];
        djvu_data.extend(std::iter::repeat_n(b'X', 1024));
        std::fs::write(djvu_dir.join("88f9e0a1b2c3d4e5f60123456789abcd"), &djvu_data).unwrap();
        let (_, plan) = build_plan_directory(
            &djvu_dir,
            b"Vintage.Magazine.Issue.42.1998",
            b".zip, .rar",
        );
        assert!(plan.can_rename);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            plan.actions[0].new_filename,
            b"Vintage.Magazine.Issue.42.1998.djvu"
        );

        let disc_desc_dir = base_dir.join("disc_descriptor_known");
        std::fs::create_dir_all(&disc_desc_dir).unwrap();
        std::fs::write(
            disc_desc_dir.join("Game.cue"),
            b"FILE \"Game.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        let mut bin_data = vec![0x1A, 0x45, 0xDF, 0xA3];
        bin_data.extend(std::iter::repeat_n(b'0', 1024));
        std::fs::write(disc_desc_dir.join("Game.bin"), &bin_data).unwrap();
        let (entries, res) = analyze_directory(&disc_desc_dir);
        assert!(res.disc_structure);
        assert!(!res.can_rename(&entries));
        assert!(res.main_video.is_none());

        let sep_dir = base_dir.join("collection_separators");
        std::fs::create_dir_all(&sep_dir).unwrap();
        std::fs::write(sep_dir.join("abc.mkv"), b"video").unwrap();
        let repeated = PathBuf::from(format!("{}///", sep_dir.display()));
        let (_, plan) = build_plan_directory(&repeated, b"Movie.2026", b"");
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            bytes_to_path_buf(&plan.actions[0].dst_path),
            sep_dir.join("Movie.2026.mkv")
        );

        let _ = std::fs::remove_dir_all(&base_dir);
    }

    #[test]
    fn move_controller_full_suite_port_from_cpp() {
        let base_dir = std::env::temp_dir().join(format!(
            "nzbget_rust_movecontroller_suite_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base_dir);
        std::fs::create_dir_all(&base_dir).unwrap();

        // 1-5: UniqueFilename tests
        let dir1 = base_dir.join("unique");
        std::fs::create_dir_all(&dir1).unwrap();
        let target = dir1.join("file.mkv");
        assert_eq!(make_unique_path(&target), target);

        std::fs::write(&target, b"existing").unwrap();
        assert_eq!(make_unique_path(&target), dir1.join("file (1).mkv"));

        std::fs::write(dir1.join("file (1).mkv"), b"collision1").unwrap();
        assert_eq!(make_unique_path(&target), dir1.join("file (2).mkv"));

        let makefile = dir1.join("Makefile");
        std::fs::write(&makefile, b"existing").unwrap();
        assert_eq!(make_unique_path(&makefile), dir1.join("Makefile (1)"));

        let utf8_target = dir1.join("vidéo_déjà_vu.mkv");
        std::fs::write(&utf8_target, b"existing").unwrap();
        assert_eq!(
            make_unique_path(&utf8_target),
            dir1.join("vidéo_déjà_vu (1).mkv")
        );

        // 6: MoveControllerCollisionKeepsBothAndRenamesRecord
        let m1_src = base_dir.join("m1/src");
        let m1_dst = base_dir.join("m1/dst");
        std::fs::create_dir_all(&m1_src).unwrap();
        std::fs::create_dir_all(&m1_dst).unwrap();
        std::fs::write(m1_src.join("video.mkv"), b"new data").unwrap();
        std::fs::write(m1_dst.join("video.mkv"), b"old data").unwrap();
        let mut renames = Vec::new();
        assert!(move_files(
            &m1_src,
            &m1_dst,
            || false,
            |_, _| {},
            |old_r, new_r| renames.push((old_r.to_vec(), new_r.to_vec())),
        ));
        assert_eq!(std::fs::read(m1_dst.join("video.mkv")).unwrap(), b"old data");
        assert_eq!(
            std::fs::read(m1_dst.join("video (1).mkv")).unwrap(),
            b"new data"
        );
        assert!(!m1_src.join("video.mkv").exists());
        assert_eq!(
            renames,
            vec![(b"video.mkv".to_vec(), b"video (1).mkv".to_vec())]
        );

        // 7: MoveControllerNoCollisionMovesFile
        let m2_src = base_dir.join("m2/src");
        let m2_dst = base_dir.join("m2/dst");
        std::fs::create_dir_all(&m2_src).unwrap();
        std::fs::create_dir_all(&m2_dst).unwrap();
        std::fs::write(m2_src.join("video.mkv"), b"data").unwrap();
        assert!(move_files(&m2_src, &m2_dst, || false, |_, _| {}, |_, _| {}));
        assert!(m2_dst.join("video.mkv").exists());
        assert!(!m2_src.join("video.mkv").exists());

        // 8: MoveControllerSameInodeRemovesSource
        let m3_src = base_dir.join("m3/src");
        let m3_dst = base_dir.join("m3/dst");
        std::fs::create_dir_all(&m3_src).unwrap();
        std::fs::create_dir_all(&m3_dst).unwrap();
        std::fs::write(m3_dst.join("video.mkv"), b"data").unwrap();
        std::fs::hard_link(m3_dst.join("video.mkv"), m3_src.join("video.mkv")).unwrap();
        assert!(move_files(&m3_src, &m3_dst, || false, |_, _| {}, |_, _| {}));
        assert!(m3_dst.join("video.mkv").exists());
        assert!(!m3_src.join("video.mkv").exists());

        // 9: MoveControllerNestedDirectories
        let m4_src = base_dir.join("m4/src");
        let m4_dst = base_dir.join("m4/dst");
        std::fs::create_dir_all(m4_src.join("sub/dir")).unwrap();
        std::fs::create_dir_all(&m4_dst).unwrap();
        std::fs::write(m4_src.join("sub/dir/inner.mkv"), b"data").unwrap();
        assert!(move_files(&m4_src, &m4_dst, || false, |_, _| {}, |_, _| {}));
        assert!(m4_dst.join("sub/dir/inner.mkv").exists());
        assert!(!m4_src.join("sub/dir/inner.mkv").exists());

        // 10: MoveControllerSameDirectoryNoOp
        let m5_same = base_dir.join("m5/same");
        std::fs::create_dir_all(&m5_same).unwrap();
        std::fs::write(m5_same.join("video.mkv"), b"keep me safe").unwrap();
        assert!(move_files(&m5_same, &m5_same, || false, |_, _| {}, |_, _| {}));
        assert_eq!(
            std::fs::read(m5_same.join("video.mkv")).unwrap(),
            b"keep me safe"
        );

        // 11: MoveControllerCascadingCollision
        let m6_src = base_dir.join("m6/src");
        let m6_dst = base_dir.join("m6/dst");
        std::fs::create_dir_all(&m6_src).unwrap();
        std::fs::create_dir_all(&m6_dst).unwrap();
        std::fs::write(m6_src.join("video.mkv"), b"new data 2").unwrap();
        std::fs::write(m6_dst.join("video.mkv"), b"original data").unwrap();
        std::fs::write(m6_dst.join("video (1).mkv"), b"collision 1").unwrap();
        let mut renames = Vec::new();
        assert!(move_files(
            &m6_src,
            &m6_dst,
            || false,
            |_, _| {},
            |old_r, new_r| renames.push((old_r.to_vec(), new_r.to_vec())),
        ));
        assert_eq!(
            std::fs::read(m6_dst.join("video (2).mkv")).unwrap(),
            b"new data 2"
        );
        assert_eq!(
            renames,
            vec![(b"video.mkv".to_vec(), b"video (2).mkv".to_vec())]
        );

        // 12: MoveControllerNestedDirectoryCollision
        let m7_src = base_dir.join("m7/src");
        let m7_dst = base_dir.join("m7/dst");
        std::fs::create_dir_all(m7_src.join("sub/dir")).unwrap();
        std::fs::create_dir_all(m7_dst.join("sub/dir")).unwrap();
        std::fs::write(m7_src.join("sub/dir/inner.mkv"), b"new nested data").unwrap();
        std::fs::write(m7_dst.join("sub/dir/inner.mkv"), b"existing nested data").unwrap();
        let mut renames = Vec::new();
        assert!(move_files(
            &m7_src,
            &m7_dst,
            || false,
            |_, _| {},
            |old_r, new_r| renames.push((old_r.to_vec(), new_r.to_vec())),
        ));
        assert_eq!(
            std::fs::read(m7_dst.join("sub/dir/inner (1).mkv")).unwrap(),
            b"new nested data"
        );
        assert_eq!(
            renames,
            vec![(
                b"sub/dir/inner.mkv".to_vec(),
                b"sub/dir/inner (1).mkv".to_vec()
            )]
        );

        // 13: MoveControllerNonExistentSourceFails
        let m8_src = base_dir.join("m8/non_existent_src");
        let m8_dst = base_dir.join("m8/dst");
        assert!(!move_files(
            &m8_src,
            &m8_dst,
            || false,
            |_, _| {},
            |_, _| {}
        ));

        let _ = std::fs::remove_dir_all(&base_dir);
    }

    #[test]
    fn archive_processor_full_suite_port_from_cpp() {
        let env_path = std::env::var("PATH").unwrap_or_default();
        let Some(sevenzip_bytes) =
            crate::util::util_resolve_path_from_env(b"7z", env_path.as_bytes())
        else {
            return;
        };
        let sevenzip_path = bytes_to_path_buf(&sevenzip_bytes);

        let run_archive_test = |archive_path: &Path, dest_dir: &Path| {
            let parent = dest_dir.parent().unwrap();
            let unpack_dir = parent.join("unpack").join("unique_unpack");
            let processed_dir = parent.join("_processed");
            let broken_dir = parent.join("_broken");

            let mut moved = Vec::new();
            let ok = archive_processor_process(
                archive_path,
                &unpack_dir,
                dest_dir,
                &processed_dir,
                &broken_dir,
                0, // Delete
                |arc, unp| {
                    let supported = crate::unpack::sevenzip_is_supported(os_str_to_bytes(
                        arc.as_os_str(),
                    ));
                    if !supported {
                        return (false, false);
                    }
                    let pw_arg =
                        String::from_utf8(crate::unpack::extractor_make_password(b"")).unwrap();
                    let status = std::process::Command::new(&sevenzip_path)
                        .args([
                            "x",
                            "-y",
                            crate::unpack::sevenzip_overwrite_switch(1)
                                .to_str()
                                .unwrap(),
                            &pw_arg,
                            &format!("-o{}", unp.display()),
                            arc.to_str().unwrap(),
                        ])
                        .stdout(std::process::Stdio::null())
                        .status()
                        .unwrap();
                    (true, status.success())
                },
                |_, _| {},
                |p| moved.push(bytes_to_path_buf(p)),
            );
            assert!(ok);
            let expected = dest_dir.join("test.nzb");
            assert_eq!(moved, vec![expected.clone()]);
            assert!(expected.exists());
        };

        // 1. ArchiveCategoryPreservationTest & 2. ArchiveSubdirEscapeTest
        let base_dir = std::env::temp_dir().join(format!(
            "nzbget_rust_archiveproc_suite_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base_dir);
        let cat_dir = base_dir.join("cat/nzbdir/TV");
        std::fs::create_dir_all(&cat_dir).unwrap();
        let nzb_source = base_dir.join("cat/test.nzb");
        std::fs::write(
            &nzb_source,
            b"<?xml version=\"1.0\"?>\n<nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n</nzb>\n",
        )
        .unwrap();
        let archive_path = cat_dir.join("test.nzb.gz");
        let status = std::process::Command::new(&sevenzip_path)
            .current_dir(base_dir.join("cat"))
            .args([
                "a",
                "-tgzip",
                archive_path.to_str().unwrap(),
                "test.nzb",
            ])
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        run_archive_test(&archive_path, &cat_dir);

        let esc_base = base_dir.join("esc");
        let nzb_dir = esc_base.join("nzbdir");
        let outside_dir = esc_base.join("tmp");
        std::fs::create_dir_all(&nzb_dir).unwrap();
        std::fs::create_dir_all(&outside_dir).unwrap();
        std::fs::write(
            esc_base.join("test.nzb"),
            b"<?xml version=\"1.0\"?>\n<nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n</nzb>\n",
        )
        .unwrap();
        let archive_path2 = outside_dir.join("test.nzb.gz");
        let status2 = std::process::Command::new(&sevenzip_path)
            .current_dir(&esc_base)
            .args([
                "a",
                "-tgzip",
                archive_path2.to_str().unwrap(),
                "test.nzb",
            ])
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status2.success());
        run_archive_test(&archive_path2, &nzb_dir);

        let _ = std::fs::remove_dir_all(&base_dir);
    }
}
