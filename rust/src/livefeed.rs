//! Live extraction of 7z, ZIP and split-file sets while they download.
//!
//! RAR sets are read by zipfast's progressive session (`progressive_unpack`). The other sets a
//! download carries are read here, member by member, with zipfast's feed decoders
//! (`zipfast::map::FeedDecoder`) over the volumes already on disk, the way zipfast's
//! `examples/live_feed` plays a Usenet download:
//!
//! - A byte-split 7z set (`name.7z.001`, ...), a byte-split ZIP (`name.zip.001`, ...) and an
//!   Info-ZIP split set (`name.z01`, ..., `name.zip`) are mapped (`zipfast::map::MapBuilder`)
//!   once the bytes holding their headers are on disk: the first volume and the last one, whose
//!   download is moved up for that. Volumes not downloaded yet stand in with the first volume's
//!   length (7-Zip and `split` cut every volume but the last to one size); each is checked when
//!   it arrives.
//! - Every member then gets a feed decoder writing straight into its output file. The decoders
//!   run again as each volume completes. A damaged volume's lost ranges are holes: read past
//!   where the member's layer allows, and filled when a repair releases the volume.
//! - A split file (`movie.mkv.001`, ...) has no container: each piece is placed at its offset
//!   in the joined output as it completes, so nothing is joined after the download.
//!
//! A set this cannot extract live (a coder zipfast does not feed, a nested archive, encrypted
//! headers without the password) is declined before anything is written, and post-processing
//! extracts it as before.

use std::fs::File;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use zipfast::map::{
    FeedDecoder, FeedError, FeedOptions, FeedSink, MapBuilder, MapError, MapOptions, MemberMap,
    PackedRead, PackedSource,
};
use zipfast::progressive::{
    ProgressiveResult, RangeRead, RangeSource, SourceId, SourceInfo, SourceLength, VolumeAnswer,
    VolumeRequest, VolumeSource,
};

use crate::progressive_unpack::{VolumeFeed, VolumeView};

/// How long the extraction sleeps when no volume brought anything new.
const IDLE: Duration = Duration::from_millis(100);
/// How often a map that could not be built yet is tried again without any volume arriving.
const MAP_RETRY: Duration = Duration::from_secs(2);
/// Members of one 7z folder decoded live at most: each decoder of a later member decodes the
/// folder's output in front of it again.
const MAX_FOLDER_MEMBERS: u64 = 64;

/// What kind of set a volume belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetKind {
    /// `name.7z.001`, `name.7z.002`, ...: one 7z archive cut into byte pieces.
    SevenZip,
    /// `name.zip.001`, `name.zip.002`, ...: one ZIP archive cut into byte pieces.
    ZipBytes,
    /// `name.z01`, `name.z02`, ..., `name.zip`: an Info-ZIP split set.
    ZipSplit,
    /// `name.ext.001`, `name.ext.002`, ...: a file cut into pieces, no container.
    Split,
}

/// A set's volumes as the download names them, first volume first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveSet {
    pub kind: SetKind,
    pub volumes: Vec<Vec<u8>>,
}

impl LiveSet {
    /// The volume holding the set's directory (7z header, ZIP central directory).
    pub fn last(&self) -> &[u8] {
        self.volumes.last().map_or(&[][..], Vec::as_slice)
    }
}

/// What a name says: the set kind, the lower-case name in front of the volume number, and the
/// volume number (0 for an Info-ZIP set's `.zip`, which comes last) with its digit count.
fn classify(name: &[u8]) -> Option<(SetKind, Vec<u8>, u32, usize)> {
    let dot = name.iter().rposition(|&b| b == b'.')?;
    let (stem, ext) = (&name[..dot], &name[dot + 1..]);
    if stem.is_empty() || ext.is_empty() {
        return None;
    }
    let number = |digits: &[u8]| -> Option<u32> {
        (digits.iter().all(u8::is_ascii_digit) && digits.len() <= 9)
            .then(|| std::str::from_utf8(digits).ok()?.parse().ok())
            .flatten()
    };
    // Info-ZIP: name.z01 .. name.z99, name.z100 ..; name.zip is its last volume.
    if ext.len() >= 3 && ext[0].eq_ignore_ascii_case(&b'z') {
        if let Some(n) = number(&ext[1..]).filter(|&n| n >= 1) {
            return Some((
                SetKind::ZipSplit,
                stem.to_ascii_lowercase(),
                n,
                ext.len() - 1,
            ));
        }
    }
    if ext.eq_ignore_ascii_case(b"zip") {
        return Some((SetKind::ZipSplit, stem.to_ascii_lowercase(), 0, 0));
    }
    // name.<inner>.NNN
    if ext.len() < 3 {
        return None;
    }
    let n = number(ext)?;
    let inner_dot = stem.iter().rposition(|&b| b == b'.')?;
    let inner = &stem[inner_dot + 1..];
    if inner_dot == 0 {
        return None;
    }
    let kind = if inner.eq_ignore_ascii_case(b"7z") {
        SetKind::SevenZip
    } else if inner.eq_ignore_ascii_case(b"zip") {
        SetKind::ZipBytes
    } else if inner.eq_ignore_ascii_case(b"rar") {
        return None;
    } else if ext.len() == 3
        && inner.len() == 3
        && inner
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b',')
    {
        // As post-processing's join recognises a split file (`.xxx.001`).
        SetKind::Split
    } else {
        return None;
    };
    Some((kind, stem.to_ascii_lowercase(), n, ext.len()))
}

