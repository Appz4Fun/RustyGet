//! Keep writers that started before metadata in the same download/root barrier.
use super::source_identity::{blocked, refused, FileIdentity, RootIdentity, SourceRoot};
use super::write_control::{SourceWritePermit, WriteBlocked};
use super::{SessionGeneration, WriteCoordinator, WriterIdentity};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};

#[derive(Default)]
struct WriteRegistry {
    physical: HashMap<RootIdentity, Weak<WriteCoordinator>>,
    roots: HashMap<PathBuf, Weak<WriteCoordinator>>,
    downloads: HashMap<i32, (PathBuf, Weak<WriteCoordinator>)>,
}
static WRITES: LazyLock<Mutex<WriteRegistry>> =
    LazyLock::new(|| Mutex::new(WriteRegistry::default()));

#[derive(Clone)]
pub(crate) struct SourceWriteContext {
    coordinator: Arc<WriteCoordinator>,
    identity: WriterIdentity,
    epoch: u64,
    job: i32,
    root: PathBuf,
    opened_leaf: Option<Arc<std::ffi::OsStr>>,
    temporary: Option<Arc<ArticleTemporary>>,
    opened_temporary: bool,
    foreign_writer: bool,
}
struct ArticleTemporary {
    root: Arc<SourceRoot>,
    spelling: PathBuf,
    coordinator: Arc<WriteCoordinator>,
    epoch: u64,
    basename: String,
}
pub(crate) struct AttestedWritePermit {
    _source: SourceWritePermit,
    _temporary: Option<SourceWritePermit>,
}
impl SourceWriteContext {
    pub fn with_article_temporary(&self, root: &Path, article: i32) -> std::io::Result<Self> {
        let WriterIdentity::FileAndPath(id, _) = &self.identity else { return Err(refused()); };
        if article <= 0 { return Err(refused()); }
        let basename = format!("{id}.{article:03}");
        let target = root.join(&basename);
        let candidate = donor_write_context(root, &target).map_err(blocked)?;
        let (candidate, physical) = candidate.attested_root()?;
        let mut result = self.clone();
        result.temporary = Some(Arc::new(ArticleTemporary { root: physical, spelling: root.to_owned(),
            coordinator: candidate.coordinator, epoch: candidate.epoch, basename }));
        result.opened_temporary = false;
        Ok(result)
    }
    fn attested_permit(&self, source: &SourceWriteContext, leaf: &std::ffi::OsStr, temporary: bool) -> std::io::Result<AttestedWritePermit> {
        let source_permit = source.permit().map_err(blocked)?;
        let second = if temporary {
            let temp = self.temporary.as_ref().ok_or_else(refused)?;
            let current = SourceRoot::open(&temp.spelling, self.job)?;
            if current.identity != temp.root.identity { return Err(refused()); }
            let physical_path = temp.root.spelling.join(leaf);
            if temp.coordinator.foreign_source_conflicts(&physical_path, false) { return Err(refused()); }
            if Arc::ptr_eq(&source.coordinator, &temp.coordinator) { None } else {
                let identity = WriterIdentity::Path(physical_path);
                Some(temp.coordinator.writer_in_epoch(&identity, temp.epoch, false).map_err(blocked)?)
            }
        } else { None };
        Ok(AttestedWritePermit { _source: source_permit, _temporary: second })
    }
    /// Open/fstat without a data read or destructive operation, outside queue.
    pub fn attest_existing_source(&self) -> std::io::Result<()> {
        let (context, root) = self.attested_root()?;
        let path = context.identity_path()?;
        match root.open_leaf(path.file_name().ok_or_else(refused)?, false, false) {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                context.invalidate_identity();
                Err(error)
            }
        }
    }
    fn identity_path(&self) -> std::io::Result<&Path> {
        match &self.identity {
            WriterIdentity::Path(path) | WriterIdentity::FileAndPath(_, path) => Ok(path),
            #[cfg(test)]
            WriterIdentity::File(_) => Err(refused()),
        }
    }
    fn attested_root(&self) -> std::io::Result<(Self, Arc<SourceRoot>)> {
        if self.epoch != self.coordinator.writer_epoch()
            || self.coordinator.closed_and_quiescent().0
        {
            return Err(refused());
        }
        let root = Arc::new(SourceRoot::open(&self.root, self.job)?);
        let mut registry = WRITES.lock().unwrap_or_else(|e| e.into_inner());
        registry.physical.retain(|_, weak| weak.strong_count() != 0);
        let canonical = match registry.physical.get(&root.identity).and_then(Weak::upgrade) {
            Some(owner) => match owner.closed_and_quiescent() {
                (true, false) => return Err(blocked(WriteBlocked::Busy)),
                (true, true) => None,
                (false, _) => Some(owner),
            },
            None => None,
        };
        let foreign = self.job != 0 && canonical.as_ref().is_some_and(|owner| {
            registry.downloads.iter().any(|(&job, (_, weak))| job != self.job
                && weak.upgrade().is_some_and(|other| Arc::ptr_eq(&other, owner)))
        });
        if foreign {
            let owner = canonical.as_ref().ok_or_else(refused)?;
            let retained = owner.physical_root().ok_or_else(refused)?;
            let path = retained.spelling.join(self.identity_path()?.file_name().ok_or_else(refused)?);
            if owner.foreign_source_conflicts(&path, !Arc::ptr_eq(owner, &self.coordinator)) { return Err(refused()); }
        }
        let owner = match canonical {
            Some(owner) if !Arc::ptr_eq(&owner, &self.coordinator) => {
                if !self.coordinator.provisional_and_quiescent() {
                    self.coordinator.invalidate_sources();
                    owner.invalidate_sources();
                    return Err(refused());
                }
                for weak in registry.roots.values_mut() {
                    if weak
                        .upgrade()
                        .is_some_and(|old| Arc::ptr_eq(&old, &self.coordinator))
                    {
                        *weak = Arc::downgrade(&owner);
                    }
                }
                for (_, weak) in registry.downloads.values_mut() {
                    if weak
                        .upgrade()
                        .is_some_and(|old| Arc::ptr_eq(&old, &self.coordinator))
                    {
                        *weak = Arc::downgrade(&owner);
                    }
                }
                owner
            }
            Some(owner) => owner,
            None => {
                self.coordinator
                    .bind_physical_root(Arc::clone(&root))
                    .map_err(blocked)?;
                registry
                    .physical
                    .insert(root.identity, Arc::downgrade(&self.coordinator));
                Arc::clone(&self.coordinator)
            }
        };
        let retained = owner.physical_root().ok_or_else(refused)?;
        if retained.identity != root.identity {
            return Err(refused());
        }
        let mut context = self.clone();
        context.epoch = owner.writer_epoch();
        context.coordinator = owner;
        context.foreign_writer = foreign;
        Ok((context, retained))
    }
    /// Validate the actual target before open, then the descriptor before any
    /// truncate or write. The caller retains the permit through initial resize.
    pub fn open_attested(
        &self,
        actual: &Path,
        create: bool,
    ) -> std::io::Result<(std::fs::File, Self, AttestedWritePermit)> {
        let expected = self.identity_path()?;
        let own_output = match &self.identity {
            // `<id>.out.tmp` is written in the download's root, also for a file named into a
            // subfolder (DirectRename of a file in progress to a par2's "Sub/name").
            WriterIdentity::FileAndPath(id, _) => actual.parent() == Some(self.root.as_path())
                && actual.file_name().is_some_and(|leaf| leaf.as_encoded_bytes() == format!("{id}.out.tmp").as_bytes()),
            _ => false,
        };
        let article_temp = self.temporary.as_ref().is_some_and(|temp| {
            actual.parent() == Some(temp.spelling.as_path()) && actual.file_name().is_some_and(|leaf| {
                let bytes = leaf.as_encoded_bytes();
                bytes == temp.basename.as_bytes() || bytes.strip_suffix(b".tmp") == Some(temp.basename.as_bytes())
            })
        });
        let assembly = actual.parent() == expected.parent()
            && matches!((expected.file_name(), actual.file_name()), (Some(e), Some(a))
                if assembly_output_of(e.as_encoded_bytes(), a.as_encoded_bytes()));
        if actual != expected && !own_output && !article_temp && !assembly {
            self.invalidate_identity();
            return Err(refused());
        }
        let (mut context, root) = self.attested_root()?;
        let leaf = actual.file_name().ok_or_else(refused)?;
        let permit = self.attested_permit(&context, leaf, article_temp)?;
        let actual_root = if article_temp { &self.temporary.as_ref().ok_or_else(refused)?.root } else { &root };
        let file = actual_root.open_leaf(leaf, true, create);
        if file.is_err() {
            context.invalidate_identity();
        }
        context.opened_leaf = actual.file_name().map(Arc::from);
        context.opened_temporary = article_temp;
        Ok((file?, context, permit))
    }
    pub fn descriptor_permit(&self, file: &std::fs::File) -> std::io::Result<AttestedWritePermit> {
        let (context, root) = self.attested_root()?;
        let leaf = self.opened_leaf.as_deref().or_else(|| self.identity_path().ok()?.file_name()).ok_or_else(refused)?;
        let permit = self.attested_permit(&context, leaf, self.opened_temporary)?;
        let actual_root = if self.opened_temporary { &self.temporary.as_ref().ok_or_else(refused)?.root } else { &root };
        let actual = actual_root.open_leaf(
            leaf,
            false,
            false,
        )?;
        if FileIdentity::of(file)? != FileIdentity::of(&actual)? {
            context.invalidate_identity();
            return Err(refused());
        }
        Ok(permit)
    }
    pub fn permit(&self) -> Result<SourceWritePermit, WriteBlocked> {
        if self.foreign_writer {
            let path = self.identity_path().map_err(|_| WriteBlocked::InvalidTarget)?;
            let root = self.coordinator.physical_root().ok_or(WriteBlocked::InvalidTarget)?;
            let leaf = path.file_name().ok_or(WriteBlocked::InvalidTarget)?;
            self.coordinator.writer_in_epoch(&WriterIdentity::Path(root.spelling.join(leaf)), self.epoch, true)
        } else {
            self.coordinator.writer_in_epoch(&self.identity, self.epoch, true)
        }
    }
    /// Run only filesystem work here; queue callbacks belong after the permit drops.
    /// A failure that found nothing (a file or folder already gone) changed nothing on disk
    /// and leaves live PAR's sources valid.
    #[track_caller]
    pub fn perform_io<T>(&self, action: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T> {
        // No target naming lock here: the permit already excludes a live repair admission
        // (its write barrier drains permits and grants none while up), and taking the lock
        // before waiting for a permit would invert the admission's barrier-then-lock order.
        let _permit = self.permit().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "live repair owns target",
            )
        })?;
        let result = action();
        if result
            .as_ref()
            .is_err_and(|e| e.kind() != std::io::ErrorKind::NotFound)
        {
            self.invalidate();
        }
        result
    }
    pub fn for_path(&self, path: &Path) -> Self {
        Self {
            coordinator: Arc::clone(&self.coordinator),
            identity: WriterIdentity::Path(path.to_path_buf()),
            epoch: self.epoch,
            job: self.job,
            root: self.root.clone(),
            opened_leaf: None,
            temporary: None,
            opened_temporary: false,
            foreign_writer: false,
        }
    }
    pub fn try_permit(&self) -> Result<SourceWritePermit, WriteBlocked> {
        self.coordinator
            .writer_in_epoch(&self.identity, self.epoch, false)
    }
    /// Independent replacement invalidates this cohort even before metadata.
    pub fn invalidate_identity(&self) -> bool {
        let changed = self
            .coordinator
            .invalidate_source_identity(&self.identity, self.epoch);
        if changed {
            self.cancel_live_sessions();
        }
        changed
    }
    /// Callers are tracked so live PAR's stop line names the write that failed, not this.
    #[track_caller]
    pub fn invalidate(&self) {
        self.coordinator.invalidate_sources();
        self.cancel_live_sessions();
    }
    /// Whether a live PAR session reads the download this context writes into.
    pub fn watched_by_live_par(&self) -> bool {
        self.live_jobs()
            .into_iter()
            .any(crate::parchecker::streaming_session_has_session)
    }
    /// A rewrite of `size` bytes at `offset`, already on disk, that live PAR takes in by
    /// re-verifying the slices it touches: true when every live session watching this
    /// download reconciled it. False when one could not, or this context names no file; the
    /// caller then invalidates the identity as before.
    pub fn range_replaced(&self, offset: u64, size: u64) -> bool {
        #[allow(unreachable_patterns)]
        let path = match &self.identity {
            WriterIdentity::Path(path) | WriterIdentity::FileAndPath(_, path) => path,
            _ => return false,
        };
        let Some(name) = path.file_name() else {
            return false;
        };
        let jobs = self.live_jobs();
        !jobs.is_empty()
            && jobs.into_iter().all(|id| {
                crate::parchecker::streaming_session_on_range_replaced_by_name(
                    id,
                    name.as_encoded_bytes(),
                    offset,
                    size,
                ) != Some(false)
            })
    }
    fn live_jobs(&self) -> Vec<i32> {
        WRITES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .downloads
            .iter()
            .filter_map(|(&id, (_, owner))| {
                owner
                    .upgrade()
                    .filter(|owner| Arc::ptr_eq(owner, &self.coordinator))
                    .map(|_| id)
            })
            .collect()
    }
    #[track_caller]
    fn cancel_live_sessions(&self) {
        let owners: Vec<_> = WRITES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .downloads
            .iter()
            .filter_map(|(&id, (_, owner))| {
                owner
                    .upgrade()
                    .filter(|owner| Arc::ptr_eq(owner, &self.coordinator))
                    .map(|_| id)
            })
            .collect();
        for id in owners {
            crate::parchecker::streaming_session_cancel(id);
        }
    }
}

