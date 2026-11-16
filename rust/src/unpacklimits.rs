//! Limits on what an unpack may create, checked as each member starts.
//!
//! Every extractor logs `Unrar: Extracting  <name>` or `7-Zip: Extracting  <name>` before it
//! writes a member. [`Guard::admit`] checks each name as it is logged:
//! - no absolute paths and no `..`;
//! - at most `UnpackMaxFiles` files, `UnpackMaxFolders` folders and `UnpackMaxDepth` levels of
//!   nesting per job;
//! - names and paths within the filesystem limits of both the folder the job unpacks to and
//!   the folder it ends up in.
//!
//! The first breach stops the unpack: the guard reports it as cancelled and the unpack fails
//! with a message that starts with [`LIMIT_PREFIX`] and names the limit and the member.
//! Once the unpack is done, [`asciify_new_entries`] renames every new file and folder with
//! non-ASCII characters to an ASCII name that keeps upper and lower case.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// The start of every message for a breached limit; post-processing fails such a job without
/// a par repair.
pub const LIMIT_PREFIX: &str = "Unpack limit: ";

pub const DEFAULT_MAX_FILES: u32 = 100;
pub const DEFAULT_MAX_DEPTH: u32 = 2;
pub const DEFAULT_MAX_FOLDERS: u32 = 10;

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

/// Tracks what one job's unpack has created so far.
#[derive(Debug)]
pub struct Guard {
    limits: Limits,
    /// The folder the job unpacks to, and the one it ends up in when that differs.
    roots: Vec<Root>,
    files: u64,
    folders: BTreeSet<Vec<String>>,
    /// The ASCII form of every name component members were written under.
    written: BTreeSet<String>,
    breach: Option<String>,
}

impl Guard {
    /// `unpack_dir`: where members are written; `final_dir`: where the job's files end up,
    /// when they are moved after unpacking.
    pub fn new(limits: Limits, unpack_dir: &Path, final_dir: Option<&Path>) -> Self {
        let mut roots = vec![Root::new(unpack_dir)];
        if let Some(f) = final_dir.filter(|f| !f.as_os_str().is_empty() && *f != unpack_dir) {
            roots.push(Root::new(f));
        }
        Self {
            limits,
            roots,
            files: 0,
            folders: BTreeSet::new(),
            written: BTreeSet::new(),
            breach: None,
        }
    }

    /// Whether an entry named `name` may be one this unpack wrote.
    pub fn wrote_name(&self, name: &str) -> bool {
        self.written.contains(&ascii_name(name))
    }

    /// The first breached limit, as the message the unpack fails with.
    pub fn breach(&self) -> Option<&str> {
        self.breach.as_deref()
    }

    /// Checks one extractor log line. `prefix` is where the extractor writes, relative to the
    /// unpack folder (a nested archive unpacks next to itself).
    pub fn observe(&mut self, prefix: &Path, msg: &str) {
        if let Some(name) = msg
            .strip_prefix("Unrar: Extracting  ")
            .or_else(|| msg.strip_prefix("7-Zip: Extracting  "))
        {
            self.admit(prefix, name);
        }
    }

    /// Admits the member `name`, written under `prefix` (relative to the unpack folder).
    /// Records the first breach; later members are ignored once one is recorded.
    pub fn admit(&mut self, prefix: &Path, name: &str) {
        if self.breach.is_some() {
            return;
        }
        if let Err(why) = self.check(prefix, name) {
            self.breach = Some(format!("{LIMIT_PREFIX}{why}"));
        }
    }

