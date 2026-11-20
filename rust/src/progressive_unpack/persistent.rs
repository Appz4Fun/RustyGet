//! Direct unpack of stored RAR members into zipfast persistent partial files.
//!
//! Each stored member is written as a persistent `PartialFile` into a private stage under the
//! destination. Lost article ranges become holes in that file instead of stopping extraction,
//! and once the damaged volumes are repaired (from par2 or a twin's articles written into the
//! volume files) only those ranges are read back in with `refill_from_packed`. Verified outputs
//! are then moved into place (a rename); nothing already extracted is redone when a primary
//! dies and the fleet takes over. On by default; `RUSTYGET_PERSISTENT_UNPACK=0` turns it off.
//!
//! The persistent path runs only when it can: stored members of known size in the first
//! volume, an expected volume count, overwrite mode and a Unix build. Anything else, and any
//! failure, falls back to the progressive path in the parent module with no persistent output
//! left behind (`Outcome::Fallback`).

use super::*;
use std::io::Seek;
use anchors::DurableAnchors;
use zipfast::partial::{
    ArchiveBinding, HoleReport, MemberKey, PartialFile, PartialLimits, PartialNaming,
    PartialOptions, Verdict,
};
use zipfast::progressive::{
    ProtectedVolume, ProtectionDeclaration, ProtectionInventory, SessionControl, VolumeBacking,
};

/// On by default on Linux since zipfast groups persistent commits (#121): on a Linux VM a 15 GB
/// stored member took 348 s persistent against 320 s for the progressive copy path, and 4 GB
/// took 22.8 s against 33.9 s. Off by default on macOS, where each group commit's F_FULLFSYNCs
/// made 4 GB about 2.5x slower than the copy path. `RUSTYGET_PERSISTENT_UNPACK` overrides
/// either way: 0/false/off/no or 1/true/on/yes (any case).
pub(super) fn enabled() -> bool {
    if cfg!(test) {
        return true;
    }
    let value = std::env::var("RUSTYGET_PERSISTENT_UNPACK").unwrap_or_default();
    switch_value(&value, !cfg!(target_os = "macos"))
}

/// Reads the switch's value; anything unrecognised (including empty) keeps the default.
pub(super) fn switch_value(value: &str, default: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "0" | "false" | "off" | "no" => false,
        "1" | "true" | "on" | "yes" => true,
        _ => default,
    }
}

/// Most members a set's first volume may hold for the persistent path: zipfast keeps every
/// member's state for the session (about 3 of its 128 state pages and 6 descriptors each).
const MAX_FIRST_VOLUME_MEMBERS: usize = 30;

