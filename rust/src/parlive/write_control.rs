//! Cooperative ownership of actual source writes and live target patching.
use super::{ControlLease, SessionGeneration};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WriterTarget {
    pub file_id: i32,
    pub path: PathBuf,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WriterIdentity {
    #[cfg(test)]
    File(i32),
    Path(PathBuf),
    FileAndPath(i32, PathBuf),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WriteBlocked {
    Busy,
    Cancelled,
    Closed,
    StaleGeneration,
    Certified,
    InvalidTarget,
}

struct CertifiedTargets {
    generation: SessionGeneration,
    targets: Vec<WriterTarget>,
    crcs: Vec<(i32, u32)>,
    /// Each target's file as published, so a CRC still holds for it after a rename
    /// (post-processing's par-rename gives an obfuscated target its par2 name).
    stamps: Vec<(i32, FileStamp)>,
    _reservation: Arc<ControlLease>,
}

/// A file as it was when its CRC was certified: the same inode, length and modification
/// time mean the same bytes, under whatever name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileStamp {
    dev: u64,
    ino: u64,
    len: u64,
    mtime_ns: i128,
}

impl FileStamp {
    #[cfg(unix)]
    pub(crate) fn of(path: &std::path::Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(path).ok()?;
        meta.is_file().then(|| Self {
            dev: meta.dev(),
            ino: meta.ino(),
            len: meta.len(),
            mtime_ns: i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec()),
        })
    }
    #[cfg(not(unix))]
    pub(crate) fn of(_path: &std::path::Path) -> Option<Self> {
        None
    }
}
struct WriteState {
    physical_root: Option<Arc<super::source_identity::SourceRoot>>,
    generation: SessionGeneration,
    writer_epoch: u64,
    cancelled: bool,
    source_identity_changed: bool,
    closed: bool,
    writers: usize,
    // Leaf names of active writers (with counts) for cross-job alias checks.
    writer_leaves: Vec<(std::ffi::OsString, usize)>,
    // A pending barrier is installed before waiting, so new writers cannot starve it.
    repair: Option<(SessionGeneration, bool)>,
    certified: Vec<CertifiedTargets>,
    namespace: Option<crate::parchecker::live_namespace::NamespaceProof>,
    /// Why the cohort was invalidated, first reason only, for the log.
    stop_reason: Option<String>,
}

pub(crate) struct WriteCoordinator {
    state: Mutex<WriteState>,
    changed: Condvar,
}
#[cfg(test)]
pub(crate) struct WriterStatus {
    pub active_writers: usize,
    pub pending_repair: bool,
}
pub(crate) struct SourceWritePermit {
    coordinator: Arc<WriteCoordinator>,
    leaf: Option<std::ffi::OsString>,
}
// Error paths must drop owned target metadata before returning its reservation.
struct TargetRequest {
    targets: Vec<WriterTarget>,
    reservation: Arc<ControlLease>,
}

pub(crate) struct RepairWriteLease {
    coordinator: Arc<WriteCoordinator>,
    generation: SessionGeneration,
    targets: Vec<WriterTarget>,
    active: bool,
    reservation: Option<Arc<ControlLease>>,
}