    fn check(&mut self, prefix: &Path, name: &str) -> Result<(), String> {
        let shown = name.escape_debug().to_string();
        if name.starts_with('/') || name.starts_with('\\') || has_drive(name) {
            return Err(format!("absolute path refused: \"{shown}\""));
        }
        let mut parts: Vec<String> = Vec::new();
        for c in prefix.components() {
            match c {
                Component::Normal(p) => parts.push(ascii_name(&p.to_string_lossy())),
                _ => return Err(format!("path outside the unpack folder: \"{shown}\"")),
            }
        }
        let nested = parts.len();
        let is_dir_entry = name.ends_with('/') || name.ends_with('\\');
        for part in name.split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => return Err(format!("relative path refused: \"{shown}\"")),
                p => {
                    let a = ascii_name(p);
                    self.written.insert(a.clone());
                    parts.push(a);
                }
            }
        }
        if parts.len() == nested {
            return Ok(());
        }
        let folders_in = if is_dir_entry {
            parts.len()
        } else {
            parts.len() - 1
        };
        let depth = folders_in as u64;
        if self.limits.max_depth > 0 && depth > u64::from(self.limits.max_depth) {
            return Err(format!(
                "\"{shown}\" is {depth} folders deep, more than {} (UnpackMaxDepth)",
                self.limits.max_depth
            ));
        }
        for n in 1..=folders_in {
            self.folders.insert(parts[..n].to_vec());
        }
        if self.limits.max_folders > 0
            && self.folders.len() as u64 > u64::from(self.limits.max_folders)
        {
            return Err(format!(
                "more than {} folders (UnpackMaxFolders), at \"{shown}\"",
                self.limits.max_folders
            ));
        }
        if !is_dir_entry {
            self.files += 1;
            if self.limits.max_files > 0 && self.files > u64::from(self.limits.max_files) {
                return Err(format!(
                    "more than {} files (UnpackMaxFiles), at \"{shown}\"",
                    self.limits.max_files
                ));
            }
        }
        let rel = parts.join("/");
        for root in &self.roots {
            if let Some(long) = parts.iter().find(|p| p.len() as u64 > root.name_max) {
                return Err(format!(
                    "name \"{}\" is {} bytes, more than the {} bytes {} allows",
                    long.escape_debug(),
                    long.len(),
                    root.name_max,
                    root.dir.display()
                ));
            }
            // PATH_MAX counts the terminating NUL.
            let len = root.dir.as_os_str().len() as u64 + 1 + rel.len() as u64;
            if len >= root.path_max {
                return Err(format!(
                    "path to \"{shown}\" is {len} bytes in {}, more than its limit of {}",
                    root.dir.display(),
                    root.path_max - 1
                ));
            }
        }
        Ok(())
    }
}

