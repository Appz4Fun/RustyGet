//! RAR extraction through zipfast's progressive session.
//!
//! One session reads a RAR set from its first volume to the end. Volumes are handed to the
//! session as they become available through a [`VolumeFeed`]: a volume that is still
//! downloading parks the session until the feed wakes it, a damaged volume is handed over with
//! its lost byte ranges marked as holes, and extraction runs past them. Stored members with
//! holes come back as partial outputs; once the damaged volumes are repaired (twin article
//! swap, alt borrow or par2, all of which repair the volume files in place) the missing inner
//! bytes are read back out of the repaired volumes and patched into the output, which is then
//! verified against the archive checksums and moved into place.
//!
//! Covered here: RAR5 with every method, RAR4 plain and salted-AES stored members, encrypted
//! headers and split members. Everything zipfast reports as unsupported (RAR4 compressed and
//! older codecs) is left to `archive_native`, see [`Unsupported`].

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use zipfast::progressive::{
    Artifact, ArtifactAftermath, ArtifactEvidence, ArtifactPublication, ArtifactRead, FailureKind,
    HoleFill, HolePolicy, MemberOutcome, ProgressiveEvent, ProgressiveFailure, ProgressiveOptions,
    ProgressiveResult, ProgressiveSession, RangeRead, RangeSource, RecoveryPublication,
    SourceInfo, VolumeAnswer, VolumeRequest, VolumeSource,
};

/// How long the driver sleeps between cancellation checks while the session is parked.
const PARK_SLICE: Duration = Duration::from_millis(50);
/// Largest patch zipfast accepts in one call.
const PATCH_CHUNK: u64 = 1 << 20;

/// What the feed knows about one volume right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VolumeView {
    /// The volume file is complete on disk. `holes` are byte ranges that never arrived intact
    /// (missing or CRC-failed articles), sorted and disjoint.
    Ready { holes: Vec<(u64, u64)> },
    /// Not downloaded yet: the session parks and asks again shortly.
    NotYet,
    /// Will never arrive.
    Missing,
}

/// The download side of a session: which volumes exist and which bytes in them are lost.
pub trait VolumeFeed: Send + 'static {
    fn volume(&mut self, path: &Path) -> VolumeView;
    /// Blocks (polling `is_cancelled`) until every damaged volume this feed handed out has been
    /// repaired in place. Returns false when the repair was given up or cancelled.
    fn wait_repaired(&mut self, _is_cancelled: &mut dyn FnMut() -> bool) -> bool {
        false
    }
}

/// Volume bytes at the start and end that hold archive headers. A hole there cannot be read
/// past, so such a volume waits for its repair instead.
const HEADER_GUARD_HEAD: i64 = 64 * 1024;
const HEADER_GUARD_TAIL: i64 = 4 * 1024;

/// Converts a damaged volume's lost ranges into holes the session can skip, or an empty list
/// when extraction should wait for the repair instead (no known ranges, or a range that touches
/// the volume's headers).
pub fn skippable_holes(
    holes: &[crate::dupefallback::NzbgetRsByteRange],
    file_size: i64,
) -> Vec<(u64, u64)> {
    if file_size <= HEADER_GUARD_HEAD + HEADER_GUARD_TAIL {
        return Vec::new();
    }
    let mut out: Vec<(u64, u64)> = Vec::with_capacity(holes.len());
    for h in holes {
        if h.size <= 0 {
            continue;
        }
        if h.offset < HEADER_GUARD_HEAD || h.offset + h.size > file_size - HEADER_GUARD_TAIL {
            return Vec::new();
        }
        out.push((h.offset as u64, h.size as u64));
    }
    out.sort_unstable();
    // Coalesce overlapping or touching ranges.
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(out.len());
    for (s, l) in out {
        match merged.last_mut() {
            Some((ms, ml)) if *ms + *ml >= s => *ml = (*ml).max(s + l - *ms),
            _ => merged.push((s, l)),
        }
    }
    merged
}

/// A set whose volumes are all on disk: present volumes are complete, absent ones missing.
pub struct CompleteSet;