/// Completion with DirectWrite off assembles into `name.tmp`, and a name
/// collision makes it `make_unique_filename`'s `stem.duplicateN.ext`. Those are
/// this queue file's own outputs, still subject to its certified identity.
fn assembly_output_of(expected: &[u8], actual: &[u8]) -> bool {
    let actual = actual.strip_suffix(b".tmp").unwrap_or(actual);
    if actual == expected {
        return true;
    }
    let digits = |middle: &[u8]| !middle.is_empty() && middle.iter().all(u8::is_ascii_digit);
    let mut stem = expected;
    let mut ext: &[u8] = b"";
    if let Some(at) = expected.iter().rposition(|&b| b == b'.').filter(|&at| at > 0) {
        stem = &expected[..at];
        ext = &expected[at..];
        if ext.eq_ignore_ascii_case(b".par2") {
            if let Some(vol) = stem.iter().rposition(|&b| b == b'.').filter(|&v| v > 0) {
                if stem[vol..].len() >= 4 && stem[vol..vol + 4].eq_ignore_ascii_case(b".vol") {
                    ext = &expected[vol..];
                    stem = &expected[..vol];
                }
            }
        }
    }
    actual
        .strip_prefix(stem)
        .and_then(|rest| rest.strip_prefix(b".duplicate"))
        .and_then(|rest| rest.strip_suffix(ext))
        .is_some_and(digits)
}

