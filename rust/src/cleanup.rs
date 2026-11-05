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
}
