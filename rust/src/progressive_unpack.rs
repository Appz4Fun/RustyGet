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
//! Covered here: every RAR version zipfast decodes (RAR 1.5 to 5, every method, encryption,
//! encrypted headers, solid sets and split members). What zipfast still refuses is reported
//! as [`Unsupported`]; RustyGet has no other RAR decoder.

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
    ProgressiveResult, ProgressiveSession, RangeRead, RangeSource, RecoveryPublication, SourceInfo,
    VolumeAnswer, VolumeRequest, VolumeSource,
};

/// Whether sets are declared to zipfast as protected native files (Complete), letting it
/// rename verified outputs into place. zipfast before #116 rechecked every protected volume
/// on each read step and ran large sets out of SourceBookkeeping (a 159-volume 15.6 GiB set on
/// 6790); #116 bounds the per-step recheck. `big_final_set_unpacks` is the gate.
const DECLARE_PROTECTION: bool = true;

mod anchors;
mod persistent;

/// Directory under an unpack destination holding persistent direct-unpack state.
pub(crate) use persistent::STATE_DIR as PERSISTENT_STATE_DIR;

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
    /// Never arrived, but its part was borrowed from a duplicate: read the rebuilt volume
    /// instead (see [`crate::deadvolume`]).
    Virtual(crate::deadvolume::VirtualVolume),
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
    /// Upper bound on the set's volume count while it is still downloading
    /// (0 when unknown). Sizes the session's per-volume handle budget.
    fn expected_volumes(&self) -> u64 {
        0
    }
    /// True once a set that was still downloading has become final: the download ended, every
    /// repair is done and nothing writes the volumes again. Outputs verified from then on can
    /// be moved into place by the session instead of copied (see `FeedProvider::protection`).
    fn set_became_final(&self) -> bool {
        false
    }
    /// True once the download has ended (volumes may still be held for their repair).
    fn download_finished(&self) -> bool {
        false
    }
    /// True when some volume of the download was rebuilt around data borrowed from a duplicate
    /// (see [`crate::deadvolume`]): it has no file, so the set on disk is not the whole set.
    fn has_borrowed(&self) -> bool {
        false
    }
    /// Puts `path` back on hold with its lost ranges: an earlier run (before a restart) was
    /// handed it damaged and its repair had not happened yet, so `wait_repaired` must wait for
    /// it again. A feed that cannot hold a volume ignores this.
    fn remember_damaged(&mut self, _path: &Path, _holes: Vec<(u64, u64)>) {}
    /// False once damaged volume `path` was repaired and released. A feed that does not track
    /// holds says true.
    fn still_held(&self, _path: &Path) -> bool {
        true
    }
    /// Asked only of a volume this feed handed out with holes: true once it was repaired in
    /// place and released while the download goes on (not given up). Its lost ranges can then be read back into the output at once,
    /// while extraction carries on, instead of after the whole set. A feed that does not track
    /// repairs says false, and the holes are refilled after the set as before.
    fn repaired(&self, _path: &Path) -> bool {
        false
    }
    /// Asked of a volume the feed answers `NotYet` for: the volume the download is writing
    /// now, article by article, when its articles go straight into place (direct write). The
    /// extraction then reads each article as soon as it is written instead of waiting for the
    /// finished volume (see [`crate::livewrite`]). `None`: wait for the finished volume.
    fn growing(&mut self, _path: &Path) -> Option<Arc<crate::livewrite::LiveVolume>> {
        None
    }
}

/// Volume bytes at the start and end that may hold archive headers, for a volume whose headers
/// cannot be read (encrypted headers, see [`place_holes`]): a hole there cannot be read past, so
/// such a volume waits for its repair instead. Every other volume is placed by its own headers.
const HEADER_GUARD_HEAD: i64 = 64 * 1024;
const HEADER_GUARD_TAIL: i64 = 4 * 1024;

/// Converts a damaged volume's lost ranges into holes the session can skip, or an empty list
/// when extraction should wait for the repair instead (no known ranges, or a range that touches
/// the volume's headers).
pub fn skippable_holes(
    holes: &[crate::dupefallback::NzbgetRsByteRange],
    file_size: i64,
) -> Vec<(u64, u64)> {
    skippable_holes_behind_head(holes, file_size, HEADER_GUARD_HEAD, 0)
}

/// [`skippable_holes`] for a volume whose headers are known to end at `head_end` (rebuilt from
/// its quick-open record) and whose first file data ends at `data_end`: a hole inside that
/// data is read past, as is any hole the usual guard allows; one between them may hold the
/// next file's header and waits for the repair.
pub fn skippable_holes_behind_head(
    holes: &[crate::dupefallback::NzbgetRsByteRange],
    file_size: i64,
    head_end: i64,
    data_end: i64,
) -> Vec<(u64, u64)> {
    let head_end = head_end.max(1);
    if file_size <= head_end + HEADER_GUARD_TAIL {
        return Vec::new();
    }
    let in_first_data = |h: &crate::dupefallback::NzbgetRsByteRange| {
        h.offset >= head_end && h.offset + h.size <= data_end
    };
    let mut out: Vec<(u64, u64)> = Vec::with_capacity(holes.len());
    for h in holes {
        if h.size <= 0 {
            continue;
        }
        if !(in_first_data(h) || h.offset >= HEADER_GUARD_HEAD)
            || h.offset + h.size > file_size - HEADER_GUARD_TAIL
        {
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

/// Where the lost ranges of a downloaded RAR volume fall, read from the volume's own header
/// chain (zipfast's single-volume member map): the exact data area of every member piece in
/// it, instead of a fixed guard at the volume's edges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HolePlacement {
    /// Every hole lies inside a member's data (sorted, merged): extraction reads past them.
    Data(Vec<(u64, u64)>),
    /// Every hole lies inside member data or behind the last member's data, which ends at
    /// `data_end`: the headers there (quick-open data and the end of archive header) can be
    /// rebuilt (RAR 5.0, see `deadvolume::restore_tail_guarded`), then the rest is read past.
    Tail { data_end: u64 },
    /// A hole covers a header in front of member data: extraction waits for the repair.
    Header,
    /// The headers cannot be read (encrypted without a password, not a RAR volume, unreadable).
    Unknown,
}

/// Sorts `holes` (`offset`, `len`) and joins overlapping or touching ones, dropping empty ones.
pub fn merge_holes(holes: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let mut out: Vec<(u64, u64)> = holes.iter().copied().filter(|&(_, l)| l > 0).collect();
    out.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(out.len());
    for (s, l) in out {
        match merged.last_mut() {
            Some((ms, ml)) if *ms + *ml >= s => *ml = (*ml).max(s + l - *ms),
            _ => merged.push((s, l)),
        }
    }
    merged
}

/// Reads the header chain of the volume at `path` with `holes` (`offset`, `len`) unreadable,
/// stopping at byte `limit` when given. Only header bytes are read.
fn map_holed_volume(
    path: &Path,
    holes: &[(u64, u64)],
    limit: Option<u64>,
) -> Option<Result<zipfast::map::VolumeMap, zipfast::map::MapError>> {
    let mut source = HoledVolume::open(path, holes.to_vec()).ok()?;
    if let Some(limit) = limit.filter(|&l| l < source.info.extent) {
        source.info.extent = limit;
        source.info.length = zipfast::progressive::SourceLength::Final(limit);
    }
    let name = path.file_name()?.to_os_string();
    let mut builder =
        zipfast::map::VolumeMapBuilder::new(Box::new(source), zipfast::map::MapOptions::new(name));
    match builder.poll_map(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(result) => Some(result),
        // A file on disk never answers NotYet.
        Poll::Pending => None,
    }
}

/// Places the lost ranges `holes` (`offset`, `len`) of the complete RAR volume at `path`
/// against its headers. A hole fully inside a member piece's data is skippable; a hole behind
/// the last piece's data lies in the volume's tail; anything else covers a header.
pub fn place_holes(path: &Path, holes: &[(u64, u64)]) -> HolePlacement {
    use zipfast::map::{MapError, VolumeEnd};
    let holes = merge_holes(holes);
    if holes.is_empty() {
        return HolePlacement::Data(Vec::new());
    }
    let Ok(file_len) = std::fs::metadata(path).map(|m| m.len()) else {
        return HolePlacement::Unknown;
    };
    // A file that does not start like a RAR volume (a self-extractor, another format) is not
    // read here; one whose start is lost is placed below (its headers are lost).
    if holes[0].0 >= 8 {
        let mut sig = [0u8; 8];
        let read = std::fs::File::open(path).and_then(|f| {
            use std::io::Read;
            f.take(8).read_exact(&mut sig)
        });
        if read.is_err() || &sig[..6] != b"Rar!\x1a\x07" {
            return HolePlacement::Unknown;
        }
    }
    let spans_of = |map: &zipfast::map::VolumeMap| -> Vec<(u64, u64)> {
        map.members
            .iter()
            .map(|m| (m.packed.start, m.packed.end.min(file_len)))
            .filter(|&(s, e)| s < e)
            .collect()
    };
    let inside =
        |spans: &[(u64, u64)], (s, l): (u64, u64)| spans.iter().any(|&(a, b)| a <= s && s + l <= b);
    // Whole volume first: when its headers are all readable, every hole is in data or in the
    // tail (quick-open data, padding). A hole in a header read on the way stops the walk.
    let (map, cut) = match map_holed_volume(path, &holes, None) {
        None => return HolePlacement::Unknown,
        Some(Ok(map)) => match map.end {
            VolumeEnd::Complete => (map, None),
            // A RAR 1.5-2.x volume may end without an end of archive header: the walk ran to the
            // end of the file right behind the last member's data.
            VolumeEnd::Truncated { offset } if offset >= file_len => (map, None),
            VolumeEnd::Locked { .. } => return HolePlacement::Unknown,
            _ => return HolePlacement::Header,
        },
        Some(Err(MapError::HeaderHole { offset, .. })) => {
            // Read again up to the hole that hit a header: the members in front of it.
            let cut = holes
                .iter()
                .find(|&&(s, l)| s <= offset && offset < s + l)
                .map_or(offset, |&(s, _)| s);
            match map_holed_volume(path, &holes, Some(cut)) {
                Some(Ok(map)) if matches!(map.end, VolumeEnd::Truncated { .. }) => {
                    let VolumeEnd::Truncated { offset: at } = map.end else {
                        unreachable!()
                    };
                    // Only headers behind the last piece's data (the tail: quick-open data,
                    // end of archive) can be rebuilt; the walk lists every file header in
                    // front of where it stopped, so none lies between.
                    let last_end = map.members.last().map(|m| m.packed.end);
                    if last_end.is_none_or(|end| end > at) {
                        return HolePlacement::Header;
                    }
                    (map, Some(at))
                }
                Some(Ok(_)) | Some(Err(_)) => return HolePlacement::Header,
                None => return HolePlacement::Unknown,
            }
        }
        Some(Err(MapError::WrongPassword | MapError::PasswordRequired)) => {
            return HolePlacement::Unknown
        }
        Some(Err(MapError::NotArchive)) => return HolePlacement::Unknown,
        Some(Err(_)) => return HolePlacement::Header,
    };
    let spans = spans_of(&map);
    if cut.is_none() && holes.iter().all(|&h| inside(&spans, h)) {
        return HolePlacement::Data(holes);
    }
    let Some(data_end) = map.members.iter().map(|m| m.packed.end).max() else {
        return HolePlacement::Header;
    };
    if data_end >= file_len {
        return HolePlacement::Header;
    }
    // Every hole is data, behind the last data (the tail), or runs from data into the tail.
    let placed = holes.iter().all(|&(s, l)| {
        s >= data_end
            || inside(&spans, (s, l))
            || (s + l > data_end && inside(&spans, (s, data_end - s)))
    });
    if placed {
        HolePlacement::Tail { data_end }
    } else {
        HolePlacement::Header
    }
}

/// The holes of a volume extraction can read past now: `Data` placements as they are, the
/// fixed edge guard for a volume whose headers cannot be read, nothing (wait for the repair)
/// otherwise.
pub fn skippable_placed(
    placement: &HolePlacement,
    holes: &[crate::dupefallback::NzbgetRsByteRange],
    file_size: i64,
) -> Vec<(u64, u64)> {
    match placement {
        HolePlacement::Data(h) => h.clone(),
        HolePlacement::Unknown => skippable_holes(holes, file_size),
        HolePlacement::Tail { .. } | HolePlacement::Header => Vec::new(),
    }
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
    /// A check failed after decryption: a wrong password or damaged data (RAR4 has no
    /// password verifier). Reported so the next candidate password is tried.
    EncryptedChecksum(String),
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
            Failure::Checksum(_)
                | Failure::EncryptedChecksum(_)
                | Failure::MissingVolume(_)
                | Failure::Damaged(_)
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
                    R::Integrity { .. } => Failure::Checksum(format!("{at} ({r})")),
                    R::EncryptedIntegrity { .. } => Failure::EncryptedChecksum(at.clone()),
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
            Failure::EncryptedChecksum(at) => {
                write!(
                    f,
                    "checksum mismatch in encrypted data{at} (wrong password or damage)"
                )
            }
            Failure::MissingVolume(at) => write!(f, "missing volume{at}"),
            Failure::Damaged(what) => write!(f, "damaged archive data: {what}"),
            Failure::UnsafePath(at) => write!(f, "unsafe path{at}"),
            Failure::Canceled => write!(f, "Aborted"),
            Failure::Other(what) => write!(f, "{what}"),
        }
    }
}