#[cfg(test)]
#[test]
fn assembly_outputs_match_only_this_files_names() {
    assert!(assembly_output_of(b"foo.rar", b"foo.rar.tmp"));
    assert!(assembly_output_of(b"foo.rar", b"foo.duplicate1.rar"));
    assert!(assembly_output_of(b"foo.rar", b"foo.duplicate12.rar.tmp"));
    assert!(assembly_output_of(b"a.vol0+1.par2", b"a.duplicate2.vol0+1.par2"));
    assert!(assembly_output_of(b"README", b"README.duplicate3"));
    assert!(!assembly_output_of(b"foo.rar", b"bar.rar"));
    assert!(!assembly_output_of(b"foo.rar", b"foo.duplicate.rar"));
    assert!(!assembly_output_of(b"foo.rar", b"foo.duplicate1x.rar"));
    assert!(!assembly_output_of(b"foo.rar", b"foo.r00"));
}

fn root_coordinator(
    registry: &mut WriteRegistry,
    root: &Path,
) -> Result<Arc<WriteCoordinator>, WriteBlocked> {
    if let Some(old) = registry.roots.get(root).and_then(Weak::upgrade) {
        let (closed, quiescent) = old.closed_and_quiescent();
        if !closed {
            return Ok(old);
        }
        if !quiescent {
            return Err(WriteBlocked::Busy);
        }
    }
    let coordinator = WriteCoordinator::new(SessionGeneration(0));
    registry
        .roots
        .insert(root.to_path_buf(), Arc::downgrade(&coordinator));
    Ok(coordinator)
}