impl VolumeFeed for CompleteSet {
    fn volume(&mut self, path: &Path) -> VolumeView {
        if path.is_file() {
            VolumeView::Ready { holes: Vec::new() }
        } else {
            VolumeView::Missing
        }
    }
}

/// Why a session ended without extracting everything. Each kind maps to a typed zipfast failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    PasswordRequired,
    WrongPassword,
    Checksum(String),
    MissingVolume(String),
    /// A hole that could not be skipped (compressed data, header, unknown encrypted size) and
    /// was not repaired.
    Damaged(String),
    UnsafePath(String),
    Canceled,
    Other(String),
}

impl Failure {
    pub fn is_password(&self) -> bool {
        matches!(self, Failure::PasswordRequired | Failure::WrongPassword)
    }
    pub fn is_damage(&self) -> bool {
        matches!(
            self,
            Failure::Checksum(_) | Failure::MissingVolume(_) | Failure::Damaged(_)
        )
    }
    fn from_progressive(e: &ProgressiveFailure) -> Self {
        let at = describe_context(e);
        match &e.kind {
            FailureKind::PasswordRequired => Failure::PasswordRequired,
            FailureKind::WrongPassword => Failure::WrongPassword,
            FailureKind::ChecksumMismatch { .. } => Failure::Checksum(at),
            FailureKind::MissingVolume => Failure::MissingVolume(at),
            FailureKind::HeaderHole | FailureKind::DataHole | FailureKind::Truncated => {
                Failure::Damaged(format!("{:?}{at}", e.kind))
            }
            FailureKind::UnsafePath => Failure::UnsafePath(at),
            FailureKind::Canceled => Failure::Canceled,
            FailureKind::Rar(r) => {
                use zipfast::error::RarFailure as R;
                match &**r {
                    R::PasswordRequired { .. } => Failure::PasswordRequired,
                    R::WrongPassword { .. } => Failure::WrongPassword,
                    // RAR4 has no password verifier: a failed check after decryption is either
                    // a wrong password or damaged data. Both are reported as a checksum failure.
                    R::Integrity { .. } | R::EncryptedIntegrity { .. } => {
                        Failure::Checksum(format!("{at} ({r})"))
                    }
                    R::MissingVolume { path } => {
                        Failure::MissingVolume(format!(" {}", path.display()))
                    }
                    R::Canceled => Failure::Canceled,
                    other => Failure::Other(format!("{other}{at}")),
                }
            }
            other => Failure::Other(format!("{other:?}{at}")),
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::PasswordRequired => write!(f, "password required"),
            Failure::WrongPassword => write!(f, "wrong password"),
            Failure::Checksum(at) => write!(f, "checksum mismatch{at}"),
            Failure::MissingVolume(at) => write!(f, "missing volume{at}"),
            Failure::Damaged(what) => write!(f, "damaged archive data: {what}"),
            Failure::UnsafePath(at) => write!(f, "unsafe path{at}"),
            Failure::Canceled => write!(f, "Aborted"),
            Failure::Other(what) => write!(f, "{what}"),
        }
    }
}

/// zipfast cannot read this set through a session (RAR4 compressed or older codecs, a format
/// that is not RAR). Nothing was written; the caller uses its fallback extractor.
#[derive(Debug)]
pub struct Unsupported(pub String);

pub enum Error {
    Unsupported(Unsupported),
    Failed(Failure),
}

impl From<Failure> for Error {
    fn from(f: Failure) -> Self {
        Error::Failed(f)
    }
}

fn describe_context(e: &ProgressiveFailure) -> String {
    let c = &e.context;
    let mut s = String::new();
    if let Some(v) = &c.expected {
        s.push_str(&format!(" in {}", v.to_string_lossy()));
    } else if let Some(i) = c.volume_index {
        s.push_str(&format!(" in volume {}", i + 1));
    }
    if let Some(off) = c.inner_offset {
        s.push_str(&format!(" at inner offset {off}"));
    } else if let Some(off) = c.packed_offset {
        s.push_str(&format!(" at offset {off}"));
    }
    s
}

