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
    targets: Vec<WriterTarget>,
    crcs: Vec<(i32, u32)>,
    _reservation: Arc<ControlLease>,
}
struct WriteState {
    generation: SessionGeneration,
    writer_epoch: u64,
    cancelled: bool,
    closed: bool,
    writers: usize,
    // A pending barrier is installed before waiting, so new writers cannot starve it.
    repair: Option<(SessionGeneration, bool)>,
    certified: Vec<CertifiedTargets>,
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
    pub fn new(generation: SessionGeneration) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(WriteState {
                generation,
                writer_epoch: 0,
                cancelled: false,
                closed: false,
                writers: 0,
                repair: None,
                certified: Vec::new(),
            }),
            changed: Condvar::new(),
        })
    }
    pub(super) fn closed_and_quiescent(&self) -> (bool, bool) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.closed, state.repair.is_none() && state.writers == 0)
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
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return Err(WriteBlocked::Closed);
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
            state.certified.clear();
        }
        let result = bind();
        self.changed.notify_all();
        Ok(result)
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
            set.targets
                .iter()
                .any(|target| target.file_id == id && target.path == path)
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
    pub(super) fn invalidate_sources(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.cancelled = true;
        self.changed.notify_all();
    }

    pub fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        state.cancelled = true;
        state.certified.clear();
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
        if state
            .certified
            .iter()
            .flat_map(|set| set.targets.iter())
            .any(|target| match identity {
                #[cfg(test)]
                WriterIdentity::File(id) => target.file_id == *id,
                WriterIdentity::Path(path) => target.path == *path,
                WriterIdentity::FileAndPath(id, path) => {
                    target.file_id == *id || target.path == *path
                }
            })
        {
            return Err(WriteBlocked::Certified);
        }
        state.writers = state.writers.checked_add(1).ok_or(WriteBlocked::Busy)?;
        Ok(SourceWritePermit {
            coordinator: Arc::clone(self),
        })
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
                    || targets[..index]
                        .iter()
                        .any(|old| old.file_id == target.file_id || old.path == target.path)
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
        state.certified.push(CertifiedTargets {
            targets: std::mem::take(&mut self.targets),
            crcs: verified.to_vec(),
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
