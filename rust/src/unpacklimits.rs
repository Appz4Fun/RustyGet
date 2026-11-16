//! Limits on what an unpack may create, checked before each file or folder is created.
//!
//! The extractors create every file and folder through a few helpers, which call [`check`]
//! with the path they are about to create. While a [`Scope`] for a job's [`Guard`] is active,
//! [`check`] refuses:
//! - a path with `..` or one that is not under the job's unpack folder;
//! - more than `UnpackMaxFiles` files, `UnpackMaxFolders` folders or `UnpackMaxDepth` levels
//!   of nesting, counted over the whole job (every archive, every pass);
//! - names and paths over the filesystem limits of the folder the job unpacks to or the one
//!   it ends up in.
//!
//! The first breach stops the unpack, which fails with a message that starts with
//! [`LIMIT_PREFIX`] and names the limit and the path. Once the unpack is done,
//! [`Guard::asciify`] renames every file and folder it created with non-ASCII characters to an
//! ASCII name that keeps upper and lower case.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The start of every message for a breached limit; post-processing fails such a job without
/// a par repair.
pub const LIMIT_PREFIX: &str = "Unpack limit: ";

pub const DEFAULT_MAX_FILES: u32 = 100;
pub const DEFAULT_MAX_DEPTH: u32 = 2;
pub const DEFAULT_MAX_FOLDERS: u32 = 10;

/// Archive volumes waiting to be unpacked further may number this many times UnpackMaxFiles.
pub const VOLUME_FACTOR: u64 = 100;

/// The configured limits; 0 means no limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_files: u32,
    pub max_depth: u32,
    pub max_folders: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files: DEFAULT_MAX_FILES,
            max_depth: DEFAULT_MAX_DEPTH,
            max_folders: DEFAULT_MAX_FOLDERS,
        }
    }
}

impl Limits {
    /// The limits from the active options, or the defaults without any.
    pub fn from_options() -> Self {
        crate::options::active_options().map_or_else(Self::default, |o| {
            let s = o.scalars();
            let n = |v: i32| u32::try_from(v).unwrap_or(0);
            Self {
                max_files: n(s.unpack_max_files),
                max_depth: n(s.unpack_max_depth),
                max_folders: n(s.unpack_max_folders),
            }
        })
    }
}

/// A folder members land under, with its filesystem's name and path limits.
#[derive(Clone, Debug)]
struct Root {
    dir: PathBuf,
    name_max: u64,
    path_max: u64,
}

impl Root {
    fn new(dir: &Path) -> Self {
        let (name_max, path_max) = fs_limits(dir);
        Self {
            dir: dir.to_path_buf(),
            name_max,
            path_max,
        }
    }
}

/// `NAME_MAX` and `PATH_MAX` of the filesystem holding `dir` (or its nearest existing
/// ancestor), falling back to 255 and 4096.
pub fn fs_limits(dir: &Path) -> (u64, u64) {
    let mut probe = Some(dir);
    while let Some(p) = probe {
        if p.exists() {
            return (
                pathconf(p, PathConf::NameMax).unwrap_or(255),
                pathconf(p, PathConf::PathMax).unwrap_or(4096),
            );
        }
        probe = p.parent();
    }
    (255, 4096)
}

enum PathConf {
    NameMax,
    PathMax,
}

#[cfg(unix)]
fn pathconf(path: &Path, what: PathConf) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let name = match what {
        PathConf::NameMax => libc::_PC_NAME_MAX,
        PathConf::PathMax => libc::_PC_PATH_MAX,
    };
    // SAFETY: `c` is a valid NUL-terminated path for the duration of the call.
    let v = unsafe { libc::pathconf(c.as_ptr(), name) };
    u64::try_from(v).ok().filter(|&v| v > 0)
}

#[cfg(not(unix))]
fn pathconf(_path: &Path, what: PathConf) -> Option<u64> {
    Some(match what {
        PathConf::NameMax => 255,
        PathConf::PathMax => 260,
    })
}

/// A guard shared by the extractors of one job.
pub type SharedGuard = Arc<Mutex<Guard>>;

/// Tracks what one job's unpack has created so far.
#[derive(Debug)]
pub struct Guard {
    limits: Limits,
    /// The folder the job unpacks to, then the one it ends up in when that differs.
    roots: Vec<Root>,
    /// Files admitted, relative to the unpack folder: a member written again (an extraction
    /// retried, a direct unpack redone after the download) counts once.
    files: BTreeSet<PathBuf>,
    folders: BTreeSet<PathBuf>,
    /// ASCII names given by [`Guard::asciify`] so far.
    produced: BTreeSet<PathBuf>,
    /// Files named like archive volumes, not counted yet: a nested archive unpacked further
    /// consumes them ([`Guard::consumed`]); [`Guard::settle`] counts the rest.
    deferred: BTreeSet<PathBuf>,
    /// Volumes of nested archives that were unpacked further: never counted, but still made
    /// ASCII if they stay on disk.
    intermediate: BTreeSet<PathBuf>,
    breach: Option<String>,
}

impl Guard {
    /// `unpack_dir`: where members are written; `moved_to`: the folders the job's files may
    /// be moved to after unpacking, keeping their paths relative to `unpack_dir`.
    pub fn new(limits: Limits, unpack_dir: &Path, moved_to: &[&Path]) -> Self {
        let mut roots = vec![Root::new(unpack_dir)];
        for f in moved_to {
            if !f.as_os_str().is_empty() && !roots.iter().any(|r| r.dir == *f) {
                roots.push(Root::new(f));
            }
        }
        Self {
            limits,
            roots,
            files: BTreeSet::new(),
            folders: BTreeSet::new(),
            produced: BTreeSet::new(),
            deferred: BTreeSet::new(),
            intermediate: BTreeSet::new(),
            breach: None,
        }
    }