pub(crate) fn download_writes(
    nzb_id: i32,
    root: &Path,
) -> Result<Arc<WriteCoordinator>, WriteBlocked> {
    let mut registry = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    registry
        .downloads
        .retain(|_, (_, owner)| owner.strong_count() != 0);
    registry.roots.retain(|_, owner| owner.strong_count() != 0);
    if let Some((old_root, owner)) = registry.downloads.get(&nzb_id) {
        if old_root != root
            && owner
                .upgrade()
                .is_some_and(|owner| !owner.closed_and_quiescent().0)
        {
            return Err(WriteBlocked::InvalidTarget);
        }
    }
    let coordinator = root_coordinator(&mut registry, root)?;
    registry
        .downloads
        .insert(nzb_id, (root.to_path_buf(), Arc::downgrade(&coordinator)));
    Ok(coordinator)
}

pub(crate) fn source_write_context(
    nzb_id: i32,
    file_id: i32,
    root: &Path,
    name: &[u8],
) -> Result<SourceWriteContext, WriteBlocked> {
    let coordinator = download_writes(nzb_id, root)?;
    let path = root.join(crate::fsops::path(name));
    Ok(SourceWriteContext {
        epoch: coordinator.writer_epoch(),
        coordinator,
        identity: WriterIdentity::FileAndPath(file_id, path),
        job: nzb_id,
        root: root.to_owned(),
        opened_leaf: None,
        temporary: None,
        opened_temporary: false,
        foreign_writer: false,
    })
}