/// Directory under the destination that holds each set's partial state and trusted anchors.
pub(crate) const STATE_DIR: &str = ".rustyget-partial";

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
    let damaged = run.damaged.clone();
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
        Err(Step::GaveUp) => {
            remove_state_root(&plan.root);
            Outcome::Done(Err(Error::Failed(Failure::Damaged(
                "the damaged volumes were not repaired".into(),
            ))))
        }
        Err(Step::Fail(f)) => {
            // Everything this run wrote is in its own state directory, unpublished. A cancel
            // keeps it: a daemon shutdown resumes from it on the next start, and deleting the
            // job removes it with the direct-unpack cleanup (unpack::direct_unpack_cleanup).
            if !matches!(f, Failure::Canceled) {
                remove_state_root(&plan.root);
            }
            Outcome::Done(Err(Error::Failed(f)))
        }
        Err(Step::Fallback(why)) => {
            remove_state_root(&plan.root);
            match Arc::try_unwrap(feed) {
                Ok(m) => {
                    let inner = m.into_inner().unwrap_or_else(|e| e.into_inner());
                    let handed =
                        std::mem::take(&mut *damaged.lock().unwrap_or_else(|e| e.into_inner()));
                    let feed: Box<dyn VolumeFeed> = if handed.is_empty() {
                        inner
                    } else {
                        Box::new(RememberedHoles {
                            inner,
                            holes: handed,
                            repaired: false,
                        })
                    };
                    Outcome::Fallback(feed, why)
                }
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
    /// The repair was given up: intact members are published, the job fails as damaged.
    GaveUp,
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
        let members = first_volume_qualifies(req.first_volume, req.password)?;
        // The session holds about one descriptor per volume and six per member; a set that would
        // crowd the process's descriptors (and its sockets and article files) stays on the copy
        // path.
        if !fits_open_file_limit(names.len() + 6 * members) {
            return Err("not enough file descriptors".into());
        }
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
fn first_volume_qualifies(first: &Path, password: &str) -> Result<usize, String> {
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
    if members > MAX_FIRST_VOLUME_MEMBERS {
        return Err(format!("{members} members in the first volume"));
    }
    if members == 0 {
        return Err("no member starts in the first volume".into());
    }
    Ok(members)
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

/// File in a set's state directory naming the damaged volumes still waiting for their repair.
const PENDING_REPAIR: &str = "pending-repair";

/// Saves the damaged volumes still waiting for their repair, one per line as
/// `offset:len,offset:len<TAB>file name`, replacing the file atomically; removes it when there
/// are none. Best effort: without it a restart reads the damaged bytes and falls back.
pub(super) fn save_pending_repair(file: &Path, damaged: &std::collections::HashMap<PathBuf, Vec<(u64, u64)>>) {
    if damaged.is_empty() {
        let _ = std::fs::remove_file(file);
        return;
    }
    let mut entries: Vec<_> = damaged.iter().collect();
    entries.sort();
    let mut out = Vec::new();
    for (path, holes) in entries {
        let Some(name) = path.file_name() else {
            continue;
        };
        let name = name.as_encoded_bytes();
        if name.contains(&b'\n') || holes.is_empty() {
            continue;
        }
        let ranges: Vec<String> = holes.iter().map(|(o, l)| format!("{o}:{l}")).collect();
        out.extend_from_slice(ranges.join(",").as_bytes());
        out.push(b'\t');
        out.extend_from_slice(name);
        out.push(b'\n');
    }
    let tmp = file.with_extension("tmp");
    let written = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&out)?;
        f.sync_all()?;
        std::fs::rename(&tmp, file)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// The damaged volumes `save_pending_repair` saved, as paths under `dir`.
pub(super) fn load_pending_repair(file: &Path, dir: &Path) -> Vec<(PathBuf, Vec<(u64, u64)>)> {
    let Ok(bytes) = std::fs::read(file) else {
        return Vec::new();
    };
    bytes
        .split(|&b| b == b'\n')
        .filter_map(|line| {
            let tab = line.iter().position(|&b| b == b'\t')?;
            let (ranges, name) = (&line[..tab], &line[tab + 1..]);
            if name.is_empty() || name.contains(&b'/') || name == b"." || name == b".." {
                return None;
            }
            let holes: Option<Vec<(u64, u64)>> = std::str::from_utf8(ranges)
                .ok()?
                .split(',')
                .map(|r| {
                    let (o, l) = r.split_once(':')?;
                    Some((o.parse().ok()?, l.parse().ok()?))
                })
                .collect();
            let holes = holes.filter(|h| !h.is_empty())?;
            // SAFETY: the bytes were written from `OsStr::as_encoded_bytes` on this platform.
            let name = unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(name) };
            Some((dir.join(name), holes))
        })
        .collect()
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

/// Holes handed to the session, by volume path, until a repair succeeds.
type HandedHoles = Arc<Mutex<std::collections::HashMap<PathBuf, Vec<(u64, u64)>>>>;

/// The feed handed to the progressive path after a fallback. The direct-unpack feed forgets
/// a volume's holes when it gives up a repair; the progressive run must still see the holes
/// the persistent run was handed, or it reads the lost ranges as data.
struct RememberedHoles {
    inner: Box<dyn VolumeFeed>,
    holes: std::collections::HashMap<PathBuf, Vec<(u64, u64)>>,
    repaired: bool,
}

impl VolumeFeed for RememberedHoles {
    fn volume(&mut self, path: &Path) -> VolumeView {
        match self.inner.volume(path) {
            VolumeView::Ready { holes } if holes.is_empty() && !self.repaired => {
                VolumeView::Ready {
                    holes: self.holes.get(path).cloned().unwrap_or_default(),
                }
            }
            other => other,
        }
    }
    fn wait_repaired(&mut self, is_cancelled: &mut dyn FnMut() -> bool) -> bool {
        let repaired = self.inner.wait_repaired(is_cancelled);
        self.repaired |= repaired;
        repaired
    }
    fn set_is_final(&self) -> bool {
        self.inner.set_is_final()
    }
    fn expected_volumes(&self) -> u64 {
        self.inner.expected_volumes()
    }
    fn set_became_final(&self) -> bool {
        self.inner.set_became_final()
    }
    fn has_borrowed(&self) -> bool {
        self.inner.has_borrowed()
    }
    fn remember_damaged(&mut self, path: &Path, holes: Vec<(u64, u64)>) {
        self.inner.remember_damaged(path, holes)
    }
    fn still_held(&self, path: &Path) -> bool {
        self.inner.still_held(path)
    }
    fn repaired(&self, path: &Path) -> bool {
        self.inner.repaired(path)
    }
}

/// Volume provider of a persistent session: every expected volume is declared up front as a
/// non-file source, handed over once the feed has it.
struct Provider {
    dir: PathBuf,
    feed: Arc<Mutex<Box<dyn VolumeFeed>>>,
    pending: PendingWait,
    declaration: ProtectionDeclaration,
    /// Volumes handed out with holes: (index, path).
    damaged: HandedHoles,
    /// Where the damaged volumes still waiting for their repair are saved (see
    /// `save_pending_repair`).
    pending_repair: PathBuf,
    /// The damaged volumes in that file.
    saved: std::collections::HashSet<PathBuf>,
}

impl VolumeSource for Provider {
    fn volume(
        &mut self,
        request: &VolumeRequest,
        cx: &mut Context<'_>,
    ) -> ProgressiveResult<VolumeAnswer> {
        let path = self.dir.join(&request.expected);
        let view = {
            let mut feed = self.feed.lock().unwrap_or_else(|e| e.into_inner());
            // Damaged volumes repaired and released since: a restart must not hold them again.
            // Saved below whatever the answer is (a volume not here yet included).
            let before = self.saved.len();
            self.saved.retain(|p| feed.still_held(p));
            let mut changed = self.saved.len() != before;
            let view = feed.volume(&path);
            drop(feed);
            let mut damaged = self.damaged.lock().unwrap_or_else(|e| e.into_inner());
            if let VolumeView::Ready { holes } = &view {
                if !holes.is_empty() {
                    changed |= damaged.get(&path) != Some(holes) || !self.saved.contains(&path);
                    damaged.insert(path.clone(), holes.clone());
                    self.saved.insert(path.clone());
                }
            }
            if changed {
                let pending: std::collections::HashMap<PathBuf, Vec<(u64, u64)>> = damaged
                    .iter()
                    .filter(|(p, _)| self.saved.contains(*p))
                    .map(|(p, h)| (p.clone(), h.clone()))
                    .collect();
                save_pending_repair(&self.pending_repair, &pending);
            }
            view
        };
        match view {
            VolumeView::Ready { holes } => {
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
                    // A stand-in with holes is read here: it ends the wait.
                    Some((path, cx.waker().clone(), false));
                Ok(VolumeAnswer::Wait)
            }
            VolumeView::Missing => Ok(VolumeAnswer::Missing),
            // A dead volume rebuilt around bytes borrowed from a duplicate (see `deadvolume`).
            VolumeView::Virtual(v) => Ok(VolumeAnswer::Ready(Box::new(
                super::VirtualSource::open(&path, v).map_err(|cause| {
                    ProgressiveFailure::new(FailureKind::Input {
                        operation: zipfast::progressive::InputOperation::Open,
                        cause,
                    })
                })?,
            ))),
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
    damaged: HandedHoles,
    /// Each member's handle and output path, by archive order.
    handles: Vec<(MemberKey, PartialFile, Member)>,
    holes: Vec<HoleReport>,
    /// Members adopted from an earlier run's state (a restart) that still have holes: their
    /// holes were reported to that run, so they are refilled from every volume.
    adopted_holed: Vec<MemberKey>,
    /// Every member zipfast adopted from an earlier run's state.
    adopted: Vec<MemberKey>,
    anchors: Option<Arc<DurableAnchors>>,
    /// Holes of this run not read back yet, patched as soon as their volume is repaired.
    inline: InlineRefill,
}

/// The holes of a running session whose volumes have not been repaired yet. Each one is read
/// back into its member as soon as the live repair releases its volume, while extraction goes
/// on, so the output is whole when the session reaches the end of the set (see
/// `refill_released`).
#[derive(Default)]
struct InlineRefill {
    /// (member, volume, packed range) of every reported hole not refilled yet.
    pending: Vec<(MemberKey, u32, std::ops::Range<u64>)>,
    /// A refill here failed: what is left waits for the refill after the set, which reports
    /// the failure as it always did.
    failed: bool,
    /// When the volumes were last asked about (asked at most every `INLINE_CHECK`).
    checked: Option<std::time::Instant>,
}

/// How often a running session asks whether the volumes of its holes were repaired.
const INLINE_CHECK: Duration = Duration::from_millis(100);

impl InlineRefill {
    /// True when every hole this run reported was patched while extracting.
    fn all_patched(&self) -> bool {
        self.pending.is_empty() && !self.failed
    }
}

/// A volume's bytes for reading holes back: its file, or the finished stand-in of a dead volume
/// that was rebuilt from a duplicate (it has no file). `None` when neither is there.
fn open_repaired(
    feed: &Mutex<Box<dyn VolumeFeed>>,
    path: &Path,
) -> Option<std::io::Result<Box<dyn ReadSeek>>> {
    // A stand-in wins over a file at the volume's path (a dead or placeholder one), as it does
    // when the extraction asks for the volume: the holes were recorded against its layout.
    match feed.lock().unwrap_or_else(|e| e.into_inner()).volume(path) {
        VolumeView::Virtual(v) if v.holes.is_empty() => {
            Some(super::VirtualFile::open(&v).map(|f| Box::new(f) as Box<dyn ReadSeek>))
        }
        VolumeView::Virtual(_) => None,
        _ if path.is_file() => {
            Some(std::fs::File::open(path).map(|f| Box::new(f) as Box<dyn ReadSeek>))
        }
        _ => None,
    }
}

/// What reading holes back needs of a volume.
trait ReadSeek: std::io::Read + std::io::Seek {}
impl<T: std::io::Read + std::io::Seek> ReadSeek for T {}

/// Reads the holes of `inline` whose volume the repair has released back into their members.
/// `force` skips the rate limit (the session ended). Runs on the session's own thread,
/// between its steps and while it is parked. zipfast takes a fill on a member's handle while
/// the session is still writing that member: it commits the open group first, and only bytes
/// that are still holes are read.
fn refill_released<F>(
    inline: &mut InlineRefill,
    handles: &mut [(MemberKey, PartialFile, Member)],
    damaged: &HandedHoles,
    plan: &Plan,
    feed: &Mutex<Box<dyn VolumeFeed>>,
    log_cb: &mut F,
    force: bool,
) where
    F: FnMut(i32, &str),
{
    if inline.pending.is_empty() || inline.failed {
        return;
    }
    let now = std::time::Instant::now();
    if !force && inline.checked.is_some_and(|t| now.duration_since(t) < INLINE_CHECK) {
        return;
    }
    inline.checked = Some(now);
    let mut volumes: Vec<u32> = inline.pending.iter().map(|(_, v, _)| *v).collect();
    volumes.sort_unstable();
    volumes.dedup();
    let repaired: Vec<(u32, PathBuf)> = {
        let feed = feed.lock().unwrap_or_else(|e| e.into_inner());
        volumes
            .into_iter()
            .filter_map(|v| {
                let path = plan.dir.join(plan.names.get(v as usize)?);
                feed.repaired(&path).then_some((v, path))
            })
            .collect()
    };
    for (v, path) in repaired {
        let volume = path.file_name().unwrap_or_default().to_string_lossy();
        let mut f = match open_repaired(feed, &path) {
            Some(Ok(f)) => f,
            Some(Err(e)) => {
                log_cb(
                    2,
                    &format!("Unrar: cannot read repaired {volume} while extracting ({e}); its holes are filled after the set"),
                );
                inline.failed = true;
                return;
            }
            // Released without a file or a finished stand-in (recreated later, say): the
            // refill after the set reads it.
            None => continue,
        };
        let mut patched: Vec<(String, u64)> = Vec::new();
        let mut left = Vec::new();
        for (key, hv, range) in std::mem::take(&mut inline.pending) {
            if hv != v || inline.failed {
                left.push((key, hv, range));
                continue;
            }
            let Some((_, file, member)) = handles.iter_mut().find(|(k, _, _)| *k == key) else {
                left.push((key, hv, range));
                continue;
            };
            match file.refill_from_packed(v, range.clone(), &mut f) {
                Ok(r) => {
                    let name = String::from_utf8_lossy(&member.name).into_owned();
                    let bytes: u64 = r.applied.iter().map(|a| a.end - a.start).sum();
                    match patched.iter_mut().find(|(n, _)| *n == name) {
                        Some((_, b)) => *b += bytes,
                        None => patched.push((name, bytes)),
                    }
                }
                Err(e) => {
                    log_cb(
                        2,
                        &format!(
                            "Unrar: patching {} from {volume} while extracting failed ({:?}); its holes are filled after the set",
                            String::from_utf8_lossy(&member.name),
                            e.kind
                        ),
                    );
                    inline.failed = true;
                    left.push((key, hv, range));
                }
            }
        }
        inline.pending = left;
        if inline.failed {
            return;
        }
        // Nothing of this volume waits any more: a fallback must not hand it out holed again.
        damaged
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&path);
        for (name, bytes) in patched {
            log_cb(
                0,
                &format!("Unrar: patched {name} from repaired {volume} while extracting ({bytes} bytes)"),
            );
        }
    }
}

impl<'a> Run<'a> {
    fn new(req: &'a Request<'a>, plan: &'a Plan, feed: Arc<Mutex<Box<dyn VolumeFeed>>>) -> Self {
        Self {
            req,
            plan,
            feed,
            damaged: Arc::new(Mutex::new(std::collections::HashMap::new())),
            handles: Vec::new(),
            holes: Vec::new(),
            adopted_holed: Vec::new(),
            adopted: Vec::new(),
            anchors: None,
            inline: InlineRefill::default(),
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
        // An earlier run's state for this set (a restart) is kept: zipfast adopts the members it
        // anchored and resumes the one that was mid-extraction from its committed output.
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
        self.remember_pending_repair(log_cb);
        let volumes = self.plan.names.len() as u64;
        let pending: PendingWait = Arc::new(Mutex::new(None));
        let provider = Provider {
            dir: self.plan.dir.clone(),
            feed: self.feed.clone(),
            pending: pending.clone(),
            declaration: self.plan.inventory(),
            damaged: self.damaged.clone(),
            pending_repair: self.plan.root.join(PENDING_REPAIR),
            saved: self
                .damaged
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect(),
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
                windows_safe_names: false,
            },
            first_name,
        );
        // A persistent session keeps every volume it opened.
        opts.limits.volume_handles = opts.limits.volume_handles.max(volumes + 16).min(4096);
        opts.limits.protection_paths = opts.limits.protection_paths.max(volumes + 16);
        // Every hole costs metadata (about 1.4 MB per 1 MiB hole on 6790): a whole dead volume
        // handed over before its borrow finished (about 90 holes) used up the default 128 MiB
        // and failed the session. Same headroom as `rarreader` gives its sessions.
        opts.limits.metadata_bytes = opts.limits.metadata_bytes.max(1 << 31);
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
                let event = drive.next_event_or_tick(is_cancelled, &mut || {
                    refill_released(
                        &mut self.inline,
                        &mut self.handles,
                        &self.damaged,
                        self.plan,
                        &self.feed,
                        log_cb,
                        false,
                    )
                });
                // Also between steps: a session that never parks still patches as it goes.
                refill_released(
                    &mut self.inline,
                    &mut self.handles,
                    &self.damaged,
                    self.plan,
                    &self.feed,
                    log_cb,
                    false,
                );
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
                        super::admit_member(self.req.dest, &name, meta).map_err(Step::Fail)?;
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
                    Ok(Some(ProgressiveEvent::PartialAdopted {
                        member,
                        produced,
                        finished,
                        ..
                    })) => {
                        let file = drive
                            .session
                            .partial_file(&member)
                            .map_err(|e| Step::Fallback(format!("partial handle: {e:?}")))?;
                        log_cb(
                            3,
                            &format!(
                                "Unrar: resuming {} from its saved state ({produced} bytes{})",
                                String::from_utf8_lossy(&current_member.name),
                                if finished { ", finished" } else { "" }
                            ),
                        );
                        if file.remaining().map(|r| !r.is_empty()).unwrap_or(true) && finished {
                            self.adopted_holed.push(member.clone());
                        }
                        self.adopted.push(member.clone());
                        self.handles.push((member, file, current_member.clone()));
                    }
                    Ok(Some(ProgressiveEvent::Hole(report))) => {
                        log_cb(
                            3,
                            &format!(
                                "Unrar: {} has a hole at {}..{} ({:?}); patched once its volume is repaired",
                                String::from_utf8_lossy(&report.name),
                                report.out.start,
                                report.out.end,
                                report.cause
                            ),
                        );
                        self.inline.pending.extend(
                            report
                                .packed
                                .iter()
                                .map(|s| (report.member.clone(), s.volume, s.range.clone())),
                        );
                        self.holes.push(report);
                    }
                    Ok(Some(ProgressiveEvent::BytesWritten { valid_bytes, .. })) => {
                        summary.bytes += valid_bytes;
                    }
                    Ok(Some(ProgressiveEvent::MemberFinished {
                        outcome: zipfast::progressive::MemberOutcome::Refused { reason },
                        ..
                    })) => {
                        // zipfast wrote nothing for this member: an ordinary session extracts it.
                        return Err(Step::Fallback(format!(
                            "zipfast refused {}: {reason:?}",
                            current
                                .as_deref()
                                .map(String::from_utf8_lossy)
                                .unwrap_or("a member".into())
                        )));
                    }
                    // A directory entry: zipfast #130 creates it inside this run's stage, but
                    // publishing moves only files out of the stage, so an empty folder would be
                    // lost. Hand the set to the progressive path, as before #130 (which refused
                    // directory entries in persistent sessions).
                    Ok(Some(ProgressiveEvent::MemberFinished {
                        outcome: zipfast::progressive::MemberOutcome::Directory { .. },
                        ..
                    })) => {
                        return Err(Step::Fallback(format!(
                            "directory entry{}",
                            current
                                .as_deref()
                                .map(|m| format!(" {}", String::from_utf8_lossy(m)))
                                .unwrap_or_default()
                        )));
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
        // Volumes repaired since the last look: patch them now, before deciding to wait.
        refill_released(
            &mut self.inline,
            &mut self.handles,
            &self.damaged,
            self.plan,
            &self.feed,
            log_cb,
            true,
        );
        // A resumed member that was mid-extraction may have been finished by this run: check
        // every adopted member for holes now. Holes this run reported do not name the volumes
        // of holes the earlier run reported, so an adopted member with any hole left is
        // refilled from every volume.
        for (key, file, _) in &self.handles {
            if self.adopted.contains(key)
                && !self.adopted_holed.contains(key)
                && file.remaining().map(|r| !r.is_empty()).unwrap_or(false)
            {
                self.adopted_holed.push(key.clone());
            }
        }
        if !self.holes.is_empty() || !self.adopted_holed.is_empty() {
            match self.refill(log_cb, is_cancelled) {
                Err(Step::GaveUp) => {
                    self.publish_intact(is_cancelled).map_err(Step::Fallback)?;
                    return Err(Step::Fail(Failure::Damaged(
                        "the damaged volumes were not repaired".into(),
                    )));
                }
                other => other?,
            }
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
    /// A restart: the volumes the earlier run was handed damaged and that were not repaired
    /// before it stopped go back on hold with their lost ranges, so the refill waits for their
    /// repair instead of reading the damaged bytes.
    fn remember_pending_repair<F>(&mut self, log_cb: &mut F)
    where
        F: FnMut(i32, &str),
    {
        let saved = load_pending_repair(&self.plan.root.join(PENDING_REPAIR), &self.plan.dir);
        if saved.is_empty() {
            return;
        }
        let mut feed = self.feed.lock().unwrap_or_else(|e| e.into_inner());
        let mut damaged = self.damaged.lock().unwrap_or_else(|e| e.into_inner());
        for (path, holes) in saved {
            let known = path
                .file_name()
                .is_some_and(|n| self.plan.names.iter().any(|v| v.as_os_str() == n));
            if !known || !path.is_file() {
                continue;
            }
            log_cb(
                3,
                &format!(
                    "Unrar: {} is still waiting for its repair (from before the restart)",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
            );
            feed.remember_damaged(&path, holes.clone());
            damaged.insert(path, holes);
        }
    }

    fn refill<F>(
        &mut self,
        log_cb: &mut F,
        is_cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), Step>
    where
        F: FnMut(i32, &str),
    {
        if self.adopted_holed.is_empty() && self.inline.all_patched() {
            // Every hole was patched from its repaired volume while extracting: nothing to
            // wait for (verify still checks every member).
            log_cb(
                0,
                "Unrar: every hole was patched while extracting; nothing waits for a repair",
            );
            let mut damaged = self.damaged.lock().unwrap_or_else(|e| e.into_inner());
            damaged.clear();
            save_pending_repair(&self.plan.root.join(PENDING_REPAIR), &damaged);
            return Ok(());
        }
        let mut damaged: Vec<String> = self
            .holes
            .iter()
            .map(|h| String::from_utf8_lossy(&h.name).into_owned())
            .collect();
        damaged.dedup();
        if damaged.is_empty() {
            // Only damage an earlier run saw (a restart): name its volumes instead.
            let mut names: Vec<String> = self
                .damaged
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .collect();
            names.sort();
            damaged = names;
        }
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
        if repaired {
            // The holes are really gone now: a later fallback reads the repaired bytes.
            let mut damaged = self.damaged.lock().unwrap_or_else(|e| e.into_inner());
            damaged.clear();
            save_pending_repair(&self.plan.root.join(PENDING_REPAIR), &damaged);
        }
        if let Some(cb) = self.req.on_wait {
            cb(false);
        }
        if !repaired {
            // Repair given up: the intact members are placed and the damage reported, as the
            // progressive path does (a fallback would not see the holes any more: the feed
            // forgets them when it gives up).
            return Err(if is_cancelled() {
                Step::Fail(Failure::Canceled)
            } else {
                Step::GaveUp
            });
        }
        let mut volumes: Vec<u32> = if self.adopted_holed.is_empty() {
            self.holes
                .iter()
                .flat_map(|h| h.packed.iter().map(|s| s.volume))
                .collect()
        } else {
            (0..self.plan.names.len() as u32).collect()
        };
        volumes.sort_unstable();
        volumes.dedup();
        // Volumes a hole of this run named must be on disk; the rest of an all-volumes refill
        // skips a volume that has no file (one rebuilt from a duplicate): a hole it was needed
        // for stays and verify sends the set to the progressive path.
        let required: Vec<u32> = self
            .holes
            .iter()
            .flat_map(|h| h.packed.iter().map(|s| s.volume))
            .collect();
        for (key, file, member) in self.handles.iter_mut() {
            let name = if self.holes.iter().any(|h| &h.member == key)
                || self.adopted_holed.contains(key)
            {
                String::from_utf8_lossy(&member.name).into_owned()
            } else {
                continue;
            };
            for &v in &volumes {
                let Some(name) = self.plan.names.get(v as usize) else {
                    continue;
                };
                let path = self.plan.dir.join(name);
                let opened = open_repaired(&self.feed, &path);
                if !required.contains(&v) && opened.is_none() {
                    continue;
                }
                let mut f = match opened {
                    Some(r) => r.map_err(|e| {
                        Step::Fallback(format!("repaired volume {}: {e}", path.display()))
                    })?,
                    None => {
                        return Err(Step::Fallback(format!(
                            "repaired volume {}: not on disk",
                            path.display()
                        )))
                    }
                };
                let len = f
                    .seek(std::io::SeekFrom::End(0))
                    .map_err(|e| Step::Fallback(format!("repaired volume {}: {e}", path.display())))?;
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
        for i in 0..self.handles.len() {
            match self.verify_one(i, is_cancelled)? {
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

    /// Polls member `i`'s verification to its verdict.
    fn verify_one(
        &mut self,
        i: usize,
        is_cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<zipfast::partial::PartialResult<Verdict>, Step> {
        let parker = Arc::new(Parker {
            thread: std::thread::current(),
            woken: AtomicBool::new(false),
        });
        let waker = Waker::from(parker.clone());
        let mut cx = Context::from_waker(&waker);
        let file = &mut self.handles[i].1;
        loop {
            if is_cancelled() {
                return Err(Step::Fail(Failure::Canceled));
            }
            parker.woken.store(false, Ordering::Release);
            match file.poll_verify(&mut cx) {
                Poll::Ready(r) => return Ok(r),
                Poll::Pending => {
                    if !parker.woken.load(Ordering::Acquire) {
                        std::thread::park_timeout(PARK_SLICE);
                    }
                }
            }
        }
    }

    /// The repair was given up: publishes the members that had no hole and verify (or have no
    /// checksum to verify against), as the progressive path places them; the rest is dropped
    /// with the state.
    fn publish_intact(&mut self, is_cancelled: &mut dyn FnMut() -> bool) -> Result<(), String> {
        let mut keep = std::collections::HashSet::new();
        for i in 0..self.handles.len() {
            let key = self.handles[i].0.clone();
            // A member whose holes were all patched from repaired volumes while extracting is
            // judged like any other below; one with a hole still pending is not intact.
            if self.inline.pending.iter().any(|(k, _, _)| *k == key) {
                continue;
            }
            let intact = match self.verify_one(i, is_cancelled) {
                Ok(Ok(Verdict::Ok { .. })) => true,
                Ok(Ok(Verdict::Incomplete { holes, .. })) => holes.is_empty(),
                _ => false,
            };
            if intact {
                let m = &self.handles[i].2;
                if let Ok(rel) =
                    zipfast::path::sanitize(&String::from_utf8_lossy(&m.name), m.meta.windows_names)
                {
                    keep.insert(rel);
                }
            }
        }
        self.publish_where(Some(&keep))
    }

    /// Every member verified: moves the outputs from the stage into the destination (with the
    /// copy path's modes and folder modes) and drops the partial state.
    fn publish(&mut self) -> Result<(), String> {
        self.publish_where(None)
    }

    /// Publishes the stage's files (only those in `keep`, when given).
    fn publish_where(
        &mut self,
        keep: Option<&std::collections::HashSet<PathBuf>>,
    ) -> Result<(), String> {
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
            if keep.is_some_and(|k| !k.contains(rel)) {
                continue;
            }
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