    /// Counts what an earlier unpack of the same job left in the unpack folder (after a
    /// restart, when its guard is gone). Nothing is refused here: the counts make the next
    /// file or folder over a limit fail. RustyGet's own working files (`.rustyget-*`) are
    /// skipped.
    pub fn seed_existing(&mut self) {
        let base = self.roots[0].dir.clone();
        let mut stack = vec![PathBuf::new()];
        while let Some(rel_dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(base.join(&rel_dir)) else {
                continue;
            };
            for e in rd.flatten() {
                let n = e.file_name();
                let n = n.to_string_lossy();
                if n.starts_with(".rustyget-") || n.starts_with(".zipfast-") {
                    continue;
                }
                let rel = rel_dir.join(e.file_name());
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    self.folders.insert(rel.clone());
                    stack.push(rel);
                } else {
                    self.files.insert(rel);
                }
            }
        }
    }

    /// A guard for one job with the configured limits.
    pub fn shared(unpack_dir: &Path, moved_to: &[&Path]) -> SharedGuard {
        Arc::new(Mutex::new(Self::new(
            Limits::from_options(),
            unpack_dir,
            moved_to,
        )))
    }

    /// The folder members are written to.
    pub fn unpack_dir(&self) -> &Path {
        &self.roots[0].dir
    }

    /// The first breached limit, as the message the unpack fails with.
    pub fn breach(&self) -> Option<&str> {
        self.breach.as_deref()
    }

    /// Admits `path`, a file or folder about to be created. Once a limit is breached every
    /// later path is refused with the same message.
    pub fn admit(&mut self, path: &Path, is_dir: bool) -> Result<(), String> {
        self.admit_inner(path, is_dir, true)
    }

    /// Checks `path` like [`Guard::admit`] without counting it: for a name looked up before
    /// the one finally created is known.
    pub fn probe(&mut self, path: &Path, is_dir: bool) -> Result<(), String> {
        self.admit_inner(path, is_dir, false)
    }

    fn admit_inner(&mut self, path: &Path, is_dir: bool, record: bool) -> Result<(), String> {
        if let Some(b) = &self.breach {
            return Err(b.clone());
        }
        self.check(path, is_dir, record).map_err(|why| {
            let msg = format!("{LIMIT_PREFIX}{why}");
            self.breach = Some(msg.clone());
            msg
        })
    }

    fn check(&mut self, path: &Path, is_dir: bool, record: bool) -> Result<(), String> {
        let shown = path.display().to_string();
        let Ok(rel) = path.strip_prefix(&self.roots[0].dir) else {
            return Err(format!(
                "\"{shown}\" is outside the unpack folder {}",
                self.roots[0].dir.display()
            ));
        };
        let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
        for c in rel.components() {
            match c {
                Component::Normal(p) => parts.push(p),
                Component::CurDir => {}
                _ => return Err(format!("\"{shown}\" leaves the unpack folder")),
            }
        }
        if parts.is_empty() {
            return Ok(());
        }
        let rel: PathBuf = parts.iter().collect();
        let folders_in = if is_dir { parts.len() } else { parts.len() - 1 };
        if self.limits.max_depth > 0 && folders_in as u64 > u64::from(self.limits.max_depth) {
            return Err(format!(
                "\"{}\" is {folders_in} folders deep, more than {} (UnpackMaxDepth)",
                rel.display(),
                self.limits.max_depth
            ));
        }
        for root in &self.roots {
            if let Some(long) = parts.iter().find(|p| p.len() as u64 > root.name_max) {
                return Err(format!(
                    "name \"{}\" is {} bytes, more than the {} bytes {} allows",
                    long.to_string_lossy(),
                    long.len(),
                    root.name_max,
                    root.dir.display()
                ));
            }
            // PATH_MAX counts the terminating NUL.
            let len = root.dir.as_os_str().len() as u64 + 1 + rel.as_os_str().len() as u64;
            if len >= root.path_max {
                return Err(format!(
                    "path to \"{}\" is {len} bytes in {}, more than its limit of {}",
                    rel.display(),
                    root.dir.display(),
                    root.path_max - 1
                ));
            }
        }
        let new_folders: Vec<PathBuf> = (1..=folders_in)
            .map(|n| parts[..n].iter().collect::<PathBuf>())
            .filter(|f| !self.folders.contains(f))
            .collect();
        if self.limits.max_folders > 0
            && (self.folders.len() + new_folders.len()) as u64 > u64::from(self.limits.max_folders)
        {
            return Err(format!(
                "more than {} folders (UnpackMaxFolders), at \"{}\"",
                self.limits.max_folders,
                rel.display()
            ));
        }
        // An archive volume is counted once it is clear it stays (see `settle`): the volumes
        // of an archive inside an archive are unpacked further and are not output.
        let volume = !is_dir
            && !self.files.contains(&rel)
            && crate::unpack::may_be_archive_volume(crate::fsops::path_to_bytes(Path::new(
                parts[parts.len() - 1],
            )));
        if volume {
            // Still bounded: a flood of "volumes" is refused like any other.
            let cap = u64::from(self.limits.max_files).saturating_mul(VOLUME_FACTOR);
            if self.limits.max_files > 0
                && !self.deferred.contains(&rel)
                && self.deferred.len() as u64 >= cap
            {
                return Err(format!(
                    "more than {cap} archive volumes ({VOLUME_FACTOR} x UnpackMaxFiles), at \"{}\"",
                    rel.display()
                ));
            }
        } else if !is_dir
            && !self.files.contains(&rel)
            && self.limits.max_files > 0
            && self.files.len() as u64 >= u64::from(self.limits.max_files)
        {
            return Err(format!(
                "more than {} files (UnpackMaxFiles), at \"{}\"",
                self.limits.max_files,
                rel.display()
            ));
        }
        if record {
            self.folders.extend(new_folders);
            if volume {
                self.intermediate.remove(&rel);
                self.deferred.insert(rel);
            } else if !is_dir {
                self.files.insert(rel);
            }
        }
        Ok(())
    }

    /// The volumes of a nested archive that was unpacked further: not output, never counted.
    pub fn consumed(&mut self, volumes: &[PathBuf]) {
        for v in volumes {
            if let Ok(rel) = v.strip_prefix(&self.roots[0].dir) {
                if self.deferred.remove(rel) {
                    self.intermediate.insert(rel.to_path_buf());
                }
            }
        }
    }

    /// Counts the archive volumes no nested unpack consumed: they are output. Called when an
    /// archive and everything nested in it is unpacked.
    pub fn settle(&mut self) -> Result<(), String> {
        if let Some(b) = &self.breach {
            return Err(b.clone());
        }
        let base = self.roots[0].dir.clone();
        for rel in std::mem::take(&mut self.deferred) {
            if std::fs::symlink_metadata(base.join(&rel)).is_err() {
                continue;
            }
            if self.limits.max_files > 0
                && !self.files.contains(&rel)
                && self.files.len() as u64 >= u64::from(self.limits.max_files)
            {
                let msg = format!(
                    "{LIMIT_PREFIX}more than {} files (UnpackMaxFiles), at \"{}\"",
                    self.limits.max_files,
                    rel.display()
                );
                self.breach = Some(msg.clone());
                return Err(msg);
            }
            self.files.insert(rel);
        }
        Ok(())
    }

    /// Renames every admitted file and folder that exists and whose name is not ASCII to its
    /// [`ascii_name`], deepest first. Run once, when the job's unpack is done.
    ///
    /// When the ASCII name exists already, the renamed entry gets `_2`, `_3`, ... before its
    /// extension, unless what is there is an ASCII name this guard gave in an earlier run (an
    /// older copy of the same member, unpacked again): a file replaces it and a folder is
    /// merged into it. Returns the renamed paths as (old, new).
    pub fn asciify(&mut self) -> std::io::Result<Vec<(PathBuf, PathBuf)>> {
        let base = self.roots[0].dir.clone();
        let mut entries: Vec<PathBuf> = self
            .files
            .iter()
            .chain(self.folders.iter())
            .chain(self.intermediate.iter())
            .chain(self.deferred.iter())
            .cloned()
            .collect();
        // Deepest first: a folder is renamed after everything in it.
        entries.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        let mut names = Names {
            base: &base,
            admitted: &self.files,
            earlier: std::mem::take(&mut self.produced),
            now: BTreeSet::new(),
        };
        let mut renamed = Vec::new();
        let mut result = Ok(());
        for rel in entries {
            let path = base.join(&rel);
            let Some(name) = path.file_name() else {
                continue;
            };
            if is_ascii(name) || std::fs::symlink_metadata(&path).is_err() {
                continue;
            }
            let dir = path.parent().unwrap_or(&base).to_path_buf();
            match names.place(&path, &dir, &ascii_name(&name.to_string_lossy())) {
                Ok(target) => renamed.push((path, target)),
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        let Names { earlier, now, .. } = names;
        self.produced = earlier;
        self.produced.extend(now);
        result.map(|()| renamed)
    }
}

/// The ASCII names given while renaming, relative to the unpack folder.
struct Names<'a> {
    base: &'a Path,
    /// Files the job wrote: never an older copy. (A folder is merged into, which keeps
    /// everything in it; its files are checked one by one.)
    admitted: &'a BTreeSet<PathBuf>,
    /// Given by earlier runs of the same guard.
    earlier: BTreeSet<PathBuf>,
    /// Given by this run.
    now: BTreeSet<PathBuf>,
}

