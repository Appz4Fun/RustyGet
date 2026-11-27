//! Moving a queued job's completed files onto another disk without the queue lock.
//!
//! A new category (or name) gives a queued job a new directory, and its completed
//! files move there (`QueueCoordinator::SetQueueEntryCategory`, `MoveCompletedFiles`).
//! nzbget moves them under the queue lock; on another file system that is a copy of
//! every byte, and the whole daemon (downloads, every RPC) waits for it (6790: a bulk
//! recategorize froze it for about 4 minutes).
//!
//! Here, when a file has to cross file systems, the edit keeps the job's directory as
//! it was (the category is set at once) and leaves a [`Job`] for this thread. When the
//! thread lets go of the queue lock, the files are copied next to their new place,
//! without the lock; then, under the lock again, the move ends as before
//! (`article_writer_move_completed_files_precopied`): the directory changes, each copy
//! is renamed into place and its source removed, with the same paths, unique names,
//! messages and old-directory clean-up. The edit's caller (the RPC) still returns only
//! after that, as before.
//!
//! Meanwhile the job may change. It is left as it is when it was deleted, when its
//! directory changed (another move ended first), when a later edit gives it another
//! directory (that edit's own move does the work) or when it went to post-processing
//! (it runs where the files are). A completed file that changed or came after the copy
//! is moved under the lock, as before; copies not used are removed.

use crate::artwritercore::PreCopied;
use crate::queuesubsys_host::{NzbRef, QueueSubsysHost};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// A move left for after the queue lock.
#[derive(Debug, Clone)]
pub(crate) struct Job {
    pub nzb_id: i32,
    /// The job's directory until the move ends.
    pub old_dir: Vec<u8>,
    /// Its new directory, as `NzbInfo::BuildDestDirName` gave it at the edit.
    pub target: Vec<u8>,
    /// Its completed files at the edit.
    pub files: Vec<Vec<u8>>,
}

thread_local! {
    static PENDING: RefCell<Vec<Job>> = const { RefCell::new(Vec::new()) };
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

/// Makes the copies' names unique, also between moves that run at once.
static COPY_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The device of `path`, or of its nearest existing parent.
fn device_of(path: &Path) -> Option<u64> {
    let mut p = Some(path);
    while let Some(cur) = p {
        if let Ok(m) = std::fs::metadata(cur) {
            return Some(m.dev());
        }
        p = cur.parent().filter(|q| !q.as_os_str().is_empty());
    }
    None
}

fn bytes_path(b: &[u8]) -> &Path {
    Path::new(std::ffi::OsStr::from_bytes(b))
}

/// Whether any of `files` in `old_dir` lies on another file system than `target`.
pub(crate) fn crosses_devices(old_dir: &[u8], target: &[u8], files: &[Vec<u8>]) -> bool {
    let Some(target_dev) = device_of(bytes_path(target)) else {
        return false;
    };
    let old = bytes_path(old_dir);
    files.iter().any(|f| {
        std::fs::metadata(old.join(bytes_path(f)))
            .is_ok_and(|m| m.is_file() && m.dev() != target_dev)
    })
}

/// Under the queue lock, right after the edit rebuilt the job's directory from
/// `old_dir` to `target`: when its completed files would be copied onto another file
/// system, puts the directory back to `old_dir`, leaves the move for when this thread
/// lets go of the lock and returns true. False: move now, as before (no completed
/// file crosses devices, the lock is not held through `CurrentQueueLock`, or a move
/// is being ended).
pub(crate) fn defer(
    host: &QueueSubsysHost,
    nzb_ptr: NzbRef,
    old_dir: &[u8],
    target: &[u8],
) -> bool {
    if !host.use_active
        || nzb_ptr.is_none()
        || crate::statesaver::queue_lock_depth() == 0
        || RUNNING.with(Cell::get)
    {
        return false;
    }
    let nzb = host.nzb_snap(nzb_ptr, false);
    if nzb.skip_disk_write {
        return false;
    }
    let old_dir = crate::cstr::until_nul(old_dir);
    let target = crate::cstr::until_nul(target);
    let files: Vec<Vec<u8>> = host
        .nzb_completed_files(nzb_ptr)
        .into_iter()
        .map(|c| c.filename)
        .collect();
    if files.is_empty() || !crosses_devices(old_dir, target, &files) {
        return false;
    }
    host.nzb_set_str(nzb_ptr, crate::queuesubsys_host::NZB_STR_DEST_DIR, old_dir);
    PENDING.with(|p| {
        p.borrow_mut().push(Job {
            nzb_id: nzb.id,
            old_dir: old_dir.to_vec(),
            target: target.to_vec(),
            files,
        })
    });
    true
}

/// `qqueue::CurrentQueueLock` let go of the queue lock and this thread holds it no
/// more: runs the moves the thread's edits left.
pub(crate) fn run_after_unlock() {
    if RUNNING.with(Cell::get) {
        return;
    }
    let jobs = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
    if jobs.is_empty() || std::thread::panicking() {
        return;
    }
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            RUNNING.with(|r| r.set(false));
        }
    }
    RUNNING.with(|r| r.set(true));
    let _running = Running;
    for job in jobs {
        run_job(&host, &job);
    }
}