fn basename(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/' || b == crate::paths::PATH_SEPARATOR) {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// Tells the caller once when the extraction starts waiting for volumes and once when it works
/// again.
struct Waiting<'a> {
    on_wait: &'a dyn Fn(bool),
    waiting: bool,
}

impl Waiting<'_> {
    fn idle(&mut self) {
        if !self.waiting {
            self.waiting = true;
            (self.on_wait)(true);
        }
        std::thread::sleep(IDLE);
    }
    fn busy(&mut self) {
        if self.waiting {
            self.waiting = false;
            (self.on_wait)(false);
        }
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.busy();
    }
}

/// Whether `name` is the first volume of a set extracted here.
pub fn is_live_first(name: &[u8]) -> bool {
    matches!(classify(basename(name)), Some((_, _, 1, _)))
}

/// Whether `name` is a volume of a set extracted here (an Info-ZIP set's `.zip` is not counted:
/// alone it is an ordinary archive).
pub fn is_live_volume(name: &[u8]) -> bool {
    matches!(classify(basename(name)), Some((_, _, n, _)) if n >= 1)
}

/// The set whose first volume is `first`, from the names of the download's files: every volume
/// numbered from 1 without a gap, at least two of them.
pub fn live_set(first: &[u8], names: &[Vec<u8>]) -> Option<LiveSet> {
    let (kind, stem, 1, _) = classify(first)? else {
        return None;
    };
    let mut numbered: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut last_zip: Option<Vec<u8>> = None;
    for name in names {
        let Some((k, s, n, _)) = classify(name) else {
            continue;
        };
        if k != kind || s != stem {
            continue;
        }
        if n == 0 {
            if last_zip.replace(name.clone()).is_some() {
                return None;
            }
            continue;
        }
        if numbered.iter().any(|(m, _)| *m == n) {
            return None;
        }
        numbered.push((n, name.clone()));
    }
    numbered.sort();
    if numbered
        .iter()
        .enumerate()
        .any(|(i, (n, _))| *n as usize != i + 1)
    {
        return None;
    }
    let mut volumes: Vec<Vec<u8>> = numbered.into_iter().map(|(_, v)| v).collect();
    if kind == SetKind::ZipSplit {
        volumes.push(last_zip?);
    }
    (volumes.len() >= 2).then_some(LiveSet { kind, volumes })
}

/// Whether `name` is a volume of a set whose first volume is one of `firsts` (the archives
/// direct unpack extracted): post-processing leaves such volumes alone.
pub fn in_extracted_set(name: &[u8], firsts: &[&[u8]]) -> bool {
    let Some((kind, stem, _, _)) = classify(name) else {
        return false;
    };
    firsts
        .iter()
        .any(|first| matches!(classify(first), Some((k, s, 1, _)) if k == kind && s == stem))
}

// ---- Volumes on disk, as the download feed sees them ----

/// What one volume is right now.
enum View {
    /// On disk, `len` bytes, with these lost ranges (`offset`, `len`).
    Ready {
        len: u64,
        holes: Vec<(u64, u64)>,
    },
    NotYet,
    Missing,
}

/// The set's volumes in the download folder, read through the download's feed.
struct Volumes<'f> {
    paths: Vec<PathBuf>,
    feed: &'f mut dyn VolumeFeed,
    /// Open handles; `clean` once complete without holes (not asked of the feed again).
    files: Vec<Option<File>>,
    clean: Vec<bool>,
    /// The length each volume must have (the map was built with it).
    expect: Vec<Option<u64>>,
    /// A volume whose length differs from the one the map was built with.
    wrong: Option<String>,
}

impl<'f> Volumes<'f> {
    fn new(dir: &Path, set: &LiveSet, feed: &'f mut dyn VolumeFeed) -> Self {
        let n = set.volumes.len();
        Self {
            paths: set
                .volumes
                .iter()
                .map(|v| dir.join(crate::fsops::path(v)))
                .collect(),
            feed,
            files: (0..n).map(|_| None).collect(),
            clean: vec![false; n],
            expect: vec![None; n],
            wrong: None,
        }
    }

