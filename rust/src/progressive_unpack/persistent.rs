//! Direct unpack of stored RAR members into zipfast persistent partial files.
//!
//! Each stored member is written straight to its final name as a persistent `PartialFile`.
//! Lost article ranges become holes in that file instead of stopping extraction, and once the
//! damaged volumes are repaired (from par2 or a twin's articles written into the volume files)
//! only those ranges are read back in with `refill_from_packed`. Nothing is copied into place
//! and nothing already extracted is redone when a primary dies and the fleet takes over.
//!
//! The persistent path runs only when it can: stored members of known size in the first
//! volume, an expected volume count, overwrite mode and a Unix build. Anything else, and any
//! failure, falls back to the progressive path in the parent module with no persistent output
//! left behind (`Outcome::Fallback`).

use super::*;
use anchors::DurableAnchors;
use zipfast::partial::{
    ArchiveBinding, HoleReport, MemberKey, PartialFile, PartialLimits, PartialNaming,
    PartialOptions, Verdict,
};
use zipfast::progressive::{
    ProtectedVolume, ProtectionDeclaration, ProtectionInventory, SessionControl, VolumeBacking,
};

/// Off by default: zipfast commits every 1 MiB write durably, which made a 15 GB member take
/// 19 minutes against 95 s for the progressive copy path (Linux VM). Set
/// `RUSTYGET_PERSISTENT_UNPACK=1` to use it until zipfast groups commits.
pub(super) fn enabled() -> bool {
    cfg!(test) || std::env::var("RUSTYGET_PERSISTENT_UNPACK").as_deref() == Ok("1")
}

/// Directory under the destination that holds each set's partial state and trusted anchors.
pub(super) const STATE_DIR: &str = ".rustyget-partial";

pub(super) enum Outcome {
    Done(Result<zipfast::extract::Summary, Error>),
    /// The persistent path does not apply, or failed and removed what it wrote: run the
    /// progressive path with this feed.
    Fallback(Box<dyn VolumeFeed>, String),
}

/// Extracts the set with persistent partial files when it qualifies.
pub(super) fn try_extract<F>(
    req: &Request<'_>,
    feed: Box<dyn VolumeFeed>,
    log_cb: &mut F,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Outcome
where
    F: FnMut(i32, &str),
{
    let plan = match Plan::new(req, feed.as_ref()) {
        Ok(plan) => plan,
        Err(why) => {
            // State a crashed earlier run left for this set is no use without its outputs.
            remove_state_root(
                &req.dest
                    .join(STATE_DIR)
                    .join(hex(&binding_for(req).archive[..8])),
            );
            return Outcome::Fallback(feed, why);
        }
    };
    let feed = Arc::new(Mutex::new(feed));
    let mut run = Run::new(req, &plan, feed.clone());
    let result = run.extract(log_cb, is_cancelled);
    let outcome = match result {
        Ok(summary) => run.publish().map(|()| summary).map_err(Step::Fallback),
        Err(e) => Err(e),
    };
    drop(run);
    match outcome {
        Ok(summary) => {
            log_cb(4, "Unrar: All OK");
            Outcome::Done(Ok(summary))
        }
        Err(Step::Fail(f)) => {
            // Everything this run wrote is in its own state directory, unpublished.
            remove_state_root(&plan.root);
            Outcome::Done(Err(Error::Failed(f)))
        }
        Err(Step::Fallback(why)) => {
            remove_state_root(&plan.root);
            match Arc::try_unwrap(feed) {
                Ok(m) => Outcome::Fallback(m.into_inner().unwrap_or_else(|e| e.into_inner()), why),
                // The session that held the other reference is gone with `run`.
                Err(_) => Outcome::Done(Err(Error::Failed(Failure::Other(format!(
                    "direct unpack: {why}"
                ))))),
            }
        }
    }
}

/// Why the persistent path stopped: give the set to the progressive path, or fail the job.
enum Step {
    Fallback(String),
    Fail(Failure),
}

/// Everything decided before the session starts.
struct Plan {
    dir: PathBuf,
    names: Vec<OsString>,
    root: PathBuf,
    binding: ArchiveBinding,
    /// Where the session writes its outputs, inside `root`: nothing reaches the destination
    /// until every member verified, and anything left on failure goes with `root`.
    stage: PathBuf,
}

impl Plan {
    fn new(req: &Request<'_>, feed: &dyn VolumeFeed) -> Result<Self, String> {
        if cfg!(not(unix)) {
            return Err("persistent partial files need Linux or macOS".into());
        }
        if req.overwrite != Overwrite::Always {
            return Err("not in overwrite mode".into());
        }
        let expected = feed.expected_volumes();
        if expected == 0 {
            return Err("volume count unknown".into());
        }
        let dir = req
            .first_volume
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut names = Vec::new();
        let mut p = req.first_volume.to_path_buf();
        for i in 0..expected {
            names.push(p.file_name().ok_or("volume name")?.to_os_string());
            if i + 1 < expected {
                p = zipfast::volume::next_volume(&p).ok_or("volume naming")?;
            }
        }
        first_volume_qualifies(req.first_volume, req.password)?;
        let binding = binding_for(req);
        let root = req.dest.join(STATE_DIR).join(hex(&binding.archive[..8]));
        let stage = root.join("out");
        Ok(Self {
            dir,
            names,
            root,
            binding,
            stage,
        })
    }

    fn inventory(&self) -> ProtectionDeclaration {
        ProtectionDeclaration::Complete {
            epoch: 1,
            inventory: ProtectionInventory {
                volumes: self
                    .names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| ProtectedVolume {
                        index: i as u32,
                        name: name.clone(),
                        backing: VolumeBacking::NonFile,
                    })
                    .collect(),
                auxiliary_paths: Vec::new(),
            },
        }
    }
}