impl Names<'_> {
    fn rel(&self, p: &Path) -> PathBuf {
        p.strip_prefix(self.base).unwrap_or(p).to_path_buf()
    }

    /// An older copy: inside an ASCII name an earlier run gave, and not given by this run.
    fn stale(&self, p: &Path) -> bool {
        let rel = self.rel(p);
        !self.now.contains(&rel)
            && !self.admitted.contains(&rel)
            && rel.ancestors().any(|a| self.earlier.contains(a))
    }

    /// Moves `src` to `dir/name` as [`Guard::asciify`] describes; returns where it went.
    fn place(&mut self, src: &Path, dir: &Path, name: &str) -> std::io::Result<PathBuf> {
        let first = dir.join(name);
        let target = match std::fs::symlink_metadata(&first) {
            Err(_) => {
                std::fs::rename(src, &first)?;
                first
            }
            Ok(meta) if self.stale(&first) && !meta.file_type().is_symlink() => {
                let src_is_dir = std::fs::symlink_metadata(src)?.is_dir();
                if src_is_dir && meta.is_dir() {
                    for child in std::fs::read_dir(src)? {
                        let child = child?.path();
                        if let Some(n) = child.file_name() {
                            let n = ascii_name(&n.to_string_lossy());
                            self.place(&child, &first, &n)?;
                        }
                    }
                    std::fs::remove_dir(src)?;
                    first
                } else if !src_is_dir && !meta.is_dir() {
                    std::fs::rename(src, &first)?;
                    first
                } else {
                    let t = free_name(dir, name);
                    std::fs::rename(src, &t)?;
                    t
                }
            }
            Ok(_) => {
                let t = free_name(dir, name);
                std::fs::rename(src, &t)?;
                t
            }
        };
        let rel = self.rel(&target);
        self.now.insert(rel);
        Ok(target)
    }
}

fn is_ascii(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|n| n.bytes().all(|b| (0x20..0x7f).contains(&b)))
}

static ACTIVE: Mutex<Vec<(u64, SharedGuard)>> = Mutex::new(Vec::new());
static NEXT_SCOPE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// While alive, [`check`] applies its guard to every path under the guard's unpack folder,
/// on any thread. Scopes nest; the guard with the longest unpack folder holding a path
/// applies.
#[must_use]
pub struct Scope(u64);

impl Scope {
    pub fn enter(guard: &SharedGuard) -> Self {
        let id = NEXT_SCOPE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        lock(&ACTIVE).push((id, Arc::clone(guard)));
        Self(id)
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        lock(&ACTIVE).retain(|(id, _)| *id != self.0);
    }
}

fn guard_for(path: &Path) -> Option<SharedGuard> {
    let active = lock(&ACTIVE);
    let mut best: Option<(usize, &SharedGuard)> = None;
    for (_, g) in active.iter().rev() {
        let depth = {
            let g = lock(g);
            let dir = g.unpack_dir();
            if !path.starts_with(dir) {
                continue;
            }
            dir.components().count()
        };
        if best.is_none_or(|(d, _)| depth > d) {
            best = Some((depth, g));
        }
    }
    best.map(|(_, g)| Arc::clone(g))
}

/// Called by the extractors with each file (`is_dir` false) or folder they are about to
/// create. A path no active [`Scope`] covers passes.
pub fn check(path: &Path, is_dir: bool) -> Result<(), String> {
    match guard_for(path) {
        Some(g) => lock(&g).admit(path, is_dir),
        None => Ok(()),
    }
}

/// [`check`] without counting the path: for a name looked up before the one created is
/// known.
pub fn probe(path: &Path, is_dir: bool) -> Result<(), String> {
    match guard_for(path) {
        Some(g) => lock(&g).probe(path, is_dir),
        None => Ok(()),
    }
}

/// An ASCII version of one path component: accented Latin letters lose their accent and keep
/// their case, other non-ASCII characters and control characters become `_`. ASCII names are
/// returned unchanged.
pub fn ascii_name(name: &str) -> String {
    if name.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return name.to_string();
    }
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            ' '..='~' => out.push(c),
            _ => out.push_str(fold(c)),
        }
    }
    out
}