/// True when a failure means "this format or method is not handled by the session" (so a
/// fallback extractor should try) rather than "the archive is bad".
fn is_unsupported(e: &ProgressiveFailure) -> bool {
    match &e.kind {
        FailureKind::UnsupportedMethod => true,
        FailureKind::Rar(r) => matches!(
            **r,
            zipfast::error::RarFailure::Unsupported(_) | zipfast::error::RarFailure::NotRar
        ),
        _ => false,
    }
}

/// Parameters of one extraction.
pub struct Request<'a> {
    pub first_volume: &'a Path,
    pub dest: &'a Path,
    pub password: &'a str,
    /// Overwrite files that are already in `dest`.
    pub overwrite: bool,
}

/// Extracts the RAR set starting at `req.first_volume` into `req.dest`.
///
/// `log_cb` receives NZBGet-style unrar lines (`Unrar: Extracting  <name>` per member,
/// `Unrar: All OK` at the end) so existing output tracking keeps working. The returned error is
/// typed; callers should branch on it rather than on the log text.
pub fn extract<F>(
    req: &Request<'_>,
    feed: Box<dyn VolumeFeed>,
    log_cb: &mut F,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<zipfast::extract::Summary, Error>
where
    F: FnMut(i32, &str),
{
    let dir = req
        .first_volume
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let first_name: OsString = req
        .first_volume
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    let feed = Arc::new(Mutex::new(feed));
    let pending: PendingWait = Arc::new(Mutex::new(None));
    let provider = FeedProvider {
        dir: dir.clone(),
        feed: feed.clone(),
        pending: pending.clone(),
    };
    let mut opts = ProgressiveOptions::new(
        zipfast::extract::Options {
            mode: zipfast::extract::Mode::Extract,
            dest: req.dest.to_path_buf(),
            overwrite: if req.overwrite {
                zipfast::extract::Overwrite::Always
            } else {
                zipfast::extract::Overwrite::Skip
            },
            flatten: false,
            threads: None,
            protected_paths: Default::default(),
            parallel_min: None,
        },
        first_name,
    );
    opts.hole_policy = HolePolicy::PartialStored {
        fill: HoleFill::Sparse,
    };
    opts.artifact_aftermath = ArtifactAftermath::RetainProvisional;
    opts.recovery_publication = RecoveryPublication::VerifiedInner;
    if !req.password.is_empty() {
        opts.set_password(Some(req.password.to_string()))
            .map_err(|e| Failure::from_progressive(&e))?;
    }
    let mut session = ProgressiveSession::new(Box::new(provider), opts).map_err(|e| {
        if is_unsupported(&e) {
            Error::Unsupported(Unsupported(format!("{:?}", e.kind)))
        } else {
            Error::Failed(Failure::from_progressive(&e))
        }
    })?;
    let mut drive = Driver::new(&mut session, feed.clone(), pending);
    let mut summary = zipfast::extract::Summary::default();
    let mut names: std::collections::HashMap<zipfast::progressive::MemberId, Vec<u8>> =
        Default::default();
    let mut partial: Vec<(Vec<u8>, Artifact)> = Vec::new();
    let mut provisional: Vec<(Vec<u8>, Artifact)> = Vec::new();
    let mut anything_written = false;
    let mut first_failure: Option<Failure> = None;

    loop {
        let event = match drive.next_event(is_cancelled) {
            Ok(Some(ev)) => ev,
            Ok(None) => break,
            Err(e) => {
                if !anything_written && is_unsupported(&e) {
                    return Err(Error::Unsupported(Unsupported(format!("{:?}", e.kind))));
                }
                return Err(Failure::from_progressive(&e).into());
            }
        };
        match event {
            ProgressiveEvent::VolumeOpened { index, .. } => {
                let name = volume_display_name(req.first_volume, index);
                log_cb(4, &format!("Unrar: Extracting from {name}"));
            }
            ProgressiveEvent::MemberStarted { member, name, .. } => {
                let display = String::from_utf8_lossy(&name).into_owned();
                log_cb(4, &format!("Unrar: Extracting  {display}"));
                names.insert(member, name.to_vec());
            }
            ProgressiveEvent::BytesWritten { valid_bytes, .. } => {
                anything_written |= valid_bytes > 0;
                summary.bytes += valid_bytes;
            }
            ProgressiveEvent::MemberFinished { member, outcome } => {
                let name = names.remove(&member).unwrap_or_default();
                match outcome {
                    MemberOutcome::Verified(_) | MemberOutcome::RecoveredInner(_) => {
                        summary.files += 1;
                    }
                    MemberOutcome::Directory { .. } => summary.dirs += 1,
                    MemberOutcome::Skipped(_) => summary.skipped += 1,
                    MemberOutcome::Tested(_) => {}
                    MemberOutcome::Provisional(a) => provisional.push((name, a)),
                    MemberOutcome::Partial(a) => partial.push((name, a)),
                    MemberOutcome::PublishedWithFailure { failure, .. } => {
                        summary.files += 1;
                        first_failure.get_or_insert(Failure::from_progressive(&failure));
                    }
                    MemberOutcome::Stopped {
                        failure, artifact, ..
                    } => {
                        if let Some(mut a) = artifact {
                            let _ = a.discard();
                        }
                        if !anything_written && is_unsupported(&failure) {
                            return Err(Error::Unsupported(Unsupported(format!(
                                "{:?}",
                                failure.kind
                            ))));
                        }
                        let f = Failure::from_progressive(&failure);
                        log_cb(1, &unrar_error_line(&f, &name));
                        first_failure.get_or_insert(f);
                    }
                    _ => {}
                }
            }
            ProgressiveEvent::CapacityNeeded { .. } => {
                // Retained provisional/partial artifacts hit the session's slot limit. Publish
                // the provisional ones now to free slots; partial ones must wait for repair.
                for (name, mut a) in provisional.drain(..) {
                    if let Err(f) = publish_copy(drive.session, &mut a, req, &name) {
                        first_failure.get_or_insert(f);
                    } else {
                        summary.files += 1;
                    }
                }
                if partial.len() >= drive.session_artifact_limit() {
                    let f = Failure::Damaged("too many damaged members waiting for repair".into());
                    first_failure.get_or_insert(f);
                    drive.session.control().cancel();
                }
            }
            ProgressiveEvent::SessionTerminated {
                failure, artifact, ..
            } => {
                if let Some(mut a) = artifact {
                    let _ = a.discard();
                }
                if !anything_written && is_unsupported(&failure) {
                    return Err(Error::Unsupported(Unsupported(format!("{:?}", failure.kind))));
                }
                let f = Failure::from_progressive(&failure);
                log_cb(1, &unrar_error_line(&f, b""));
                first_failure.get_or_insert(f);
                break;
            }
            _ => {}
        }
    }

    for (name, mut a) in provisional.drain(..) {
        match publish_copy(drive.session, &mut a, req, &name) {
            Ok(()) => summary.files += 1,
            Err(f) => {
                first_failure.get_or_insert(f);
            }
        }
    }

    if !partial.is_empty() && first_failure.is_none() {
        let names: Vec<String> = partial
            .iter()
            .map(|(n, a)| {
                format!(
                    "{} ({} bytes missing)",
                    String::from_utf8_lossy(n),
                    a.holes().iter().map(|h| h.len).sum::<u64>()
                )
            })
            .collect();
        log_cb(
            0,
            &format!(
                "Unrar: extracted past damage in {}; waiting for the volumes to be repaired",
                names.join(", ")
            ),
        );
        let repaired = feed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .wait_repaired(is_cancelled);
        if !repaired {
            first_failure = Some(if is_cancelled() {
                Failure::Canceled
            } else {
                Failure::Damaged("the damaged volumes were not repaired".into())
            });
        } else {
            for (name, mut a) in partial.drain(..) {
                match patch_from_repaired_volumes(drive.session, &mut a, &dir, req, &name, is_cancelled)
                    .and_then(|()| publish_copy(drive.session, &mut a, req, &name))
                {
                    Ok(()) => {
                        summary.files += 1;
                        log_cb(
                            0,
                            &format!(
                                "Unrar: repaired {} from the repaired volumes",
                                String::from_utf8_lossy(&name)
                            ),
                        );
                    }
                    Err(f) => {
                        log_cb(1, &unrar_error_line(&f, &name));
                        first_failure.get_or_insert(f);
                    }
                }
            }
        }
    }
    for (_, mut a) in partial {
        let _ = a.discard();
    }

    match first_failure {
        None => {
            log_cb(4, "Unrar: All OK");
            Ok(summary)
        }
        Some(f) => Err(f.into()),
    }
}

fn unrar_error_line(f: &Failure, name: &[u8]) -> String {
    let who = if name.is_empty() {
        String::new()
    } else {
        format!(" in {}", String::from_utf8_lossy(name))
    };
    match f {
        Failure::PasswordRequired | Failure::WrongPassword => {
            "Unrar: The specified password is incorrect.".to_string()
        }
        Failure::Checksum(at) => format!("Unrar: CRC error{who}{at}"),
        Failure::Canceled => "Aborted".to_string(),
        other => format!("Unrar: ERROR{who}: {other}"),
    }
}

fn volume_display_name(first: &Path, index: u32) -> String {
    let mut p = first.to_path_buf();
    for _ in 0..index {
        match zipfast::volume::next_volume(&p) {
            Some(n) => p = n,
            None => break,
        }
    }
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Reads the missing inner bytes of `artifact` out of the (now repaired) volume set in `dir`
/// and patches them in.
fn patch_from_repaired_volumes(
    session: &mut ProgressiveSession,
    artifact: &mut Artifact,
    dir: &Path,
    req: &Request<'_>,
    name: &[u8],
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), Failure> {
    use crate::streamrepair::{ContentMapper, DiskSourceSet, MemberFormat};

    let members = rar_set_members(req.first_volume);
    if members.is_empty() {
        return Err(Failure::Damaged("no volumes to read the repair from".into()));
    }
    let dir_bytes = path_bytes(dir);
    let sources = DiskSourceSet::new(&dir_bytes, &members);
    let set = crate::streamrepair::MemberSet {
        format: MemberFormat::Rar,
        members: (0..members.len() as i32).collect(),
    };
    let map = ContentMapper::build_map(
        &members,
        &set,
        &sources,
        (!req.password.is_empty()).then_some(req.password),
    )
    .map_err(|e| Failure::Damaged(format!("cannot map the repaired volumes: {e}")))?;
    if map.inner_name_bytes() != name || Some(map.inner_size() as u64) != artifact.size() {
        return Err(Failure::Damaged(format!(
            "repaired volumes map {} ({} bytes), not {}",
            map.inner_name(),
            map.inner_size(),
            String::from_utf8_lossy(name)
        )));
    }
    let mut buf = Vec::new();
    for hole in artifact.holes().to_vec() {
        let mut at = hole.offset;
        let end = hole.offset + hole.len;
        while at < end {
            if is_cancelled() {
                return Err(Failure::Canceled);
            }
            let n = (end - at).min(PATCH_CHUNK);
            let range = crate::dupefallback::NzbgetRsByteRange {
                offset: at as i64,
                size: n as i64,
            };
            if !map.read_donor_inner(&sources, range, &mut buf) || buf.len() as u64 != n {
                return Err(Failure::Damaged(format!(
                    "cannot read inner bytes {at}..{} from the repaired volumes",
                    at + n
                )));
            }
            session
                .patch(artifact, at, &buf)
                .map_err(|e| Failure::from_progressive(&e))?;
            at += n;
        }
    }
    Ok(())
}

/// Volume files of the set starting at `first`, in order, that exist on disk.
fn rar_set_members(first: &Path) -> Vec<crate::streamrepair::SetMember> {
    let mut out = Vec::new();
    let mut p = first.to_path_buf();
    while let Ok(meta) = std::fs::metadata(&p) {
        let name = p
            .file_name()
            .map(|n| path_bytes(Path::new(n)))
            .unwrap_or_default();
        out.push(crate::streamrepair::SetMember::new(name, meta.len() as i64));
        match zipfast::volume::next_volume(&p) {
            Some(n) if n != p => p = n,
            _ => break,
        }
    }
    out
}

fn path_bytes(p: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        p.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        p.to_string_lossy().as_bytes().to_vec()
    }
}

/// Verifies `artifact` (when its evidence is not already a pass) and moves it into place.
///
/// zipfast publishes a member itself only when the volumes are declared as protected native
/// files for the whole set; a download in progress cannot promise that, so verified outputs come
/// back unpublished and are copied to their final name here.
fn publish_copy(
    session: &mut ProgressiveSession,
    artifact: &mut Artifact,
    req: &Request<'_>,
    name: &[u8],
) -> Result<(), Failure> {
    if artifact.publication() != ArtifactPublication::Unpublished {
        return Ok(());
    }
    let verified = match artifact.evidence() {
        ArtifactEvidence::Verified(_) => true,
        ArtifactEvidence::Provisional => false,
    };
    if !verified {
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        loop {
            match session.poll_verify(artifact, &mut cx) {
                Poll::Ready(Ok(_)) => break,
                Poll::Ready(Err(e)) => {
                    let _ = artifact.discard();
                    return Err(Failure::from_progressive(&e));
                }
                Poll::Pending => std::thread::sleep(Duration::from_millis(1)),
            }
        }
        if artifact.publication() != ArtifactPublication::Unpublished {
            return Ok(());
        }
        if !matches!(artifact.evidence(), ArtifactEvidence::Verified(_)) {
            let _ = artifact.discard();
            return Err(Failure::Checksum(format!(
                " in {}: no archive checksum to verify against",
                String::from_utf8_lossy(name)
            )));
        }
    }
    let rel = zipfast::path::sanitize(&String::from_utf8_lossy(name), false)
        .map_err(|e| Failure::UnsafePath(format!(" {}: {e}", String::from_utf8_lossy(name))))?;
    let result = copy_artifact(artifact, req.dest, &rel, req.overwrite);
    let _ = artifact.discard();
    result
}

fn copy_artifact(
    artifact: &mut Artifact,
    dest: &Path,
    rel: &Path,
    overwrite: bool,
) -> Result<(), Failure> {
    let target = dest.join(rel);
    let target = target.as_path();
    let io = |e: std::io::Error| Failure::Other(format!("cannot write {}: {e}", target.display()));
    if target.exists() && !overwrite {
        return Ok(());
    }
    if let Some(parent_rel) = rel.parent().filter(|p| !p.as_os_str().is_empty()) {
        crate::archive_native::create_dir_all_no_follow(dest, parent_rel)
            .map_err(|e| Failure::Other(format!("cannot create {}: {e}", parent_rel.display())))?;
    } else {
        std::fs::create_dir_all(dest).map_err(io)?;
    }
    let mut tmp_name = target.as_os_str().to_os_string();
    tmp_name.push(".rgpart");
    let tmp = PathBuf::from(tmp_name);
    let mut out = std::fs::File::create(&tmp).map_err(io)?;
    let mut buf = vec![0u8; PATCH_CHUNK as usize];
    let mut at = 0u64;
    loop {
        match artifact.read_at(at, &mut buf) {
            Ok(ArtifactRead::Filled { count }) => {
                out.write_all(&buf[..count]).map_err(io)?;
                at += count as u64;
            }
            Ok(ArtifactRead::End) => break,
            Ok(ArtifactRead::Hole { .. }) => {
                drop(out);
                let _ = std::fs::remove_file(&tmp);
                return Err(Failure::Damaged(format!(
                    "{} still has a hole at {at}",
                    target.display()
                )));
            }
            Err(e) => {
                drop(out);
                let _ = std::fs::remove_file(&tmp);
                return Err(Failure::from_progressive(&e));
            }
        }
    }
    out.flush().map_err(io)?;
    drop(out);
    std::fs::rename(&tmp, target).map_err(io)
}

/// The volume the session is parked on and the waker that resumes it.
type PendingWait = Arc<Mutex<Option<(PathBuf, Waker)>>>;

/// Thread-parking driver around `poll_event`.
struct Driver<'s> {
    session: &'s mut ProgressiveSession,
    parker: Arc<Parker>,
    feed: Arc<Mutex<Box<dyn VolumeFeed>>>,
    pending: PendingWait,
}

