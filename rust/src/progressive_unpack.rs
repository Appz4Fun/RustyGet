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
    /// True when every volume of the set is on disk and will not change: the session can then
    /// publish verified members itself instead of having them copied into place.
    fn set_is_final(&self) -> bool {
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
    fn set_is_final(&self) -> bool {
        true
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

/// What happens to a file that already exists where a member is extracted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overwrite {
    Skip,
    Always,
    /// Keep both: the member gets a free `<stem>_<n>.<ext>` name.
    Rename,
}

/// Parameters of one extraction.
pub struct Request<'a> {
    pub first_volume: &'a Path,
    pub dest: &'a Path,
    pub password: &'a str,
    /// What to do with a file that is already in `dest`.
    pub overwrite: Overwrite,
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
    // A set that is complete on disk is declared as protected native files, which lets the
    // session publish verified members itself. Each volume holds one handle.
    // In rename mode every output is copied into place, so each member can get a free name.
    let protected = (feed.set_is_final() && req.overwrite != Overwrite::Rename)
        .then(|| protect_volumes(req.first_volume))
        .flatten();
    let final_set = protected.is_some();
    let feed = Arc::new(Mutex::new(feed));
    let pending: PendingWait = Arc::new(Mutex::new(None));
    let volume_count = protected
        .as_ref()
        .map_or(0, |inv| inv.volumes.len() as u64);
    let provider = FeedProvider {
        dir: dir.clone(),
        feed: feed.clone(),
        pending: pending.clone(),
        protected,
    };
    let mut opts = ProgressiveOptions::new(
        zipfast::extract::Options {
            mode: zipfast::extract::Mode::Extract,
            dest: req.dest.to_path_buf(),
            overwrite: match req.overwrite {
                Overwrite::Skip => zipfast::extract::Overwrite::Skip,
                // Rename: nothing is published by zipfast (no protection declaration); the
                // copy into place picks the name.
                Overwrite::Always | Overwrite::Rename => zipfast::extract::Overwrite::Always,
            },
            flatten: false,
            threads: None,
            protected_paths: Default::default(),
            parallel_min: None,
        },
        first_name,
    );
    opts.limits.volume_handles = opts.limits.volume_handles.max(2 * volume_count + 16);
    opts.limits.protection_paths = opts.limits.protection_paths.max(volume_count + 16);
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
    let mut names: std::collections::HashMap<zipfast::progressive::MemberId, Member> =
        Default::default();
    let mut partial: Vec<(Member, Artifact)> = Vec::new();
    let mut provisional: Vec<(Member, Artifact)> = Vec::new();
    let mut current_volume = req.first_volume.to_path_buf();
    let mut meta_cache = MetaCache::default();
    // Outputs this call copied into place, removed again if the fallback decoder has to redo
    // the set in rename mode (it would otherwise add renamed second copies).
    let mut published: Vec<PathBuf> = Vec::new();
    let mut first_failure: Option<Failure> = None;

    loop {
        let event = match drive.next_event(is_cancelled) {
            Ok(Some(ev)) => ev,
            Ok(None) => break,
            Err(e) => {
                if is_unsupported(&e) {
                    undo_for_fallback(req, &published);
                    return Err(Error::Unsupported(Unsupported(format!("{:?}", e.kind))));
                }
                return Err(Failure::from_progressive(&e).into());
            }
        };
        match event {
            ProgressiveEvent::VolumeOpened { index, .. } => {
                current_volume = volume_path(req.first_volume, index);
                let name = current_volume
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                log_cb(4, &format!("Unrar: Extracting from {name}"));
            }
            ProgressiveEvent::MemberStarted { member, name, .. } => {
                let display = String::from_utf8_lossy(&name).into_owned();
                log_cb(4, &format!("Unrar: Extracting  {display}"));
                // zipfast restores metadata on what it publishes itself (a final set); only
                // copied outputs need it, read once per volume.
                let meta = if final_set {
                    MemberMeta::default()
                } else {
                    meta_cache.lookup(&current_volume, req.password, &name)
                };
                names.insert(
                    member,
                    Member {
                        name: name.to_vec(),
                        meta,
                    },
                );
            }
            ProgressiveEvent::BytesWritten { valid_bytes, .. } => {
                summary.bytes += valid_bytes;
            }
            ProgressiveEvent::MemberFinished { member, outcome } => {
                let member = names.remove(&member).unwrap_or_default();
                let name = member.name.clone();
                match outcome {
                    MemberOutcome::Verified(_) | MemberOutcome::RecoveredInner(_) => {
                        summary.files += 1;
                    }
                    MemberOutcome::Directory { .. } => summary.dirs += 1,
                    MemberOutcome::Skipped(_) => summary.skipped += 1,
                    MemberOutcome::Tested(_) => {}
                    MemberOutcome::Provisional(mut a) => {
                        // Already verified by the session: move it into place now, so the file
                        // is available while later members extract and its slot is freed.
                        if matches!(a.evidence(), ArtifactEvidence::Verified(_)) {
                            match publish_copy(drive.session, &mut a, req, &member, is_cancelled, &mut published) {
                                Ok(()) => summary.files += 1,
                                Err(f) => {
                                    log_cb(1, &unrar_error_line(&f, &name));
                                    first_failure.get_or_insert(f);
                                }
                            }
                        } else {
                            provisional.push((member, a));
                        }
                    }
                    MemberOutcome::Partial(a) => partial.push((member, a)),
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
                        if is_unsupported(&failure) {
                            // A member the session cannot decode (RAR4 compressed after stored
                            // members): the fallback decoder extracts the whole set again.
                            for (_, mut a) in partial.drain(..).chain(provisional.drain(..)) {
                                let _ = a.discard();
                            }
                            undo_for_fallback(req, &published);
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
                for (member, mut a) in provisional.drain(..) {
                    if let Err(f) = publish_copy(drive.session, &mut a, req, &member, is_cancelled, &mut published) {
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
                if is_unsupported(&failure) {
                    for (_, mut a) in partial.drain(..).chain(provisional.drain(..)) {
                        let _ = a.discard();
                    }
                    undo_for_fallback(req, &published);
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

    for (member, mut a) in provisional.drain(..) {
        match publish_copy(drive.session, &mut a, req, &member, is_cancelled, &mut published) {
            Ok(()) => summary.files += 1,
            Err(f) => {
                first_failure.get_or_insert(f);
            }
        }
    }

    if !partial.is_empty() && first_failure.is_none() {
        let names: Vec<String> = partial
            .iter()
            .map(|(m, a)| {
                format!(
                    "{} ({} bytes missing)",
                    String::from_utf8_lossy(&m.name),
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
            for (member, mut a) in partial.drain(..) {
                let name = member.name.clone();
                match patch_from_repaired_volumes(drive.session, &mut a, &dir, req, &name, is_cancelled)
                    .and_then(|()| publish_copy(drive.session, &mut a, req, &member, is_cancelled, &mut published))
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

fn volume_path(first: &Path, index: u32) -> PathBuf {
    let mut p = first.to_path_buf();
    for _ in 0..index {
        match zipfast::volume::next_volume(&p) {
            Some(n) => p = n,
            None => break,
        }
    }
    p
}

/// A member being extracted: its raw name and the metadata restored on a copied output.
#[derive(Clone, Default)]
struct Member {
    name: Vec<u8>,
    meta: MemberMeta,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct MemberMeta {
    /// Unix permission bits, when the archive was made on Unix.
    unix_mode: Option<u32>,
    /// Modification time (seconds, nanoseconds).
    mtime: Option<(i64, u32)>,
}

/// Member metadata by volume, each volume's headers read once.
#[derive(Default)]
struct MetaCache {
    volume: PathBuf,
    members: std::collections::HashMap<Vec<u8>, MemberMeta>,
}

impl MetaCache {
    fn lookup(&mut self, volume: &Path, password: &str, name: &[u8]) -> MemberMeta {
        if self.volume != volume {
            self.volume = volume.to_path_buf();
            self.members = volume_metadata(volume, password);
        }
        self.members.get(name).copied().unwrap_or_default()
    }
}

/// Every member header in `volume`, for the metadata zipfast restores only on the outputs it
/// publishes itself.
fn volume_metadata(volume: &Path, password: &str) -> std::collections::HashMap<Vec<u8>, MemberMeta> {
    use zipfast::rar5::reader::{Block, Volume};
    let mut out = std::collections::HashMap::new();
    let pw = (!password.is_empty()).then_some(password);
    if let Ok(mut vol) = Volume::open_with_password(volume, pw) {
        for _ in 0..1_000_000 {
            match vol.next_block() {
                Ok(Some(Block::File(h))) => {
                    out.entry(h.name.as_bytes().to_vec()).or_insert(MemberMeta {
                        unix_mode: h.is_unix().then_some(h.attributes as u32),
                        mtime: h.mtime.map(|s| (s, h.mtime_nanos)),
                    });
                }
                Ok(Some(Block::End { .. })) | Ok(None) | Err(_) => break,
                Ok(Some(_)) => {}
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    // RAR 1.5-4.x headers: DOS time, and a Unix mode when the attributes are one.
    let (_, _, files) =
        crate::rarreader::read_volume_file(&path_bytes(volume), password.as_bytes(), true);
    for f in files {
        out.entry(f.filename.clone()).or_insert(MemberMeta {
            unix_mode: (f.attr & 0o170000 == 0o100000).then_some(f.attr & 0o7777),
            mtime: crate::archive_native::rar4_dos_mtime(f.time),
        });
    }
    out
}

fn apply_meta(file: &std::fs::File, meta: MemberMeta) {
    #[cfg(unix)]
    if let Some(mode) = meta.unix_mode {
        use std::os::unix::fs::PermissionsExt;
        // Owner keeps read/write; set-id and sticky bits are never restored.
        let _ = file.set_permissions(std::fs::Permissions::from_mode((mode & 0o777) | 0o600));
    }
    if let Some((secs, nanos)) = meta.mtime {
        use std::time::{Duration, UNIX_EPOCH};
        let at = if secs >= 0 {
            UNIX_EPOCH.checked_add(Duration::new(secs as u64, nanos.min(999_999_999)))
        } else {
            UNIX_EPOCH.checked_sub(Duration::from_secs(secs.unsigned_abs()))
        };
        if let Some(at) = at {
            let _ = file.set_modified(at);
        }
    }
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
    // The content map covers the set's main stored payload. Any other member (a second video,
    // a document) is read back by extracting the repaired set again.
    let map = ContentMapper::build_map(
        &members,
        &set,
        &sources,
        (!req.password.is_empty()).then_some(req.password),
    )
    .ok();
    // The content map names the member without its directory.
    let base = name
        .rsplit(|&b| b == b'/' || b == b'\\')
        .next()
        .unwrap_or(name);
    let Some(map) = map.filter(|m| {
        m.inner_name_bytes() == base && Some(m.inner_size() as u64) == artifact.size()
    }) else {
        return patch_by_reextracting(session, artifact, req, name, is_cancelled);
    };
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

/// Extracts the repaired set into a private directory and patches `artifact`'s holes from the
/// member's copy there.
fn patch_by_reextracting(
    session: &mut ProgressiveSession,
    artifact: &mut Artifact,
    req: &Request<'_>,
    name: &[u8],
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), Failure> {
    let scratch = create_unique_dir(req.dest)
        .map_err(|e| Failure::Other(format!("cannot create a repair directory: {e}")))?;
    let result = (|| {
        let sub = Request {
            first_volume: req.first_volume,
            dest: &scratch,
            password: req.password,
            overwrite: Overwrite::Always,
        };
        match extract(&sub, Box::new(CompleteSet), &mut |_, _| {}, is_cancelled) {
            Ok(_) => {}
            Err(Error::Failed(f)) => return Err(f),
            Err(Error::Unsupported(u)) => return Err(Failure::Other(u.0)),
        }
        let rel = zipfast::path::sanitize(&String::from_utf8_lossy(name), false)
            .map_err(|e| Failure::UnsafePath(format!(" {e}")))?;
        let copy = std::fs::File::open(scratch.join(rel))
            .map_err(|e| Failure::Damaged(format!("repaired copy missing: {e}")))?;
        let mut buf = vec![0u8; PATCH_CHUNK as usize];
        for hole in artifact.holes().to_vec() {
            let mut at = hole.offset;
            let end = hole.offset + hole.len;
            while at < end {
                if is_cancelled() {
                    return Err(Failure::Canceled);
                }
                let n = (end - at).min(PATCH_CHUNK) as usize;
                let mut got = 0;
                while got < n {
                    match read_at(&copy, &mut buf[got..n], at + got as u64) {
                        Ok(0) => return Err(Failure::Damaged("repaired copy is short".into())),
                        Ok(k) => got += k,
                        Err(e) => return Err(Failure::Other(e.to_string())),
                    }
                }
                session
                    .patch(artifact, at, &buf[..n])
                    .map_err(|e| Failure::from_progressive(&e))?;
                at += n as u64;
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

/// Creates a new directory under `parent` that did not exist before.
fn create_unique_dir(parent: &Path) -> std::io::Result<PathBuf> {
    use std::sync::atomic::AtomicU64;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(parent)?;
    loop {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = parent.join(format!(".rustyget-repair-{}-{n}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
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
/// Before the fallback decoder extracts the whole set again: in rename mode it would add a
/// renamed second copy of every output already in place, so those are removed first.
fn undo_for_fallback(req: &Request<'_>, published: &[PathBuf]) {
    if req.overwrite == Overwrite::Rename {
        for p in published {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn publish_copy(
    session: &mut ProgressiveSession,
    artifact: &mut Artifact,
    req: &Request<'_>,
    member: &Member,
    is_cancelled: &mut dyn FnMut() -> bool,
    published: &mut Vec<PathBuf>,
) -> Result<(), Failure> {
    let name = member.name.as_slice();
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
            if is_cancelled() {
                session.control().cancel();
                let _ = artifact.discard();
                return Err(Failure::Canceled);
            }
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
    let result = copy_artifact(artifact, req.dest, &rel, req.overwrite, member.meta, is_cancelled)
        .map(|written| published.extend(written));
    let _ = artifact.discard();
    result
}

fn copy_artifact(
    artifact: &mut Artifact,
    dest: &Path,
    rel: &Path,
    overwrite: Overwrite,
    meta: MemberMeta,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<Option<PathBuf>, Failure> {
    let mut target = dest.join(rel);
    let taken = |p: &Path| std::fs::symlink_metadata(p).is_ok();
    match overwrite {
        Overwrite::Skip if taken(&target) => return Ok(None),
        Overwrite::Rename if taken(&target) => target = free_name(&target, &taken),
        _ => {}
    }
    let target = target.as_path();
    let io = |e: std::io::Error| Failure::Other(format!("cannot write {}: {e}", target.display()));
    if let Some(parent_rel) = rel.parent().filter(|p| !p.as_os_str().is_empty()) {
        crate::archive_native::create_dir_all_no_follow(dest, parent_rel)
            .map_err(|e| Failure::Other(format!("cannot create {}: {e}", parent_rel.display())))?;
    } else {
        std::fs::create_dir_all(dest).map_err(io)?;
    }
    let (tmp, mut out) = create_unique_sibling(target).map_err(io)?;
    let fail = |out: std::fs::File, f: Failure| {
        drop(out);
        let _ = std::fs::remove_file(&tmp);
        Err(f)
    };
    let mut buf = vec![0u8; PATCH_CHUNK as usize];
    let mut at = 0u64;
    loop {
        if is_cancelled() {
            return fail(out, Failure::Canceled);
        }
        match artifact.read_at(at, &mut buf) {
            Ok(ArtifactRead::Filled { count }) => {
                if let Err(e) = out.write_all(&buf[..count]) {
                    return fail(out, io(e));
                }
                at += count as u64;
            }
            Ok(ArtifactRead::End) => break,
            Ok(ArtifactRead::Hole { .. }) => {
                let f = Failure::Damaged(format!("{} still has a hole at {at}", target.display()));
                return fail(out, f);
            }
            Err(e) => return fail(out, Failure::from_progressive(&e)),
        }
    }
    if let Err(e) = out.flush() {
        return fail(out, io(e));
    }
    apply_meta(&out, meta);
    drop(out);
    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(io(e));
    }
    Ok(Some(target.to_path_buf()))
}

/// `<stem>_<n>.<ext>` next to `target`, the first one not taken.
fn free_name(target: &Path, taken: &dyn Fn(&Path) -> bool) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let stem = target
        .file_stem()
        .map(|s| s.to_os_string())
        .unwrap_or_else(|| "file".into());
    let ext = target.extension().map(|e| e.to_os_string());
    for n in 1..u32::MAX {
        let mut name = stem.clone();
        name.push(format!("_{n}"));
        if let Some(e) = &ext {
            name.push(".");
            name.push(e);
        }
        let candidate = parent.join(name);
        if !taken(&candidate) {
            return candidate;
        }
    }
    target.to_path_buf()
}

/// Creates a new, uniquely named file next to `target` without touching any existing path.
fn create_unique_sibling(target: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    use std::sync::atomic::AtomicU64;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    loop {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(".rustyget-{}-{n}.part", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
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
    protected: Option<zipfast::progressive::ProtectionInventory>,
}

/// The volumes of the set starting at `first`, opened, as a protection inventory. `None` when a
/// volume cannot be opened.
fn protect_volumes(first: &Path) -> Option<zipfast::progressive::ProtectionInventory> {
    use zipfast::progressive::{ProtectedPath, ProtectedVolume, ProtectionInventory, VolumeBacking};
    let mut volumes = Vec::new();
    let mut p = first.to_path_buf();
    while p.is_file() {
        let file = std::fs::File::open(&p).ok()?;
        let path = std::path::absolute(&p).ok()?;
        volumes.push(ProtectedVolume {
            index: volumes.len() as u32,
            name: p.file_name()?.to_os_string(),
            backing: VolumeBacking::Native(vec![ProtectedPath {
                path,
                file: Some(Arc::new(file)),
            }]),
        });
        match zipfast::volume::next_volume(&p) {
            Some(n) if n != p => p = n,
            _ => break,
        }
        if volumes.len() > 4096 {
            return None;
        }
    }
    (!volumes.is_empty()).then(|| ProtectionInventory {
        volumes,
        auxiliary_paths: Vec::new(),
    })
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
            VolumeView::Ready { holes } => Ok(VolumeAnswer::Ready(Box::new(
                HoledVolume::open(&path, holes).map_err(|cause| {
                    ProgressiveFailure::new(FailureKind::Input {
                        operation: zipfast::progressive::InputOperation::Open,
                        cause,
                    })
                })?,
            ))),
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

    fn protection(&mut self) -> ProgressiveResult<zipfast::progressive::ProtectionDeclaration> {
        Ok(match &self.protected {
            Some(inventory) => zipfast::progressive::ProtectionDeclaration::Complete {
                epoch: 1,
                inventory: inventory.clone(),
            },
            None => zipfast::progressive::ProtectionDeclaration::Opaque,
        })
    }
}

/// A complete volume file whose lost ranges read as permanent holes.
///
/// The repair of a damaged volume writes the lost ranges into the file while the session may
/// still be reading it. Those ranges are holes for the session and are never read, so the
/// file's identity and length are taken once at open and the bytes are read directly rather
/// than through zipfast's immutable-file source, which treats any write as a change.
struct HoledVolume {
    file: std::fs::File,
    info: SourceInfo,
    holes: Vec<(u64, u64)>,
}

impl HoledVolume {
    fn open(path: &Path, holes: Vec<(u64, u64)>) -> std::io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let meta = file.metadata()?;
        #[cfg(unix)]
        let id = {
            use std::os::unix::fs::MetadataExt;
            (u128::from(meta.dev()) << 64) | u128::from(meta.ino())
        };
        #[cfg(not(unix))]
        let id = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            path.hash(&mut h);
            u128::from(h.finish())
        };
        let len = meta.len();
        Ok(Self {
            file,
            info: SourceInfo {
                id: zipfast::progressive::SourceId(id),
                generation: 0,
                extent: len,
                length: zipfast::progressive::SourceLength::Final(len),
            },
            holes,
        })
    }
}

impl RangeSource for HoledVolume {
    fn info(&self) -> SourceInfo {
        self.info
    }
    fn backing_files(&self) -> &[std::fs::File] {
        std::slice::from_ref(&self.file)
    }
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        _cx: &mut Context<'_>,
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
        let mut n = buf.len().min(self.info.extent.saturating_sub(offset) as usize);
        if let Some(&(start, _)) = self.holes.iter().find(|&&(s, _)| s > offset) {
            n = n.min((start - offset) as usize);
        }
        let read = read_at(&self.file, &mut buf[..n], offset);
        match read {
            Ok(0) if n > 0 => Err(ProgressiveFailure::new(FailureKind::Truncated)),
            Ok(count) => Ok(RangeRead::Filled { count }),
            Err(cause) => {
                let mut e = ProgressiveFailure::new(FailureKind::Input {
                    operation: zipfast::progressive::InputOperation::Read,
                    cause,
                });
                e.context.packed_offset = Some(offset);
                Err(e)
            }
        }
    }
}

#[cfg(unix)]
fn read_at(file: &std::fs::File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    use std::os::unix::fs::FileExt;
    loop {
        match file.read_at(buf, offset) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            r => return r,
        }
    }
}

#[cfg(windows)]
fn read_at(file: &std::fs::File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_read(buf, offset)
}

#[cfg(test)]
mod tests;