/// The ASCII stand-in for a non-ASCII or control character.
fn fold(c: char) -> &'static str {
    match c {
        'À'..='Å' | 'Ā' | 'Ă' | 'Ą' => "A",
        'à'..='å' | 'ā' | 'ă' | 'ą' | 'ª' => "a",
        'Æ' => "AE",
        'æ' => "ae",
        'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => "C",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'Ð' | 'Ď' | 'Đ' => "D",
        'ð' | 'ď' | 'đ' => "d",
        'È'..='Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => "E",
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'Ĝ' | 'Ğ' | 'Ġ' | 'Ģ' => "G",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'Ĥ' | 'Ħ' => "H",
        'ĥ' | 'ħ' => "h",
        'Ì'..='Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'İ' => "I",
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'Ĳ' => "IJ",
        'ĳ' => "ij",
        'Ĵ' => "J",
        'ĵ' => "j",
        'Ķ' => "K",
        'ķ' | 'ĸ' => "k",
        'Ĺ' | 'Ļ' | 'Ľ' | 'Ŀ' | 'Ł' => "L",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'Ñ' | 'Ń' | 'Ņ' | 'Ň' | 'Ŋ' => "N",
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ŉ' | 'ŋ' => "n",
        'Ò'..='Ö' | 'Ø' | 'Ō' | 'Ŏ' | 'Ő' => "O",
        'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'º' => "o",
        'Œ' => "OE",
        'œ' => "oe",
        'Ŕ' | 'Ŗ' | 'Ř' => "R",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'Ś' | 'Ŝ' | 'Ş' | 'Š' => "S",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ſ' => "s",
        'ß' => "ss",
        'Ţ' | 'Ť' | 'Ŧ' => "T",
        'ţ' | 'ť' | 'ŧ' => "t",
        'Þ' => "TH",
        'þ' => "th",
        'Ù'..='Ü' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' => "U",
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'Ŵ' => "W",
        'ŵ' => "w",
        'Ý' | 'Ŷ' | 'Ÿ' => "Y",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'Ź' | 'Ż' | 'Ž' => "Z",
        'ź' | 'ż' | 'ž' => "z",
        '‘' | '’' | '´' | '`' => "'",
        '“' | '”' => "\"",
        '–' | '—' | '‐' | '‑' => "-",
        '…' => "...",
        _ => "_",
    }
}

fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if std::fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2u32..)
        .map(|n| dir.join(format!("{stem}_{n}{ext}")))
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .expect("a free name")
}

/// Renames `path` to its [`ascii_name`] when its name is not ASCII, with `_2`, `_3`, ... when
/// that name is taken; returns the new path. For names given to unpacked files after the
/// unpack (a par rename in the final folder).
pub fn ascii_rename(path: &Path) -> std::io::Result<Option<PathBuf>> {
    let (Some(name), Some(dir)) = (path.file_name(), path.parent()) else {
        return Ok(None);
    };
    if is_ascii(name) || std::fs::symlink_metadata(path).is_err() {
        return Ok(None);
    }
    let target = free_name(dir, &ascii_name(&name.to_string_lossy()));
    std::fs::rename(path, &target)?;
    Ok(Some(target))
}

/// Handed from a finished direct unpack to the job's post-processing, by unpack folder, with
/// when.
static HANDED_OVER: Mutex<Vec<(std::time::Instant, SharedGuard)>> = Mutex::new(Vec::new());

/// A hand-over older than this belongs to a job that was never post-processed.
const HAND_OVER_TTL: std::time::Duration = std::time::Duration::from_secs(12 * 3600);

/// Keeps a finished direct unpack's guard for the job's post-processing, so the limits count
/// both.
pub fn hand_over(guard: &SharedGuard) {
    let dir = lock(guard).unpack_dir().to_path_buf();
    let mut h = lock(&HANDED_OVER);
    // Jobs deleted before post-processing never take theirs.
    h.retain(|(t, g)| t.elapsed() < HAND_OVER_TTL && lock(g).unpack_dir() != dir);
    h.push((std::time::Instant::now(), Arc::clone(guard)));
    if h.len() > 64 {
        h.remove(0);
    }
}

/// The guard a direct unpack into `unpack_dir` handed over, if any.
pub fn take_handed_over(unpack_dir: &Path) -> Option<SharedGuard> {
    let mut h = lock(&HANDED_OVER);
    let i = h
        .iter()
        .position(|(_, g)| lock(g).unpack_dir() == unpack_dir)?;
    let (t, g) = h.remove(i);
    (t.elapsed() < HAND_OVER_TTL).then_some(g)
}

/// [`Guard::consumed`] on a shared guard.
pub fn consumed(guard: &SharedGuard, volumes: &[PathBuf]) {
    lock(guard).consumed(volumes);
}

/// [`Guard::settle`] on a shared guard.
pub fn settle(guard: &SharedGuard) -> Result<(), String> {
    lock(guard).settle()
}

/// The breach `guard` recorded, if any.
pub fn breach_of(guard: &SharedGuard) -> Option<String> {
    lock(guard).breach().map(str::to_string)
}