    fn view(&mut self, i: usize) -> View {
        if self.clean[i] {
            let len = self.files[i]
                .as_ref()
                .and_then(|f| f.metadata().ok())
                .map_or(0, |m| m.len());
            return View::Ready {
                len,
                holes: Vec::new(),
            };
        }
        let holes = match self.feed.volume(&self.paths[i]) {
            VolumeView::Ready { holes } => holes,
            VolumeView::Missing => return View::Missing,
            VolumeView::NotYet | VolumeView::Virtual(_) => return View::NotYet,
        };
        if self.files[i].is_none() {
            match File::open(&self.paths[i]) {
                Ok(f) => self.files[i] = Some(f),
                Err(_) => return View::NotYet,
            }
        }
        let len = self.files[i]
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map_or(0, |m| m.len());
        if let Some(want) = self.expect[i].filter(|&w| w != len) {
            self.wrong.get_or_insert_with(|| {
                format!(
                    "{} has {len} bytes, not the {want} its set was mapped with",
                    self.paths[i].display()
                )
            });
        }
        if holes.is_empty() {
            self.clean[i] = true;
        }
        View::Ready { len, holes }
    }

    fn read(&mut self, i: usize, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        let Some(f) = self.files[i].as_ref() else {
            return Ok(0);
        };
        read_at(f, buf, offset)
    }
}

impl PackedSource for Volumes<'_> {
    fn read_packed(&mut self, volume: u32, offset: u64, buf: &mut [u8]) -> PackedRead {
        let i = volume as usize;
        if i >= self.paths.len() {
            return PackedRead::Hole { len: u64::MAX };
        }
        let (len, holes) = match self.view(i) {
            View::Ready { len, holes } => (len, holes),
            View::NotYet => return PackedRead::NotYet { len: u64::MAX },
            View::Missing => return PackedRead::Hole { len: u64::MAX },
        };
        if offset >= len {
            return PackedRead::Hole { len: u64::MAX };
        }
        if let Some(&(s, l)) = holes.iter().find(|&&(s, l)| s <= offset && offset < s + l) {
            return PackedRead::Hole {
                len: s + l - offset,
            };
        }
        let mut n = (buf.len() as u64).min(len - offset);
        if let Some(&(s, _)) = holes.iter().filter(|&&(s, _)| s > offset).min() {
            n = n.min(s - offset);
        }
        match self.read(i, offset, &mut buf[..n as usize]) {
            Ok(0) | Err(_) => PackedRead::Hole { len: n.max(1) },
            Ok(k) => PackedRead::Data(k),
        }
    }
}