fn run_job(host: &QueueSubsysHost, job: &Job) {
    let target = bytes_path(&job.target);
    let target_existed = std::fs::metadata(target).is_ok();
    let target_dev = device_of(target);
    let mut copies = copy_files(job, |m| Some(m.dev()) != target_dev);
    let moved = crate::qqueue::with_current_locked(|dq| commit(host, dq, job, &mut copies));
    for c in copies.values() {
        if c.err == 0 {
            let _ = std::fs::remove_file(&c.temp);
        }
    }
    if !moved && !target_existed {
        remove_empty_dirs(target, &job.files);
    }
}

/// Copies each of the job's files that `crosses` (by its metadata) next to its new
/// place, without the queue lock. A file that is missing, already in place or not
/// crossing is left out; a failed copy is kept with its `errno`.
pub(crate) fn copy_files(
    job: &Job,
    crosses: impl Fn(&std::fs::Metadata) -> bool,
) -> HashMap<Vec<u8>, PreCopied> {
    let mut out = HashMap::new();
    let old_dir = bytes_path(&job.old_dir);
    let target = bytes_path(&job.target);
    if std::fs::create_dir_all(target).is_err() {
        return out; // the move under the lock reports it
    }
    for f in &job.files {
        let old_path = old_dir.join(bytes_path(f));
        let new_path = target.join(bytes_path(f));
        if old_path == new_path {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&old_path) else {
            continue;
        };
        if !meta.is_file() || !crosses(&meta) {
            continue;
        }
        let (Some(parent), Some(base)) = (new_path.parent(), new_path.file_name()) else {
            continue;
        };
        if std::fs::create_dir_all(parent).is_err() {
            continue;
        }
        let seq = COPY_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut name = b".".to_vec();
        name.extend_from_slice(base.as_bytes());
        name.extend_from_slice(format!(".rgmove-{}-{seq}", std::process::id()).as_bytes());
        let temp = parent.join(std::ffi::OsStr::from_bytes(&name));
        let mut pre = PreCopied {
            temp: temp.clone(),
            err: 0,
            len: meta.len(),
            mtime: (meta.mtime(), meta.mtime_nsec()),
            dev: meta.dev(),
            ino: meta.ino(),
        };
        let (Ok(c_src), Ok(c_dst)) = (
            std::ffi::CString::new(old_path.as_os_str().as_bytes()),
            std::ffi::CString::new(temp.as_os_str().as_bytes()),
        ) else {
            continue;
        };
        if let Err(e) = crate::fsops::copy_file_exclusive(&c_src, &c_dst) {
            pre.err = e;
            pre.temp = PathBuf::new();
        } else if !std::fs::metadata(&old_path).is_ok_and(|m| pre.same_source(&m)) {
            // written meanwhile: moved under the lock
            let _ = std::fs::remove_file(&temp);
            continue;
        }
        out.insert(f.clone(), pre);
    }
    out
}