/// Removes state a crashed or killed earlier persistent run left for this set, when the
/// persistent path is not taken now.
pub(super) fn forget_state(req: &Request<'_>) {
    let root = req
        .dest
        .join(STATE_DIR)
        .join(hex(&binding_for(req).archive[..8]));
    if root.exists() {
        remove_state_root(&root);
    }
}

/// Members that start in the first volume must all be stored files of known size (RAR5 plain
/// or AES, RAR4 plain), with names both path rules agree on. Later volumes are checked by
/// zipfast when reached; a refusal there falls back.
fn first_volume_qualifies(first: &Path, password: &str) -> Result<(), String> {
    use zipfast::rar5::reader::{Block, Volume};
    let pw = (!password.is_empty()).then_some(password);
    let mut vol = Volume::open_with_password(first, pw).map_err(|e| format!("headers: {e}"))?;
    let mut members = 0;
    for _ in 0..100_000 {
        match vol.next_block() {
            Ok(Some(Block::File(h))) => {
                if h.split_before() {
                    continue;
                }
                if h.is_dir() {
                    return Err("directory entry".into());
                }
                if h.method != 0 {
                    return Err("compressed member".into());
                }
                if h.size_unknown() {
                    return Err("member of unknown size".into());
                }
                if h.name.contains(['\\', '<', '>', ':', '"', '|', '?', '*']) {
                    return Err("member name differs between path rules".into());
                }
                members += 1;
            }
            Ok(Some(Block::End { .. })) | Ok(None) => break,
            Ok(Some(_)) => {}
            Err(e) => return Err(format!("headers: {e}")),
        }
    }
    if members == 0 {
        return Err("no member starts in the first volume".into());
    }
    Ok(())
}

fn binding_for(req: &Request<'_>) -> ArchiveBinding {
    use md5::{Digest, Md5};
    let digest = |parts: &[&[u8]]| -> [u8; 16] {
        let mut h = Md5::new();
        for p in parts {
            h.update((p.len() as u64).to_le_bytes());
            h.update(p);
        }
        h.finalize().into()
    };
    let dest = path_bytes(req.dest);
    let first = path_bytes(req.first_volume);
    let job = digest(&[b"rustyget-job", &dest]);
    let a = digest(&[b"rustyget-archive", &first]);
    let b = digest(&[b"rustyget-archive-2", &first, &dest]);
    let mut archive = [0u8; 32];
    archive[..16].copy_from_slice(&a);
    archive[16..].copy_from_slice(&b);
    ArchiveBinding { job, archive }
}