#[cfg(unix)]
fn read_at(f: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        match std::os::unix::fs::FileExt::read_at(f, &mut buf[done..], offset + done as u64) {
            Ok(0) => break,
            Ok(k) => done += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(done)
}

#[cfg(not(unix))]
fn read_at(f: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(f, buf, offset)
}

#[cfg(unix)]
fn write_at(f: &File, buf: &[u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(f, buf, offset)
}

#[cfg(not(unix))]
fn write_at(f: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        let n = std::os::windows::fs::FileExt::seek_write(f, buf, offset)?;
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

// ---- The map, built from the volumes on disk ----

/// One volume as the map walk reads it: a file (lost ranges unreadable), or a stand-in of the
/// length the volume will have, which has no bytes yet.
struct MapVolume {
    file: Option<File>,
    holes: Vec<(u64, u64)>,
    info: SourceInfo,
}

impl RangeSource for MapVolume {
    fn info(&self) -> SourceInfo {
        self.info
    }
    fn backing_files(&self) -> &[File] {
        &[]
    }
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        _cx: &mut Context<'_>,
    ) -> ProgressiveResult<RangeRead> {
        let Some(file) = self.file.as_ref() else {
            return Ok(RangeRead::NotYet);
        };
        if let Some(&(s, l)) = self
            .holes
            .iter()
            .find(|&&(s, l)| s <= offset && offset < s + l)
        {
            return Ok(RangeRead::Hole {
                len: (s + l - offset).min(buf.len() as u64),
            });
        }
        let mut n = buf
            .len()
            .min(self.info.extent.saturating_sub(offset) as usize);
        if let Some(&(s, _)) = self.holes.iter().filter(|&&(s, _)| s > offset).min() {
            n = n.min((s - offset) as usize);
        }
        match read_at(file, &mut buf[..n], offset) {
            Ok(k) if k > 0 => Ok(RangeRead::Filled { count: k }),
            _ => Err(zipfast::progressive::ProgressiveFailure::new(
                zipfast::progressive::FailureKind::Truncated,
            )),
        }
    }
}

/// What the map walk is handed: each volume's state when the attempt started.
struct MapVolumes {
    volumes: Vec<Option<MapVolume>>,
}

impl VolumeSource for MapVolumes {
    fn volume(
        &mut self,
        request: &VolumeRequest,
        _cx: &mut Context<'_>,
    ) -> ProgressiveResult<VolumeAnswer> {
        let i = request.index as usize;
        let Some(slot) = self.volumes.get_mut(i) else {
            return Ok(VolumeAnswer::Missing);
        };
        // Asked again (the walk closes volumes past its handle budget): the same answer.
        Ok(match slot {
            Some(v) => VolumeAnswer::Ready(Box::new(MapVolume {
                file: v.file.as_ref().and_then(|f| f.try_clone().ok()),
                holes: v.holes.clone(),
                info: v.info,
            })),
            None => VolumeAnswer::Wait,
        })
    }
}

fn source_id(file: &File, fallback: u128) -> SourceId {
    #[cfg(unix)]
    if let Ok(meta) = file.metadata() {
        use std::os::unix::fs::MetadataExt;
        return SourceId((u128::from(meta.dev()) << 64) | u128::from(meta.ino()));
    }
    let _ = file;
    SourceId(fallback)
}

/// Why a map attempt gave none.
enum NoMap {
    /// Not yet: a volume the headers need has not arrived (or is held for its repair).
    Later,
    /// Never live: post-processing extracts the set.
    Declined(String),
}

/// One attempt at mapping the set from what is on disk. Volumes not there yet stand in with
/// the first volume's length, except the last (it carries the directory and its own length).
fn try_map(
    vols: &mut Volumes<'_>,
    set: &LiveSet,
    password: &str,
) -> Result<(MemberMap, Vec<Option<u64>>), NoMap> {
    let n = set.volumes.len();
    let mut lens: Vec<Option<u64>> = vec![None; n];
    let mut volumes: Vec<Option<MapVolume>> = Vec::with_capacity(n);
    let first_len = match vols.view(0) {
        View::Ready { len, .. } => len,
        View::NotYet => return Err(NoMap::Later),
        View::Missing => return Err(NoMap::Declined("its first volume is missing".into())),
    };
    for i in 0..n {
        let view = vols.view(i);
        let vol = match view {
            View::Ready { len, holes } => {
                let Some(file) = vols.files[i].as_ref().and_then(|f| f.try_clone().ok()) else {
                    return Err(NoMap::Later);
                };
                lens[i] = Some(len);
                Some(MapVolume {
                    info: SourceInfo {
                        id: source_id(&file, i as u128),
                        generation: 0,
                        extent: len,
                        length: SourceLength::Final(len),
                    },
                    file: Some(file),
                    holes,
                })
            }
            View::Missing => return Err(NoMap::Declined(format!("volume {} is missing", i + 1))),
            View::NotYet if i + 1 < n => {
                lens[i] = Some(first_len);
                Some(MapVolume {
                    file: None,
                    holes: Vec::new(),
                    info: SourceInfo {
                        id: SourceId(u128::MAX - i as u128),
                        generation: 0,
                        extent: first_len,
                        length: SourceLength::Final(first_len),
                    },
                })
            }
            View::NotYet => None,
        };
        volumes.push(vol);
    }
    if volumes.last().is_some_and(Option::is_none) {
        return Err(NoMap::Later);
    }
    let first_name = std::ffi::OsString::from(crate::fsops::path(&set.volumes[0]).as_os_str());
    let pw = (!password.is_empty()).then_some(password);
    let mut builder = MapBuilder::new(
        Box::new(MapVolumes { volumes }),
        MapOptions::new(first_name).password(pw),
    );
    match builder.poll_map(&mut Context::from_waker(Waker::noop())) {
        Poll::Pending => Err(NoMap::Later),
        Poll::Ready(Ok(map)) if map.complete => Ok((map, lens)),
        Poll::Ready(Ok(_)) => Err(NoMap::Later),
        Poll::Ready(Err(MapError::HeaderHole { .. } | MapError::MissingDirectory { .. })) => {
            Err(NoMap::Later)
        }
        Poll::Ready(Err(e)) => Err(NoMap::Declined(e.to_string())),
    }
}

// ---- Output ----

/// Sorted, disjoint, merged output ranges a sink holds.
#[derive(Default)]
struct Ranges(Vec<Range<u64>>);

impl Ranges {
    fn insert(&mut self, mut r: Range<u64>) {
        let at = self.0.partition_point(|x| x.end < r.start);
        let mut end = at;
        while end < self.0.len() && self.0[end].start <= r.end {
            r.start = r.start.min(self.0[end].start);
            r.end = r.end.max(self.0[end].end);
            end += 1;
        }
        self.0.splice(at..end, [r]);
    }
    fn held_from(&self, offset: u64) -> u64 {
        let at = self.0.partition_point(|x| x.end <= offset);
        self.0
            .get(at)
            .filter(|x| x.start <= offset)
            .map_or(0, |x| x.end - offset)
    }
}

/// A member's output: a new file next to its final name, written in place.
struct OutFile {
    file: File,
    written: Ranges,
}

impl FeedSink for OutFile {
    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> std::io::Result<()> {
        write_at(&self.file, bytes, offset)?;
        self.written.insert(offset..offset + bytes.len() as u64);
        Ok(())
    }
    fn read_held(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.written.held_from(offset).min(buf.len() as u64) as usize;
        if n == 0 {
            return Ok(0);
        }
        read_at(&self.file, &mut buf[..n], offset)
    }
}

/// A member being fed.
struct Member {
    name: String,
    decoder: FeedDecoder,
    out: OutFile,
    tmp: PathBuf,
    target: PathBuf,
    mtime: Option<(i64, u32)>,
    unix_mode: Option<u32>,
    done: bool,
}

/// How a live extraction ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every member written and verified against the archive's checks.
    Done { files: u64, dirs: u64, bytes: u64 },
    /// Not extracted live, nothing written: post-processing extracts the set.
    Declined(String),
    /// Failed after output began; nothing it wrote is left behind.
    Failed(String),
}

/// What [`extract`] works on.
pub struct Request<'a> {
    /// The download folder holding the volumes.
    pub dir: &'a Path,
    pub set: &'a LiveSet,
    pub password: &'a str,
}

