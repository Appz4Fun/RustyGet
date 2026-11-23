// Copyright (C) 2026 xbmc4lyfe
// SPDX-License-Identifier: RPL-1.5
//
// Unless explicitly acquired and licensed from Licensor under another license,
// the contents of this file are subject to the Reciprocal Public License
// ("RPL") Version 1.5, or subsequent versions as allowed by the RPL, and You
// may not copy or use this file in either source code or executable form,
// except in compliance with the terms and conditions of the RPL.
//
// All software distributed under the RPL is provided strictly on an "AS IS"
// basis, WITHOUT WARRANTY OF ANY KIND, EITHER EXPRESS OR IMPLIED, AND LICENSOR
// HEREBY DISCLAIMS ALL SUCH WARRANTIES, INCLUDING WITHOUT LIMITATION, ANY
// WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE, QUIET
// ENJOYMENT, OR NON-INFRINGEMENT. See the RPL for specific language governing
// rights and limitations under the RPL.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use zipfast::partial::*;
use zipfast::progressive::OutputOperation;

/// File and directory syncs issued by every `DurableAnchors` in this process, counted next to
/// zipfast's own `durable_sync_count` (which leaves the caller's anchor store out).
static ANCHOR_SYNCS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Syncs issued by anchor stores in this process so far.
pub(crate) fn anchor_sync_count() -> u64 {
    ANCHOR_SYNCS.load(std::sync::atomic::Ordering::Relaxed)
}

fn sync(file: &File) -> std::io::Result<()> {
    ANCHOR_SYNCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    file.sync_all()
}

// Copied from zipfast's docs/persistent-anchor-store.rs (the reference store its
// persistent partial files are tested against).
//
// This directory is caller-owned trusted storage, separate from output/state.
// One process holds its OS lock; threads serialize each durable CAS.
#[allow(dead_code)]
pub(crate) struct DurableAnchors {
    root: PathBuf,
    directory: File,
    _lock: File,
    serial: Mutex<u64>,
    #[cfg(test)]
    fault: Mutex<(usize, usize)>,
}
#[allow(
    clippy::result_large_err,
    reason = "the public typed error preserves context and committed-anchor receipts"
)]
impl DurableAnchors {
    pub(crate) fn open(root: &Path) -> PartialResult<Self> {
        // The caller must provision this trusted directory and durably sync
        // all newly created ancestor links before opening the store.
        let directory = File::open(root).map_err(store_io)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("store.lock"))
            .map_err(store_io)?;
        lock.lock().map_err(store_io)?;
        Ok(Self {
            root: root.into(),
            directory,
            _lock: lock,
            serial: Mutex::new(0),
            #[cfg(test)]
            fault: Mutex::new((0, 0)),
        })
    }
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn fail_after_rename(&self, count: usize) {
        self.fault.lock().unwrap().0 = count;
    }
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn visible(&self, member: &MemberKey) -> PartialResult<Option<StateAnchor>> {
        self.read(member)
    }
    fn sync_directory(&self) -> PartialResult<()> {
        #[cfg(test)]
        {
            let mut fault = self.fault.lock().unwrap();
            if fault.1 > 0 {
                fault.1 -= 1;
                return Err(store_io(std::io::Error::other(
                    "injected anchor directory sync failure",
                )));
            }
        }
        sync(&self.directory).map_err(store_io)
    }
    fn path(&self, member: &MemberKey) -> PathBuf {
        fn hex(bytes: &[u8]) -> String {
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        }
        self.root.join(format!(
            "{}-{}-{:016x}-{}",
            hex(&member.binding().job),
            hex(&member.binding().archive),
            member.ordinal(),
            hex(member.identity())
        ))
    }
    fn read(&self, member: &MemberKey) -> PartialResult<Option<StateAnchor>> {
        let file = match File::open(self.path(member)) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(store_io(e)),
        };
        let mut bytes = Vec::new();
        file.take(41).read_to_end(&mut bytes).map_err(store_io)?;
        if bytes.len() != 40 {
            return Err(PartialError::new(PartialErrorKind::StateUnavailable));
        }
        Ok(Some(StateAnchor::new(
            member.clone(),
            u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            bytes[8..].try_into().unwrap(),
        )))
    }
}
fn store_io(cause: std::io::Error) -> PartialError {
    PartialError::new(PartialErrorKind::Io {
        operation: OutputOperation::Flush,
        cause,
    })
}
impl StateAnchorStore for DurableAnchors {
    fn current(&self, member: &MemberKey) -> PartialResult<Option<StateAnchor>> {
        let _guard = self.serial.lock().unwrap();
        // A prior CAS can have renamed successfully and failed its final sync.
        // Visibility alone cannot authorize retiring the old anchored bundle.
        self.sync_directory()?;
        self.read(member)
    }
    fn compare_exchange(
        &self,
        member: &MemberKey,
        expected: Option<&StateAnchor>,
        next: &StateAnchor,
    ) -> PartialResult<()> {
        let mut serial = self.serial.lock().unwrap();
        if next.member() != member || self.read(member)?.as_ref() != expected {
            return Err(PartialError::new(PartialErrorKind::AnchorConflict));
        }
        *serial = serial
            .checked_add(1)
            .ok_or_else(|| PartialError::new(PartialErrorKind::StateUnavailable))?;
        let temporary = self
            .root
            .join(format!("new-{}-{}", std::process::id(), serial));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(store_io)?;
        file.write_all(&next.sequence().to_le_bytes())
            .map_err(store_io)?;
        file.write_all(next.digest()).map_err(store_io)?;
        sync(&file).map_err(store_io)?;
        fs::rename(&temporary, self.path(member)).map_err(store_io)?;
        // A late error is uncertain: rename may have durably installed next.
        #[cfg(test)]
        {
            let mut fault = self.fault.lock().unwrap();
            if fault.0 != 0 {
                fault.1 = fault.0;
                fault.0 = 0;
            }
        }
        self.sync_directory()
    }
}