/// Removes one set's state directory and, when empty, the shared state directory above it.
fn remove_state_root(root: &Path) {
    let _ = std::fs::remove_dir_all(root);
    if let Some(parent) = root.parent() {
        let _ = std::fs::remove_dir(parent);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Creates `dir` and every missing ancestor, syncing each new link's parent so the trusted
/// anchor store's directory is durable before it is opened.
fn provision_dir(dir: &Path) -> std::io::Result<()> {
    let mut missing = Vec::new();
    let mut p = dir;
    while !p.exists() {
        missing.push(p.to_path_buf());
        match p.parent() {
            Some(parent) => p = parent,
            None => break,
        }
    }
    for d in missing.iter().rev() {
        std::fs::create_dir(d).or_else(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(e)
            }
        })?;
        if let Some(parent) = d.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
    }
    Ok(())
}

/// Volume provider of a persistent session: every expected volume is declared up front as a
/// non-file source, handed over once the feed has it.
struct Provider {
    dir: PathBuf,
    feed: Arc<Mutex<Box<dyn VolumeFeed>>>,
    pending: PendingWait,
    declaration: ProtectionDeclaration,
    /// Volumes handed out with holes: (index, path).
    damaged: Arc<Mutex<Vec<(u32, PathBuf)>>>,
}

impl VolumeSource for Provider {
    fn volume(
        &mut self,
        request: &VolumeRequest,
        cx: &mut Context<'_>,
    ) -> ProgressiveResult<VolumeAnswer> {
        let path = self.dir.join(&request.expected);
        let view = self
            .feed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .volume(&path);
        match view {
            VolumeView::Ready { holes } => {
                if !holes.is_empty() {
                    self.damaged
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((request.index, path.clone()));
                }
                let mut volume = HoledVolume::open(&path, holes).map_err(|cause| {
                    ProgressiveFailure::new(FailureKind::Input {
                        operation: zipfast::progressive::InputOperation::Open,
                        cause,
                    })
                })?;
                // Declared NonFile: zipfast must not see a backing file.
                volume.expose_backing = false;
                Ok(VolumeAnswer::Ready(Box::new(volume)))
            }
            VolumeView::NotYet => {
                *self.pending.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((path, cx.waker().clone()));
                Ok(VolumeAnswer::Wait)
            }
            VolumeView::Missing => Ok(VolumeAnswer::Missing),
        }
    }

    fn protection(&mut self) -> ProgressiveResult<ProtectionDeclaration> {
        Ok(self.declaration.clone())
    }
}

/// One persistent extraction.
struct Run<'a> {
    req: &'a Request<'a>,
    plan: &'a Plan,
    feed: Arc<Mutex<Box<dyn VolumeFeed>>>,
    damaged: Arc<Mutex<Vec<(u32, PathBuf)>>>,
    /// Each member's handle and output path, by archive order.
    handles: Vec<(MemberKey, PartialFile, Member)>,
    holes: Vec<HoleReport>,
    anchors: Option<Arc<DurableAnchors>>,
}

impl<'a> Run<'a> {
    fn new(req: &'a Request<'a>, plan: &'a Plan, feed: Arc<Mutex<Box<dyn VolumeFeed>>>) -> Self {
        Self {
            req,
            plan,
            feed,
            damaged: Arc::new(Mutex::new(Vec::new())),
            handles: Vec::new(),
            holes: Vec::new(),
            anchors: None,
        }
    }