/// Where output goes, created once the set is known to be extracted live: the unpack folder
/// and the job's unpack limits.
pub type Prepare<'a> =
    dyn FnMut() -> Option<(PathBuf, Option<crate::unpacklimits::SharedGuard>)> + 'a;

/// Extracts `req.set` while it downloads. `feed` says which volumes are on disk and which bytes
/// in them are lost; `prepare` is called once, before the first output; `on_wait` is told
/// `true` while the extraction waits for volumes and `false` when it works again.
pub fn extract(
    req: &Request<'_>,
    feed: &mut dyn VolumeFeed,
    prepare: &mut Prepare<'_>,
    log: &mut dyn FnMut(i32, &str),
    is_cancelled: &mut dyn FnMut() -> bool,
    on_wait: &dyn Fn(bool),
) -> Outcome {
    if req.set.kind == SetKind::Split {
        return join_live(req, feed, prepare, log, is_cancelled, on_wait);
    }
    let mut vols = Volumes::new(req.dir, req.set, feed);
    let mut waiting = Waiting {
        on_wait,
        waiting: false,
    };
    // Map the set as soon as its headers are on disk.
    let mut last_ready = usize::MAX;
    let mut last_try = Instant::now() - MAP_RETRY;
    let (map, lens) = loop {
        if is_cancelled() {
            return Outcome::Declined("Aborted".into());
        }
        let ready = (0..req.set.volumes.len())
            .filter(|&i| matches!(vols.view(i), View::Ready { .. }))
            .count();
        if ready != last_ready || last_try.elapsed() >= MAP_RETRY {
            last_ready = ready;
            last_try = Instant::now();
            match try_map(&mut vols, req.set, req.password) {
                Ok(found) => break found,
                Err(NoMap::Declined(why)) => return Outcome::Declined(why),
                Err(NoMap::Later) if vols.feed.set_became_final() => {
                    return Outcome::Declined("its headers are not readable".into());
                }
                Err(NoMap::Later) => {}
            }
        }
        waiting.idle();
    };
    waiting.busy();
    vols.expect = lens;
    feed_members(req, &map, &mut vols, prepare, log, is_cancelled, &mut waiting)
}

/// A name that reads as an archive: nested extraction is post-processing's.
fn names_an_archive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let b = lower.as_bytes();
    crate::filetypes::is_rar_file(b)
        || is_live_volume(b)
        || [
            ".7z", ".zip", ".rar", ".tar", ".gz", ".tgz", ".bz2", ".xz", ".zst",
        ]
        .iter()
        .any(|e| lower.ends_with(e))
}