struct Parker {
    thread: std::thread::Thread,
    woken: AtomicBool,
}

impl Wake for Parker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

impl<'s> Driver<'s> {
    fn new(
        session: &'s mut ProgressiveSession,
        feed: Arc<Mutex<Box<dyn VolumeFeed>>>,
        pending: PendingWait,
    ) -> Self {
        Self {
            session,
            parker: Arc::new(Parker {
                thread: std::thread::current(),
                woken: AtomicBool::new(false),
            }),
            feed,
            pending,
        }
    }

    /// Wakes the session when the volume it is parked on has changed state.
    fn recheck_pending(&self) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let changed = match pending.as_ref() {
            Some((path, _)) => !matches!(
                self.feed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .volume(path),
                VolumeView::NotYet
            ),
            None => false,
        };
        if changed {
            if let Some((_, waker)) = pending.take() {
                waker.wake();
            }
        }
    }

    fn session_artifact_limit(&self) -> usize {
        zipfast::progressive::ProgressiveLimits::default().retained_artifacts as usize
    }

    fn next_event(
        &mut self,
        is_cancelled: &mut dyn FnMut() -> bool,
    ) -> ProgressiveResult<Option<ProgressiveEvent>> {
        let waker = Waker::from(self.parker.clone());
        let mut cx = Context::from_waker(&waker);
        let mut cancel_sent = false;
        loop {
            if !cancel_sent && is_cancelled() {
                self.session.control().cancel();
                cancel_sent = true;
            }
            self.parker.woken.store(false, Ordering::Release);
            match self.session.poll_event(&mut cx) {
                Poll::Ready(r) => return r,
                Poll::Pending => {
                    if !self.parker.woken.load(Ordering::Acquire) {
                        std::thread::park_timeout(PARK_SLICE);
                    }
                    self.recheck_pending();
                }
            }
        }
    }
}