    fn extract<F>(
        &mut self,
        log_cb: &mut F,
        is_cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<zipfast::extract::Summary, Step>
    where
        F: FnMut(i32, &str),
    {
        // Reopening an earlier run's state is not supported: start from an empty root.
        let _ = std::fs::remove_dir_all(&self.plan.root);
        let state_dir = self.plan.root.join("state");
        let anchor_dir = self.plan.root.join("anchors");
        provision_dir(&state_dir)
            .and_then(|_| provision_dir(&anchor_dir))
            .and_then(|_| provision_dir(&self.plan.stage))
            .map_err(|e| Step::Fallback(format!("state directory: {e}")))?;
        let anchors = Arc::new(
            DurableAnchors::open(&anchor_dir)
                .map_err(|e| Step::Fallback(format!("anchor store: {e:?}")))?,
        );
        self.anchors = Some(anchors.clone());
        let volumes = self.plan.names.len() as u64;
        let pending: PendingWait = Arc::new(Mutex::new(None));
        let provider = Provider {
            dir: self.plan.dir.clone(),
            feed: self.feed.clone(),
            pending: pending.clone(),
            declaration: self.plan.inventory(),
            damaged: self.damaged.clone(),
        };
        let first_name = self.plan.names[0].clone();
        let mut opts = ProgressiveOptions::new(
            zipfast::extract::Options {
                mode: zipfast::extract::Mode::Extract,
                dest: self.plan.stage.clone(),
                overwrite: zipfast::extract::Overwrite::Always,
                flatten: false,
                threads: None,
                protected_paths: Default::default(),
                parallel_min: None,
                select: None,
            },
            first_name,
        );
        // A persistent session keeps every volume it opened.
        opts.limits.volume_handles = opts.limits.volume_handles.max(volumes + 16).min(4096);
        opts.limits.protection_paths = opts.limits.protection_paths.max(volumes + 16);
        if !self.req.password.is_empty() {
            opts.set_password(Some(self.req.password.to_string()))
                .map_err(|e| Step::Fail(Failure::from_progressive(&e)))?;
        }
        let mut limits = PartialLimits::default();
        // Every member keeps its handle (and a few descriptors) for the session's life.
        limits.open_partials = limits.open_partials.max(256);
        limits.open_files = limits.open_files.max(2048);
        limits.fill_bytes = limits.fill_bytes.max(opts.limits.read_chunk as u32);
        let popts = PartialOptions {
            binding: self.plan.binding.clone(),
            naming: PartialNaming::ArchivePath,
            state_dir,
            limits,
            anchors,
            control: SessionControl::default(),
        };
        let mut session = ProgressiveSession::new_partial(Box::new(provider), opts, popts)
            .map_err(|e| Step::Fallback(format!("session: {:?}", e.kind)))?;
        log_cb(
            3,
            &format!("Unrar: direct unpack into persistent partial files ({volumes} volume(s))"),
        );
        let mut summary = zipfast::extract::Summary::default();
        {
            let mut drive = Driver::new(&mut session, self.feed.clone(), pending, self.req.on_wait);
            let mut current: Option<Vec<u8>> = None;
            let mut current_member = Member::default();
            let mut current_volume = self.req.first_volume.to_path_buf();
            let mut meta_cache = MetaCache::default();
            loop {
                let event = drive.next_event(is_cancelled);
                match event {
                    Ok(None) => break,
                    Ok(Some(ProgressiveEvent::VolumeOpened { index, .. })) => {
                        if let Some(name) = self.plan.names.get(index as usize) {
                            current_volume = self.plan.dir.join(name);
                            log_cb(
                                4,
                                &format!("Unrar: Extracting from {}", name.to_string_lossy()),
                            );
                        }
                    }
                    Ok(Some(ProgressiveEvent::MemberStarted { name, .. })) => {
                        let meta = meta_cache.lookup(&current_volume, self.req.password, &name);
                        let display = String::from_utf8_lossy(&name).into_owned();
                        if name.iter().any(|b| b"\\<>:\"|?*".contains(b)) {
                            return Err(Step::Fallback(
                                "member name differs between path rules".into(),
                            ));
                        }
                        log_cb(4, &format!("Unrar: Extracting  {display}"));
                        current_member = Member {
                            name: name.to_vec(),
                            meta,
                            ordinal: 0,
                            occurrence: 0,
                        };
                        current = Some(name.to_vec());
                    }
                    Ok(Some(ProgressiveEvent::PartialReady { member, .. })) => {
                        let file = drive
                            .session
                            .partial_file(&member)
                            .map_err(|e| Step::Fallback(format!("partial handle: {e:?}")))?;
                        self.handles.push((member, file, current_member.clone()));
                    }
                    Ok(Some(ProgressiveEvent::Hole(report))) => {
                        log_cb(
                            3,
                            &format!(
                                "Unrar: {} has a hole at {}..{} ({:?}); filled after the repair",
                                String::from_utf8_lossy(&report.name),
                                report.out.start,
                                report.out.end,
                                report.cause
                            ),
                        );
                        self.holes.push(report);
                    }
                    Ok(Some(ProgressiveEvent::BytesWritten { valid_bytes, .. })) => {
                        summary.bytes += valid_bytes;
                    }
                    Ok(Some(ProgressiveEvent::MemberFinished { .. })) => {
                        summary.files += 1;
                        current = None;
                    }
                    Ok(Some(ProgressiveEvent::SessionTerminated { failure, .. })) => {
                        return Err(self.stop(&failure, current.as_deref()));
                    }
                    Ok(Some(_)) => {}
                    Err(e) => return Err(self.stop(&e, current.as_deref())),
                }
            }
        }
        drop(session);
        if !self.holes.is_empty() {
            self.refill(log_cb, is_cancelled)?;
        }
        self.verify(is_cancelled)?;
        Ok(summary)
    }

    /// Unsupported members (compressed, folders, unknown size, encrypted headers) and anything
    /// else zipfast refuses go to the progressive path; cancellation and wrong passwords fail.
    fn stop(&self, e: &ProgressiveFailure, member: Option<&[u8]>) -> Step {
        match &e.kind {
            FailureKind::Canceled => Step::Fail(Failure::Canceled),
            // Password errors, like everything else zipfast refuses, go to the progressive path:
            // it reports them (the line the unpack parsers read) and places the members before
            // the encrypted one exactly as before.
            _ => Step::Fallback(format!(
                "{:?}{}",
                e.kind,
                member
                    .map(|m| format!(" in {}", String::from_utf8_lossy(m)))
                    .unwrap_or_default()
            )),
        }
    }

    /// Waits for the damaged volumes to be repaired in place (par2, or a twin's articles), then
    /// reads back only the bytes that are still holes in each member.
    fn refill<F>(
        &mut self,
        log_cb: &mut F,
        is_cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), Step>
    where
        F: FnMut(i32, &str),
    {
        let mut damaged: Vec<String> = self
            .holes
            .iter()
            .map(|h| String::from_utf8_lossy(&h.name).into_owned())
            .collect();
        damaged.dedup();
        log_cb(
            0,
            &format!(
                "Unrar: extracted past damage in {}; waiting for the volumes to be repaired",
                damaged.join(", ")
            ),
        );
        if let Some(cb) = self.req.on_wait {
            cb(true);
        }
        let repaired = self
            .feed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .wait_repaired(is_cancelled);
        if let Some(cb) = self.req.on_wait {
            cb(false);
        }
        if !repaired {
            // Repair given up: the progressive path places the intact members and reports the
            // damage as before (its wait_repaired answers again at once).
            return Err(if is_cancelled() {
                Step::Fail(Failure::Canceled)
            } else {
                Step::Fallback("the damaged volumes were not repaired".into())
            });
        }
        let mut volumes: Vec<u32> = self
            .holes
            .iter()
            .flat_map(|h| h.packed.iter().map(|s| s.volume))
            .collect();
        volumes.sort_unstable();
        volumes.dedup();
        for (key, file, path) in self.handles.iter_mut() {
            let Some(name) = self
                .holes
                .iter()
                .find(|h| &h.member == key)
                .map(|h| String::from_utf8_lossy(&h.name).into_owned())
            else {
                continue;
            };
            let _ = &path;
            for &v in &volumes {
                let Some(name) = self.plan.names.get(v as usize) else {
                    continue;
                };
                let path = self.plan.dir.join(name);
                let mut f = std::fs::File::open(&path).map_err(|e| {
                    Step::Fail(Failure::Damaged(format!(
                        "repaired volume {}: {e}",
                        path.display()
                    )))
                })?;
                let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                // A limit or any other refusal here is zipfast's, not damage: the progressive
                // path extracts the repaired set again.
                file.refill_from_packed(v, 0..len, &mut f).map_err(|e| {
                    Step::Fallback(format!("refilling from {}: {:?}", path.display(), e.kind))
                })?;
            }
            log_cb(
                0,
                &format!("Unrar: repaired {name} from the repaired volumes (holes refilled)"),
            );
        }
        Ok(())
    }

    /// Every member must verify Ok against its archive checksums.
    fn verify(&mut self, is_cancelled: &mut dyn FnMut() -> bool) -> Result<(), Step> {
        let parker = Arc::new(Parker {
            thread: std::thread::current(),
            woken: AtomicBool::new(false),
        });
        let waker = Waker::from(parker.clone());
        let mut cx = Context::from_waker(&waker);
        for (_, file, _) in self.handles.iter_mut() {
            let verdict = loop {
                if is_cancelled() {
                    return Err(Step::Fail(Failure::Canceled));
                }
                parker.woken.store(false, Ordering::Release);
                match file.poll_verify(&mut cx) {
                    Poll::Ready(r) => break r,
                    Poll::Pending => {
                        if !parker.woken.load(Ordering::Acquire) {
                            std::thread::park_timeout(PARK_SLICE);
                        }
                    }
                }
            };
            match verdict {
                Ok(Verdict::Ok { .. }) => {}
                // A failed check (bad bytes, or a wrong password on an encrypted member),
                // holes left, or no stored checksum: the progressive path decides and reports
                // exactly as before.
                Ok(other) => {
                    return Err(Step::Fallback(format!("verification: {other:?}")));
                }
                Err(e) => {
                    return Err(Step::Fallback(format!("verification: {:?}", e.kind)));
                }
            }
        }
        Ok(())
    }

    /// Every member verified: moves the outputs from the stage into the destination (with the
    /// copy path's modes and folder modes) and drops the partial state.
    fn publish(&mut self) -> Result<(), String> {
        let members: Vec<Member> = self.handles.drain(..).map(|(_, _, m)| m).collect();
        self.anchors = None;
        let stage = self.plan.stage.clone();
        let mut files = Vec::new();
        let mut stack = vec![stage.clone()];
        while let Some(d) = stack.pop() {
            let entries = std::fs::read_dir(&d).map_err(|e| format!("stage: {e}"))?;
            for e in entries.flatten() {
                let ty = e.file_type().map_err(|e| format!("stage: {e}"))?;
                if ty.is_dir() {
                    stack.push(e.path());
                } else if ty.is_file() {
                    files.push(e.path());
                }
            }
        }
        for from in files {
            let rel = from
                .strip_prefix(&stage)
                .map_err(|e| format!("stage: {e}"))?;
            let member = members.iter().find(|m| {
                zipfast::path::sanitize(&String::from_utf8_lossy(&m.name), m.meta.windows_names)
                    .is_ok_and(|r| r == rel)
            });
            set_copy_path_mode(&from, member.and_then(|m| m.meta.unix_mode));
            // Folders as the copy path makes them: never through a symlink, mode 0777 less the
            // umask.
            let to = match rel.parent().filter(|p| !p.as_os_str().is_empty()) {
                Some(parent_rel) => {
                    let dir =
                        crate::archive_native::create_dir_all_no_follow(self.req.dest, parent_rel)?;
                    dir.join(rel.file_name().unwrap_or_default())
                }
                None => self.req.dest.join(rel),
            };
            std::fs::rename(&from, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
        remove_state_root(&self.plan.root);
        Ok(())
    }
}

/// The mode the copy path gives an output: a stored Unix mode with owner read/write kept,
/// otherwise 0666 less the umask. zipfast created the stage file at 0600 or applied the
/// stored mode as is; without the header (an undecoded name) 0600 means none was stored.
fn set_copy_path_mode(path: &Path, stored: Option<u32>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = std::fs::metadata(path) else {
            return;
        };
        let current = meta.permissions().mode() & 0o777;
        let mode = match stored {
            Some(m) => (m & 0o777) | 0o600,
            None if current == 0o600 => 0o666 & !crate::lifecycle::active_umask() & 0o777,
            None => current | 0o600,
        };
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (path, stored);
}

#[cfg(test)]
pub(super) fn plant_state_for_test(req: &Request<'_>) {
    let root = req
        .dest
        .join(STATE_DIR)
        .join(hex(&binding_for(req).archive[..8]));
    std::fs::create_dir_all(root.join("state")).unwrap();
    std::fs::write(root.join("state").join("bundle"), b"x").unwrap();
}
