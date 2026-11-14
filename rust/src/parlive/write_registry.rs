//! Keep writers that started before metadata in the same download/root barrier.
use super::write_control::{SourceWritePermit, WriteBlocked};
use super::{SessionGeneration, WriteCoordinator, WriterIdentity};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};

#[derive(Default)]
struct WriteRegistry {
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
}
impl SourceWriteContext {
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
        }
    }
    pub fn try_permit(&self) -> Result<SourceWritePermit, WriteBlocked> {
        self.coordinator
            .writer_in_epoch(&self.identity, self.epoch, false)
    }
    pub fn invalidate(&self) {
        self.coordinator.invalidate_sources();
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