/// zipfast cannot read this set through a session (a method or format it refuses). The caller
/// reports the archive as unsupported; in rename mode the outputs this run placed are removed
/// first (see `undo_for_fallback`), in other modes they stay.
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

/// True when a failure means "this format or method is not handled by the session" (the set
/// is reported unsupported) rather than "the archive is bad".
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
    /// Told `true` while extraction waits for a volume (to download or be repaired) and
    /// `false` when it continues.
    pub on_wait: Option<&'a dyn Fn(bool)>,
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
    extract_recording(req, feed, log_cb, is_cancelled, None)
}

/// Where each member was written: (name, occurrence of that name, path). Only outputs copied
/// into place are recorded.
type Outputs = Vec<(Vec<u8>, u32, PathBuf)>;

fn extract_recording<F>(
    req: &Request<'_>,
    feed: Box<dyn VolumeFeed>,
    log_cb: &mut F,
    is_cancelled: &mut dyn FnMut() -> bool,
    mut outputs: Option<&mut Outputs>,
) -> Result<zipfast::extract::Summary, Error>
where
    F: FnMut(i32, &str),
{
    // A complete set is declared to zipfast up front when its estimated cost fits. Should a
    // declared session still run out of one of zipfast's limits, the set is extracted again
    // undeclared (copied into place), as before declarations: never a failed unpack.
    // A set still downloading goes to persistent partial files when it qualifies.
    let feed = if persistent::enabled() && outputs.is_none() && !feed.set_is_final() {
        match persistent::try_extract(req, feed, log_cb, is_cancelled) {
            persistent::Outcome::Done(r) => return r,
            persistent::Outcome::Fallback(feed, why) => {
                log_cb(3, &format!("Unrar: progressive direct unpack ({why})"));
                feed
            }
        }
    } else {
        persistent::forget_state(req);
        feed
    };
    let mut declared = false;
    let first = extract_once(
        req,
        feed,
        log_cb,
        is_cancelled,
        outputs.as_deref_mut(),
        true,
        &mut declared,
    );
    match first {
        Err(Error::Failed(Failure::Other(ref what)))
            if declared && req.overwrite == Overwrite::Always && is_declaration_limit(what) =>
        {
            log_cb(
                3,
                &format!(
                    "Unrar: declared extraction hit a zipfast limit ({what}); extracting again without declaring"
                ),
            );
            let mut again = false;
            extract_once(
                req,
                Box::new(CompleteSet),
                log_cb,
                is_cancelled,
                outputs,
                false,
                &mut again,
            )
        }
        other => other,
    }
}

/// A zipfast limit that a protection declaration inflates (and an undeclared run may not hit).
fn is_declaration_limit(what: &str) -> bool {
    what.starts_with("Limit {")
        && [
            "SourceBookkeeping",
            "MetadataBytes",
            "NameBytes",
            "ProtectionPaths",
            "ProtectionPathBytes",
            "VolumeHandles",
        ]
        .iter()
        .any(|r| what.starts_with(&format!("Limit {{ resource: {r},")))
}