/// Under the queue lock again: ends the move when the job is still as the edit left
/// it. True when the job's directory changed to the target.
fn commit(
    host: &QueueSubsysHost,
    dq_ptr: usize,
    job: &Job,
    copies: &mut HashMap<Vec<u8>, PreCopied>,
) -> bool {
    let nzb_ptr = host.queue_find_by_id(dq_ptr, job.nzb_id);
    if nzb_ptr.is_none() {
        return false; // deleted meanwhile
    }
    let nzb = host.nzb_snap(nzb_ptr, false);
    if crate::cstr::until_nul(&nzb.dest_dir) != job.old_dir.as_slice() {
        return false; // another move ended first
    }
    if nzb.has_post_info {
        let mut msg = b"Could not move the completed files of ".to_vec();
        msg.extend_from_slice(&nzb.name);
        msg.extend_from_slice(b" to ");
        msg.extend_from_slice(&job.target);
        msg.extend_from_slice(b": the download went to post-processing meanwhile");
        host.nzb_print_message(nzb_ptr, crate::queuesubsys_host::MK_WARNING, &msg);
        return false;
    }
    host.nzb_action(nzb_ptr, crate::queuesubsys_host::NZB_ACT_BUILD_DEST_DIR);
    let now = host.nzb_snap(nzb_ptr, false).dest_dir;
    if crate::cstr::until_nul(&now) != job.target.as_slice() {
        // a later edit gave it another directory, and moves the files itself
        host.nzb_set_str(
            nzb_ptr,
            crate::queuesubsys_host::NZB_STR_DEST_DIR,
            &job.old_dir,
        );
        return false;
    }
    crate::articledl::move_completed_files_on_qh_precopied(host, nzb_ptr, &job.old_dir, copies);
    host.dq_save(dq_ptr);
    true
}