pub(crate) fn donor_write_context(
    root: &Path,
    path: &Path,
) -> Result<SourceWriteContext, WriteBlocked> {
    let mut registry = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    registry.roots.retain(|_, owner| owner.strong_count() != 0);
    let coordinator = root_coordinator(&mut registry, root)?;
    Ok(SourceWriteContext {
        epoch: coordinator.writer_epoch(),
        coordinator,
        identity: WriterIdentity::Path(path.to_path_buf()),
        job: 0,
        root: root.to_owned(),
        opened_leaf: None,
        temporary: None,
        opened_temporary: false,
        foreign_writer: false,
    })
}

/// Open a live repair target through the attested physical root, without
/// following links. The root must already be bound by an attested source
/// writer; otherwise ownership is unproven and live repair is refused.
pub(crate) fn open_repair_target(
    coordinator: &WriteCoordinator,
    path: &Path,
) -> std::io::Result<std::fs::File> {
    let root = coordinator.physical_root().ok_or_else(refused)?;
    if path.parent() != Some(root.spelling.as_path()) {
        return Err(refused());
    }
    root.open_leaf(path.file_name().ok_or_else(refused)?, true, false)
}

fn repair_root(
    coordinator: &WriteCoordinator,
    path: &Path,
) -> std::io::Result<std::sync::Arc<super::source_identity::SourceRoot>> {
    let root = coordinator.physical_root().ok_or_else(refused)?;
    if path.parent() != Some(root.spelling.as_path()) {
        return Err(refused());
    }
    Ok(root)
}

