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
            if Arc::ptr_eq(&source.coordinator, &temp.coordinator) { None } else {
                let identity = WriterIdentity::Path(temp.root.spelling.join(leaf));
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
        let canonical = registry
            .physical
            .get(&root.identity)
            .and_then(Weak::upgrade)
            .filter(|owner| !owner.closed_and_quiescent().0);
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
            WriterIdentity::FileAndPath(id, _) => actual.parent() == expected.parent()
                && actual.file_name().is_some_and(|leaf| leaf.as_encoded_bytes() == format!("{id}.out.tmp").as_bytes()),
            _ => false,
        };
        let article_temp = self.temporary.as_ref().is_some_and(|temp| {
            actual.parent() == Some(temp.spelling.as_path()) && actual.file_name().is_some_and(|leaf| {
                let bytes = leaf.as_encoded_bytes();
                bytes == temp.basename.as_bytes() || bytes.strip_suffix(b".tmp") == Some(temp.basename.as_bytes())
            })
        });
        if actual != expected && !own_output && !article_temp {
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
        self.coordinator
            .writer_in_epoch(&self.identity, self.epoch, true)
    }
    /// Run only filesystem work here; queue callbacks belong after the permit drops.
    pub fn perform_io<T>(&self, action: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T> {
        let _permit = self.permit().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "live repair owns target",
            )
        })?;
        let result = action();
        if result.is_err() {
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
    pub fn invalidate(&self) {
        self.coordinator.invalidate_sources();
        self.cancel_live_sessions();
    }
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
    })
}

pub(crate) fn forget_download_writes(nzb_id: i32) {
    let coordinator = WRITES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .downloads
        .remove(&nzb_id)
        .and_then(|(_, owner)| owner.upgrade());
    if let Some(coordinator) = coordinator {
        coordinator.close();
    }
}