/// Makes the names a job unpacked ASCII ([`Guard::asciify`]), logging each rename. Called
/// once, when every unpack pass of the job is done; fails with the logged reason.
pub fn finish_names(guard: &SharedGuard, log_cb: &mut dyn FnMut(i32, &str)) -> Result<(), String> {
    match lock(guard).asciify() {
        Ok(renamed) => {
            for (old, new) in renamed {
                log_cb(
                    0,
                    &format!(
                        "Renamed {} to {} (names are ASCII only)",
                        old.display(),
                        new.display()
                    ),
                );
            }
            Ok(())
        }
        Err(e) => {
            let msg = format!("Could not rename unpacked files to ASCII names: {e}");
            log_cb(1, &msg);
            Err(msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/tmp/rg-root")
    }

    fn guard(limits: Limits) -> Guard {
        Guard::new(limits, &root(), &[])
    }

    /// Admits files (`dir/` names are folders); returns the first refusal.
    fn admit_all(g: &mut Guard, names: &[&str]) -> Option<String> {
        for n in names {
            let (n, is_dir) = match n.strip_suffix('/') {
                Some(d) => (d, true),
                None => (*n, false),
            };
            if let Err(e) = g.admit(&root().join(n), is_dir) {
                return Some(e);
            }
        }
        None
    }

    #[test]
    fn ascii_names_keep_case_and_fold_accents() {
        assert_eq!(ascii_name("Movie.2020.mkv"), "Movie.2020.mkv");
        assert_eq!(ascii_name("Marnûkh"), "Marnukh");
        assert_eq!(ascii_name("ÉCOLE.Ølen.straße"), "ECOLE.Olen.strasse");
        assert_eq!(ascii_name("日本.mkv"), "__.mkv");
        assert_eq!(ascii_name("a\u{7}b"), "a_b");
    }

    #[test]
    fn a_normal_release_passes() {
        let mut g = guard(Limits::default());
        let r = admit_all(
            &mut g,
            &[
                "Release.mkv",
                "Sample/",
                "Sample/release-sample.mkv",
                "Subs/English.srt",
                "Proof/Covers/",
                "Proof/Covers/front.jpg",
            ],
        );
        assert_eq!(r, None);
        assert_eq!(g.files.len(), 4);
        assert_eq!(g.folders.len(), 4);
    }

    #[test]
    fn too_deep_is_refused_for_files_and_folders() {
        let mut g = guard(Limits::default());
        let r = admit_all(&mut g, &["a/b/c/file.bin"]).unwrap();
        assert!(r.starts_with(LIMIT_PREFIX), "{r}");
        assert!(
            r.contains("3 folders deep") && r.contains("UnpackMaxDepth"),
            "{r}"
        );

        let mut g = guard(Limits::default());
        let r = admit_all(&mut g, &["a/", "a/b/", "a/b/c/"]).unwrap();
        assert!(r.contains("\"a/b/c\" is 3 folders deep"), "{r}");
    }

    #[test]
    fn too_many_files_is_refused() {
        let mut g = guard(Limits::default());
        let names: Vec<String> = (0..101).map(|i| format!("f{i}.bin")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let r = admit_all(&mut g, &refs).unwrap();
        assert!(
            r.contains("more than 100 files") && r.contains("f100.bin"),
            "{r}"
        );
        assert_eq!(g.files.len(), 100);
    }

    #[test]
    fn empty_folders_count_toward_the_folder_limit() {
        let mut g = guard(Limits::default());
        let names: Vec<String> = (0..11).map(|i| format!("d{i}/")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let r = admit_all(&mut g, &refs).unwrap();
        assert!(
            r.contains("more than 10 folders") && r.contains("d10"),
            "{r}"
        );
        assert!(g.files.is_empty());
    }

    #[test]
    fn a_breach_refuses_everything_after_it() {
        let mut g = guard(Limits::default());
        assert!(admit_all(&mut g, &["a/b/c/x"]).is_some());
        let again = admit_all(&mut g, &["fine.mkv"]).unwrap();
        assert!(again.contains("UnpackMaxDepth"), "{again}");
    }

    #[test]
    fn zero_means_no_limit() {
        let mut g = guard(Limits {
            max_files: 0,
            max_depth: 0,
            max_folders: 0,
        });
        let names: Vec<String> = (0..300).map(|i| format!("a/b/c/d{i}/f.bin")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(admit_all(&mut g, &refs), None);
    }

    #[test]
    fn escapes_are_refused() {
        for bad in ["/var/config/x", "/tmp/rg-root/../x", "/tmp/rg-rootx/y"] {
            let mut g = guard(Limits::default());
            let r = g.admit(Path::new(bad), false).unwrap_err();
            assert!(
                r.contains("outside the unpack folder") || r.contains("leaves the unpack folder"),
                "{bad}: {r}"
            );
        }
    }

    #[test]
    fn raw_names_and_paths_over_the_filesystem_limits_are_refused() {
        let mut g = guard(Limits::default());
        let long = "x".repeat(300);
        let r = admit_all(&mut g, &[&long]).unwrap();
        assert!(r.contains("300 bytes"), "{r}");

        // 150 two-byte characters: 300 bytes on disk, though its ASCII name would fit.
        let mut g = guard(Limits::default());
        let r = admit_all(&mut g, &[&"é".repeat(150)]).unwrap();
        assert!(r.contains("300 bytes"), "{r}");

        let mut g = guard(Limits::default());
        g.roots[0].path_max = 64;
        let r = admit_all(&mut g, &[&format!("d/{}", "y".repeat(60))]).unwrap();
        assert!(r.contains("more than its limit of 63"), "{r}");
    }

    #[test]
    fn both_folders_are_checked() {
        let t = std::env::temp_dir().join(format!("rg-limits-{}", std::process::id()));
        let mut g = Guard::new(Limits::default(), &t, &[&t.join("a".repeat(200))]);
        assert_eq!(g.roots.len(), 2);
        g.roots[1].path_max = 300;
        let e = g.admit(&t.join("z".repeat(100)), false).unwrap_err();
        assert!(e.contains(&"a".repeat(200)), "{e}");
    }

    #[test]
    fn a_file_written_again_counts_once() {
        let mut g = guard(Limits {
            max_files: 2,
            ..Limits::default()
        });
        for _ in 0..3 {
            assert_eq!(admit_all(&mut g, &["a.bin", "b.bin"]), None);
        }
        let r = admit_all(&mut g, &["c.bin"]).unwrap();
        assert!(r.contains("more than 2 files"), "{r}");
    }

    #[test]
    fn check_applies_the_guard_whose_scope_covers_the_path() {
        let dir = std::env::temp_dir().join(format!("rg-scope-{}", std::process::id()));
        let g = Arc::new(Mutex::new(Guard::new(
            Limits {
                max_files: 1,
                ..Limits::default()
            },
            &dir,
            &[],
        )));
        // No scope: nothing is checked.
        assert!(check(&dir.join("a"), false).is_ok());
        assert!(check(&dir.join("b"), false).is_ok());
        {
            let _scope = Scope::enter(&g);
            assert!(check(&dir.join("a"), false).is_ok());
            // Another thread is covered too.
            let d = dir.clone();
            let e = std::thread::spawn(move || check(&d.join("b"), false))
                .join()
                .unwrap()
                .unwrap_err();
            assert!(e.contains("more than 1 files"), "{e}");
            // A path outside the job's folder is not this guard's.
            assert!(check(Path::new("/elsewhere/x"), false).is_ok());
        }
        assert!(check(&dir.join("c"), false).is_ok());
    }

    #[test]
    fn admitted_non_ascii_entries_are_renamed() {
        let t = std::env::temp_dir().join(format!("rg-asciify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("Sübs")).unwrap();
        std::fs::write(t.join("Sübs").join("Españ.srt"), b"x").unwrap();
        std::fs::write(t.join("Marnûkh.mkv"), b"m").unwrap();
        std::fs::write(t.join("Marnukh.mkv"), b"taken").unwrap();
        std::fs::write(t.join("Downloading_é.rar"), b"vol").unwrap();
        let mut g = Guard::new(Limits::default(), &t, &[]);
        for (p, d) in [
            ("Sübs", true),
            ("Sübs/Españ.srt", false),
            ("Marnûkh.mkv", false),
            ("Marnukh.mkv", false),
        ] {
            g.admit(&t.join(p), d).unwrap();
        }
        let renamed = g.asciify().unwrap();
        assert_eq!(renamed.len(), 3, "{renamed:?}");
        assert!(t.join("Downloading_é.rar").exists());
        assert_eq!(
            std::fs::read(t.join("Subs").join("Espan.srt")).unwrap(),
            b"x"
        );
        assert_eq!(std::fs::read(t.join("Marnukh.mkv")).unwrap(), b"taken");
        assert_eq!(std::fs::read(t.join("Marnukh_2.mkv")).unwrap(), b"m");
        let _ = std::fs::remove_dir_all(&t);
    }

    /// A ustar archive; a name ending in `/` is a folder entry, a name over 99 bytes gets a
    /// GNU long-name entry.
    fn tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, data) in entries {
            if name.len() > 99 {
                let long = [name.as_bytes(), b"\0"].concat();
                out.extend(header("././@LongLink", long.len(), b'L'));
                out.extend_from_slice(&long);
                out.resize(out.len().div_ceil(512) * 512, 0);
            }
            let dir = name.ends_with('/');
            let short = &name[..name.len().min(99)];
            out.extend(header(short, data.len(), if dir { b'5' } else { b'0' }));
            out.extend_from_slice(data);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.resize(out.len() + 1024, 0);
        out
    }

    fn header(name: &str, size: usize, kind: u8) -> [u8; 512] {
        {
            let mut h = [0u8; 512];
            h[..name.len()].copy_from_slice(name.as_bytes());
            let dir = kind == b'5';
            h[100..108].copy_from_slice(if dir { b"0000755\0" } else { b"0000644\0" });
            h[108..116].copy_from_slice(b"0000000\0");
            h[116..124].copy_from_slice(b"0000000\0");
            h[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
            h[136..148].copy_from_slice(b"14000000000\0");
            h[156] = kind;
            h[257..263].copy_from_slice(b"ustar\0");
            h[263..265].copy_from_slice(b"00");
            h[148..156].copy_from_slice(b"        ");
            let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
            h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
            h
        }
    }

    fn files_under(dir: &Path) -> usize {
        std::fs::read_dir(dir).map_or(0, |rd| {
            rd.flatten()
                .map(|e| {
                    if e.path().is_dir() {
                        files_under(&e.path())
                    } else {
                        1
                    }
                })
                .sum()
        })
    }

    /// Unpacks tar archives into one folder with one job guard, archive by archive as
    /// post-processing does; returns each result, the output folder and the log.
    fn unpack_job(
        tag: &str,
        archives: &[&[(&str, &[u8])]],
    ) -> (Vec<Result<u64, String>>, PathBuf, Vec<String>) {
        let t = std::env::temp_dir().join(format!("rg-unpack-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let out = t.join("out");
        let guard = Guard::shared(&out, &[]);
        let mut log = Vec::new();
        let mut results = Vec::new();
        for (i, entries) in archives.iter().enumerate() {
            let archive = t.join(format!("set{i}.tar"));
            std::fs::write(&archive, tar(entries)).unwrap();
            let r = crate::unpack::extract_archive_with_feed(
                &archive,
                &out,
                "",
                1,
                false,
                |k, m| log.push(format!("{k} {m}")),
                &mut || false,
                crate::unpack::RarFeed {
                    guard: Some(&guard),
                    ..crate::unpack::RarFeed::default()
                },
            )
            .map(|s| s.files);
            results.push(r);
        }
        if results.iter().all(Result::is_ok) {
            let mut cb = |k: i32, m: &str| log.push(format!("{k} {m}"));
            if let Err(e) = finish_names(&guard, &mut cb) {
                results.push(Err(e));
            }
        }
        (results, out, log)
    }

    fn unpack(tag: &str, entries: &[(&str, &[u8])]) -> (Result<u64, String>, PathBuf, Vec<String>) {
        let (mut r, out, log) = unpack_job(tag, &[entries]);
        (r.remove(0), out, log)
    }

    #[test]
    fn an_unpack_with_too_many_files_stops_before_the_next_file() {
        let names: Vec<String> = (0..150).map(|i| format!("f{i:03}.bin")).collect();
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
        let (r, out, log) = unpack("files", &entries);
        let err = r.unwrap_err();
        assert!(
            err.starts_with(LIMIT_PREFIX) && err.contains("more than 100 files"),
            "{err}"
        );
        assert!(
            log.iter().any(|l| l.starts_with("1 Unpack limit: ")),
            "{log:?}"
        );
        assert!(!log.iter().any(|l| l.contains("All OK")), "{log:?}");
        assert_eq!(files_under(&out), 100);
    }

    #[test]
    fn folder_entries_count_toward_depth_and_folders() {
        let (r, out, _) = unpack("deepdirs", &[("a/", b""), ("a/b/", b""), ("a/b/c/", b"")]);
        let err = r.unwrap_err();
        assert!(err.contains("UnpackMaxDepth"), "{err}");
        assert!(!out.join("a/b/c").exists());

        let dirs: Vec<String> = (0..12).map(|i| format!("d{i:02}/")).collect();
        let entries: Vec<(&str, &[u8])> = dirs.iter().map(|n| (n.as_str(), &b""[..])).collect();
        let (r, _, _) = unpack("manydirs", &entries);
        assert!(r.unwrap_err().contains("UnpackMaxFolders"));
    }

    #[test]
    fn an_over_long_name_fails_with_the_reason() {
        let long = format!("{}.mkv", "n".repeat(296));
        let (r, out, log) = unpack("longname", &[(long.as_str(), b"x")]);
        let err = r.unwrap_err();
        assert!(
            err.starts_with(LIMIT_PREFIX) && err.contains("300 bytes"),
            "{err}"
        );
        assert!(
            log.iter().any(|l| l.starts_with("1 Unpack limit: ")),
            "{log:?}"
        );
        assert_eq!(files_under(&out), 0);
    }

    #[test]
    fn the_limits_count_over_all_archives_of_a_job() {
        let a: Vec<String> = (0..60).map(|i| format!("a{i:02}.bin")).collect();
        let b: Vec<String> = (0..60).map(|i| format!("b{i:02}.bin")).collect();
        let ea: Vec<(&str, &[u8])> = a.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
        let eb: Vec<(&str, &[u8])> = b.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
        let (r, _, _) = unpack_job("perjob", &[&ea, &eb]);
        assert_eq!(r[0], Ok(60));
        assert!(
            r[1].as_ref().unwrap_err().contains("more than 100 files"),
            "{r:?}"
        );
    }

    #[test]
    fn an_unpack_that_escapes_its_folder_fails() {
        let (r, out, _) = unpack("escape", &[("../escaped.bin", b"x")]);
        assert!(r.is_err());
        assert!(!out.parent().unwrap().join("escaped.bin").exists());
    }

    #[test]
    fn unpacked_names_are_made_ascii() {
        let (r, out, log) = unpack(
            "ascii",
            &[
                ("Marnûkh.mkv", b"m"),
                ("Sämple/", b""),
                ("Sämple/sämple.mkv", b"s"),
            ],
        );
        assert_eq!(r, Ok(2), "{log:?}");
        assert_eq!(std::fs::read(out.join("Marnukh.mkv")).unwrap(), b"m");
        assert_eq!(
            std::fs::read(out.join("Sample").join("sample.mkv")).unwrap(),
            b"s"
        );
        assert!(
            log.iter().any(|l| l.contains("names are ASCII only")),
            "{log:?}"
        );
    }

    #[test]
    fn without_a_guard_nothing_is_limited_or_renamed() {
        let names: Vec<String> = (0..120).map(|i| format!("f{i:03}.bin")).collect();
        let mut entries: Vec<(&str, &[u8])> =
            names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
        entries.push(("Marnûkh.mkv", b"m"));
        let t = std::env::temp_dir().join(format!("rg-unpack-noguard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let archive = t.join("set.tar");
        std::fs::write(&archive, tar(&entries)).unwrap();
        let out = t.join("out");
        let r = crate::unpack::extract_archive_with_zipfast(&archive, &out, "", 1, |_, _| {});
        assert_eq!(r.map(|s| s.files), Ok(121));
        assert!(out.join("Marnûkh.mkv").exists());
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn one_folder_written_by_two_archives_is_renamed_once() {
        let (r, out, log) = unpack_job(
            "twosets",
            &[&[("Café/a.txt", b"a")], &[("Café/b.txt", b"b")]],
        );
        assert!(r.iter().all(Result::is_ok), "{r:?} {log:?}");
        assert_eq!(names_in(&out), ["Cafe"]);
        assert_eq!(names_in(&out.join("Cafe")), ["a.txt", "b.txt"]);
    }

    #[test]
    fn a_file_written_twice_is_renamed_once() {
        let (r, out, log) = unpack_job("twice", &[&[("Café.txt", b"1")], &[("Café.txt", b"2")]]);
        assert!(r.iter().all(Result::is_ok), "{r:?} {log:?}");
        assert_eq!(names_in(&out), ["Cafe.txt"]);
        assert_eq!(std::fs::read(out.join("Cafe.txt")).unwrap(), b"2");
    }

    #[test]
    fn two_members_with_the_same_ascii_name_both_stay() {
        let (r, out, log) = unpack("clash", &[("Café.txt", b"accent"), ("Cafe.txt", b"plain")]);
        assert!(r.is_ok(), "{r:?} {log:?}");
        assert_eq!(names_in(&out), ["Cafe.txt", "Cafe_2.txt"]);
        assert_eq!(std::fs::read(out.join("Cafe.txt")).unwrap(), b"plain");
        assert_eq!(std::fs::read(out.join("Cafe_2.txt")).unwrap(), b"accent");
    }

    #[test]
    fn an_earlier_unpack_is_counted_without_refusing_anything() {
        let t = std::env::temp_dir().join(format!("rg-seed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("Sub")).unwrap();
        std::fs::write(t.join("Sub").join("a.txt"), b"a").unwrap();
        std::fs::write(t.join("Cafe.txt"), b"plain").unwrap();
        std::fs::write(t.join("Café.txt"), b"accent").unwrap();
        // A direct unpack's working state, deeper than the limit.
        std::fs::create_dir_all(t.join(".rustyget-partial/0011/out/x")).unwrap();
        std::fs::write(t.join(".rustyget-partial/0011/out/x/f"), b"p").unwrap();
        let mut g = Guard::new(
            Limits {
                max_files: 4,
                ..Limits::default()
            },
            &t,
            &[],
        );
        g.seed_existing();
        assert_eq!(g.breach(), None);
        assert_eq!(g.files.len(), 3);
        assert_eq!(g.folders.len(), 1);
        // Written again: counted once. One more file fits, the next does not.
        g.admit(&t.join("Sub").join("a.txt"), false).unwrap();
        g.admit(&t.join("new.txt"), false).unwrap();
        assert!(g.admit(&t.join("more.txt"), false).is_err());
        // Two members of the job whose ASCII names clash both stay.
        g.asciify().unwrap();
        assert_eq!(std::fs::read(t.join("Cafe.txt")).unwrap(), b"plain");
        assert_eq!(std::fs::read(t.join("Cafe_2.txt")).unwrap(), b"accent");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_direct_unpack_hands_its_guard_to_post_processing() {
        let dir = std::env::temp_dir().join(format!("rg-handover-{}", std::process::id()));
        let g = Guard::shared(&dir, &[]);
        lock(&g).admit(&dir.join("a.mkv"), false).unwrap();
        hand_over(&g);
        let got = take_handed_over(&dir).unwrap();
        assert!(Arc::ptr_eq(&g, &got));
        assert!(take_handed_over(&dir).is_none());
    }

    #[test]
    fn a_renamed_name_is_made_ascii() {
        let t = std::env::temp_dir().join(format!("rg-asciiren-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        std::fs::write(t.join("Amélie.mkv"), b"m").unwrap();
        std::fs::write(t.join("plain.mkv"), b"p").unwrap();
        assert_eq!(
            ascii_rename(&t.join("Amélie.mkv")).unwrap(),
            Some(t.join("Amelie.mkv"))
        );
        assert_eq!(ascii_rename(&t.join("plain.mkv")).unwrap(), None);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_probe_counts_nothing() {
        let mut g = guard(Limits {
            max_files: 1,
            ..Limits::default()
        });
        g.probe(&root().join("a"), false).unwrap();
        g.probe(&root().join("b"), false).unwrap();
        g.admit(&root().join("c"), false).unwrap();
        assert!(g.probe(&root().join("d"), false).is_err());
    }

    #[test]
    fn names_that_fold_alike_all_stay() {
        let (r, out, log) = unpack(
            "foldalike",
            &[
                ("東京.mkv", b"tokyo"),
                ("大阪.mkv", b"osaka"),
                ("日本/a.txt", b"jp"),
                ("中国/a.txt", b"cn"),
            ],
        );
        assert!(r.is_ok(), "{r:?} {log:?}");
        assert_eq!(names_in(&out), ["__", "__.mkv", "___2", "___2.mkv"]);
        let mut contents: Vec<Vec<u8>> = ["__.mkv", "___2.mkv"]
            .iter()
            .map(|n| std::fs::read(out.join(n)).unwrap())
            .collect();
        contents.sort();
        assert_eq!(contents, [b"osaka".to_vec(), b"tokyo".to_vec()]);
        let mut dirs: Vec<Vec<u8>> = ["__", "___2"]
            .iter()
            .map(|n| std::fs::read(out.join(n).join("a.txt")).unwrap())
            .collect();
        dirs.sort();
        assert_eq!(dirs, [b"cn".to_vec(), b"jp".to_vec()]);
    }

    #[test]
    fn a_file_the_job_did_not_unpack_is_never_replaced() {
        let t = std::env::temp_dir().join(format!("rg-keepdl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        // Downloaded next to where the archive unpacks.
        std::fs::write(t.join("Amelie.nfo"), b"downloaded").unwrap();
        std::fs::write(t.join("Amélie.nfo"), b"unpacked").unwrap();
        let mut g = Guard::new(Limits::default(), &t, &[]);
        g.admit(&t.join("Amélie.nfo"), false).unwrap();
        g.asciify().unwrap();
        assert_eq!(std::fs::read(t.join("Amelie.nfo")).unwrap(), b"downloaded");
        assert_eq!(std::fs::read(t.join("Amelie_2.nfo")).unwrap(), b"unpacked");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn an_unpack_redone_replaces_its_earlier_ascii_copies() {
        let t = std::env::temp_dir().join(format!("rg-redo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("Café")).unwrap();
        let mut g = Guard::new(Limits::default(), &t, &[]);
        let write = |g: &mut Guard, v: &[u8]| {
            std::fs::create_dir_all(t.join("Café")).unwrap();
            std::fs::write(t.join("Café.txt"), v).unwrap();
            std::fs::write(t.join("Café").join("a.txt"), v).unwrap();
            g.admit(&t.join("Café.txt"), false).unwrap();
            g.admit(&t.join("Café").join("a.txt"), false).unwrap();
        };
        write(&mut g, b"1");
        g.asciify().unwrap();
        // Direct unpack's output, unpacked again by post-processing with the same guard.
        write(&mut g, b"2");
        g.asciify().unwrap();
        assert_eq!(names_in(&t), ["Cafe", "Cafe.txt"]);
        assert_eq!(std::fs::read(t.join("Cafe.txt")).unwrap(), b"2");
        assert_eq!(names_in(&t.join("Cafe")), ["a.txt"]);
        assert_eq!(std::fs::read(t.join("Cafe").join("a.txt")).unwrap(), b"2");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_member_of_this_run_is_never_an_older_copy() {
        let t = std::env::temp_dir().join(format!("rg-crossset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let mut g = Guard::new(Limits::default(), &t, &[]);
        std::fs::write(t.join("Café.txt"), b"set1").unwrap();
        g.admit(&t.join("Café.txt"), false).unwrap();
        g.asciify().unwrap();
        // Unpacked again: set 1's member, and set 2's own Cafe.txt over the earlier copy.
        std::fs::write(t.join("Café.txt"), b"set1").unwrap();
        g.admit(&t.join("Café.txt"), false).unwrap();
        std::fs::write(t.join("Cafe.txt"), b"set2").unwrap();
        g.admit(&t.join("Cafe.txt"), false).unwrap();
        g.asciify().unwrap();
        assert_eq!(std::fs::read(t.join("Cafe.txt")).unwrap(), b"set2");
        assert_eq!(std::fs::read(t.join("Cafe_2.txt")).unwrap(), b"set1");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_redone_folder_merges_into_its_copy_another_archive_also_wrote_into() {
        let t = std::env::temp_dir().join(format!("rg-sharedredo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let mut g = Guard::new(Limits::default(), &t, &[]);
        let set_a = |g: &mut Guard, v: &[u8]| {
            std::fs::create_dir_all(t.join("Dossié")).unwrap();
            std::fs::write(t.join("Dossié").join("foo.txt"), v).unwrap();
            g.admit(&t.join("Dossié").join("foo.txt"), false).unwrap();
        };
        set_a(&mut g, b"a1");
        g.asciify().unwrap();
        set_a(&mut g, b"a2");
        std::fs::write(t.join("Dossie").join("other.txt"), b"b").unwrap();
        g.admit(&t.join("Dossie").join("other.txt"), false).unwrap();
        g.asciify().unwrap();
        assert_eq!(names_in(&t), ["Dossie"]);
        assert_eq!(names_in(&t.join("Dossie")), ["foo.txt", "other.txt"]);
        assert_eq!(
            std::fs::read(t.join("Dossie").join("foo.txt")).unwrap(),
            b"a2"
        );
        let _ = std::fs::remove_dir_all(&t);
    }

    /// A release whose outer archive holds a 150-volume RAR set: the inner volumes are
    /// unpacked further, so only the mkv inside counts.
    #[test]
    fn the_volumes_of_a_nested_archive_do_not_count() {
        let data: Vec<u8> = (0..150 * 512).map(|i| (i % 251) as u8).collect();
        let vols = crate::progressive_unpack::tests::rar4_store_volumes("movie.mkv", &data, 512);
        assert_eq!(vols.len(), 150);
        let names: Vec<String> = (1..=vols.len())
            .map(|i| format!("inner.part{i:03}.rar"))
            .collect();
        let mut entries: Vec<(&str, &[u8])> = names
            .iter()
            .zip(&vols)
            .map(|(n, v)| (n.as_str(), v.as_slice()))
            .collect();
        entries.push(("info.nfo", b"nfo"));
        let (r, out, log) = unpack("nestedvols", &entries);
        assert!(r.is_ok(), "{r:?} {log:?}");
        assert_eq!(
            std::fs::read(out.join("movie.mkv")).unwrap(),
            data,
            "{log:?}"
        );
    }

    #[test]
    fn archive_volumes_nothing_unpacks_further_count_at_the_end() {
        // 101 split pieces of a plain file: not an archive inside, so they are output.
        let names: Vec<String> = (1..=101).map(|i| format!("movie.mkv.{i:03}")).collect();
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
        let (r, _, log) = unpack("leftvols", &entries);
        let err = r.unwrap_err();
        assert!(err.contains("more than 100 files"), "{err} {log:?}");
        assert!(
            log.iter().any(|l| l.starts_with("1 Unpack limit: ")),
            "{log:?}"
        );
    }

    #[test]
    fn a_flood_of_volume_names_is_still_refused() {
        let mut g = guard(Limits {
            max_files: 2,
            ..Limits::default()
        });
        let names: Vec<String> = (0..201).map(|i| format!("x.part{i:03}.rar")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let r = admit_all(&mut g, &refs).unwrap();
        assert!(r.contains("more than 200 archive volumes"), "{r}");
    }

    /// `NzbgetRsParsedOptions` is shared with C++: the header must list the same fields in
    /// the same order, all `int32_t`.
    #[test]
    fn the_options_struct_matches_the_c_header() {
        let header = include_str!("../include/nzbget_rs.h");
        let start = header
            .find("typedef struct NzbgetRsParsedOptions {")
            .unwrap();
        let end = start + header[start..].find("} NzbgetRsParsedOptions;").unwrap();
        let c: Vec<&str> = header[start..end]
            .lines()
            .skip(1)
            .filter_map(|l| l.trim().strip_prefix("int32_t "))
            .map(|l| l.trim_end_matches(';'))
            .collect();
        let rust_src = include_str!("options.rs");
        let start = rust_src.find("pub struct NzbgetRsParsedOptions {").unwrap();
        let end = start + rust_src[start..].find("\n}").unwrap();
        let r: Vec<&str> = rust_src[start..end]
            .lines()
            .filter_map(|l| l.trim().strip_prefix("pub "))
            .filter_map(|l| l.strip_suffix(": i32,"))
            .collect();
        assert_eq!(c, r);
        assert_eq!(
            std::mem::size_of::<crate::options::NzbgetRsParsedOptions>(),
            4 * c.len()
        );
    }
}