/// Feeds every member of `map` from the volumes on disk into its output.
fn feed_members(
    req: &Request<'_>,
    map: &MemberMap,
    vols: &mut Volumes<'_>,
    prepare: &mut Prepare<'_>,
    log: &mut dyn FnMut(i32, &str),
    is_cancelled: &mut dyn FnMut() -> bool,
    waiting: &mut Waiting<'_>,
) -> Outcome {
    // Everything that could decline first: nothing is written before the set is known to go.
    let pw = (!req.password.is_empty()).then_some(req.password);
    if map.folders.iter().any(|f| f.members > MAX_FOLDER_MEMBERS) {
        return Outcome::Declined("a 7z folder holds too many members to feed live".into());
    }
    struct Planned {
        name: String,
        rel: PathBuf,
        size: u64,
        decoder: Option<FeedDecoder>,
        is_dir: bool,
        mtime: Option<(i64, u32)>,
        unix_mode: Option<u32>,
    }
    let mut planned: Vec<Planned> = Vec::with_capacity(map.members.len());
    for (index, m) in map.members.iter().enumerate() {
        let name = m
            .name
            .clone()
            .unwrap_or_else(|| String::from_utf8_lossy(&m.raw_name).into_owned());
        let Ok(rel) = zipfast::path::sanitize(&name, false) else {
            return Outcome::Declined(format!("{name:?} is not a safe path"));
        };
        if m.is_dir {
            planned.push(Planned {
                name,
                rel,
                size: 0,
                decoder: None,
                is_dir: true,
                mtime: m.mtime,
                unix_mode: m.unix_mode,
            });
            continue;
        }
        if names_an_archive(&name) {
            return Outcome::Declined(format!("it holds the archive {name}"));
        }
        let Some(size) = m.unpacked_size else {
            return Outcome::Declined(format!("{name} has no recorded size"));
        };
        let decoder = if size == 0 {
            None
        } else {
            let options = FeedOptions {
                max_needs: 8,
                ..FeedOptions::new(pw)
            };
            match FeedDecoder::new(map, index, options) {
                Ok(d) => Some(d),
                Err(FeedError::Unsupported(why)) => {
                    return Outcome::Declined(format!("{name} cannot be fed live ({why:?})"))
                }
                Err(e) => return Outcome::Declined(format!("{name}: {e}")),
            }
        };
        planned.push(Planned {
            name,
            rel,
            size,
            decoder,
            is_dir: false,
            mtime: m.mtime,
            unix_mode: m.unix_mode,
        });
    }
    let Some((dest, guard)) = prepare() else {
        return Outcome::Declined("no unpack folder".into());
    };
    let _scope = guard.as_ref().map(crate::unpacklimits::Scope::enter);
    let first = String::from_utf8_lossy(&req.set.volumes[0]).into_owned();
    log(4, &format!("Unrar: Extracting from {first}"));
    // Admit and create every output before any byte is written.
    let mut members: Vec<Member> = Vec::new();
    let mut dirs: Vec<(PathBuf, Option<(i64, u32)>)> = Vec::new();
    let mut summary = (0u64, 0u64, 0u64);
    let fail = |members: &[Member], why: String| {
        for m in members {
            let _ = std::fs::remove_file(if m.done { &m.target } else { &m.tmp });
        }
        Outcome::Failed(why)
    };
    for p in planned {
        let target = dest.join(&p.rel);
        if let Err(why) = crate::unpacklimits::check(&target, p.is_dir) {
            log(1, &why);
            return fail(&members, why);
        }
        if p.is_dir {
            match crate::archive_native::create_dir_all_no_follow(&dest, &p.rel) {
                Ok(dir) => dirs.push((dir, p.mtime)),
                Err(e) => return fail(&members, format!("cannot create {}: {e}", p.rel.display())),
            }
            summary.1 += 1;
            continue;
        }
        if let Some(parent) = p.rel.parent().filter(|d| !d.as_os_str().is_empty()) {
            if let Err(e) = crate::archive_native::create_dir_all_no_follow(&dest, parent) {
                return fail(&members, format!("cannot create {}: {e}", parent.display()));
            }
        } else if let Err(e) = std::fs::create_dir_all(&dest) {
            return fail(&members, format!("cannot create {}: {e}", dest.display()));
        }
        let (tmp, file) = match crate::progressive_unpack::create_unique_sibling(&target) {
            Ok(created) => created,
            Err(e) => return fail(&members, format!("cannot write {}: {e}", target.display())),
        };
        let Some(decoder) = p.decoder else {
            // An empty file: nothing to feed.
            drop(file);
            if let Err(e) = std::fs::rename(&tmp, &target) {
                let _ = std::fs::remove_file(&tmp);
                return fail(&members, format!("cannot write {}: {e}", target.display()));
            }
            summary.0 += 1;
            continue;
        };
        let _ = file.set_len(p.size);
        log(4, &format!("Unrar: Extracting  {}", p.name));
        members.push(Member {
            name: p.name,
            decoder,
            out: OutFile {
                file,
                written: Ranges::default(),
            },
            tmp,
            target,
            mtime: p.mtime,
            unix_mode: p.unix_mode,
            done: false,
        });
    }
    // Decode as the volumes arrive.
    loop {
        if is_cancelled() {
            return fail(&members, "Aborted".into());
        }
        let mut progressed = false;
        let mut failure: Option<String> = None;
        for m in members.iter_mut().filter(|m| !m.done) {
            if let Err(why) = step(m, vols, &mut progressed) {
                failure = Some(why);
                break;
            }
            if m.done {
                summary.0 += 1;
                summary.2 += m.decoder.size();
            }
        }
        let failure = failure.or_else(|| vols.wrong.take().map(|why| format!("Unrar: ERROR: {why}")));
        if let Some(why) = failure {
            log(1, &why);
            return fail(&members, why);
        }
        if members.iter().all(|m| m.done) {
            break;
        }
        if progressed {
            waiting.busy();
        } else if vols.feed.set_became_final() {
            // Every volume is on disk and none will change: what is still lost stays lost.
            let names: Vec<&str> = members
                .iter()
                .filter(|m| !m.done)
                .map(|m| m.name.as_str())
                .collect();
            let why = format!(
                "Unrar: ERROR: {} could not be completed from the volumes",
                names.join(", ")
            );
            log(1, &why);
            return fail(&members, why);
        } else {
            waiting.idle();
        }
    }
    set_directory_times(dirs);
    Outcome::Done {
        files: summary.0,
        dirs: summary.1,
        bytes: summary.2,
    }
}