/// Creates a new empty file for missing repair target `path` under a temporary name in
/// the same folder; [`link_repair_temp`] gives it the target's name later.
pub(crate) fn create_repair_temp(
    coordinator: &WriteCoordinator,
    path: &Path,
) -> std::io::Result<(std::fs::File, std::ffi::CString)> {
    repair_root(coordinator, path)?.create_temp_exclusive()
}

/// Per-download lock around naming live repair targets (and rolling them back) and around
/// the renames and hard links in the download's folder that take no write permit: the
/// direct renamer's (`rename_file_on_disk`, `try_create_hard_link_host`). A file moved onto
/// a target name can never land between a rollback's identity check and its unlink. Moves
/// through `SourceWriteContext::perform_io` are excluded by their permit instead (an
/// admission's write barrier grants none), so they never take this lock. An admission
/// holding it takes no queue lock, cache flush lock or permit; the direct renamer may hold
/// the queue lock while it waits for it.
pub(crate) fn target_naming_lock(job: i32) -> Arc<std::sync::Mutex<()>> {
    static LOCKS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<i32, std::sync::Weak<std::sync::Mutex<()>>>>,
    > = std::sync::LazyLock::new(Default::default);
    let mut locks = LOCKS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(lock) = locks.get(&job).and_then(std::sync::Weak::upgrade) {
        return lock;
    }
    locks.retain(|_, lock| lock.strong_count() > 0);
    let lock = Arc::new(std::sync::Mutex::new(()));
    locks.insert(job, Arc::downgrade(&lock));
    lock
}

/// Runs `action` (a rename or link in download `job`'s folder) under its target naming lock.
pub(crate) fn with_target_naming<T>(job: i32, action: impl FnOnce() -> T) -> T {
    let lock = target_naming_lock(job);
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    action()
}

/// What an error of [`name_repair_temp`] left behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NamingLeft {
    /// The temporary file, under its temporary name only.
    Temp,
    /// The target name points at the temporary file (linked; its temporary name stayed).
    Named,
    /// Nothing: the temporary name is already gone.
    Nothing,
}

/// How a missing repair target got its name.
#[derive(Debug)]
pub(crate) enum TargetNaming {
    /// The temporary file was hard-linked to the name, and its temporary name dropped.
    Linked,
    /// The temporary file was renamed to the name without replacing anything.
    Renamed,
    /// The filesystem can neither (exFAT, FAT, smbfs): the temporary file was dropped and
    /// the name created directly; this is the file to use.
    Created(std::fs::File),
}

/// Whether the name of `path` is free in the download folder.
pub(crate) fn repair_target_name_is_free(
    coordinator: &WriteCoordinator,
    path: &Path,
) -> std::io::Result<bool> {
    repair_root(coordinator, path)?.leaf_is_free(path.file_name().ok_or_else(refused)?)
}

/// The download folder is still the one live PAR bound (not moved or replaced).
pub(crate) fn repair_root_is_current(
    coordinator: &WriteCoordinator,
    path: &Path,
) -> std::io::Result<()> {
    repair_root(coordinator, path)?.is_current()
}