fn extract_once<F>(
    req: &Request<'_>,
    feed: Box<dyn VolumeFeed>,
    log_cb: &mut F,
    is_cancelled: &mut dyn FnMut() -> bool,
    mut outputs: Option<&mut Outputs>,
    declare_at_start: bool,
    declared: &mut bool,
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
    // Overwrite mode only: skip mode would publish by hard link, which some destinations
    // (exFAT, SMB) lack, and rename mode needs RustyGet's own numbering.
    let mut declared_cost = None;
    let protected = (DECLARE_PROTECTION
        && declare_at_start
        && feed.set_is_final()
        && req.overwrite == Overwrite::Always)
        .then(|| protect_volumes(req.first_volume))
        .flatten()
        .filter(|inv| {
            declared_cost = session_declaration_cost(inv, req.first_volume, req.dest, req.password);
            #[cfg(test)]
            if tests::FORCE_DECLARE.with(|f| f.get()) {
                declared_cost = Some(DeclarationCost {
                    bookkeeping: 0,
                    metadata: 0,
                    names: 0,
                });
            }
            declared_cost.is_some()
        });
    *declared = protected.is_some();
    // A final set's volumes are all on disk; count them when no protected
    // inventory exists (rename mode) so the handle budget still fits the set.
    let expected = if feed.set_is_final() {
        feed.expected_volumes()
            .max(rar_set_members(req.first_volume).len() as u64)
    } else {
        feed.expected_volumes()
    };
    let feed = Arc::new(Mutex::new(feed));
    let pending: PendingWait = Arc::new(Mutex::new(None));
    let volume_count = protected
        .as_ref()
        .map_or(expected, |inv| (inv.volumes.len() as u64).max(expected));
    let protection: ProtectionSlot = Arc::new(Mutex::new(protected));
    let opened: OpenedVolumes = Arc::new(Mutex::new(Vec::new()));
    let mut provider = FeedProvider {
        dir: dir.clone(),
        feed: feed.clone(),
        pending: pending.clone(),
        protection: protection.clone(),
        opened: opened.clone(),
        written_damaged: Arc::new(AtomicBool::new(false)),
        late: LateProtection {
            first: req.first_volume.to_path_buf(),
            password: req.password.to_string(),
            allowed: req.overwrite == Overwrite::Always,
            handle_limit: 0,
            saw_holes: false,
            refused: false,
            members: None,
            tried_at: None,
        },
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
            select: None,
            windows_safe_names: false,
            special_entries: Default::default(),
            fallback_mtime: None,
            compressed_tar: Default::default(),
            version_naming: Default::default(),
            preexisting_links: Default::default(),
            refused_link_placeholder: false,
        },
        first_name,
    );
    opts.limits.volume_handles = opts.limits.volume_handles.max(2 * volume_count + 16);
    let handle_limit = opts.limits.volume_handles;
    provider.late.handle_limit = handle_limit;
    // Headroom for the per-read protection checks once the volumes are declared protected.
    opts.limits.source_bookkeeping = opts
        .limits
        .source_bookkeeping
        .saturating_add(PROTECTED_READ_BUDGET);
    // A set declared at session start gets its estimated protection cost on top, so the
    // declaration never eats the budget the extraction itself needs.
    if let Some(cost) = declared_cost {
        opts.limits.source_bookkeeping = opts
            .limits
            .source_bookkeeping
            .saturating_add(cost.bookkeeping);
        opts.limits.metadata_bytes = opts.limits.metadata_bytes.saturating_add(cost.metadata);
        opts.limits.name_bytes = opts.limits.name_bytes.saturating_add(cost.names);
    }
    opts.limits.protection_paths = opts.limits.protection_paths.max(volume_count + 16);
    // Tests force a declared set past a limit so the undeclared retry runs.
    #[cfg(test)]
    if tests::FORCE_DECLARE.with(|f| f.get()) && *declared {
        opts.limits.protection_path_bytes = 4096;
    }
    log_cb(
        3,
        &format!(
            "Unrar: volume budget {} handle(s) for {volume_count} volume(s)",
            opts.limits.volume_handles
        ),
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
    let mut drive = Driver::new(&mut session, feed.clone(), pending, req.on_wait);
    let mut summary = zipfast::extract::Summary::default();
    let mut names: std::collections::HashMap<zipfast::progressive::MemberId, Member> =
        Default::default();
    let mut partial: Vec<(Member, Artifact)> = Vec::new();
    let mut provisional: Vec<(Member, Artifact)> = Vec::new();
    let mut current_volume = req.first_volume.to_path_buf();
    let mut meta_cache = MetaCache::default();
    // Outputs this call copied into place, removed again in rename mode when the set turns out
    // to be unsupported.
    let mut published: Outputs = Vec::new();
    // Directory entries created here (undeclared sets), with their stored times.
    let mut placed_dirs: Vec<(PathBuf, Option<(i64, u32)>)> = Vec::new();
    let mut next_ordinal = 0u64;
    let mut occurrences: std::collections::HashMap<Vec<u8>, u32> = Default::default();
    // Highest member ordinal already in place per name: a member repaired later must not
    // replace the output of a later member with the same name.
    let mut placed: std::collections::HashMap<Vec<u8>, u64> = Default::default();
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
                let failure = Failure::from_progressive(&e);
                // zipfast refuses an unsafe path (an entry's name, a link's target, an output
                // through a symlink or onto a volume) without naming the member.
                if matches!(e.kind, FailureKind::UnsafePath) {
                    let why = format!(
                        "an archive entry or link would land outside the unpack folder (zipfast: {failure})"
                    );
                    crate::unpacklimits::refuse_unsafe(req.dest, &why)
                        .map_err(|why| Failure::UnsafePath(format!(": {why}")))?;
                }
                return Err(failure.into());
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
                // Needed for outputs copied into place (zipfast restores it on what it publishes
                // itself; a checksum-less member of a final set is still copied). Each volume's
                // headers are read once.
                let meta = meta_cache.lookup(&current_volume, req.password, &name);
                // Placed where unrar and zipfast place it: a RAR 1.5-4.x backslash separates
                // directories.
                let name = if meta.separator_mapped {
                    name.iter()
                        .map(|&b| if b == b'\\' { b'/' } else { b })
                        .collect()
                } else {
                    name.to_vec()
                };
                if let Err(f) = admit_member(req.dest, &name, meta) {
                    for (_, mut a) in partial.drain(..).chain(provisional.drain(..)) {
                        let _ = a.discard();
                    }
                    return Err(f.into());
                }
                let display = String::from_utf8_lossy(&name).into_owned();
                log_cb(4, &format!("Unrar: Extracting  {display}"));
                next_ordinal += 1;
                let seen = occurrences.entry(name.to_vec()).or_insert(0u32);
                let occurrence = *seen;
                *seen += 1;
                names.insert(
                    member,
                    Member {
                        name: name.to_vec(),
                        meta,
                        ordinal: next_ordinal,
                        occurrence,
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
                        fix_published_mode(req.dest, &member);
                        summary.files += 1;
                        note_placed(&mut placed, &member);
                    }
                    MemberOutcome::Directory {
                        deferred: Some(_), ..
                    } => {
                        // A directory entry of a set that is not declared: zipfast creates
                        // directories only with publication authority and leaves this one to
                        // the caller, so make it here, as the copy path places files.
                        match place_directory(req.dest, &member) {
                            Ok(dir) => {
                                summary.dirs += 1;
                                // Its time is applied after its children are placed.
                                placed_dirs.retain(|(d, _)| *d != dir);
                                placed_dirs.push((dir, member.meta.mtime));
                            }
                            Err(f) => {
                                log_cb(1, &unrar_error_line(&f, &name));
                                first_failure.get_or_insert(f);
                            }
                        }
                    }
                    MemberOutcome::Directory { .. } => summary.dirs += 1,
                    MemberOutcome::Skipped(_) => summary.skipped += 1,
                    MemberOutcome::Tested(_) => {}
                    MemberOutcome::Provisional(mut a) => {
                        // Already verified by the session: move it into place now, so the file
                        // is available while later members extract and its slot is freed.
                        // Outside overwrite mode an earlier pending member with the same name
                        // goes first: skip keeps the first, rename numbers them in archive order
                        // (and zipfast refuses to verify a member whose target path changed).
                        let behind_earlier = held_behind(req, &member, &partial, &provisional);
                        // Verified, or decoded whole with no checksum stored.
                        let complete = matches!(a.evidence(), ArtifactEvidence::Verified(_))
                            || (a.holes().is_empty() && a.size() == Some(a.produced_extent()));
                        if complete && !behind_earlier {
                            match publish_copy(
                                drive.session,
                                &mut a,
                                req,
                                &member,
                                false,
                                is_cancelled,
                                &mut published,
                            ) {
                                Ok(()) => {
                                    summary.files += 1;
                                    note_placed(&mut placed, &member);
                                }
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
                            // A member the session cannot decode: the set is reported
                            // unsupported (rename mode removes the outputs it placed).
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
                provisional.sort_by_key(|(m, _)| m.ordinal);
                let mut keep = Vec::new();
                for (member, mut a) in provisional.drain(..) {
                    let behind_earlier = held_behind(req, &member, &partial, &[]);
                    if behind_earlier {
                        keep.push((member, a));
                    } else if superseded(&placed, &member, req) {
                        let _ = a.discard();
                    } else if let Err(f) = publish_copy(
                        drive.session,
                        &mut a,
                        req,
                        &member,
                        false,
                        is_cancelled,
                        &mut published,
                    ) {
                        first_failure.get_or_insert(f);
                    } else {
                        summary.files += 1;
                        note_placed(&mut placed, &member);
                    }
                }
                if !keep.is_empty() && partial.len() + keep.len() >= drive.session_artifact_limit()
                {
                    // Out of slots with duplicates held behind a damaged member. Skip mode would
                    // place them only if that member fails (which is reported): drop them.
                    // Rename mode must number them in archive order, which it cannot while the
                    // damaged one waits: give that member up (reported as damage, so
                    // post-processing extracts the set again) and place the rest in order.
                    if req.overwrite == Overwrite::Rename {
                        let mut still = Vec::new();
                        for (m, mut a) in partial.drain(..) {
                            if keep.iter().any(|(k, _)| k.ordinal > m.ordinal) {
                                let _ = a.discard();
                                first_failure.get_or_insert(Failure::Damaged(format!(
                                    "{}: too many members with this name behind its damage",
                                    String::from_utf8_lossy(&m.name)
                                )));
                            } else {
                                still.push((m, a));
                            }
                        }
                        partial = still;
                    }
                    for (member, mut a) in keep.drain(..) {
                        if req.overwrite == Overwrite::Rename {
                            match publish_copy(
                                drive.session,
                                &mut a,
                                req,
                                &member,
                                false,
                                is_cancelled,
                                &mut published,
                            ) {
                                Ok(()) => {
                                    summary.files += 1;
                                    note_placed(&mut placed, &member);
                                }
                                Err(f) => {
                                    first_failure.get_or_insert(f);
                                }
                            }
                        } else {
                            let _ = a.discard();
                        }
                    }
                }
                provisional = keep;
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
                    return Err(Error::Unsupported(Unsupported(format!(
                        "{:?}",
                        failure.kind
                    ))));
                }
                let f = Failure::from_progressive(&failure);
                log_cb(1, &unrar_error_line(&f, b""));
                first_failure.get_or_insert(f);
                break;
            }
            _ => {}
        }
    }

    // Deferred outputs go into place in archive order, so duplicate names resolve as in a
    // sequential extraction: the first one wins when skipping, the last when overwriting.
    let mut deferred: Vec<(Member, Artifact, bool)> = provisional
        .drain(..)
        .map(|(m, a)| (m, a, false))
        .chain(partial.drain(..).map(|(m, a)| (m, a, true)))
        .collect();
    deferred.sort_by_key(|(m, _, _)| m.ordinal);
    let damaged: Vec<String> = deferred
        .iter()
        .filter(|(_, _, is_partial)| *is_partial)
        .map(|(m, a, _)| {
            format!(
                "{} ({} bytes missing)",
                String::from_utf8_lossy(&m.name),
                a.holes().iter().map(|h| h.len).sum::<u64>()
            )
        })
        .collect();
    let mut patch = false;
    if !damaged.is_empty() && first_failure.is_none() {
        log_cb(
            0,
            &format!(
                "Unrar: extracted past damage in {}; waiting for the volumes to be repaired",
                damaged.join(", ")
            ),
        );
        drive.set_waiting(true);
        patch = feed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .wait_repaired(is_cancelled);
        drive.set_waiting(false);
        if !patch {
            first_failure = Some(if is_cancelled() {
                Failure::Canceled
            } else {
                Failure::Damaged("the damaged volumes were not repaired".into())
            });
        }
    }
    // zipfast reads the declaration at its next publication; from then on every read step
    // rechecks every protected volume. Upgrade only for a single remaining output, so only its
    // own publication pays that, and nothing waits behind it.
    if DECLARE_PROTECTION
        && deferred.len() == 1
        // zipfast names a Windows-made member with Windows path rules (a backslash splits); the copy
        // path, which placed the rest of the set, does not.
        && !deferred[0].0.meta.windows_names
        && first_failure.is_none()
        // zipfast publishes by rename only when it may replace; skip mode would need hard
        // links, which some destinations (exFAT, SMB) do not support.
        && req.overwrite == Overwrite::Always
        // A dead volume rebuilt from a duplicate is no file of the set: the declaration would
        // cover only the files before it, and the publication then trips over the rest.
        && !rar_set_members_with(req.first_volume, &mut |path: &Path| {
            match feed.lock().unwrap_or_else(|e| e.into_inner()).volume(path) {
                VolumeView::Virtual(v) => Some(v),
                _ => None,
            }
        })
        .1
        .iter()
        .any(Option::is_some)
        && upgrade_protection(
            req.first_volume,
            &protection,
            &opened,
            handle_limit,
            16u64.saturating_add(path_depth(&deferred[0].0.name).saturating_mul(14)),
        )
    {
        log_cb(
            4,
            "Unrar: all volumes on disk; verified files are moved into place instead of copied",
        );
    }
    for (member, mut a, is_partial) in deferred {
        let name = member.name.clone();
        if (is_partial && !patch) || superseded(&placed, &member, req) {
            let _ = a.discard();
            continue;
        }
        let result = if is_partial {
            patch_from_repaired_volumes(
                drive.session,
                &mut a,
                &dir,
                req,
                &member,
                &mut |path: &Path| {
                    match feed.lock().unwrap_or_else(|e| e.into_inner()).volume(path) {
                        VolumeView::Virtual(v) if v.holes.is_empty() => Some(v),
                        _ => None,
                    }
                },
                is_cancelled,
                log_cb,
            )
            .and_then(|()| {
                publish_copy(
                    drive.session,
                    &mut a,
                    req,
                    &member,
                    true,
                    is_cancelled,
                    &mut published,
                )
            })
        } else {
            publish_copy(
                drive.session,
                &mut a,
                req,
                &member,
                false,
                is_cancelled,
                &mut published,
            )
        };
        match result {
            Ok(()) => {
                summary.files += 1;
                note_placed(&mut placed, &member);
                if is_partial {
                    log_cb(
                        0,
                        &format!(
                            "Unrar: repaired {} from the repaired volumes",
                            String::from_utf8_lossy(&name)
                        ),
                    );
                }
            }
            Err(f) => {
                if is_partial {
                    log_cb(1, &unrar_error_line(&f, &name));
                }
                first_failure.get_or_insert(f);
            }
        }
    }

    set_directory_times(placed_dirs);
    if let Some(out) = outputs.as_deref_mut() {
        out.extend(published);
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
        // Recognised by the unpack message parser as a decryption error, which lets the
        // password list try its next candidate.
        Failure::EncryptedChecksum(_) => format!(
            "Unrar: CRC failed in the encrypted file {}. Corrupt file or wrong password.",
            String::from_utf8_lossy(name)
        ),
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
    /// Position in the archive; a later member with the same name wins.
    ordinal: u64,
    /// How many earlier members had the same name.
    occurrence: u32,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct MemberMeta {
    /// Unix permission bits, when the archive was made on Unix.
    unix_mode: Option<u32>,
    /// Modification time (seconds, nanoseconds).
    mtime: Option<(i64, u32)>,
    /// The archive was not made on Unix: zipfast names its output with Windows path rules.
    windows_names: bool,
    /// A RAR 1.5-4.x member: a stored backslash is a directory separator whatever the host,
    /// as unrar and zipfast read it, so the output is placed under the name with slashes.
    separator_mapped: bool,
    /// A folder entry.
    is_dir: bool,
}

/// Member metadata by volume, each volume's headers read once. Members with the same name get
/// their own header's metadata, in archive order.
#[derive(Default)]
struct MetaCache {
    volume: PathBuf,
    members: std::collections::HashMap<Vec<u8>, std::collections::VecDeque<MemberMeta>>,
}

impl MetaCache {
    fn lookup(&mut self, volume: &Path, password: &str, name: &[u8]) -> MemberMeta {
        if self.volume != volume {
            self.volume = volume.to_path_buf();
            self.members = volume_metadata(volume, password);
        }
        // zipfast's decoded RAR 1.5-4.x names have slashes where the stored name (the one
        // zipfast reports while extracting) has backslashes.
        let mapped;
        let key = if !self.members.contains_key(name) && name.contains(&b'\\') {
            mapped = name
                .iter()
                .map(|&b| if b == b'\\' { b'/' } else { b })
                .collect::<Vec<u8>>();
            mapped.as_slice()
        } else {
            name
        };
        self.members
            .get_mut(key)
            .and_then(|q| q.pop_front())
            .unwrap_or_default()
    }
}

/// The headers of members that start in `volume` (not continuations), in order, for the
/// metadata zipfast restores only on the outputs it publishes itself.
fn volume_metadata(
    volume: &Path,
    password: &str,
) -> std::collections::HashMap<Vec<u8>, std::collections::VecDeque<MemberMeta>> {
    use zipfast::rar5::reader::{Block, Volume};
    let mut out: std::collections::HashMap<Vec<u8>, std::collections::VecDeque<MemberMeta>> =
        std::collections::HashMap::new();
    let pw = (!password.is_empty()).then_some(password);
    if let Ok(mut vol) = Volume::open_with_password(volume, pw) {
        for _ in 0..1_000_000 {
            match vol.next_block() {
                Ok(Some(Block::File(h))) if !h.split_before() => {
                    out.entry(h.name.as_bytes().to_vec())
                        .or_default()
                        .push_back(MemberMeta {
                            unix_mode: h.is_unix().then_some(h.attributes as u32),
                            mtime: h.mtime.map(|s| (s, h.mtime_nanos)),
                            windows_names: !h.is_unix(),
                            separator_mapped: h.rar4.is_some(),
                            is_dir: h.is_dir(),
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
    for f in files.into_iter().filter(|f| !f.split_before) {
        out.entry(f.filename.clone())
            .or_default()
            .push_back(MemberMeta {
                unix_mode: (f.attr & 0o170000 == 0o100000).then_some(f.attr & 0o7777),
                mtime: crate::archive_native::rar4_dos_mtime(f.time),
                windows_names: f.attr & 0o170000 != 0o100000,
                separator_mapped: true,
                is_dir: if f.attr & 0o170000 != 0 {
                    f.attr & 0o170000 == 0o040000
                } else {
                    f.attr & 0x10 != 0
                },
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
/// and patches them in. `virtual_of` gives a dead volume rebuilt from a duplicate in place of
/// its missing file (a primary that died part way: every volume after the one it died in is
/// one), so the set is read to its end.
#[allow(clippy::too_many_arguments)]
fn patch_from_repaired_volumes(
    session: &mut ProgressiveSession,
    artifact: &mut Artifact,
    dir: &Path,
    req: &Request<'_>,
    member: &Member,
    virtual_of: &mut dyn FnMut(&Path) -> Option<crate::deadvolume::VirtualVolume>,
    is_cancelled: &mut dyn FnMut() -> bool,
    log_cb: &mut dyn FnMut(i32, &str),
) -> Result<(), Failure> {
    let name = member.name.as_slice();
    use crate::streamrepair::{ContentMapper, DiskSourceSet, MemberFormat};

    let (members, virtuals) = rar_set_members_with(req.first_volume, virtual_of);
    if members.is_empty() {
        return Err(Failure::Damaged(
            "no volumes to read the repair from".into(),
        ));
    }
    let dir_bytes = path_bytes(dir);
    let sources = PatchSources {
        disk: DiskSourceSet::new(&dir_bytes, &members),
        virtuals,
    };
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
    let Some(map) = map
        .filter(|m| m.inner_name_bytes() == base && Some(m.inner_size() as u64) == artifact.size())
    else {
        // This re-extracts the whole repaired set: worth knowing when a patch is slow.
        log_cb(
            0,
            &format!(
                "Unrar: {} is not the set's main payload; extracting the repaired set again to patch it",
                String::from_utf8_lossy(name)
            ),
        );
        return patch_by_reextracting(session, artifact, req, member, is_cancelled);
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
            if map.read_donor_inner(&sources, range, &mut buf).is_err() || buf.len() as u64 != n {
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
    member: &Member,
    is_cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), Failure> {
    let name = member.name.as_slice();
    let scratch = create_unique_dir(req.dest)
        .map_err(|e| Failure::Other(format!("cannot create a repair directory: {e}")))?;
    let result = (|| {
        let sub = Request {
            first_volume: req.first_volume,
            dest: &scratch,
            password: req.password,
            // Every occurrence of a name gets its own file; where each went is recorded.
            overwrite: Overwrite::Rename,
            on_wait: None,
        };
        let mut outputs: Outputs = Vec::new();
        match extract_recording(
            &sub,
            Box::new(CompleteSet),
            &mut |_, _| {},
            is_cancelled,
            Some(&mut outputs),
        ) {
            Ok(_) => {}
            Err(Error::Failed(f)) => return Err(f),
            Err(Error::Unsupported(u)) => return Err(Failure::Other(u.0)),
        }
        let path = outputs
            .iter()
            .find(|(n, occ, _)| n == name && *occ == member.occurrence)
            .map(|(_, _, p)| p.clone())
            .ok_or_else(|| Failure::Damaged("the repaired set did not yield this member".into()))?;
        let copy = std::fs::File::open(&path)
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
/// The volumes of the RAR set from `first`, like [`rar_set_members`], going on past a missing
/// file `virtual_of` has a rebuilt dead volume for (each such member comes with its source).
fn rar_set_members_with(
    first: &Path,
    virtual_of: &mut dyn FnMut(&Path) -> Option<crate::deadvolume::VirtualVolume>,
) -> (Vec<crate::streamrepair::SetMember>, Vec<Option<PatchVirtual>>) {
    let mut out = Vec::new();
    let mut virtuals = Vec::new();
    let mut p = first.to_path_buf();
    loop {
        let name = p
            .file_name()
            .map(|n| path_bytes(Path::new(n)))
            .unwrap_or_default();
        if let Ok(meta) = std::fs::metadata(&p) {
            out.push(crate::streamrepair::SetMember::new(name, meta.len() as i64));
            virtuals.push(None);
        } else if let Some(source) = virtual_of(&p).and_then(|v| PatchVirtual::open(&v).ok()) {
            out.push(crate::streamrepair::SetMember::new(name, source.len() as i64));
            virtuals.push(Some(source));
        } else {
            break;
        }
        match zipfast::volume::next_volume(&p) {
            Some(n) if n != p => p = n,
            _ => break,
        }
    }
    (out, virtuals)
}

/// A finished dead volume read back by offset (headers, borrowed part, tail).
struct PatchVirtual {
    head: Vec<u8>,
    data: Mutex<std::fs::File>,
    data_len: u64,
    tail: Vec<u8>,
}

impl PatchVirtual {
    fn open(v: &crate::deadvolume::VirtualVolume) -> std::io::Result<Self> {
        let data = std::fs::File::open(&v.data)?;
        if data.metadata()?.len() < v.data_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "borrowed data is shorter than its part",
            ));
        }
        Ok(Self { head: v.head.clone(), data: Mutex::new(data), data_len: v.data_len, tail: v.tail.clone() })
    }

    fn len(&self) -> u64 {
        self.head.len() as u64 + self.data_len + self.tail.len() as u64
    }
}

impl crate::streamrepair::ContentSource for PatchVirtual {
    fn size(&self) -> i64 {
        self.len() as i64
    }

    fn read(&self, offset: i64, buf: &mut [u8]) -> crate::streamrepair::ReadResult {
        use crate::streamrepair::ReadError;
        use std::io::{Read, Seek, SeekFrom};
        let start = u64::try_from(offset).map_err(|_| ReadError::OutOfRange)?;
        if start.checked_add(buf.len() as u64).is_none_or(|end| end > self.len()) {
            return Err(ReadError::OutOfRange);
        }
        let head = self.head.len() as u64;
        let data_end = head + self.data_len;
        let mut filled = 0usize;
        while filled < buf.len() {
            let at = start + filled as u64;
            let rest = &mut buf[filled..];
            let n = if at < head {
                let n = rest.len().min((head - at) as usize);
                rest[..n].copy_from_slice(&self.head[at as usize..at as usize + n]);
                n
            } else if at < data_end {
                let n = rest.len().min((data_end - at) as usize);
                let mut f = self.data.lock().unwrap_or_else(|e| e.into_inner());
                f.seek(SeekFrom::Start(at - head)).map_err(|_| ReadError::Unavailable)?;
                f.read_exact(&mut rest[..n]).map_err(|_| ReadError::Unavailable)?;
                n
            } else {
                let from = (at - data_end) as usize;
                let n = rest.len().min(self.tail.len() - from);
                rest[..n].copy_from_slice(&self.tail[from..from + n]);
                n
            };
            filled += n;
        }
        Ok(())
    }
}

/// The repaired set as the patch reads it: files on disk, and rebuilt dead volumes in place of
/// missing ones.
struct PatchSources<'a> {
    disk: crate::streamrepair::DiskSourceSet<'a>,
    virtuals: Vec<Option<PatchVirtual>>,
}

impl crate::streamrepair::ContentSourceSet for PatchSources<'_> {
    fn get_source(&self, member_index: i32) -> Option<&dyn crate::streamrepair::ContentSource> {
        match usize::try_from(member_index).ok().and_then(|i| self.virtuals.get(i)) {
            Some(Some(v)) => Some(v),
            _ => self.disk.get_source(member_index),
        }
    }
}

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
///
/// When the set turns out to be unsupported in rename mode: the outputs this run already put in
/// place (all under new names) are removed. Other modes keep what was written.
fn undo_for_fallback(req: &Request<'_>, published: &Outputs) {
    if req.overwrite == Overwrite::Rename {
        for (_, _, p) in published {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Whether `member` must wait for earlier members still pending. Skip mode: an earlier member
/// with the same name goes first (it keeps the name). Rename mode: every earlier pending
/// member goes first, so collision-resolved names follow archive order. Overwrite mode: never.
fn held_behind(
    req: &Request<'_>,
    member: &Member,
    partial: &[(Member, Artifact)],
    provisional: &[(Member, Artifact)],
) -> bool {
    let earlier = |m: &Member| m.ordinal < member.ordinal;
    match req.overwrite {
        Overwrite::Always => false,
        Overwrite::Skip => partial
            .iter()
            .chain(provisional.iter())
            .any(|(m, _)| m.name == member.name && earlier(m)),
        Overwrite::Rename => partial
            .iter()
            .chain(provisional.iter())
            .any(|(m, _)| earlier(m)),
    }
}

fn note_placed(placed: &mut std::collections::HashMap<Vec<u8>, u64>, member: &Member) {
    let e = placed.entry(member.name.clone()).or_insert(0);
    *e = (*e).max(member.ordinal);
}

/// True when a later member with the same name is already in place (outside rename mode,
/// where each member keeps its own name).
fn superseded(
    placed: &std::collections::HashMap<Vec<u8>, u64>,
    member: &Member,
    req: &Request<'_>,
) -> bool {
    req.overwrite != Overwrite::Rename
        && placed
            .get(&member.name)
            .is_some_and(|&ord| ord > member.ordinal)
}

/// `repaired` outputs (patched after a hole) must verify against an archive checksum; an output
/// the session decoded whole is accepted without one when the archive stores none, as before.
fn publish_copy(
    session: &mut ProgressiveSession,
    artifact: &mut Artifact,
    req: &Request<'_>,
    member: &Member,
    repaired: bool,
    is_cancelled: &mut dyn FnMut() -> bool,
    published: &mut Outputs,
) -> Result<(), Failure> {
    let name = member.name.as_slice();
    if artifact.publication() != ArtifactPublication::Unpublished {
        fix_published_mode(req.dest, member);
        return Ok(());
    }
    let verified = match artifact.evidence() {
        ArtifactEvidence::Verified(_) => true,
        ArtifactEvidence::Provisional => {
            // Decoded whole with no checksum stored: complete when its size is the declared one.
            !repaired
                && artifact.holes().is_empty()
                && artifact.size() == Some(artifact.produced_extent())
        }
    };
    if !verified {
        // zipfast yields once per bounded chunk and wakes the task when it can go on; park only
        // when it has not, and never sleep a fixed time per chunk.
        let parker = Arc::new(Parker {
            thread: std::thread::current(),
            woken: AtomicBool::new(false),
        });
        let waker = Waker::from(parker.clone());
        let mut cx = Context::from_waker(&waker);
        loop {
            if is_cancelled() {
                session.control().cancel();
                let _ = artifact.discard();
                return Err(Failure::Canceled);
            }
            parker.woken.store(false, Ordering::Release);
            match session.poll_verify(artifact, &mut cx) {
                Poll::Ready(Ok(_)) => break,
                Poll::Ready(Err(e)) => {
                    // Placed, but cleaning up its stage failed afterwards: the file is there.
                    if artifact.publication() != ArtifactPublication::Unpublished {
                        fix_published_mode(req.dest, member);
                        return Ok(());
                    }
                    let _ = artifact.discard();
                    return Err(Failure::from_progressive(&e));
                }
                Poll::Pending => {
                    if !parker.woken.load(Ordering::Acquire) {
                        std::thread::park_timeout(PARK_SLICE);
                    }
                }
            }
        }
        if artifact.publication() != ArtifactPublication::Unpublished {
            fix_published_mode(req.dest, member);
            return Ok(());
        }
        // A repaired encrypted member verifies on its plaintext only; with the volumes
        // protected zipfast can still publish it itself. The file can be in place even when
        // cleanup afterwards reports an error, so the publication decides.
        if matches!(artifact.evidence(), ArtifactEvidence::Verified(_)) {
            let _ = session.publish_recovered(artifact);
            if artifact.publication() != ArtifactPublication::Unpublished {
                fix_published_mode(req.dest, member);
                return Ok(());
            }
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
    let result = copy_artifact(
        artifact,
        req.dest,
        &rel,
        req.overwrite,
        member.meta,
        is_cancelled,
    )
    .map(|written| {
        if let Some(path) = written {
            published.push((member.name.clone(), member.occurrence, path));
        }
    });
    let _ = artifact.discard();
    result
}

/// Checks a member against the job's unpack limits (`unpacklimits`) before it is written
/// under `dest`. A name zipfast's path rules refuse (`..`, absolute) breaches them; with no
/// job's limits active, zipfast refuses it.
fn admit_member(dest: &Path, name: &[u8], meta: MemberMeta) -> Result<(), Failure> {
    let limit = |e: String| Failure::UnsafePath(format!(": {e}"));
    let shown = String::from_utf8_lossy(name);
    let Ok(rel) = zipfast::path::sanitize(&shown, meta.windows_names) else {
        let why = format!("{shown:?} is not a safe path inside the unpack folder");
        return crate::unpacklimits::refuse_unsafe(dest, &why).map_err(limit);
    };
    let is_dir = meta.is_dir || name.ends_with(b"/") || name.ends_with(b"\\");
    crate::unpacklimits::check(&dest.join(&rel), is_dir).map_err(limit)?;
    // zipfast publishes under the member's own path rules, the copy path under Unix rules:
    // both names are checked when they differ.
    if meta.windows_names {
        if let Ok(unix_rel) = zipfast::path::sanitize(&String::from_utf8_lossy(name), false) {
            if unix_rel != rel {
                crate::unpacklimits::check(&dest.join(unix_rel), is_dir).map_err(limit)?;
            }
        }
    }
    Ok(())
}

/// zipfast publishes from a private 0600 stage and applies a stored Unix mode as is. Give the
/// output the mode the copy path does: a stored mode with owner read/write kept, otherwise 0666
/// less the umask.
fn fix_published_mode(dest: &Path, member: &Member) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(rel) = zipfast::path::sanitize(
            &String::from_utf8_lossy(&member.name),
            member.meta.windows_names,
        ) else {
            return;
        };
        let mode = match member.meta.unix_mode {
            Some(m) => (m & 0o777) | 0o600,
            None => 0o666 & !crate::lifecycle::active_umask() & 0o777,
        };
        let _ = std::fs::set_permissions(dest.join(rel), std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (dest, member);
}

/// Creates a directory member under `dest` the way the copy path creates an output's
/// folders: never through a symlink. A stored Unix mode is applied with owner access kept.
fn place_directory(dest: &Path, member: &Member) -> Result<PathBuf, Failure> {
    let display = String::from_utf8_lossy(&member.name);
    // Unix rules, as publish_copy names the directory's files.
    let rel = zipfast::path::sanitize(&display, false)
        .map_err(|e| Failure::UnsafePath(format!(" {display}: {e}")))?;
    let dir = crate::archive_native::create_dir_all_no_follow(dest, &rel)
        .map_err(|e| Failure::Other(format!("cannot create {}: {e}", rel.display())))?;
    #[cfg(unix)]
    if let Some(mode) = member.meta.unix_mode {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            &dir,
            std::fs::Permissions::from_mode((mode & 0o777) | 0o700),
        );
    }
    Ok(dir)
}

/// Applies the stored modification times of the directories placed here, deepest first and
/// after their children, as zipfast's declared path does for the directories it publishes.
fn set_directory_times(mut dirs: Vec<(PathBuf, Option<(i64, u32)>)>) {
    use std::time::{Duration, UNIX_EPOCH};
    dirs.sort_by_key(|(d, _)| std::cmp::Reverse(d.components().count()));
    for (dir, mtime) in dirs {
        let Some((secs, nanos)) = mtime else {
            continue;
        };
        let at = if secs >= 0 {
            UNIX_EPOCH.checked_add(Duration::new(secs as u64, nanos.min(999_999_999)))
        } else {
            UNIX_EPOCH.checked_sub(Duration::from_secs(secs.unsigned_abs()))
        };
        if let (Some(at), Ok(f)) = (at, std::fs::File::open(&dir)) {
            let _ = f.set_modified(at);
        }
    }
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
    clean_stale_parts_once(target.parent().unwrap_or_else(|| Path::new(".")));
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
    // Renamed while still open: its write lock keeps a stale-part cleaner off it.
    let renamed = std::fs::rename(&tmp, target);
    drop(out);
    if let Err(e) = renamed {
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

/// Runs [`remove_stale_parts`] on `dir` once per process: files left by an earlier process
/// are there before this one starts, so a folder needs no second scan.
fn clean_stale_parts_once(dir: &Path) {
    use std::collections::HashSet;
    static CLEANED: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
    {
        let mut cleaned = CLEANED.lock().unwrap_or_else(|e| e.into_inner());
        let set = cleaned.get_or_insert_with(HashSet::new);
        if set.len() >= 4096 {
            set.clear();
        }
        if !set.insert(dir.to_path_buf()) {
            return;
        }
    }
    remove_stale_parts(dir);
}

/// Removes the `.rustyget-<pid>-<n>.part` files (see [`create_unique_sibling`]) that a daemon
/// process no longer running left in `dir`, such as one that crashed mid-copy. Only regular
/// files, never this process's own or a running process's, and never one whose write lock
/// is held: a writer keeps its part file locked, which also covers a daemon on another host
/// or in another PID namespace sharing the folder, where its pid means nothing here.
/// Returns how many it removed.
fn remove_stale_parts(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let own = std::process::id();
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(stale_part_pid) else {
            continue;
        };
        if pid == own || process_running(pid) {
            continue;
        }
        let path = entry.path();
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()) {
            continue;
        }
        // Held while unlinking, so a writer cannot take the file over meanwhile.
        let Some(_lock) = try_lock_unused(&path) else {
            continue;
        };
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Opens `path` (never following a link) and takes its write lock, unless another open file
/// holds it. `None` when it is in use or cannot be opened.
fn try_lock_unused(path: &Path) -> Option<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::io::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .ok()?;
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        locked.then_some(file)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// Takes `file`'s write lock for as long as it is open (see [`remove_stale_parts`]).
fn lock_part(file: &std::fs::File) {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    }
    #[cfg(not(unix))]
    let _ = file;
}

/// The pid in a `.rustyget-<pid>-<n>.part` name.
fn stale_part_pid(name: &str) -> Option<u32> {
    let (pid, n) = name
        .strip_prefix(".rustyget-")?
        .strip_suffix(".part")?
        .split_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(pid) || !digits(n) {
        return None;
    }
    pid.parse().ok().filter(|&p| p > 0)
}

/// Whether a process `pid` exists (one that can't be signalled still counts).
fn process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return true;
        };
        let signalled = unsafe { libc::kill(pid, 0) } == 0;
        signalled || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// Creates a new, uniquely named file next to `target` without touching any existing path.
fn create_unique_sibling(target: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    use std::sync::atomic::AtomicU64;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    loop {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(".rustyget-{}-{n}.part", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => {
                lock_part(&f);
                return Ok((tmp, f));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// The volume the session is parked on and the waker that resumes it.
/// The volume the session waits on, the waker to call once that changes, and whether a stand-in
/// handed over with holes still counts as waiting (the copy path waits for the finished one), and
/// whether the provider asked (a volume the download starts writing then is handed over as being
/// written) rather than a volume already read while written (see `WrittenSource`).
type PendingWait = Arc<Mutex<Option<(PathBuf, Waker, bool, bool)>>>;

/// Thread-parking driver around `poll_event`.
struct Driver<'s> {
    session: &'s mut ProgressiveSession,
    parker: Arc<Parker>,
    feed: Arc<Mutex<Box<dyn VolumeFeed>>>,
    pending: PendingWait,
    on_wait: Option<&'s dyn Fn(bool)>,
    waiting: bool,
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
        on_wait: Option<&'s dyn Fn(bool)>,
    ) -> Self {
        Self {
            session,
            parker: Arc::new(Parker {
                thread: std::thread::current(),
                woken: AtomicBool::new(false),
            }),
            feed,
            pending,
            on_wait,
            waiting: false,
        }
    }

    fn set_waiting(&mut self, waiting: bool) {
        if self.waiting != waiting {
            self.waiting = waiting;
            if let Some(cb) = self.on_wait {
                cb(waiting);
            }
        }
    }

    /// Wakes the session when the volume it is parked on has changed state.
    fn recheck_pending(&self) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        // Woken only for an answer the provider would not wait on again: waking it for one it
        // waits on spins the session until zipfast's wake limit fails it.
        let changed = match pending.as_ref() {
            Some((path, _, holed_waits, asked)) => {
                let mut feed = self.feed.lock().unwrap_or_else(|e| e.into_inner());
                match feed.volume(path) {
                    // Started being written since: the provider hands it over now.
                    VolumeView::NotYet => *asked && feed.growing(path).is_some(),
                    VolumeView::Virtual(v) if !v.holes.is_empty() => !*holed_waits,
                    _ => true,
                }
            }
            None => false,
        };
        if changed {
            if let Some((_, waker, _, _)) = pending.take() {
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
        self.next_event_or_tick(is_cancelled, &mut || {})
    }

    /// As `next_event`, calling `tick` each time the session has nothing to report yet
    /// (parked on a volume, or between steps), so work that must not wait for the next event,
    /// such as patching holes from a volume repaired meanwhile, still runs.
    fn next_event_or_tick(
        &mut self,
        is_cancelled: &mut dyn FnMut() -> bool,
        tick: &mut dyn FnMut(),
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
                Poll::Ready(r) => {
                    self.set_waiting(false);
                    return r;
                }
                Poll::Pending => {
                    let on_volume = self
                        .pending
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .is_some();
                    self.set_waiting(on_volume);
                    if !self.parker.woken.load(Ordering::Acquire) {
                        std::thread::park_timeout(PARK_SLICE);
                    }
                    self.recheck_pending();
                    tick();
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
    protection: ProtectionSlot,
    opened: OpenedVolumes,
    late: LateProtection,
    /// A volume read while it was written finished with holes or as another file: the set is
    /// never declared (as `late.saw_holes` / `late.refused`).
    written_damaged: Arc<AtomicBool>,
}

/// What deciding a declaration for a set that becomes final mid-session needs.
struct LateProtection {
    first: PathBuf,
    password: String,
    /// Overwrite mode: zipfast renames over a target; skip mode would need hard links and
    /// rename mode RustyGet's own numbering.
    allowed: bool,
    handle_limit: u64,
    /// A volume was handed out with holes: its member is deferred and patched after the
    /// repair, which declares the set itself when it is the only output left.
    saw_holes: bool,
    /// The set has a member the declaration must not cover; not checked again.
    refused: bool,
    /// Header summary of the set, read once after it became final.
    members: Option<SetMembers>,
    /// Volumes the session had read at the last attempt: budgets are retried once per volume.
    tried_at: Option<usize>,
}

/// The protection declaration handed to zipfast: `None` (opaque) until the set is known to be
/// complete on disk, then the protected native volumes. zipfast accepts one opaque-to-complete
/// upgrade and no later change.
type ProtectionSlot = Arc<Mutex<Option<zipfast::progressive::ProtectionInventory>>>;
/// Each volume handed to the session, with the (device, inode) of the file it read.
type OpenedVolumes = Arc<Mutex<Vec<(PathBuf, (u64, u64))>>>;

fn file_identity(meta: &std::fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        (0, 0)
    }
}

/// Once every volume is on disk (after the session read them all and the damage is repaired),
/// declares them as protected native files. zipfast then publishes verified members itself by
/// renaming them into place, instead of RustyGet copying each one. Only done when every volume
/// is still the file the session read: a repair that replaced a volume file would make zipfast
/// refuse the declaration, so those sets keep the copy path.
fn upgrade_protection(
    first: &Path,
    slot: &ProtectionSlot,
    opened: &OpenedVolumes,
    handle_limit: u64,
    steps: u64,
) -> bool {
    let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        return true;
    }
    // zipfast checks protected volumes by (device, inode), which only Unix builds report.
    if cfg!(not(unix)) {
        return false;
    }
    let opened = opened.lock().unwrap_or_else(|e| e.into_inner());
    if opened.is_empty() {
        return false;
    }
    // Check the budgets with the volumes read so far before opening them all again.
    if 2 * opened.len() as u64 + 8 > handle_limit || !fits_open_file_limit(opened.len()) {
        return false;
    }
    for (path, id) in opened.iter() {
        match std::fs::metadata(path) {
            Ok(meta) if file_identity(&meta) == *id => {}
            _ => return false,
        }
    }
    let Some(inventory) = protect_volumes(first) else {
        return false;
    };
    // While opaque, zipfast holds a handle for every volume it read; the declaration adds one
    // per protected volume. The session's handle budget is fixed at creation.
    if inventory.volumes.len() < opened.len()
        || (opened.len() + inventory.volumes.len()) as u64 + 8 > handle_limit
        || !fits_open_file_limit(inventory.volumes.len())
        || !fits_protected_read_budget(&inventory, steps)
    {
        return false;
    }
    *slot = Some(inventory);
    true
}

/// Extra zipfast source-bookkeeping budget for the publication after the protection upgrade.
const PROTECTED_READ_BUDGET: u64 = 32 * 1024 * 1024;

/// With the volumes declared protected, each zipfast validation rechecks every protected volume
/// against every admitted one: roughly 2V^2 + V(2P + 4) bookkeeping units, V volumes with paths
/// up to P bytes. `steps` counts the checks still to come: a publication costs about 16 (it
/// re-validates and sorts the inventory) plus about 14 per parent directory it may create.
/// Upgrade only when they fit the headroom added at session creation (zipfast's default
/// budget, barely used without a declaration, is a further margin).
fn fits_protected_read_budget(
    inventory: &zipfast::progressive::ProtectionInventory,
    steps: u64,
) -> bool {
    let v = inventory.volumes.len() as u64;
    let p = inventory
        .volumes
        .iter()
        .flat_map(|vol| match &vol.backing {
            zipfast::progressive::VolumeBacking::Native(paths) => paths.as_slice(),
            zipfast::progressive::VolumeBacking::NonFile => &[],
        })
        .map(|pp| pp.path.as_os_str().len() as u64)
        .max()
        .unwrap_or(0);
    let per_check = 2 * v * v + v * (2 * p + 4);
    per_check.saturating_mul(steps) <= PROTECTED_READ_BUDGET
}

/// What declaring a set costs zipfast on top of an undeclared extraction.
#[derive(Clone, Copy, Debug)]
struct DeclarationCost {
    bookkeeping: u64,
    metadata: u64,
    names: u64,
}

/// Whether a final set may be declared at session start, and what declaring costs on top of
/// an undeclared extraction (SourceBookkeeping, MetadataBytes, NameBytes). zipfast must place every
/// member where the copy path would (no Windows-made name that Windows rules change, no
/// duplicate names). Even with the per-step recheck bounded (zipfast #116), every volume
/// admission sweeps the whole inventory (about 3.3 V^2 (P + 15) bookkeeping units for V
/// volumes with paths up to P bytes) and every member boundary and publication runs a full
/// refresh (about 7-9 M V (P + 16) for M members, more with deep paths), and each of those
/// validations also charges metadata bytes; each read step re-observes one volume. The estimates (with
/// margin) are added to the session's limits; sets whose cost is too large to grant are left
/// undeclared and copied.
fn session_declaration_cost(
    inventory: &zipfast::progressive::ProtectionInventory,
    first: &Path,
    dest: &Path,
    password: &str,
) -> Option<DeclarationCost> {
    /// Largest extra budget granted for a declaration (bookkeeping is CPU work, metadata and
    /// names cumulative accountings of small allocations).
    const MAX_EXTRA_BOOKKEEPING: u64 = 512 * 1024 * 1024;
    const MAX_EXTRA_METADATA: u64 = 256 * 1024 * 1024;
    const MAX_EXTRA_NAMES: u64 = 256 * 1024 * 1024;
    let members = set_members(first, password)?;
    if members.names_differ || members.duplicate_names {
        return None;
    }
    let v = inventory.volumes.len() as u64;
    let p = inventory
        .volumes
        .iter()
        .flat_map(|vol| match &vol.backing {
            zipfast::progressive::VolumeBacking::Native(paths) => paths.as_slice(),
            zipfast::progressive::VolumeBacking::NonFile => &[],
        })
        .map(|pp| pp.path.as_os_str().len() as u64)
        .max()
        .unwrap_or(0);
    // Longest volume file name: every refresh also compares and charges each volume's name.
    let n = inventory
        .volumes
        .iter()
        .map(|vol| vol.name.len() as u64)
        .max()
        .unwrap_or(0);
    // Margins over the measured costs: 4 for 3.3, 12 for 7-9.
    let unit = p.saturating_add(n.saturating_mul(2)).saturating_add(16);
    let admissions = 4u64
        .saturating_mul(v)
        .saturating_mul(v)
        .saturating_mul(unit);
    // Each publication check (the member's file and every parent directory it creates) runs
    // full refreshes plus a target check of 1 + T + P per volume, T the absolute target path.
    let target = (std::path::absolute(dest)
        .map(|d| d.as_os_str().len())
        .unwrap_or(dest.as_os_str().len()) as u64)
        .saturating_add(members.max_name)
        .saturating_add(1);
    let checks = 1u64.saturating_add(members.max_depth);
    let per_member = 12u64
        .saturating_mul(v)
        .saturating_mul(unit.saturating_add(target))
        .saturating_mul(checks);
    // Every read step (about 1 MiB) re-observes one protected volume: 1 + P.
    let data_mib: u64 = inventory
        .volumes
        .iter()
        .flat_map(|vol| match &vol.backing {
            zipfast::progressive::VolumeBacking::Native(paths) => paths.as_slice(),
            zipfast::progressive::VolumeBacking::NonFile => &[],
        })
        .filter_map(|pp| pp.file.as_ref()?.metadata().ok())
        .map(|m| m.len().div_ceil(1 << 20))
        .sum();
    let reads = data_mib.saturating_mul(p.saturating_add(64));
    let bookkeeping = admissions
        .saturating_add(members.count.saturating_mul(per_member))
        .saturating_add(reads);
    let wide = p.saturating_add(n).saturating_add(128);
    let metadata = v.saturating_mul(v).saturating_mul(wide).saturating_add(
        members.count.saturating_mul(
            4u64.saturating_mul(v)
                .saturating_mul(wide)
                .saturating_mul(checks)
                .saturating_add(4u64.saturating_mul(members.max_name)),
        ),
    );
    // Each volume opened and each publication charges every volume's name: V (V + M) N,
    // granted with margin.
    let names = 2u64
        .saturating_mul(v)
        .saturating_mul(v.saturating_add(4u64.saturating_mul(members.count)))
        .saturating_mul(n);
    (bookkeeping <= MAX_EXTRA_BOOKKEEPING
        && metadata <= MAX_EXTRA_METADATA
        && names <= MAX_EXTRA_NAMES)
        .then_some(DeclarationCost {
            bookkeeping,
            metadata,
            names,
        })
}

/// Path components of a member's output name (as the copy path names it).
fn path_depth(name: &[u8]) -> u64 {
    zipfast::path::sanitize(&String::from_utf8_lossy(name), false)
        .map_or(u64::MAX / 8, |p| p.components().count() as u64)
}

/// Whether the process, with the files it has open now, can open `added` more for the
/// declaration and still re-extract the set once (about two handles per volume: the repaired
/// volume sources stay open beside the sub-session), with room left
/// for sockets, article files and other jobs.
fn fits_open_file_limit(added: usize) -> bool {
    #[cfg(unix)]
    {
        let mut lim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: getrlimit writes into the struct we own.
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) } != 0 {
            return false;
        }
        if lim.rlim_cur == libc::RLIM_INFINITY {
            return true;
        }
        let Ok(fds) = std::fs::read_dir("/dev/fd") else {
            return false;
        };
        let open_now = fds.count() as u64;
        let need = open_now
            .saturating_add(3 * added as u64)
            .saturating_add(256);
        need <= lim.rlim_cur as u64
    }
    #[cfg(not(unix))]
    {
        let _ = added;
        false
    }
}

/// The volumes of the set starting at `first`, opened, as a protection inventory. `None` when a
/// volume cannot be opened.
fn protect_volumes(first: &Path) -> Option<zipfast::progressive::ProtectionInventory> {
    use zipfast::progressive::{
        ProtectedPath, ProtectedVolume, ProtectionInventory, VolumeBacking,
    };
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

impl FeedProvider {
    /// Declares a set that was still downloading once the feed reports it final, so members
    /// verified from then on are moved into place instead of copied. Only when that cannot fail
    /// or place a file differently from the copy path: overwrite mode, no deferred (holed)
    /// member, no Windows-named or duplicate-name member, every volume still the file the
    /// session read, and the handle, open-file and bookkeeping budgets fit the rest of the set.
    fn declare_if_became_final(&mut self) {
        if !DECLARE_PROTECTION {
            return;
        }
        let late = &mut self.late;
        if !late.allowed
            || late.saw_holes
            || late.refused
            || self.written_damaged.load(Ordering::Acquire)
        {
            return;
        }
        if self
            .protection
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
        {
            return;
        }
        let became_final = self
            .feed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_became_final();
        if !became_final {
            return;
        }
        // A dead volume borrowed from a duplicate has no file: the set on disk stops at it, and
        // a declaration of that set would leave the borrowed (virtual) volumes out. zipfast
        // asks for the declaration before it asks for the next volume, so this has to be known
        // from the feed, not from the volumes handed out so far.
        if self
            .feed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .has_borrowed()
        {
            late.refused = true;
            return;
        }
        if late.members.is_none() {
            match set_members(&late.first, &late.password) {
                Some(m) if !m.windows_names && !m.duplicate_names => late.members = Some(m),
                _ => {
                    late.refused = true;
                    return;
                }
            }
        }
        let read = self.opened.lock().unwrap_or_else(|e| e.into_inner()).len();
        if late.tried_at == Some(read)
            || late
                .members
                .as_ref()
                .is_some_and(|m| (read as u64) < m.volumes)
        {
            return;
        }
        late.tried_at = Some(read);
        let Some(members) = late.members.as_ref() else {
            return;
        };
        let Some((remaining, unread)) = remaining_bytes(&late.first, &self.opened) else {
            return;
        };
        // Once declared, every source step (each header read of a volume still to open, a
        // byte at a time for its signature) rechecks every protected volume; that cost is
        // not bounded well enough to budget. Declare only when the last volume is open: the
        // set's last member, usually the main payload, is still renamed into place.
        if unread != 0 {
            return;
        }
        // Each 1 MiB data read of the open volume rechecks the declaration, and so do its
        // remaining header steps; each publication costs about 16 checks plus about 14 per
        // parent directory.
        let steps = remaining
            .div_ceil(1024 * 1024)
            .saturating_add(64)
            .saturating_add(
                members
                    .count
                    .saturating_mul(16u64.saturating_add(members.max_depth.saturating_mul(14))),
            );
        upgrade_protection(
            &late.first,
            &self.protection,
            &self.opened,
            late.handle_limit,
            steps,
        );
    }
}

/// Members that start in each volume of a set, from its headers.
struct SetMembers {
    volumes: u64,
    count: u64,
    max_depth: u64,
    /// Longest member name, bytes.
    max_name: u64,
    windows_names: bool,
    /// A Windows-made member whose name Windows path rules change (a backslash or one of
    /// `<>:"|?*`): zipfast would place it elsewhere than the copy path.
    names_differ: bool,
    duplicate_names: bool,
}

/// `None` when a volume's headers cannot be read.
fn set_members(first: &Path, password: &str) -> Option<SetMembers> {
    let mut out = SetMembers {
        volumes: 0,
        count: 0,
        max_depth: 0,
        max_name: 0,
        windows_names: false,
        names_differ: false,
        duplicate_names: false,
    };
    let mut seen = std::collections::HashSet::new();
    let mut p = first.to_path_buf();
    for _ in 0..4096 {
        if !p.is_file() {
            break;
        }
        out.volumes += 1;
        for (name, metas) in volume_metadata(&p, password) {
            out.count += metas.len() as u64;
            out.duplicate_names |= metas.len() > 1 || !seen.insert(name.clone());
            out.windows_names |= metas.iter().any(|m| m.windows_names);
            out.names_differ |= metas.iter().any(|m| m.windows_names)
                && name.iter().any(|b| b"\\<>:\"|?*".contains(b));
            out.max_depth = out.max_depth.max(path_depth(&name));
            out.max_name = out.max_name.max(name.len() as u64);
        }
        match zipfast::volume::next_volume(&p) {
            Some(n) => p = n,
            None => break,
        }
    }
    (out.count > 0).then_some(out)
}

/// Bytes of the set the session has not read yet (every volume it has not opened, plus the
/// one it is reading) and how many volumes it has not opened. `None` when a volume's size
/// cannot be read.
fn remaining_bytes(first: &Path, opened: &OpenedVolumes) -> Option<(u64, u64)> {
    let opened = opened.lock().unwrap_or_else(|e| e.into_inner());
    let seen: std::collections::HashSet<&Path> = opened.iter().map(|(p, _)| p.as_path()).collect();
    let mut total = 0u64;
    let mut unread = 0u64;
    let mut p = first.to_path_buf();
    for _ in 0..4096 {
        if !p.is_file() {
            break;
        }
        let len = std::fs::metadata(&p).ok()?.len();
        let fresh = !seen.contains(p.as_path());
        unread += u64::from(fresh);
        if fresh || opened.last().is_some_and(|(l, _)| *l == p) {
            total = total.saturating_add(len);
        }
        match zipfast::volume::next_volume(&p) {
            Some(n) => p = n,
            None => break,
        }
    }
    Some((total, unread))
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
                self.late.saw_holes |= !holes.is_empty();
                let mut volume = HoledVolume::open(&path, holes).map_err(|cause| {
                    ProgressiveFailure::new(FailureKind::Input {
                        operation: zipfast::progressive::InputOperation::Open,
                        cause,
                    })
                })?;
                volume.count_as = Some(crate::livewrite::ReadKind::Finished);
                if let Ok(meta) = volume.file.metadata() {
                    self.opened
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((path.clone(), file_identity(&meta)));
                }
                Ok(VolumeAnswer::Ready(Box::new(volume)))
            }
            VolumeView::NotYet => {
                // Being written: read its articles as they land.
                let growing = feed.growing(&path);
                // Asked again once it starts being written, unless it is and could not be read
                // that way (then only once it finishes).
                let asked = growing.is_none();
                if let Some(live) = growing {
                    drop(feed);
                    let damaged = self.written_damaged.clone();
                    let on_finished: OnFinished =
                        Arc::new(move |_: &Path, _: &[(u64, u64)], _: bool| {
                            damaged.store(true, Ordering::Release)
                        });
                    if let Ok(volume) = WrittenSource::open(
                        &path,
                        live,
                        self.feed.clone(),
                        self.pending.clone(),
                        true,
                        false,
                        on_finished,
                    ) {
                        let identity = volume.identity();
                        self.opened
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push((path.clone(), identity));
                        return Ok(VolumeAnswer::Ready(Box::new(volume)));
                    }
                }
                // The driver asks the feed again while parked and wakes the session when the
                // volume's state changes.
                *self.pending.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((path, cx.waker().clone(), true, asked));
                Ok(VolumeAnswer::Wait)
            }
            VolumeView::Missing => Ok(VolumeAnswer::Missing),
            // A dead volume handed over before its borrow finished: this path patches holes
            // only from volumes on disk, so it waits for the finished volume.
            VolumeView::Virtual(v) if !v.holes.is_empty() => {
                *self.pending.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((path, cx.waker().clone(), true, true));
                Ok(VolumeAnswer::Wait)
            }
            VolumeView::Virtual(v) => {
                // A virtual volume is no file of the set: never declare the set complete.
                self.late.refused = true;
                Ok(VolumeAnswer::Ready(Box::new(
                    VirtualSource::open(&path, v).map_err(|cause| {
                        ProgressiveFailure::new(FailureKind::Input {
                            operation: zipfast::progressive::InputOperation::Open,
                            cause,
                        })
                    })?,
                )))
            }
        }
    }

    fn protection(&mut self) -> ProgressiveResult<zipfast::progressive::ProtectionDeclaration> {
        self.declare_if_became_final();
        let protected = self.protection.lock().unwrap_or_else(|e| e.into_inner());
        Ok(match &*protected {
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
    /// Whether zipfast sees `file` as the volume's backing (false when it is declared NonFile).
    expose_backing: bool,
    /// Data areas `[start, end)` of the file members in this volume, from its checked headers.
    /// Set only for a persistent session (see `allow_long_holes`): a hole inside one of them is
    /// answered in one go, up to `LONG_HOLE`, instead of once per read.
    data_spans: Vec<(u64, u64)>,
    /// What the bytes read count as in a live extraction's summary (not counted: `None`).
    count_as: Option<crate::livewrite::ReadKind>,
}

/// Longest hole a persistent session takes as one answer (zipfast's cap).
const LONG_HOLE: u64 = 256 << 20;

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
            expose_backing: true,
            data_spans: Vec::new(),
            count_as: None,
        })
    }

    /// Lets holes inside member data be answered past the read's buffer, as a persistent session
    /// accepts (`RangeRead::Hole`): each answer is a `Hole` event and a durable commit, so one
    /// lost stretch of many articles costs one instead of one per 1 MiB read. Only data areas of
    /// file members whose headers check out count; a hole anywhere else (a header, a service
    /// block) is answered per read as before. Volumes without holes are not parsed.
    fn allow_long_holes(&mut self, path: &Path) {
        if self.holes.is_empty() {
            return;
        }
        use zipfast::rar5::reader::{Block, Volume};
        let Ok(mut vol) = Volume::open(path) else {
            return;
        };
        let extent = self.info.extent;
        for _ in 0..100_000 {
            match vol.next_block() {
                Ok(Some(Block::File(h))) => {
                    let end = h.data_at.saturating_add(h.packed_size).min(extent);
                    if h.data_at < end {
                        self.data_spans.push((h.data_at, end));
                    }
                }
                Ok(Some(Block::End { .. })) | Ok(None) | Err(_) => break,
                Ok(Some(_)) => {}
            }
        }
    }
}

impl RangeSource for HoledVolume {
    fn info(&self) -> SourceInfo {
        self.info
    }
    fn backing_files(&self) -> &[std::fs::File] {
        if self.expose_backing {
            std::slice::from_ref(&self.file)
        } else {
            &[]
        }
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
            let hole_end = start + len;
            // A hole answer may not be longer than the read it answers, except inside a
            // member's data area in a persistent session (see `allow_long_holes`).
            let mut end = hole_end.min(offset + buf.len() as u64);
            if let Some(&(_, span_end)) = self
                .data_spans
                .iter()
                .find(|&&(s, e)| s <= offset && offset < e)
            {
                end = end.max(hole_end.min(span_end).min(offset + LONG_HOLE));
            }
            return Ok(RangeRead::Hole { len: end - offset });
        }
        let mut n = buf
            .len()
            .min(self.info.extent.saturating_sub(offset) as usize);
        if let Some(&(start, _)) = self.holes.iter().find(|&&(s, _)| s > offset) {
            n = n.min((start - offset) as usize);
        }
        let read = read_at(&self.file, &mut buf[..n], offset);
        match read {
            Ok(0) if n > 0 => Err(ProgressiveFailure::new(FailureKind::Truncated)),
            Ok(count) => {
                if let Some(kind) = self.count_as {
                    crate::livewrite::count_read(count as u64, kind);
                }
                Ok(RangeRead::Filled { count })
            }
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

/// Called when a volume read while it was written turns out finished with lost ranges, or
/// finished as another file than the one written (`replaced`): the provider records it as it
/// records a damaged volume handed out whole.
pub(crate) type OnFinished = Arc<dyn Fn(&Path, &[(u64, u64)], bool) + Send + Sync>;

/// A volume the download is still writing, read article by article as the writer reports
/// them (writer-fed, see [`crate::livewrite`]): written bytes are read at once through
/// zipfast's `map::VolumeFiles`, bytes not written yet park the session until they are. Once
/// the download's feed has the finished volume (renamed into place, with its lost ranges if
/// articles failed), the rest is read from it exactly as a volume handed over finished
/// ([`HoledVolume`]).
struct WrittenSource {
    path: PathBuf,
    live: Arc<crate::livewrite::LiveVolume>,
    files: zipfast::map::VolumeFiles,
    synced: u64,
    feed: Arc<Mutex<Box<dyn VolumeFeed>>>,
    pending: PendingWait,
    info: SourceInfo,
    /// The output's handle: what zipfast sees as the volume's backing, unless declared NonFile.
    file: std::fs::File,
    expose_backing: bool,
    /// The finished volume once the feed has it: its file, or the stand-in rebuilt from a
    /// duplicate when the download died in it (see `crate::deadvolume`).
    done: Option<Box<dyn RangeSource>>,
    on_finished: OnFinished,
    /// A persistent session: holes inside member data are answered past the buffer.
    long_holes: bool,
    /// Byte ranges handed to the session from what the download wrote, merged.
    served: Vec<std::ops::Range<u64>>,
    /// `done` is a stand-in rebuilt from a duplicate: every byte is read from it.
    rebuilt: bool,
}

impl WrittenSource {
    #[allow(clippy::too_many_arguments)]
    fn open(
        path: &Path,
        live: Arc<crate::livewrite::LiveVolume>,
        feed: Arc<Mutex<Box<dyn VolumeFeed>>>,
        pending: PendingWait,
        expose_backing: bool,
        long_holes: bool,
        on_finished: OnFinished,
    ) -> std::io::Result<Self> {
        let file = std::fs::File::open(live.out_path())?;
        let meta = file.metadata()?;
        if meta.len() != live.size() {
            // Not the output created at the volume's length: wait for the finished volume.
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "the output being written does not have the volume's length",
            ));
        }
        let (dev, ino) = file_identity(&meta);
        let mut files = zipfast::map::VolumeFiles::new();
        files.set_file(0, file.try_clone()?);
        let len = live.size();
        Ok(Self {
            path: path.to_path_buf(),
            live,
            files,
            synced: 0,
            feed,
            pending,
            info: SourceInfo {
                id: zipfast::progressive::SourceId((u128::from(dev) << 64) | u128::from(ino)),
                generation: 0,
                extent: len,
                length: zipfast::progressive::SourceLength::Final(len),
            },
            file,
            expose_backing,
            done: None,
            on_finished,
            long_holes,
            served: Vec::new(),
            rebuilt: false,
        })
    }

    /// Whether every byte served so far reads the same from `other` (the finished volume, or a
    /// stand-in rebuilt from a duplicate).
    fn served_matches(&mut self, other: &mut dyn RangeSource) -> bool {
        use zipfast::map::{PackedRead, PackedSource};
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut ours = vec![0u8; 1 << 20];
        let mut theirs = vec![0u8; 1 << 20];
        for r in self.served.clone() {
            let mut at = r.start;
            while at < r.end {
                let want = ((r.end - at) as usize).min(ours.len());
                let n = match self.files.read_packed(0, at, &mut ours[..want]) {
                    PackedRead::Data(n) => n,
                    _ => return false,
                };
                let mut got = 0;
                while got < n {
                    match other.read_at(at + got as u64, &mut theirs[got..n], &mut cx) {
                        Ok(RangeRead::Filled { count }) if count > 0 => got += count,
                        _ => return false,
                    }
                }
                if ours[..n] != theirs[..n] {
                    return false;
                }
                at += n as u64;
            }
        }
        true
    }

    /// Drops the wait this source left for its own volume once it reads again (the driver
    /// would report the session as waiting for a volume while it decodes).
    fn clear_own_pending(&self) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending
            .as_ref()
            .is_some_and(|(path, _, _, asked)| !*asked && *path == self.path)
        {
            *pending = None;
        }
    }

    fn note_served(&mut self, offset: u64, count: usize) {
        let r = offset..offset + count as u64;
        match self.served.last_mut() {
            Some(last) if last.end == r.start => last.end = r.end,
            _ => {
                self.served.push(r);
                self.served.sort_by_key(|r| r.start);
                let mut merged: Vec<std::ops::Range<u64>> = Vec::with_capacity(self.served.len());
                for r in self.served.drain(..) {
                    match merged.last_mut() {
                        Some(m) if m.end >= r.start => m.end = m.end.max(r.end),
                        _ => merged.push(r),
                    }
                }
                self.served = merged;
            }
        }
    }

    fn identity(&self) -> (u64, u64) {
        let id = self.info.id.0;
        ((id >> 64) as u64, id as u64)
    }

    /// The download finished the volume: read the rest from the finished file, with `holes`.
    fn finish(&mut self, holes: Vec<(u64, u64)>) -> ProgressiveResult<()> {
        let mut v = HoledVolume::open(&self.path, holes.clone()).map_err(|cause| {
            ProgressiveFailure::new(FailureKind::Input {
                operation: zipfast::progressive::InputOperation::Open,
                cause,
            })
        })?;
        if v.info.length != self.info.length {
            return Err(ProgressiveFailure::new(FailureKind::Truncated));
        }
        let replaced = v
            .file
            .metadata()
            .map(|m| file_identity(&m) != self.identity())
            .unwrap_or(true);
        // Finished as another file than the one written (a copy, or another file of the same
        // name): only if what was read already reads the same from it.
        if replaced && !self.served_matches(&mut v) {
            return Err(ProgressiveFailure::new(FailureKind::Input {
                operation: zipfast::progressive::InputOperation::Read,
                cause: std::io::Error::other("the volume read as it was written finished as another file"),
            }));
        }
        v.expose_backing = self.expose_backing;
        v.count_as = Some(crate::livewrite::ReadKind::Back);
        if self.long_holes {
            v.allow_long_holes(&self.path);
        }
        if !holes.is_empty() || replaced {
            (self.on_finished)(&self.path, &holes, replaced);
        }
        self.live.stop_waiting();
        self.done = Some(Box::new(v));
        Ok(())
    }

    /// The download died in this volume and its part was borrowed from a duplicate: read the
    /// rest from the rebuilt volume, which has the volume's layout and length.
    /// Only when every byte already read from what the download wrote reads the same from the
    /// stand-in (a twin's bytes): a stand-in with its own layout (rebuilt headers, an alt) cannot
    /// be mixed into one read. The session then fails and the extraction starts over on the
    /// copy path, which reads the stand-in from its start (see `persistent::Run::stop`).
    fn finish_virtual(&mut self, v: crate::deadvolume::VirtualVolume) -> ProgressiveResult<()> {
        let mut source = VirtualSource::open(&self.path, v).map_err(|cause| {
            ProgressiveFailure::new(FailureKind::Input {
                operation: zipfast::progressive::InputOperation::Open,
                cause,
            })
        })?;
        if source.info.length != self.info.length || !self.served_matches(&mut source) {
            return Err(ProgressiveFailure::new(FailureKind::Input {
                operation: zipfast::progressive::InputOperation::Read,
                cause: std::io::Error::other(
                    "the volume being read as it was written was rebuilt from a duplicate",
                ),
            }));
        }
        self.rebuilt = true;
        // No file of the set: never declared.
        (self.on_finished)(&self.path, &[], true);
        self.live.stop_waiting();
        self.done = Some(Box::new(source));
        Ok(())
    }
}

impl Drop for WrittenSource {
    fn drop(&mut self) {
        // A session cancelled or ended while parked here waits no more.
        self.live.stop_waiting();
    }
}

impl RangeSource for WrittenSource {
    fn info(&self) -> SourceInfo {
        self.info
    }
    fn backing_files(&self) -> &[std::fs::File] {
        if self.expose_backing {
            std::slice::from_ref(&self.file)
        } else {
            &[]
        }
    }
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        cx: &mut Context<'_>,
    ) -> ProgressiveResult<RangeRead> {
        use zipfast::map::{PackedRead, PackedSource};
        let input = |cause: std::io::Error| {
            let mut e = ProgressiveFailure::new(FailureKind::Input {
                operation: zipfast::progressive::InputOperation::Read,
                cause,
            });
            e.context.packed_offset = Some(offset);
            e
        };
        loop {
            if self.rebuilt {
                if let Some(v) = self.done.as_mut() {
                    return v.read_at(offset, buf, cx);
                }
            }
            // Bytes the writer reported are read from what it wrote, also once the volume is
            // finished (the same file, or the same bytes).
            let n = buf.len().min(self.info.extent.saturating_sub(offset) as usize);
            self.live.sync(&mut self.files, 0, &mut self.synced);
            let unreadable = match self.files.read_packed(0, offset, &mut buf[..n]) {
                PackedRead::Data(count) => {
                    self.note_served(offset, count);
                    self.clear_own_pending();
                    self.live.count_live(count as u64);
                    return Ok(RangeRead::Filled { count });
                }
                // A written article that cannot be read back (VolumeFiles answers a hole): the
                // finished volume answers for it, or it fails the read below.
                PackedRead::Hole { .. } if self.done.is_none() && !self.path.is_file() => {
                    return Err(input(std::io::Error::other("cannot read a written article back")))
                }
                PackedRead::Hole { .. } => true,
                PackedRead::NotYet { .. } => false,
            };
            if let Some(v) = self.done.as_mut() {
                // Never reported (lost, or repaired since): the finished volume's answer.
                return v.read_at(offset, buf, cx);
            }
            let view = self.feed.lock().unwrap_or_else(|e| e.into_inner()).volume(&self.path);
            match view {
                VolumeView::Ready { holes } => {
                    self.finish(holes)?;
                    continue;
                }
                VolumeView::NotYet => {
                    let parked = !self.path.is_file() && self.live.wait_for(offset, cx.waker());
                    if !parked && !unreadable && self.live.written_end(offset).is_some() {
                        // Written meanwhile.
                        continue;
                    }
                    // Also woken when the feed's answer changes: the finished volume can bring
                    // bytes no article write reported (the end of a file the writer assembles).
                    *self.pending.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some((self.path.clone(), cx.waker().clone(), true, false));
                    return Ok(RangeRead::NotYet);
                }
                // A persistent session (its volumes declared NonFile) reads a stand-in whose
                // borrow is not finished yet, as it does one handed over (`persistent::Provider`);
                // the copy path waits for it.
                VolumeView::Virtual(v) if v.holes.is_empty() || (!self.expose_backing && self.served.is_empty()) => {
                    self.finish_virtual(v)?;
                    continue;
                }
                VolumeView::Virtual(_) => {
                    // Its borrow is not finished: woken once the feed's answer changes.
                    *self.pending.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some((self.path.clone(), cx.waker().clone(), true, false));
                    return Ok(RangeRead::NotYet);
                }
                VolumeView::Missing => {
                    // The download ended without finishing this volume: what it did not write
                    // is lost, as in a damaged volume (read past; the set fails as damaged
                    // unless something repairs it).
                    (self.on_finished)(&self.path, &[], true);
                    self.live.stop_waiting();
                    let next = self.live.next_written(offset).unwrap_or(self.info.extent);
                    if next <= offset && !unreadable {
                        // Written meanwhile.
                        continue;
                    }
                    let end = if unreadable {
                        self.live.written_end(offset).unwrap_or(offset + 1)
                    } else {
                        next
                    };
                    let len = end.max(offset + 1).min(offset + n.max(1) as u64) - offset;
                    return Ok(RangeRead::Hole { len });
                }
            }
        }
    }
}

/// A dead volume as rebuilt from a duplicate: headers in memory, the borrowed part in a
/// scratch file. Bytes not borrowed yet (a volume handed over before its borrow finished) are
/// answered as holes; everything else is filled, and the extraction's own checks decide
/// whether the borrowed bytes were right.
struct VirtualSource {
    head: Vec<u8>,
    data: std::fs::File,
    data_len: u64,
    tail: Vec<u8>,
    holes: Vec<(u64, u64)>,
    info: SourceInfo,
}

impl VirtualSource {
    fn open(path: &Path, v: crate::deadvolume::VirtualVolume) -> std::io::Result<Self> {
        let data = std::fs::File::open(&v.data)?;
        if data.metadata()?.len() < v.data_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "borrowed data is shorter than its part",
            ));
        }
        // An identity no file on disk has: the volume's name, hashed, in the upper half.
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut h);
        let len = v.len();
        Ok(Self {
            info: SourceInfo {
                id: zipfast::progressive::SourceId(u128::from(h.finish()) << 64 | 0xdead),
                generation: 0,
                extent: len,
                length: zipfast::progressive::SourceLength::Final(len),
            },
            head: v.head,
            data,
            data_len: v.data_len,
            tail: v.tail,
            holes: v.holes,
        })
    }
}

/// A finished dead volume as one readable file (headers, borrowed part, tail), for reading its
/// bytes back into an output whose holes it fills.
pub(crate) struct VirtualFile {
    head: Vec<u8>,
    data: std::fs::File,
    data_len: u64,
    tail: Vec<u8>,
    pos: u64,
}

impl VirtualFile {
    pub(crate) fn open(v: &crate::deadvolume::VirtualVolume) -> std::io::Result<Self> {
        let data = std::fs::File::open(&v.data)?;
        if data.metadata()?.len() < v.data_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "borrowed data is shorter than its part",
            ));
        }
        Ok(Self {
            head: v.head.clone(),
            data,
            data_len: v.data_len,
            tail: v.tail.clone(),
            pos: 0,
        })
    }

    pub(crate) fn len(&self) -> u64 {
        self.head.len() as u64 + self.data_len + self.tail.len() as u64
    }
}

impl std::io::Read for VirtualFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let head = self.head.len() as u64;
        let data_end = head + self.data_len;
        let n = buf.len().min(self.len().saturating_sub(self.pos) as usize);
        if n == 0 {
            return Ok(0);
        }
        let at = self.pos;
        let count = if at < head {
            let k = n.min((head - at) as usize);
            buf[..k].copy_from_slice(&self.head[at as usize..at as usize + k]);
            k
        } else if at < data_end {
            let k = n.min((data_end - at) as usize);
            match read_at(&self.data, &mut buf[..k], at - head)? {
                0 => return Err(std::io::ErrorKind::UnexpectedEof.into()),
                c => c,
            }
        } else {
            let t = (at - data_end) as usize;
            let k = n.min(self.tail.len() - t);
            buf[..k].copy_from_slice(&self.tail[t..t + k]);
            k
        };
        self.pos += count as u64;
        Ok(count)
    }
}

impl std::io::Seek for VirtualFile {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        let target = match to {
            std::io::SeekFrom::Start(p) => Some(p),
            std::io::SeekFrom::End(d) => self.len().checked_add_signed(d),
            std::io::SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        self.pos = target.ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        Ok(self.pos)
    }
}

impl RangeSource for VirtualSource {
    fn info(&self) -> SourceInfo {
        self.info
    }
    fn backing_files(&self) -> &[std::fs::File] {
        // The scratch file holds only the borrowed part, at other offsets than the volume's:
        // it is not the volume's backing file.
        &[]
    }
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        _cx: &mut Context<'_>,
    ) -> ProgressiveResult<RangeRead> {
        let head = self.head.len() as u64;
        let data_end = head + self.data_len;
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
        let mut n = buf
            .len()
            .min(self.info.extent.saturating_sub(offset) as usize);
        if let Some(&(start, _)) = self.holes.iter().find(|&&(s, _)| s > offset) {
            n = n.min((start - offset) as usize);
        }
        if n == 0 {
            return Err(ProgressiveFailure::new(FailureKind::Truncated));
        }
        let count = if offset < head {
            let k = n.min((head - offset) as usize);
            buf[..k].copy_from_slice(&self.head[offset as usize..offset as usize + k]);
            k
        } else if offset < data_end {
            let k = n.min((data_end - offset) as usize);
            match read_at(&self.data, &mut buf[..k], offset - head) {
                Ok(0) => return Err(ProgressiveFailure::new(FailureKind::Truncated)),
                Ok(c) => c,
                Err(cause) => {
                    let mut e = ProgressiveFailure::new(FailureKind::Input {
                        operation: zipfast::progressive::InputOperation::Read,
                        cause,
                    });
                    e.context.packed_offset = Some(offset);
                    return Err(e);
                }
            }
        } else {
            let at = (offset - data_end) as usize;
            let k = n.min(self.tail.len() - at);
            buf[..k].copy_from_slice(&self.tail[at..at + k]);
            k
        };
        Ok(RangeRead::Filled { count })
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
pub(crate) mod tests;