/// Runs one member's decoder over what is on disk; publishes its output once it is complete and
/// verified.
fn step(m: &mut Member, vols: &mut Volumes<'_>, progressed: &mut bool) -> Result<(), String> {
    let before = m.decoder.stats().written;
    let status = loop {
        match m.decoder.run(vols, &mut m.out) {
            Ok(s) if s.more => continue,
            Ok(s) => break s,
            Err(e) => return Err(format!("Unrar: ERROR in {}: {e}", m.name)),
        }
    };
    *progressed |= m.decoder.stats().written != before;
    if !status.done {
        return Ok(());
    }
    if status.crc_ok == Some(false) || status.auth_ok == Some(false) {
        return Err(format!("Unrar: {} - CRC failed", m.name));
    }
    let io = |e: std::io::Error| format!("cannot write {}: {e}", m.target.display());
    m.out.file.sync_all().map_err(io)?;
    apply_meta(&m.out.file, m.unix_mode, m.mtime);
    std::fs::rename(&m.tmp, &m.target).map_err(io)?;
    m.done = true;
    *progressed = true;
    Ok(())
}

fn apply_meta(file: &File, unix_mode: Option<u32>, mtime: Option<(i64, u32)>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = match unix_mode {
            // Owner keeps read/write; set-id and sticky bits are never restored.
            Some(m) => (m & 0o777) | 0o600,
            None => 0o666 & !crate::lifecycle::active_umask() & 0o777,
        };
        let _ = file.set_permissions(std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = unix_mode;
    if let Some(at) = mtime.and_then(system_time) {
        let _ = file.set_modified(at);
    }
}

fn system_time((secs, nanos): (i64, u32)) -> Option<std::time::SystemTime> {
    use std::time::UNIX_EPOCH;
    if secs >= 0 {
        UNIX_EPOCH.checked_add(Duration::new(secs as u64, nanos.min(999_999_999)))
    } else {
        UNIX_EPOCH.checked_sub(Duration::from_secs(secs.unsigned_abs()))
    }
}

fn set_directory_times(mut dirs: Vec<(PathBuf, Option<(i64, u32)>)>) {
    dirs.sort_by_key(|(d, _)| std::cmp::Reverse(d.components().count()));
    for (dir, mtime) in dirs {
        if let (Some(at), Ok(f)) = (mtime.and_then(system_time), File::open(&dir)) {
            let _ = f.set_modified(at);
        }
    }
}

// ---- Split files ----

/// Places each piece of a split file at its offset in the joined output as the piece completes.
/// Every piece but the last has the same length (the first complete one's).
fn join_live(
    req: &Request<'_>,
    feed: &mut dyn VolumeFeed,
    prepare: &mut Prepare<'_>,
    log: &mut dyn FnMut(i32, &str),
    is_cancelled: &mut dyn FnMut() -> bool,
    on_wait: &dyn Fn(bool),
) -> Outcome {
    let first = &req.set.volumes[0];
    let joined = &first[..first.len().saturating_sub(4)];
    let name = String::from_utf8_lossy(joined).into_owned();
    let Ok(rel) = zipfast::path::sanitize(&name, false) else {
        return Outcome::Declined(format!("{name:?} is not a safe path"));
    };
    let Some((dest, guard)) = prepare() else {
        return Outcome::Declined("no unpack folder".into());
    };
    let _scope = guard.as_ref().map(crate::unpacklimits::Scope::enter);
    let target = dest.join(&rel);
    if let Err(why) = crate::unpacklimits::check(&target, false) {
        log(1, &why);
        return Outcome::Failed(why);
    }
    if let Err(e) = std::fs::create_dir_all(&dest) {
        return Outcome::Failed(format!("cannot create {}: {e}", dest.display()));
    }
    let (tmp, out) = match crate::progressive_unpack::create_unique_sibling(&target) {
        Ok(created) => created,
        Err(e) => return Outcome::Failed(format!("cannot write {}: {e}", target.display())),
    };
    let fail = |why: String| {
        let _ = std::fs::remove_file(&tmp);
        Outcome::Failed(why)
    };
    log(
        4,
        &format!("Unrar: Extracting from {}", String::from_utf8_lossy(first)),
    );
    log(4, &format!("Unrar: Extracting  {name}"));
    let mut vols = Volumes::new(req.dir, req.set, feed);
    let mut waiting = Waiting {
        on_wait,
        waiting: false,
    };
    let n = req.set.volumes.len();
    let mut placed = vec![false; n];
    let mut piece_len: Option<u64> = None;
    let mut last_len = 0u64;
    let (mut total, mut after_download) = (0u64, 0u64);
    loop {
        if is_cancelled() {
            return fail("Aborted".into());
        }
        let mut progressed = false;
        for k in 0..n {
            if placed[k] {
                continue;
            }
            let len = match vols.view(k) {
                // A damaged piece waits for its repair.
                View::Ready { len, holes } if holes.is_empty() => len,
                View::Ready { .. } | View::NotYet => continue,
                View::Missing => {
                    let why = format!(
                        "Unrar: ERROR: piece {} of {name} is missing",
                        String::from_utf8_lossy(&req.set.volumes[k])
                    );
                    log(1, &why);
                    return fail(why);
                }
            };
            let is_last = k + 1 == n;
            let size = match piece_len {
                Some(l) => l,
                None if !is_last => {
                    piece_len = Some(len);
                    len
                }
                // The last piece alone says nothing about the others' length.
                None => continue,
            };
            if (!is_last && len != size) || (is_last && (len == 0 || len > size)) {
                let why = format!(
                    "Unrar: ERROR: piece {} of {name} has {len} bytes, the others {size}",
                    String::from_utf8_lossy(&req.set.volumes[k])
                );
                log(1, &why);
                return fail(why);
            }
            let Some(src) = vols.files[k].as_ref() else {
                continue;
            };
            let finished = vols.feed.download_finished();
            if let Err(e) = copy_range(src, len, &out, k as u64 * size) {
                return fail(format!("cannot write {}: {e}", target.display()));
            }
            placed[k] = true;
            progressed = true;
            total += len;
            if finished {
                after_download += len;
            }
            if is_last {
                last_len = len;
            }
        }
        if placed.iter().all(|&p| p) {
            break;
        }
        if progressed {
            waiting.busy();
        } else if vols.feed.set_became_final() {
            let why = format!("Unrar: ERROR: {name} could not be joined from its pieces");
            log(1, &why);
            return fail(why);
        } else {
            waiting.idle();
        }
    }
    let size = piece_len.unwrap_or(0) * (n as u64 - 1) + last_len;
    let finish = || -> std::io::Result<()> {
        out.set_len(size)?;
        out.sync_all()?;
        apply_meta(&out, None, None);
        std::fs::rename(&tmp, &target)
    };
    if let Err(e) = finish() {
        return fail(format!("cannot write {}: {e}", target.display()));
    }
    log(
        0,
        &format!(
            "Joined {name} from {n} pieces as they arrived ({total} bytes); \
             {after_download} bytes placed after the download ended, nothing joined afterwards"
        ),
    );
    Outcome::Done {
        files: 1,
        dirs: 0,
        bytes: size,
    }
}

/// Copies `len` bytes from the start of `src` to `dst` at `at`. On Linux the kernel copies
/// (`copy_file_range`: no data through user space, a reflink where the file system shares
/// extents); elsewhere through a buffer.
fn copy_range(src: &File, len: u64, dst: &File, at: u64) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let (mut off_in, mut off_out) = (0i64, at as i64);
        let mut left = len;
        while left > 0 {
            let chunk = left.min(1 << 30) as usize;
            // SAFETY: both descriptors are open files owned by the caller for the call; the
            // offsets are local variables the kernel updates.
            let n = unsafe {
                libc::copy_file_range(
                    src.as_raw_fd(),
                    &mut off_in,
                    dst.as_raw_fd(),
                    &mut off_out,
                    chunk,
                    0,
                )
            };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                // Not supported across these files: copy the rest through a buffer.
                if matches!(
                    e.raw_os_error(),
                    Some(libc::EXDEV | libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP)
                ) {
                    return copy_buffered(src, off_in as u64, left, dst, off_out as u64);
                }
                return Err(e);
            }
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            left -= n as u64;
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    copy_buffered(src, 0, len, dst, at)
}

fn copy_buffered(src: &File, from: u64, len: u64, dst: &File, at: u64) -> std::io::Result<()> {
    let mut buf = vec![0u8; (1usize << 20).min(len as usize).max(1)];
    let mut done = 0u64;
    while done < len {
        let want = (len - done).min(buf.len() as u64) as usize;
        let n = read_at(src, &mut buf[..want], from + done)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        write_at(dst, &buf[..n], at + done)?;
        done += n as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