fn has_drive(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
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

/// Renames every entry under `dir` that is not in `pre_existing`, whose name is not ASCII and
/// that `wrote` says an extractor wrote (files still downloading into the same folder keep
/// their names), deepest first, to its [`ascii_name`]. A taken name gets `_2`, `_3`, ...
/// before its extension. Returns the renamed paths as (old, new).
pub fn asciify_new_entries(
    dir: &Path,
    pre_existing: &BTreeSet<PathBuf>,
    wrote: &dyn Fn(&str) -> bool,
) -> std::io::Result<Vec<(PathBuf, PathBuf)>> {
    let mut renamed = Vec::new();
    asciify_dir(dir, pre_existing, wrote, &mut renamed)?;
    Ok(renamed)
}

fn asciify_dir(
    dir: &Path,
    pre_existing: &BTreeSet<PathBuf>,
    wrote: &dyn Fn(&str) -> bool,
    renamed: &mut Vec<(PathBuf, PathBuf)>,
) -> std::io::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for path in entries {
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            asciify_dir(&path, pre_existing, wrote, renamed)?;
        }
        if pre_existing.contains(&path) {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        let lossy = name.to_string_lossy();
        let ascii = ascii_name(&lossy);
        if (ascii == *name.to_string_lossy() && name.to_str().is_some()) || !wrote(&lossy) {
            continue;
        }
        let target = free_name(dir, &ascii);
        std::fs::rename(&path, &target)?;
        renamed.push((path, target));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn guard(limits: Limits) -> Guard {
        Guard::new(limits, Path::new("/tmp"), None)
    }

    fn admit_all(g: &mut Guard, names: &[&str]) -> Option<String> {
        for n in names {
            g.admit(Path::new(""), n);
        }
        g.breach().map(str::to_string)
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
                "Sample/release-sample.mkv",
                "Subs/English.srt",
                "Subs\\Spanish.srt",
                "Proof/Covers/front.jpg",
            ],
        );
        assert_eq!(r, None);
    }

    #[test]
    fn too_deep_is_refused() {
        let mut g = guard(Limits::default());
        let r = admit_all(&mut g, &["a/b/c/file.bin"]).unwrap();
        assert!(r.starts_with(LIMIT_PREFIX), "{r}");
        assert!(
            r.contains("3 folders deep") && r.contains("UnpackMaxDepth"),
            "{r}"
        );
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
    }

    #[test]
    fn too_many_folders_is_refused() {
        let mut g = guard(Limits::default());
        let names: Vec<String> = (0..11).map(|i| format!("d{i}/f.bin")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let r = admit_all(&mut g, &refs).unwrap();
        assert!(r.contains("more than 10 folders"), "{r}");
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
        for bad in [
            "../etc/passwd",
            "a/../../x",
            "/var/config/x",
            "\\x",
            "C:\\x",
        ] {
            let mut g = guard(Limits::default());
            let r = admit_all(&mut g, &[bad]).unwrap();
            assert!(r.contains("refused"), "{bad}: {r}");
        }
    }

    #[test]
    fn names_and_paths_over_the_filesystem_limits_are_refused() {
        let mut g = guard(Limits::default());
        let long = "x".repeat(300);
        let r = admit_all(&mut g, &[&long]).unwrap();
        assert!(r.contains("300 bytes"), "{r}");

        let mut g = guard(Limits::default());
        g.roots[0].path_max = 64;
        let r = admit_all(&mut g, &[&format!("d/{}", "y".repeat(60))]).unwrap();
        assert!(r.contains("more than its limit of 63"), "{r}");
    }

    #[test]
    fn both_folders_are_checked() {
        let t = std::env::temp_dir().join(format!("rg-limits-{}", std::process::id()));
        let mut g = Guard::new(Limits::default(), &t, Some(&t.join("a".repeat(200))));
        assert_eq!(g.roots.len(), 2);
        g.roots[1].path_max = 300;
        g.admit(Path::new(""), &"z".repeat(100));
        assert!(
            g.breach().unwrap().contains(&"a".repeat(200)),
            "{:?}",
            g.breach()
        );
    }

    #[test]
    fn nested_prefixes_count_toward_depth() {
        let mut g = guard(Limits::default());
        g.admit(Path::new("Inner"), "Sub/file.bin");
        assert_eq!(g.breach(), None);
        g.admit(Path::new("Inner/Sub"), "More/file.bin");
        assert!(g.breach().unwrap().contains("UnpackMaxDepth"));
    }

    #[test]
    fn new_non_ascii_entries_are_renamed() {
        let t = std::env::temp_dir().join(format!("rg-asciify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("Sübs")).unwrap();
        std::fs::write(t.join("Sübs").join("Españ.srt"), b"x").unwrap();
        std::fs::write(t.join("old_ü.txt"), b"keep").unwrap();
        std::fs::write(t.join("Marnûkh.mkv"), b"m").unwrap();
        std::fs::write(t.join("Marnukh.mkv"), b"taken").unwrap();
        std::fs::write(t.join("Downloading_é.rar"), b"vol").unwrap();
        let pre: BTreeSet<PathBuf> = [t.join("old_ü.txt"), t.join("Marnukh.mkv")].into();
        asciify_new_entries(&t, &pre, &|n| n != "Downloading_é.rar").unwrap();
        assert!(t.join("Downloading_é.rar").exists());
        assert_eq!(
            std::fs::read(t.join("Subs").join("Espan.srt")).unwrap(),
            b"x"
        );
        assert!(t.join("old_ü.txt").exists());
        assert_eq!(std::fs::read(t.join("Marnukh.mkv")).unwrap(), b"taken");
        assert_eq!(std::fs::read(t.join("Marnukh_2.mkv")).unwrap(), b"m");
        let _ = std::fs::remove_dir_all(&t);
    }

    /// A ustar archive of regular files.
    fn tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, data) in entries {
            let mut h = [0u8; 512];
            h[..name.len()].copy_from_slice(name.as_bytes());
            h[100..108].copy_from_slice(b"0000644\0");
            h[108..116].copy_from_slice(b"0000000\0");
            h[116..124].copy_from_slice(b"0000000\0");
            h[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
            h[136..148].copy_from_slice(b"14000000000\0");
            h[156] = b'0';
            h[257..263].copy_from_slice(b"ustar\0");
            h[263..265].copy_from_slice(b"00");
            h[148..156].copy_from_slice(b"        ");
            let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
            h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
            out.extend_from_slice(&h);
            out.extend_from_slice(data);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.resize(out.len() + 1024, 0);
        out
    }

    fn unpack(tag: &str, entries: &[(&str, &[u8])]) -> (Result<u64, String>, PathBuf, Vec<String>) {
        let t = std::env::temp_dir().join(format!("rg-unpack-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let archive = t.join("set.tar");
        std::fs::write(&archive, tar(entries)).unwrap();
        let out = t.join("out");
        let mut log = Vec::new();
        let r = crate::unpack::extract_archive_with_zipfast(&archive, &out, "", 1, |k, m| {
            log.push(format!("{k} {m}"))
        })
        .map(|s| s.files);
        (r, out, log)
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

    #[test]
    fn an_unpack_with_too_many_files_stops_and_fails_with_the_reason() {
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
        assert!(files_under(&out) <= 101, "{}", files_under(&out));
    }

    #[test]
    fn an_unpack_nested_too_deep_fails_with_the_reason() {
        let (r, _, _) = unpack("deep", &[("a/b/c/x.bin", b"x")]);
        let err = r.unwrap_err();
        assert!(err.contains("UnpackMaxDepth"), "{err}");
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
            &[("Marnûkh.mkv", b"m"), ("Sample/sämple.mkv", b"s")],
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
}