impl WriteCoordinator {
    /// `cross_job`: another download reaches this physical root. Its write
    /// conflicts with any active pre-index writer, since leaves are unproven.
    /// A job's own scratch temporaries only conflict with live state.
    pub(super) fn foreign_source_conflicts(&self, path: &std::path::Path, cross_job: bool) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let identity = WriterIdentity::Path(path.to_owned());
        if Self::is_certified(&state, &identity) { return true; }
        if state.generation.0 == 0 && state.repair.is_none() {
            // Before live metadata only a concurrent write of the same file
            // through another job's alias conflicts; unrelated files proceed.
            let same_file = cross_job
                && state.writer_leaves.iter().any(|(leaf, _)| {
                    Self::paths_may_alias(std::path::Path::new(leaf), path.file_name().map_or(path, std::path::Path::new))
                });
            if !same_file {
                return false;
            }
        }
        let protected = state.namespace.as_ref().is_none_or(|proof| {
            path.file_name().is_none_or(|leaf| proof.protects_leaf(leaf.as_encoded_bytes()))
        });
        if protected || state.repair.is_some() {
            Self::stop_because(&mut state, || format!("another write reached source path {}", path.display()));
            state.source_identity_changed = true;
            state.cancelled = true;
            self.changed.notify_all();
            return true;
        }
        false
    }
    pub fn new(generation: SessionGeneration) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(WriteState {
                physical_root: None,
                generation,
                writer_epoch: 0,
                cancelled: false,
                source_identity_changed: false,
                closed: false,
                writers: 0,
                writer_leaves: Vec::new(),
                repair: None,
                certified: Vec::new(),
                namespace: None,
                stop_reason: None,
            }),
            changed: Condvar::new(),
        })
    }
    pub(super) fn closed_and_quiescent(&self) -> (bool, bool) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.closed, state.repair.is_none() && state.writers == 0)
    }
    pub(crate) fn has_physical_root(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .physical_root
            .is_some()
    }
    pub(super) fn physical_root(&self) -> Option<Arc<super::source_identity::SourceRoot>> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .physical_root
            .clone()
    }
    pub(super) fn bind_physical_root(
        &self,
        root: Arc<super::source_identity::SourceRoot>,
    ) -> Result<(), WriteBlocked> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed
            || state
                .physical_root
                .as_ref()
                .is_some_and(|old| old.identity != root.identity)
        {
            return Err(WriteBlocked::InvalidTarget);
        }
        if state.physical_root.is_none() {
            state.physical_root = Some(root);
        }
        Ok(())
    }
    pub(super) fn provisional_and_quiescent(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        !state.closed
            && !state.cancelled
            && !state.source_identity_changed
            && state.generation.0 == 0
            && state.writers == 0
            && state.repair.is_none()
            && state.certified.is_empty()
            && state.namespace.is_none()
    }
    #[cfg(test)]
    pub fn status(&self) -> WriterStatus {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        WriterStatus {
            active_writers: state.writers,
            pending_repair: state.repair.is_some_and(|(_, ready)| !ready),
        }
    }
    #[cfg(test)]
    pub fn advance_generation(&self, generation: SessionGeneration) -> Result<(), WriteBlocked> {
        self.bind_generation(generation, || ())
    }

    /// Bind registry membership atomically with the generation/cleanup fence.
    /// The callback may update the session registry, but must not reacquire this gate.
    pub fn bind_generation<T>(
        &self,
        generation: SessionGeneration,
        bind: impl FnOnce() -> T,
    ) -> Result<T, WriteBlocked> {
        self.bind_generation_inner(generation, None, bind)
    }

    pub fn bind_namespace_generation<T>(
        &self,
        generation: SessionGeneration,
        namespace: crate::parchecker::live_namespace::NamespaceProof,
        bind: impl FnOnce() -> T,
    ) -> Result<T, WriteBlocked> {
        self.bind_generation_inner(generation, Some(namespace), bind)
    }

    fn bind_generation_inner<T>(
        &self,
        generation: SessionGeneration,
        namespace: Option<crate::parchecker::live_namespace::NamespaceProof>,
        bind: impl FnOnce() -> T,
    ) -> Result<T, WriteBlocked> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return Err(WriteBlocked::Closed);
        }
        if state.source_identity_changed {
            return Err(WriteBlocked::Cancelled);
        }
        if generation.0 < state.generation.0 {
            return Err(WriteBlocked::StaleGeneration);
        }
        if generation == state.generation && state.cancelled {
            return Err(WriteBlocked::Cancelled);
        }
        if generation != state.generation {
            if state.generation.0 != 0 {
                state.writer_epoch = state
                    .writer_epoch
                    .checked_add(1)
                    .ok_or(WriteBlocked::StaleGeneration)?;
            }
            state.generation = generation;
            state.cancelled = false;
            state.stop_reason = None;
            // Published bytes may already be in use by unpack. Their seals and
            // metadata charge survive live-generation replacement until close.
        }
        state.namespace = namespace;
        let result = bind();
        self.changed.notify_all();
        Ok(result)
    }

    /// Outer None is the explicit legacy API; Some(None) is an unbound row.
    pub fn namespace_source_id(
        &self,
        generation: SessionGeneration,
        queue_id: Option<i32>,
        name: &[u8],
        hint: &[u8],
    ) -> Result<Option<Option<par_rs::packet::FileId>>, WriteBlocked> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed || state.cancelled || state.generation != generation {
            return Err(WriteBlocked::StaleGeneration);
        }
        let Some(proof) = state.namespace.as_mut() else {
            return Ok(None);
        };
        let resolved = match queue_id {
            None => Err("no queue id".to_owned()),
            Some(id) => proof.bind(id, name, hint).map_err(|error| {
                format!(
                    "live namespace refused queue file {id} ({}): {error}",
                    String::from_utf8_lossy(name)
                )
            }),
        };
        match resolved {
            Ok(id) => Ok(Some(id)),
            Err(why) => {
                let error = WriteBlocked::InvalidTarget;
                Self::stop_because(&mut state, || why);
                state.source_identity_changed = true;
                state.cancelled = true;
                self.changed.notify_all();
                Err(error)
            }
        }
    }
    fn stop_because(state: &mut WriteState, why: impl FnOnce() -> String) {
        if !state.cancelled && state.stop_reason.is_none() {
            state.stop_reason = Some(why());
        }
    }
    /// Why this cohort stopped, when something invalidated it.
    pub fn stop_reason(&self) -> Option<String> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stop_reason.clone()
    }
    pub fn is_current_generation(&self, generation: SessionGeneration) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        !state.closed && !state.cancelled && state.generation == generation
    }
    pub fn certified_crc(
        &self,
        generation: SessionGeneration,
        id: i32,
        path: &std::path::Path,
    ) -> Option<u32> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed || state.cancelled || state.generation != generation {
            return None;
        }
        state.certified.iter().find_map(|set| {
            if set.generation != generation {
                return None;
            }
            // Under its certified path, or renamed since: the very file as published.
            let renamed = || {
                set.stamps
                    .iter()
                    .find(|(file_id, _)| *file_id == id)
                    .is_some_and(|(_, stamp)| FileStamp::of(path) == Some(*stamp))
            };
            set.targets
                .iter()
                .any(|target| target.file_id == id && (target.path == path || renamed()))
                .then(|| {
                    set.crcs
                        .iter()
                        .find(|(file_id, _)| *file_id == id)
                        .map(|(_, crc)| *crc)
                })
                .flatten()
        })
    }
    pub fn cancel_generation(&self, generation: SessionGeneration) {
        self.cancel_generation_with(generation, || ());
    }
    pub fn cancel_generation_with<T>(
        &self,
        generation: SessionGeneration,
        action: impl FnOnce() -> T,
    ) -> T {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.generation == generation {
            state.cancelled = true;
        }
        let result = action();
        self.changed.notify_all();
        result
    }
    pub(super) fn invalidate_source_identity(&self, identity: &WriterIdentity, epoch: u64) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // A stale or already sealed writer cannot mutate and must not poison
        // another cohort or revoke a valid certificate by merely attempting it.
        if state.closed || state.writer_epoch != epoch || Self::is_certified(&state, identity) {
            return false;
        }
        let changed = !state.source_identity_changed;
        Self::stop_because(&mut state, || format!("source identity changed: {identity:?}"));
        state.source_identity_changed = true;
        state.cancelled = true;
        self.changed.notify_all();
        changed
    }
    #[track_caller]
    pub(super) fn invalidate_sources(&self) {
        let at = std::panic::Location::caller();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::stop_because(&mut state, || format!("sources invalidated from {}:{}", at.file(), at.line()));
        state.cancelled = true;
        self.changed.notify_all();
    }

    pub fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        state.cancelled = true;
        state.certified.clear();
        state.namespace = None;
        self.changed.notify_all();
    }

    #[cfg(test)]
    pub fn writer(
        self: &Arc<Self>,
        identity: &WriterIdentity,
    ) -> Result<SourceWritePermit, WriteBlocked> {
        self.admit_writer(identity, true, None)
    }
    #[cfg(test)]
    pub fn try_writer(
        self: &Arc<Self>,
        identity: &WriterIdentity,
    ) -> Result<SourceWritePermit, WriteBlocked> {
        self.admit_writer(identity, false, None)
    }
    pub fn writer_epoch(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .writer_epoch
    }
    pub fn writer_in_epoch(
        self: &Arc<Self>,
        identity: &WriterIdentity,
        epoch: u64,
        wait: bool,
    ) -> Result<SourceWritePermit, WriteBlocked> {
        self.admit_writer(identity, wait, Some(epoch))
    }
    fn admit_writer(
        self: &Arc<Self>,
        identity: &WriterIdentity,
        wait: bool,
        epoch: Option<u64>,
    ) -> Result<SourceWritePermit, WriteBlocked> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if state.closed {
                return Err(WriteBlocked::Closed);
            }
            if epoch.is_some_and(|epoch| epoch != state.writer_epoch) {
                return Err(WriteBlocked::StaleGeneration);
            }
            if state.repair.is_none() {
                break;
            }
            if !wait {
                return Err(WriteBlocked::Busy);
            }
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        if Self::is_certified(&state, identity) {
            return Err(WriteBlocked::Certified);
        }
        state.writers = state.writers.checked_add(1).ok_or(WriteBlocked::Busy)?;
        let leaf = match identity {
            #[cfg(test)]
            WriterIdentity::File(_) => None,
            WriterIdentity::Path(path) | WriterIdentity::FileAndPath(_, path) => {
                path.file_name().map(std::ffi::OsStr::to_os_string)
            }
        };
        if let Some(leaf) = &leaf {
            match state.writer_leaves.iter_mut().find(|(name, _)| name == leaf) {
                Some((_, count)) => *count += 1,
                None => state.writer_leaves.push((leaf.clone(), 1)),
            }
        }
        Ok(SourceWritePermit {
            coordinator: Arc::clone(self),
            leaf,
        })
    }
    fn is_certified(state: &WriteState, identity: &WriterIdentity) -> bool {
        state
            .certified
            .iter()
            .flat_map(|set| set.targets.iter())
            .any(|target| match identity {
                #[cfg(test)]
                WriterIdentity::File(id) => target.file_id == *id,
                WriterIdentity::Path(path) => Self::paths_may_alias(&target.path, path),
                WriterIdentity::FileAndPath(id, path) => {
                    target.file_id == *id || Self::paths_may_alias(&target.path, path)
                }
            })
    }
    fn paths_may_alias(left: &std::path::Path, right: &std::path::Path) -> bool {
        // Each coordinator belongs to an established root. A different parent
        // or an unprovable leaf cannot establish disjoint ownership here.
        if left == right || left.parent() != right.parent() {
            return true;
        }
        let flat_ascii = |name: &std::ffi::OsStr| {
            let bytes = name.as_encoded_bytes();
            !bytes.is_empty()
                && bytes.len() <= 255
                && !matches!(bytes.last(), Some(b' ' | b'.'))
                && bytes.iter().all(|&byte| {
                    (b' '..=b'~').contains(&byte)
                        && !matches!(
                            byte,
                            b'/' | b'\\' | b':' | b'"' | b'<' | b'>' | b'|' | b'?' | b'*'
                        )
                })
        };
        match (left.file_name(), right.file_name()) {
            (Some(left), Some(right)) if flat_ascii(left) && flat_ascii(right) => left
                .as_encoded_bytes()
                .eq_ignore_ascii_case(right.as_encoded_bytes()),
            _ => true,
        }
    }
    pub fn begin_repair(
        self: &Arc<Self>,
        generation: SessionGeneration,
        targets: Vec<WriterTarget>,
        reservation: impl Into<Arc<ControlLease>>,
    ) -> Result<RepairWriteLease, WriteBlocked> {
        let request = TargetRequest {
            targets,
            reservation: reservation.into(),
        };
        let targets = &request.targets;
        let bytes = targets
            .capacity()
            .checked_mul(std::mem::size_of::<WriterTarget>())
            .and_then(|base| {
                targets
                    .iter()
                    .try_fold(base, |sum, target| sum.checked_add(target.path.capacity()))
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    targets
                        .len()
                        .checked_mul(std::mem::size_of::<(i32, u32)>())?,
                )
            })
            .and_then(|bytes| bytes.checked_add(512));
        if targets.is_empty()
            || targets.len() > 512
            || bytes.is_none_or(|bytes| bytes as u64 > request.reservation.bytes())
            || targets.iter().enumerate().any(|(index, target)| {
                target.path.as_os_str().is_empty()
                    || targets[..index].iter().any(|old| {
                        old.file_id == target.file_id
                            || Self::paths_may_alias(&old.path, &target.path)
                    })
            })
        {
            return Err(WriteBlocked::InvalidTarget);
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return Err(WriteBlocked::Closed);
        }
        if state.generation != generation {
            return Err(WriteBlocked::StaleGeneration);
        }
        if state.cancelled {
            return Err(WriteBlocked::Cancelled);
        }
        if targets.iter().any(|target| {
            state
                .certified
                .iter()
                .flat_map(|set| &set.targets)
                .any(|sealed| {
                    sealed.file_id == target.file_id
                        || Self::paths_may_alias(&sealed.path, &target.path)
                })
        }) {
            return Err(WriteBlocked::Certified);
        }
        if state.repair.is_some() {
            return Err(WriteBlocked::Busy);
        }
        state.repair = Some((generation, false));
        self.changed.notify_all();
        loop {
            let error = if state.generation != generation {
                Some(WriteBlocked::StaleGeneration)
            } else if state.cancelled {
                Some(WriteBlocked::Cancelled)
            } else {
                None
            };
            if let Some(error) = error {
                state.repair = None;
                self.changed.notify_all();
                return Err(error);
            }
            if state.writers == 0 {
                break;
            }
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        state.repair = Some((generation, true));
        Ok(RepairWriteLease {
            coordinator: Arc::clone(self),
            generation,
            targets: request.targets,
            active: true,
            reservation: Some(request.reservation),
        })
    }
}