/// Removes the directories a move that did not happen created under `target`, when
/// empty: the files' parents first, then `target`.
fn remove_empty_dirs(target: &Path, files: &[Vec<u8>]) {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for f in files {
        let mut d = target.join(bytes_path(f)).parent().map(Path::to_path_buf);
        while let Some(cur) = d {
            if cur == target || !cur.starts_with(target) {
                break;
            }
            d = cur.parent().map(Path::to_path_buf);
            dirs.push(cur);
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    dirs.dedup();
    for d in dirs {
        let _ = std::fs::remove_dir(d);
    }
    let _ = std::fs::remove_dir(target);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artwritercore::article_writer_move_completed_files_precopied;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rg-recatmove-{tag}-{}-{}",
            std::process::id(),
            COPY_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn b(p: &Path) -> Vec<u8> {
        p.as_os_str().as_bytes().to_vec()
    }

    fn job(old: &Path, new: &Path, files: &[&str]) -> Job {
        Job {
            nzb_id: 1,
            old_dir: b(old),
            target: b(new),
            files: files.iter().map(|f| f.as_bytes().to_vec()).collect(),
        }
    }

    #[test]
    fn copies_then_ends_the_move_as_a_plain_move_does() {
        let root = tmp("copy");
        let old = root.join("old");
        let new = root.join("new");
        std::fs::create_dir_all(old.join("sub")).unwrap();
        std::fs::write(old.join("a.bin"), b"alpha").unwrap();
        std::fs::write(old.join("sub/b.bin"), b"beta").unwrap();
        let j = job(&old, &new, &["a.bin", "sub/b.bin", "missing.bin"]);
        let mut copies = copy_files(&j, |_| true);
        assert_eq!(copies.len(), 2);
        // the sources stay until the move ends; the copies are hidden next to their place
        assert!(old.join("a.bin").exists());
        assert!(copies.values().all(|c| c.err == 0 && c.temp.exists()));
        let names: Vec<&[u8]> = j.files.iter().map(Vec::as_slice).collect();
        let mut errors = Vec::new();
        let ok = article_writer_move_completed_files_precopied(
            &j.target,
            &j.old_dir,
            &names,
            &mut copies,
            |m| errors.push(m.to_vec()),
            || false,
        );
        assert!(ok && errors.is_empty());
        assert!(copies.is_empty());
        assert_eq!(std::fs::read(new.join("a.bin")).unwrap(), b"alpha");
        assert_eq!(std::fs::read(new.join("sub/b.bin")).unwrap(), b"beta");
        assert!(!old.exists(), "the emptied old directory is removed");
        let left: Vec<_> = std::fs::read_dir(&new)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left.len(), 2, "no copies are left: {left:?}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_source_changed_after_its_copy_is_moved_anew_and_the_copy_dropped() {
        let root = tmp("changed");
        let old = root.join("old");
        let new = root.join("new");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("a.bin"), b"first").unwrap();
        let j = job(&old, &new, &["a.bin"]);
        let mut copies = copy_files(&j, |_| true);
        let temp = copies[b"a.bin".as_slice()].temp.clone();
        std::fs::write(old.join("a.bin"), b"second, longer").unwrap();
        let names: Vec<&[u8]> = j.files.iter().map(Vec::as_slice).collect();
        assert!(article_writer_move_completed_files_precopied(
            &j.target,
            &j.old_dir,
            &names,
            &mut copies,
            |_| {},
            || false,
        ));
        assert_eq!(std::fs::read(new.join("a.bin")).unwrap(), b"second, longer");
        assert!(!temp.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_taken_name_gets_the_unique_name_a_plain_move_gives() {
        let root = tmp("unique");
        let old = root.join("old");
        let new = root.join("new");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("a.bin"), b"moved").unwrap();
        std::fs::write(new.join("a.bin"), b"there before").unwrap();
        let j = job(&old, &new, &["a.bin"]);
        let mut copies = copy_files(&j, |_| true);
        let names: Vec<&[u8]> = j.files.iter().map(Vec::as_slice).collect();
        assert!(article_writer_move_completed_files_precopied(
            &j.target,
            &j.old_dir,
            &names,
            &mut copies,
            |_| {},
            || false,
        ));
        assert_eq!(std::fs::read(new.join("a.bin")).unwrap(), b"there before");
        let unique = crate::fsops::make_unique_path(&b(&new.join("a.bin")));
        assert_ne!(unique, b(&new.join("a.bin")));
        let moved: Vec<_> = std::fs::read_dir(&new)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| std::fs::read(p).unwrap() == b"moved")
            .collect();
        assert_eq!(moved.len(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_failed_copy_fails_the_move_with_its_error_and_keeps_the_source() {
        let root = tmp("failed");
        let old = root.join("old");
        let new = root.join("new");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("a.bin"), b"kept").unwrap();
        let meta = std::fs::metadata(old.join("a.bin")).unwrap();
        let mut copies = HashMap::new();
        copies.insert(
            b"a.bin".to_vec(),
            PreCopied {
                temp: PathBuf::new(),
                err: libc::ENOSPC,
                len: meta.len(),
                mtime: (meta.mtime(), meta.mtime_nsec()),
                dev: meta.dev(),
                ino: meta.ino(),
            },
        );
        let j = job(&old, &new, &["a.bin"]);
        let names: Vec<&[u8]> = j.files.iter().map(Vec::as_slice).collect();
        let mut errors = Vec::new();
        assert!(article_writer_move_completed_files_precopied(
            &j.target,
            &j.old_dir,
            &names,
            &mut copies,
            |m| errors.push(String::from_utf8_lossy(m).into_owned()),
            || false,
        ));
        assert_eq!(errors.len(), 1);
        let want = format!(
            "Could not move file {} to {}: ",
            old.join("a.bin").display(),
            new.join("a.bin").display()
        );
        assert!(errors[0].starts_with(&want), "{}", errors[0]);
        assert!(old.join("a.bin").exists() && !new.join("a.bin").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn files_on_the_same_device_are_not_copied() {
        let root = tmp("same");
        let old = root.join("old");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("a.bin"), b"x").unwrap();
        let new = root.join("new");
        assert!(!crosses_devices(&b(&old), &b(&new), &[b"a.bin".to_vec()]));
        let dev = std::fs::metadata(&root).unwrap().dev();
        assert!(copy_files(&job(&old, &new, &["a.bin"]), |m| m.dev() != dev).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unused_dirs_of_a_move_that_did_not_happen_are_removed() {
        let root = tmp("dirs");
        let new = root.join("new");
        std::fs::create_dir_all(new.join("sub/deeper")).unwrap();
        remove_empty_dirs(&new, &[b"sub/deeper/a.bin".to_vec(), b"b.bin".to_vec()]);
        assert!(!new.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