/// Hands the feed's volumes to the session.
struct FeedProvider {
    dir: PathBuf,
    feed: Arc<Mutex<Box<dyn VolumeFeed>>>,
    pending: PendingWait,
}

impl VolumeSource for FeedProvider {
    fn volume(
        &mut self,
        request: &VolumeRequest,
        cx: &mut Context<'_>,
    ) -> ProgressiveResult<VolumeAnswer> {
        let path = self.dir.join(&request.expected);
        let mut feed = self.feed.lock().unwrap_or_else(|e| e.into_inner());
        match feed.volume(&path) {
            VolumeView::Ready { holes } => {
                let native = zipfast::progressive::ImmutableFileSource::open(&path)?;
                let info = native.info();
                Ok(VolumeAnswer::Ready(Box::new(HoledVolume {
                    native,
                    info,
                    holes,
                })))
            }
            VolumeView::NotYet => {
                // The driver asks the feed again while parked and wakes the session when the
                // volume's state changes.
                *self.pending.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((path, cx.waker().clone()));
                Ok(VolumeAnswer::Wait)
            }
            VolumeView::Missing => Ok(VolumeAnswer::Missing),
        }
    }
}

/// A complete volume file whose lost ranges read as permanent holes.
struct HoledVolume {
    native: zipfast::progressive::ImmutableFileSource,
    info: SourceInfo,
    holes: Vec<(u64, u64)>,
}

impl RangeSource for HoledVolume {
    fn info(&self) -> SourceInfo {
        self.info
    }
    fn backing_files(&self) -> &[std::fs::File] {
        self.native.backing_files()
    }
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        cx: &mut Context<'_>,
    ) -> ProgressiveResult<RangeRead> {
        if let Some(&(start, len)) = self
            .holes
            .iter()
            .find(|&&(s, l)| s <= offset && offset < s + l)
        {
            // A hole answer may not be longer than the read it answers.
            return Ok(RangeRead::Hole {
                len: (start + len - offset).min(buf.len() as u64),
            });
        }
        let mut n = buf.len();
        if let Some(&(start, _)) = self.holes.iter().find(|&&(s, _)| s > offset) {
            n = n.min((start - offset) as usize);
        }
        self.native.read_at(offset, &mut buf[..n], cx)
    }
}

#[cfg(test)]
mod tests;