impl Drop for SourceWritePermit {
    fn drop(&mut self) {
        let mut state = self
            .coordinator
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        state.writers -= 1;
        if let Some(leaf) = &self.leaf {
            if let Some(index) = state.writer_leaves.iter().position(|(name, _)| name == leaf) {
                state.writer_leaves[index].1 -= 1;
                if state.writer_leaves[index].1 == 0 {
                    state.writer_leaves.swap_remove(index);
                }
            }
        }
        self.coordinator.changed.notify_all();
    }
}
impl RepairWriteLease {
    pub fn publish<T>(
        mut self,
        verified: &[(i32, u32)],
        publish: impl FnOnce() -> T,
    ) -> Result<T, WriteBlocked> {
        let mut state = self
            .coordinator
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let error = if state.closed {
            Some(WriteBlocked::Closed)
        } else if state.generation != self.generation {
            Some(WriteBlocked::StaleGeneration)
        } else if state.cancelled {
            Some(WriteBlocked::Cancelled)
        } else if verified.is_empty()
            || verified
                .iter()
                .any(|(id, _)| !self.targets.iter().any(|target| target.file_id == *id))
        {
            Some(WriteBlocked::InvalidTarget)
        } else {
            None
        };
        if let Some(error) = error {
            drop(state);
            return Err(error);
        }
        self.targets
            .retain(|target| verified.iter().any(|(id, _)| *id == target.file_id));
        let stamps = self
            .targets
            .iter()
            .filter_map(|target| Some((target.file_id, FileStamp::of(&target.path)?)))
            .collect();
        state.certified.push(CertifiedTargets {
            generation: self.generation,
            targets: std::mem::take(&mut self.targets),
            crcs: verified.to_vec(),
            stamps,
            _reservation: self.reservation.take().unwrap(),
        });
        let result = publish();
        state.repair = None;
        self.active = false;
        self.coordinator.changed.notify_all();
        Ok(result)
    }
}
impl Drop for RepairWriteLease {
    fn drop(&mut self) {
        if self.active {
            let mut state = self
                .coordinator
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if state
                .repair
                .is_some_and(|(generation, _)| generation == self.generation)
            {
                state.repair = None;
            }
            self.coordinator.changed.notify_all();
        }
    }
}