/// Gives the temporary file the name of `path`, never replacing anything: a hard link
/// (then its temporary name is dropped), else a no-replace rename, else, where the
/// filesystem supports neither, a direct exclusive create of the name (the temporary file
/// is dropped first). On an error after the name was given, the name is still the file
/// (`Linked` with its temporary name left); the caller removes the name only while it still
/// points at that file.
pub(crate) fn name_repair_temp(
    coordinator: &WriteCoordinator,
    temp: &std::ffi::CStr,
    path: &Path,
) -> Result<TargetNaming, (std::io::Error, NamingLeft)> {
    let root = repair_root(coordinator, path).map_err(|e| (e, NamingLeft::Temp))?;
    let leaf = path.file_name().ok_or_else(|| (refused(), NamingLeft::Temp))?;
    match root.link_temp(temp, leaf) {
        Ok(()) => {
            return root
                .unlink_temp(temp)
                .map(|()| TargetNaming::Linked)
                .map_err(|e| (e, NamingLeft::Named));
        }
        Err(error) if !super::source_identity::unsupported_link(&error) => {
            return Err((error, NamingLeft::Temp));
        }
        Err(_) => {}
    }
    match root.rename_temp_noreplace(temp, leaf) {
        Ok(()) => return Ok(TargetNaming::Renamed),
        Err(error) if !super::source_identity::unsupported_link(&error) => {
            return Err((error, NamingLeft::Temp));
        }
        Err(_) => {}
    }
    root.unlink_temp(temp).map_err(|e| (e, NamingLeft::Temp))?;
    // The temporary name is gone from here on: nothing for the caller to unlink.
    root.create_leaf_exclusive(leaf)
        .map(TargetNaming::Created)
        .map_err(|e| (e, NamingLeft::Nothing))
}

/// Removes repair target `path` only while its name still points at `file` (a name this
/// admission gave); anything else under the name is left alone.
pub(crate) fn remove_named_repair_target(
    coordinator: &WriteCoordinator,
    path: &Path,
    file: &std::fs::File,
) -> std::io::Result<bool> {
    let root = repair_root(coordinator, path)?;
    let leaf = std::ffi::CString::new(path.file_name().ok_or_else(refused)?.as_encoded_bytes())
        .map_err(|_| refused())?;
    root.unlink_leaf_if_same(&leaf, file)
}

/// Removes temporary repair files an earlier daemon process left in `dir` (see
/// `SourceRoot::remove_stale_repair_temps`).
pub(crate) fn remove_stale_repair_temps(dir: &Path) -> usize {
    super::source_identity::remove_stale_repair_temps_in(dir, None)
}

/// Removes a temporary repair file by its temporary name only.
pub(crate) fn unlink_repair_temp(
    coordinator: &WriteCoordinator,
    temp: &std::ffi::CStr,
    path: &Path,
) -> std::io::Result<()> {
    repair_root(coordinator, path)?.unlink_temp(temp)
}

/// The target path must still name the descriptor admitted for repair.
pub(crate) fn repair_target_is_current(
    coordinator: &WriteCoordinator,
    path: &Path,
    file: &std::fs::File,
) -> std::io::Result<()> {
    let root = coordinator.physical_root().ok_or_else(refused)?;
    if path.parent() != Some(root.spelling.as_path()) {
        return Err(refused());
    }
    let current = root.open_leaf(path.file_name().ok_or_else(refused)?, false, false)?;
    if FileIdentity::of(file)? != FileIdentity::of(&current)? {
        coordinator.invalidate_sources();
        return Err(refused());
    }
    Ok(())
}

pub(crate) fn forget_download_writes(nzb_id: i32) {
    let mut registry = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    let coordinator = registry.downloads
        .remove(&nzb_id)
        .and_then(|(_, owner)| owner.upgrade());
    if let Some(coordinator) = coordinator {
        let still_owned = registry.downloads.values().any(|(_, weak)| {
            weak.upgrade().is_some_and(|other| Arc::ptr_eq(&other, &coordinator))
        });
        // Keep the lookup and close atomic: a second job must not join the
        // coordinator between the last-owner check and closure.
        if !still_owned { coordinator.close(); }
    }
}
