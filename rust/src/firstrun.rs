//! First run and mistake-proof settings.
//!
//! - Without a configuration file (or with `-c` naming one that is missing)
//!   the server writes one from the built-in template with safe defaults:
//!   MainDir in the user's home, the web interface on 127.0.0.1 and the first
//!   free port from 6789, no password yet (the setup guide asks for one).
//! - What went wrong while reading the configuration file is kept, so the web
//!   interface can show it (`setupstatus`).
//! - `saveconfig` checks every value it is given against the option list
//!   (the built-in template) and refuses a save that would not load cleanly.
//!   Before it writes, the current file is copied to
//!   `<file>.bak-<date>-<time>` (the last [`BACKUPS_KEPT`] are kept), and
//!   the new file replaces the old one in one step.
//! - `checkpath` tells whether a folder exists, can be created and can be
//!   written to; `configbackups` and `restoreconfig` list and bring back the
//!   copies.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The stock configuration file: option descriptions, defaults and value lists.
pub(crate) const TEMPLATE: &str = include_str!("../../rustyget.conf");

/// How many configuration backups `saveconfig` keeps.
pub const BACKUPS_KEPT: usize = 10;

/// The first port tried for the web interface of a new configuration.
pub const DEFAULT_CONTROL_PORT: u16 = 6789;

/// How many configuration problems are kept for the web interface.
const NOTES_KEPT: usize = 200;

/// The well-known password of the stock configuration.
pub const STOCK_PASSWORD: &str = "tegbzn6789";

// ------------------------------------------------------------------------
// What happened while reading the configuration file
// ------------------------------------------------------------------------

#[derive(Default)]
struct LoadState {
    /// the problems found, as logged ("file:line: what, and what was used")
    notes: Vec<String>,
    /// problems not kept because there were too many
    dropped: usize,
    /// the configuration file was written by this start
    created: bool,
    /// why the configuration file cannot be used, if it cannot
    problem: String,
    /// counts the loads (a reload is a new instance; the web page waits for it)
    instance: u64,
}

static LOAD_STATE: Mutex<Option<LoadState>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut LoadState) -> R) -> R {
    let mut g = LOAD_STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(g.get_or_insert_with(LoadState::default))
}

/// Forgets the problems of the previous load (a reload reads the file again).
/// `created` survives a reload: the file this process wrote is still new.
pub(crate) fn begin_load() {
    with_state(|s| {
        s.notes.clear();
        s.dropped = 0;
        s.problem.clear();
        // random per process start (a page can't mistake another daemon's
        // number for this one's), then one more per load (a reload)
        s.instance = if s.instance == 0 { random_u64() % (1 << 48) + 1 } else { s.instance + 1 };
    });
}

/// Keeps one configuration problem for the web interface.
pub(crate) fn note_problem(text: &[u8]) {
    let text = String::from_utf8_lossy(text).into_owned();
    with_state(|s| {
        if s.notes.len() < NOTES_KEPT {
            s.notes.push(text);
        } else {
            s.dropped += 1;
        }
    });
}

pub(crate) fn mark_created() {
    with_state(|s| s.created = true);
}

pub(crate) fn set_problem(text: &str) {
    with_state(|s| s.problem = text.to_owned());
}

// ------------------------------------------------------------------------
// Creating a configuration file
// ------------------------------------------------------------------------

/// Where a configuration file is written when none was found or given.
pub fn default_config_path() -> Vec<u8> {
    crate::fsops::expand_home_path_auto(Some(c"~/.config/rustyget/rustyget.conf"))
}

/// The main folder of a new configuration.
pub const DEFAULT_MAIN_DIR: &str = "~/RustyGet";

/// The first port from `start` that `ip` can listen on (up to 100 tried);
/// `start` when none can (the server then says why it cannot listen).
pub fn pick_control_port(ip: &str, start: u16) -> u16 {
    let ip = if ip.is_empty() || ip == "0.0.0.0" { "127.0.0.1" } else { ip };
    for port in start..start.saturating_add(100) {
        if std::net::TcpListener::bind((ip, port)).is_ok() {
            return port;
        }
    }
    start
}

/// Bytes from the system's random source (the clock as a weak fallback).
pub fn random_bytes<const N: usize>() -> [u8; N] {
    use std::io::Read as _;
    let mut raw = [0u8; N];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut raw)).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        let mut x = nanos ^ (u64::from(std::process::id()) << 32) | 1;
        for b in &mut raw {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = (x >> 24) as u8;
        }
    }
    raw
}

fn random_u64() -> u64 {
    u64::from_le_bytes(random_bytes::<8>())
}

fn random_hex() -> String {
    random_bytes::<8>().iter().map(|b| format!("{b:02x}")).collect()
}

/// Creates a new file `.<prefix><random>` in `dir` (O_CREAT|O_EXCL, and
/// O_NOFOLLOW: never through a link someone planted), with `mode`.
fn create_exclusive(dir: &Path, prefix: &str, mode: u32) -> std::io::Result<(std::fs::File, PathBuf)> {
    let mut last = std::io::Error::from(std::io::ErrorKind::AlreadyExists);
    for _ in 0..16 {
        let path = dir.join(format!(".{prefix}{}", random_hex()));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            opts.mode(mode).custom_flags(libc::O_NOFOLLOW);
        }
        #[cfg(not(unix))]
        let _ = mode;
        match opts.open(&path) {
            Ok(f) => return Ok((f, path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// Tests: make the next temporary-file write fail (as a full disk would).
#[cfg(test)]
static FAIL_NEXT_TEMP_WRITE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn temp_write(f: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(test)]
    if FAIL_NEXT_TEMP_WRITE.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(std::io::Error::from_raw_os_error(libc::ENOSPC));
    }
    f.write_all(bytes)?;
    f.sync_all()
}

/// Inside a container (Docker, Podman)?
pub fn in_container() -> bool {
    Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists()
}

/// A password of 20 letters and digits from the system's random source
/// (about 119 bits), easy to copy from the terminal.
pub fn random_password() -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let raw = random_bytes::<64>();
    // rejection sampling: no bias towards the first letters
    let limit = 256 - 256 % ALPHABET.len();
    raw.iter()
        .filter(|&&b| (b as usize) < limit)
        .take(20)
        .map(|&b| ALPHABET[b as usize % ALPHABET.len()] as char)
        .collect()
}

/// The built-in template with the defaults of a new configuration: MainDir
/// in the user's home, the web interface on this computer only, the given
/// port, the given password (the setup guide shows it and can change it),
/// the sign-in form and no example news server.
pub fn first_run_config(port: u16, password: &str) -> String {
    let mut out = String::with_capacity(TEMPLATE.len() + 256);
    out.push_str(
        "# Created by RustyGet on first start. Change it in the web interface\n\
         # (Settings) rather than here: Settings checks every value.\n",
    );
    for line in TEMPLATE.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let replaced = match body.split_once('=') {
            Some(("MainDir", _)) => Some(format!("MainDir={DEFAULT_MAIN_DIR}")),
            // in a container the port is published from outside: listen on
            // all interfaces there (the password protects it)
            Some(("ControlIP", _)) => Some(format!("ControlIP={}", if in_container() { "0.0.0.0" } else { "127.0.0.1" })),
            Some(("ControlPort", _)) => Some(format!("ControlPort={port}")),
            Some(("ControlPassword", _)) => Some(format!("ControlPassword={password}")),
            Some(("FormAuth", _)) => Some("FormAuth=yes".to_owned()),
            Some(("WebDir", _)) => Some("WebDir=".to_owned()),
            Some(("ConfigTemplate", _)) => Some("ConfigTemplate=".to_owned()),
            Some(("Server1.Host", _)) => Some("Server1.Host=".to_owned()),
            Some(("Server1.Username", _)) => Some("Server1.Username=".to_owned()),
            Some(("Server1.Password", _)) => Some("Server1.Password=".to_owned()),
            _ => None,
        };
        match replaced {
            Some(r) => {
                out.push_str(&r);
                out.push('\n');
            }
            None => out.push_str(line),
        }
    }
    out
}

/// Writes a new configuration file (never over an existing one), creating
/// its folder. The file is readable by its owner only: it will hold passwords.
pub fn create_config_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        create_private_dirs(dir).map_err(|e| format!("can't create the folder {}: {}", dir.display(), plain_io(&e)))?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let mut file = opts.open(path).map_err(|e| plain_io(&e))?;
    file.write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| plain_io(&e))
}

/// Folders drives are usually mounted under.
const MOUNT_ROOTS: [&str; 5] = ["/mnt/", "/media/", "/Volumes/", "/run/media/", "/srv/"];

/// Best effort: would creating `file` put it on the wrong disk because the
/// drive meant to hold it is not mounted? True when the nearest existing
/// folder above it is an empty folder under a usual mount root and nothing is
/// mounted on it (it is on the same device as its parent). The reason, in
/// plain words, when it looks so.
pub fn looks_unmounted(file: &Path) -> Option<String> {
    let mut dir = file.parent()?;
    while !dir.exists() {
        dir = dir.parent()?;
    }
    looks_unmounted_dir(dir, &MOUNT_ROOTS)
}

fn looks_unmounted_dir(dir: &Path, roots: &[&str]) -> Option<String> {
    let text = dir.to_string_lossy();
    let under_root = roots
        .iter()
        .any(|r| text.starts_with(r) && text.len() > r.len());
    if !under_root {
        return None;
    }
    let empty = std::fs::read_dir(dir).ok()?.next().is_none();
    if !empty {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let own = std::fs::metadata(dir).ok()?.dev();
        let parent = std::fs::metadata(dir.parent()?).ok()?.dev();
        (own == parent).then(|| {
            format!(
                "{} is an empty folder where a drive is usually mounted, and no drive is mounted on it.",
                dir.display()
            )
        })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn create_private_dirs(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        b.mode(0o700);
    }
    b.create(dir)
}

/// An I/O error in plain words.
pub fn plain_io(e: &std::io::Error) -> String {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound => "it does not exist".to_owned(),
        K::PermissionDenied => "permission denied".to_owned(),
        K::AlreadyExists => "it already exists".to_owned(),
        _ => match e.raw_os_error() {
            Some(n) => String::from_utf8_lossy(&crate::fsops::error_message(n)).into_owned(),
            None => e.to_string(),
        },
    }
}

// ------------------------------------------------------------------------
// The option list
// ------------------------------------------------------------------------

/// What the template says about one option.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OptMeta {
    /// the option's name as written in the template ("Server1.Host")
    pub name: String,
    pub default: String,
    /// the values in parentheses at the end of the description's first
    /// sentence: a value list ("yes, no") or a unit or range ("1-65535")
    pub select: Vec<String>,
}

/// The kind of value an option takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// one of these words
    Choice(Vec<String>),
    /// a whole number, maybe within a range
    Number { min: Option<i64>, max: Option<i64> },
    Text,
}

impl OptMeta {
    pub fn kind(&self) -> Kind {
        if self.select.len() > 1 {
            if self.select.iter().all(|v| v.parse::<i64>().is_ok()) {
                return Kind::Number { min: None, max: None };
            }
            return Kind::Choice(self.select.clone());
        }
        if self.default.trim().parse::<i64>().is_err() {
            return Kind::Text;
        }
        let range = self.select.first().and_then(|unit| {
            let (lo, hi) = unit.split_once('-').filter(|(lo, _)| !lo.is_empty())?;
            let (lo, hi) = (lo.trim(), hi.trim());
            // "000-1000" (UMask) is octal digits, not a range of numbers
            if lo.len() > 1 && lo.starts_with('0') {
                return None;
            }
            Some((lo.parse::<i64>().ok()?, hi.parse::<i64>().ok()?))
        });
        match range {
            Some((lo, hi)) => Kind::Number { min: Some(lo), max: Some(hi) },
            None => Kind::Number { min: None, max: None },
        }
    }
}

/// "Server12.Host" -> "server1.host": numbered sections are described once,
/// as their first instance.
pub fn canonical_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let letters = lower.bytes().take_while(u8::is_ascii_alphabetic).count();
    let digits = lower[letters..].bytes().take_while(u8::is_ascii_digit).count();
    if letters > 0 && digits > 0 && lower[letters + digits..].starts_with('.') {
        format!("{}1{}", &lower[..letters], &lower[letters + digits..])
    } else {
        lower
    }
}

/// Reads the option list from a template, as the web interface does
/// (`readConfigTemplate` in webui/config.js).
pub fn parse_template(text: &str) -> HashMap<String, OptMeta> {
    let mut map = HashMap::new();
    let mut description = String::new();
    let mut first = String::new();
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        if line.starts_with("### ") {
            description.clear();
            first.clear();
        } else if line.starts_with("# ") || line == "#" {
            if !description.is_empty() {
                description.push(' ');
            }
            description.push_str(line[1..].trim());
            if first.is_empty() && description.ends_with('.') {
                first = std::mem::take(&mut description);
            }
        } else if !line.starts_with('#') && line.contains('=') {
            let (name, value) = line.split_once('=').unwrap_or((line, ""));
            let name = name.trim();
            let mut select = Vec::new();
            if let (Some(open), true) = (first.rfind('('), first.ends_with(").")) {
                select = first[open + 1..first.len() - 2]
                    .split(',')
                    .map(|s| s.trim().to_owned())
                    .collect();
            }
            map.entry(canonical_name(name)).or_insert_with(|| OptMeta {
                name: name.to_owned(),
                default: value.trim().to_owned(),
                select,
            });
            description.clear();
            first.clear();
        } else {
            description.clear();
            first.clear();
        }
    }
    map
}

/// The built-in option list.
pub fn options_meta() -> &'static HashMap<String, OptMeta> {
    static META: OnceLock<HashMap<String, OptMeta>> = OnceLock::new();
    META.get_or_init(|| parse_template(TEMPLATE))
}

// ------------------------------------------------------------------------
// Checking values before they are saved
// ------------------------------------------------------------------------

/// Folder options that must not be empty.
const REQUIRED_DIRS: [&str; 5] = ["MainDir", "DestDir", "TempDir", "QueueDir", "NzbDir"];
/// Folder options that may be empty.
const OPTIONAL_DIRS: [&str; 1] = ["InterDir"];

fn is_yes_no(choices: &[String]) -> bool {
    choices.len() == 2
        && choices.iter().any(|c| c.eq_ignore_ascii_case("yes"))
        && choices.iter().any(|c| c.eq_ignore_ascii_case("no"))
}

fn choice_ok(name: &str, choices: &[String], value: &str) -> bool {
    if choices.iter().any(|c| c.eq_ignore_ascii_case(value)) {
        return true;
    }
    let accepted: &[&CStr] = if is_yes_no(choices) {
        crate::options::bool_value_names()
    } else {
        crate::options::known_enum_names(&canonical_name(name)).unwrap_or(&[])
    };
    accepted.iter().any(|n| n.to_bytes().eq_ignore_ascii_case(value.as_bytes()))
}

/// A friendly label for an option in a message: "News server 2: Port".
pub fn label(name: &str) -> String {
    let letters = name.bytes().take_while(u8::is_ascii_alphabetic).count();
    let digits = name[letters..].bytes().take_while(u8::is_ascii_digit).count();
    if letters > 0 && digits > 0 && name[letters + digits..].starts_with('.') {
        let section = match name[..letters].to_ascii_lowercase().as_str() {
            "server" => "News server",
            "category" => "Category",
            "feed" => "RSS feed",
            "task" => "Scheduler task",
            _ => &name[..letters],
        };
        return format!("{section} {}: {}", &name[letters..letters + digits], &name[letters + digits + 1..]);
    }
    name.to_owned()
}

fn show(value: &str) -> String {
    let mut v: String = value.chars().take(60).collect();
    if v.len() < value.len() {
        v.push('…');
    }
    v
}

/// Is `host` a plausible host name or IP address?
pub fn host_ok(host: &str) -> bool {
    let h = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    !h.is_empty()
        && h.len() <= 253
        && h.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':' | b'%'))
        && !h.starts_with('-')
        && !h.starts_with('.')
}

/// What a check of one folder found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirState {
    /// it exists and RustyGet can write to it
    Ready,
    /// it does not exist, and RustyGet can create it
    WillCreate,
    /// it cannot be used, and why (in plain words)
    Bad(String),
}

#[cfg(unix)]
fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: a NUL-terminated path.
    unsafe { libc::access(c.as_ptr(), libc::W_OK | libc::X_OK) == 0 }
}

#[cfg(not(unix))]
fn writable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| !m.permissions().readonly())
}

/// Whether RustyGet can use `path` as a folder: it exists and can be written
/// to, or it can be created.
pub fn check_dir(path: &Path) -> DirState {
    if path.as_os_str().is_empty() {
        return DirState::Bad("No folder is entered.".to_owned());
    }
    if !path.is_absolute() {
        return DirState::Bad("Use a full path, starting with / or ~/.".to_owned());
    }
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => {
            if writable(path) {
                DirState::Ready
            } else {
                DirState::Bad(format!(
                    "RustyGet is not allowed to write to {}. Choose another folder, or change its permissions.",
                    path.display()
                ))
            }
        }
        Ok(_) => DirState::Bad(format!("{} is a file, not a folder.", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound || e.raw_os_error() == Some(libc::ENOTDIR) => {
            let mut up = path.parent();
            while let Some(dir) = up {
                match std::fs::metadata(dir) {
                    Ok(m) if m.is_dir() => {
                        return if writable(dir) {
                            DirState::WillCreate
                        } else {
                            DirState::Bad(format!(
                                "It does not exist, and RustyGet is not allowed to create folders in {}.",
                                dir.display()
                            ))
                        };
                    }
                    Ok(_) => {
                        return DirState::Bad(format!(
                            "{} is a file, so no folder can be created inside it.",
                            dir.display()
                        ))
                    }
                    Err(_) => up = dir.parent(),
                }
            }
            DirState::Bad("It does not exist and cannot be created.".to_owned())
        }
        Err(e) => DirState::Bad(format!("It can't be checked: {}.", plain_io(&e))),
    }
}

/// Expands `~/`, `${Option}` (from `lookup`) and makes a relative path
/// relative to `main_dir`, as the configuration loader does.
pub fn expand_path(value: &str, main_dir: &str, lookup: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    let homed = |v: &str| -> String {
        if v.starts_with("~/") || v == "~" {
            let with_slash = if v == "~" { "~/".to_owned() } else { v.to_owned() };
            CString::new(with_slash.clone())
                .map(|c| String::from_utf8_lossy(&crate::fsops::expand_home_path_auto(Some(&c))).into_owned())
                .unwrap_or(with_slash)
        } else {
            v.to_owned()
        }
    };
    let main = homed(main_dir.trim());
    let expanded = crate::options::expand_option_variables(homed(value.trim()).as_bytes(), |name| {
        let n = name.to_str().unwrap_or("");
        if n.eq_ignore_ascii_case("MainDir") {
            Some(main.as_bytes().to_vec())
        } else {
            lookup(n).map(|v| homed(&v).into_bytes())
        }
    });
    let text = String::from_utf8_lossy(&expanded).into_owned();
    let p = PathBuf::from(&text);
    if !text.is_empty() && p.is_relative() && !main.is_empty() && !value.trim().eq_ignore_ascii_case("${MainDir}") {
        Path::new(&main).join(p)
    } else {
        p
    }
}

/// The value of a running option, if any.
pub fn running_option(name: &str) -> Option<String> {
    let opts = crate::options::active_options()?;
    let mut found = None;
    opts.for_each_option(|n, v, _| {
        if found.is_none() && n.to_bytes().eq_ignore_ascii_case(name.as_bytes()) {
            found = Some(String::from_utf8_lossy(v.to_bytes()).into_owned());
        }
    });
    found
}

/// Is `name` an option RustyGet has (in the template or the defaults)?
pub(crate) fn known_option(name: &str) -> bool {
    predefined(name)
}

fn predefined(name: &str) -> bool {
    if options_meta().contains_key(&canonical_name(name)) {
        return true;
    }
    let mut found = false;
    crate::options::init_defaults_with(true, |n, _| {
        found |= n.to_bytes().eq_ignore_ascii_case(name.as_bytes());
    });
    found
}

/// Checks the options `saveconfig` was given (see [`validate_detailed`]);
/// every folder is checked.
pub fn validate(entries: &[(Vec<u8>, Vec<u8>)], lookup: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    validate_detailed(entries, lookup, &|_| None)
        .into_iter()
        .map(|(_, m)| m)
        .collect()
}

/// Checks the options `saveconfig` was given. Each problem is the option it
/// is about and one sentence that names it, says what is wrong and what to
/// do. Options it does not know the type of are only checked for characters
/// that would break the file. `lookup` gives the running value of an option
/// the list does not set (for `${...}` in paths); `current` the value the
/// configuration file has now: a folder that did not change is not checked
/// again (a NAS that is not mounted must not block an unrelated save).
pub fn validate_detailed(
    entries: &[(Vec<u8>, Vec<u8>)],
    lookup: &dyn Fn(&str) -> Option<String>,
    current: &dyn Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let mut problems: Vec<(String, String)> = Vec::new();
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let given = |name: &str| -> Option<String> {
        entries
            .iter()
            .rev()
            .find(|(n, _)| n.eq_ignore_ascii_case(name.as_bytes()))
            .map(|(_, v)| text(v))
    };
    for (raw_name, raw_value) in entries {
        let name = text(raw_name);
        let value = text(raw_value);
        let nice = label(&name);
        let mut push = |m: String| problems.push((name.clone(), m));
        if name.trim().is_empty()
            || name.trim() != name
            || name.starts_with('#')
            || raw_name.iter().any(|&b| matches!(b, b'=' | b'\n' | b'\r' | 0))
        {
            push(format!("\"{}\" is not an option name.", show(&name)));
            continue;
        }
        if raw_value.iter().any(|&b| matches!(b, b'\n' | b'\r' | 0)) {
            push(format!("{nice}: the value has a line break in it. Enter it on one line."));
            continue;
        }
        if let Ok(c) = CString::new(raw_name.clone()) {
            let check = crate::options::validate_option_name(&c, &mut || predefined(&name));
            if check == crate::options::NameCheck::Invalid {
                push(format!("\"{}\" is not an option RustyGet knows. Remove it.", show(&name)));
                continue;
            }
        }
        let Some(meta) = options_meta().get(&canonical_name(&name)) else {
            continue;
        };
        match meta.kind() {
            Kind::Choice(choices) => {
                if !choice_ok(&name, &choices, value.trim()) {
                    push(format!(
                        "{nice}: \"{}\" is not one of the choices. Pick one of: {}.",
                        show(&value),
                        choices.join(", ")
                    ));
                }
            }
            Kind::Number { min, max } => match value.trim().parse::<i64>() {
                Err(_) if value.trim().is_empty() => {
                    push(format!("{nice}: enter a number (the default is {}).", meta.default));
                }
                Err(_) => push(format!(
                    "{nice}: \"{}\" is not a whole number. Enter digits only, for example {}.",
                    show(&value),
                    meta.default
                )),
                Ok(n) => {
                    if let (Some(lo), Some(hi)) = (min, max) {
                        if n < lo || n > hi {
                            push(format!("{nice}: {n} is out of range. Enter a number from {lo} to {hi}."));
                        }
                    }
                }
            },
            Kind::Text => {}
        }
    }

    // a news server that is on needs a host
    for (raw_name, raw_value) in entries {
        let name = text(raw_name);
        let lower = name.to_ascii_lowercase();
        if !(lower.starts_with("server") && lower.ends_with(".host")) {
            continue;
        }
        let prefix = &name[..name.len() - ".host".len()];
        let host = text(raw_value);
        let host = host.trim();
        let active = given(&format!("{prefix}.Active")).is_none_or(|a| {
            !crate::options::bool_value_names()
                .iter()
                .enumerate()
                .any(|(i, n)| i % 2 == 1 && n.to_bytes().eq_ignore_ascii_case(a.trim().as_bytes()))
        });
        if host.is_empty() {
            if active {
                problems.push((
                    name.clone(),
                    format!(
                        "{}: a news server that is on needs a host name, for example news.example.com. Enter it, or turn the server off.",
                        label(&name)
                    ),
                ));
            }
        } else if !host_ok(host) {
            problems.push((
                name.clone(),
                format!(
                    "{}: \"{}\" is not a host name or IP address. Enter only the name, for example news.example.com (no http:// and no spaces).",
                    label(&name),
                    show(host)
                ),
            ));
        }
    }

    // folders: empty required ones always; usable only when they changed (a
    // changed MainDir changes what ${MainDir} means for the others)
    let main_dir = given("MainDir").or_else(|| lookup("MainDir")).unwrap_or_default();
    let main_changed = given("MainDir").is_some_and(|m| current("MainDir").as_deref() != Some(m.as_str()));
    let resolve = |n: &str| given(n).or_else(|| lookup(n));
    let changed = |option: &str, value: &str| {
        let depends_on_main = value.contains("${") || !(value.starts_with('/') || value.starts_with('~'));
        current(option).as_deref() != Some(value) || (main_changed && depends_on_main && option != "MainDir")
    };
    for (option, required) in REQUIRED_DIRS
        .iter()
        .map(|o| (*o, true))
        .chain(OPTIONAL_DIRS.iter().map(|o| (*o, false)))
    {
        let Some(value) = given(option) else {
            continue;
        };
        if value.trim().is_empty() {
            if required {
                problems.push((option.to_owned(), format!("{option}: enter a folder. It must not be empty.")));
            }
            continue;
        }
        if !changed(option, &value) {
            continue;
        }
        let path = expand_path(&value, if option == "MainDir" { "" } else { &main_dir }, &resolve);
        if let DirState::Bad(why) = check_dir(&path) {
            problems.push((option.to_owned(), format!("{option} ({}): {why}", path.display())));
        }
    }
    for (raw_name, raw_value) in entries {
        let name = text(raw_name);
        let lower = name.to_ascii_lowercase();
        let value = text(raw_value);
        if lower.starts_with("category") && lower.ends_with(".destdir") && !value.is_empty() && changed(&name, &value) {
            let path = expand_path(&value, &main_dir, &resolve);
            if let DirState::Bad(why) = check_dir(&path) {
                problems.push((name.clone(), format!("{} ({}): {why}", label(&name), path.display())));
            }
        }
    }
    problems
}

// ------------------------------------------------------------------------
// Writing the configuration file: backups and one-step replacement
// ------------------------------------------------------------------------

fn backup_prefix(config: &Path) -> Option<String> {
    Some(format!("{}.bak-", config.file_name()?.to_string_lossy()))
}

/// Is `name` a backup RustyGet made of a file whose backups start with
/// `prefix`: `<prefix>YYYYMMDD-HHMMSS`, maybe with `-N`? A hand-made
/// `rustyget.conf.bak-original` is not, and is never pruned or listed.
pub fn is_backup_name(prefix: &str, name: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    let b = rest.as_bytes();
    if b.len() < 15 || b[8] != b'-' || !b[..8].iter().chain(&b[9..15]).all(u8::is_ascii_digit) {
        return false;
    }
    match &b[15..] {
        [] => true,
        [b'-', n @ ..] => !n.is_empty() && n.iter().all(u8::is_ascii_digit),
        _ => false,
    }
}

fn local_stamp() -> String {
    #[cfg(unix)]
    // SAFETY: localtime_r writes into a local tm.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&now, &mut tm).is_null() {
            return format!(
                "{:04}{:02}{:02}-{:02}{:02}{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_hour,
                tm.tm_min,
                tm.tm_sec
            );
        }
    }
    "19700101-000000".to_owned()
}

fn config_dir(config: &Path) -> PathBuf {
    match config.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Where backups go: next to the configuration file, or (when that folder
/// can't be written) QueueDir, then MainDir.
pub fn backup_dirs(config: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![config_dir(config)];
    for option in ["QueueDir", "MainDir"] {
        if let Some(v) = running_option(option).filter(|v| !v.is_empty()) {
            let p = PathBuf::from(v);
            if p.is_absolute() && !dirs.contains(&p) {
                dirs.push(p);
            }
        }
    }
    dirs
}

/// The backups of `config` in `dirs`, oldest first.
pub fn list_backups_in(config: &Path, dirs: &[PathBuf]) -> Vec<PathBuf> {
    let Some(prefix) = backup_prefix(config) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().into_owned();
            if is_backup_name(&prefix, &name) && e.metadata().is_ok_and(|m| m.is_file()) {
                // "...-HHMMSS" sorts before "...-HHMMSS-2"; pad the counter
                let key = match name[prefix.len()..].split_at(15) {
                    (stamp, "") => format!("{stamp}-0000"),
                    (stamp, n) => format!("{stamp}-{:0>4}", &n[1..]),
                };
                found.push((key, e.path()));
            }
        }
    }
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

/// The backups of `config`, oldest first.
pub fn list_backups(config: &Path) -> Vec<PathBuf> {
    list_backups_in(config, &backup_dirs(config))
}

/// Copies `config` to `<config>.bak-<date>-<time>` next to it (or in the
/// first of the other folders that can be written; see [`backup_dirs`]).
/// The copy is written to a temporary file, synced and renamed, readable by
/// its owner only. Afterwards only the newest [`BACKUPS_KEPT`] and the very
/// first backup are kept. `Ok(None)`: there was nothing to copy.
pub fn backup_config(config: &Path) -> Result<Option<PathBuf>, String> {
    backup_config_in(config, &backup_dirs(config))
}

/// [`backup_config`] with the folders given.
pub fn backup_config_in(config: &Path, dirs: &[PathBuf]) -> Result<Option<PathBuf>, String> {
    let bytes = match std::fs::read(config) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("can't read {} to back it up: {}", config.display(), plain_io(&e))),
    };
    let prefix = backup_prefix(config).ok_or("the configuration file has no name")?;
    let stamp = local_stamp();
    let mut last_err = String::from("no folder to keep it in");
    for dir in dirs {
        match write_backup(dir, &prefix, &stamp, &bytes) {
            Ok(target) => {
                prune_backups(config, dirs, &target);
                return Ok(Some(target));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn write_backup(dir: &Path, prefix: &str, stamp: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let fail = |what: &Path, e: &std::io::Error| format!("can't write the backup copy {}: {}", what.display(), plain_io(e));
    let (mut f, tmp) = create_exclusive(dir, &format!("{prefix}saving-"), 0o600).map_err(|e| fail(dir, &e))?;
    if let Err(e) = temp_write(&mut f, bytes) {
        drop(f);
        let _ = std::fs::remove_file(&tmp);
        return Err(fail(&tmp, &e));
    }
    drop(f);
    // a name after every one made in this second (so a pruned name is not
    // reused out of order); hard_link fails when it exists
    let base = format!("{prefix}{stamp}");
    let mut n = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let rest = name.strip_prefix(&base)?;
                    if rest.is_empty() {
                        Some(1)
                    } else {
                        rest.strip_prefix('-')?.parse::<u32>().ok()
                    }
                })
                .max()
                .map_or(1, |m| m + 1)
        })
        .unwrap_or(1);
    loop {
        let target = if n == 1 {
            dir.join(format!("{prefix}{stamp}"))
        } else {
            dir.join(format!("{prefix}{stamp}-{n}"))
        };
        match std::fs::hard_link(&tmp, &target) {
            Ok(()) => {
                let _ = std::fs::remove_file(&tmp);
                return Ok(target);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => n += 1,
            Err(_) if !target.exists() => {
                // no hard links here (some network shares): rename instead
                return match std::fs::rename(&tmp, &target) {
                    Ok(()) => Ok(target),
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp);
                        Err(fail(&target, &e))
                    }
                };
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(fail(&target, &e));
            }
        }
    }
}

/// Keeps the newest [`BACKUPS_KEPT`] backups and the oldest one (the
/// settings from before RustyGet first changed them); never touches a file
/// RustyGet did not name.
fn prune_backups(config: &Path, dirs: &[PathBuf], just_made: &Path) {
    let all = list_backups_in(config, dirs);
    if all.len() <= BACKUPS_KEPT + 1 {
        return;
    }
    for old in &all[1..all.len() - BACKUPS_KEPT] {
        if old != just_made {
            let _ = std::fs::remove_file(old);
        }
    }
}

fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(0)
}

/// Replaces `config` with `bytes` in one step: a new file next to it is
/// written, synced and renamed over the old one, so a crash or a full disk
/// leaves the old or the new file, never half of one. A symbolic link is
/// followed; the file's mode and (where allowed) owner are kept. Only when
/// the rename is impossible (a file mounted on its own, as Docker does:
/// EBUSY or EXDEV) or the folder can't be written is the file rewritten in
/// place. A configuration file RustyGet may not write is never replaced.
pub fn write_config_atomically(config: &Path, bytes: &[u8]) -> Result<(), String> {
    let target = std::fs::canonicalize(config).unwrap_or_else(|_| config.to_path_buf());
    let meta = std::fs::metadata(&target).ok();
    if meta.as_ref().is_some_and(std::fs::Metadata::is_dir) {
        return Err(format!("{} is a folder", target.display()));
    }
    if meta.is_some() && !writable_file(&target) {
        return Err(format!(
            "{} is read-only for RustyGet, so it is left as it is. Make it writable (or start RustyGet with -c <another file>) and save again",
            target.display()
        ));
    }
    let name = target.file_name().map_or_else(|| "rustyget.conf".into(), |n| n.to_string_lossy().into_owned());
    let fail = |e: &std::io::Error| format!("can't write {}: {}", target.display(), plain_io(e));
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt as _;
        meta.as_ref().map_or(0o600, |m| m.permissions().mode() & 0o7777)
    };
    #[cfg(not(unix))]
    let mode = 0o600;
    let (mut f, tmp) = match create_exclusive(&config_dir(&target), &format!("{name}.saving-"), mode) {
        Ok(pair) => pair,
        // the folder can't be written: the file itself can be, in place
        Err(e) if matches!(errno_of(&e), libc::EACCES | libc::EPERM) => return write_in_place(&target, bytes),
        Err(e) => return Err(fail(&e)),
    };
    let written = temp_write(&mut f, bytes);
    if let Err(e) = written {
        // never fall back to writing in place after this: the disk may be full
        drop(f);
        let _ = std::fs::remove_file(&tmp);
        return Err(fail(&e));
    }
    #[cfg(unix)]
    if let Some(m) = &meta {
        use std::os::unix::fs::MetadataExt as _;
        use std::os::unix::fs::PermissionsExt as _;
        let _ = f.set_permissions(std::fs::Permissions::from_mode(m.permissions().mode() & 0o7777));
        // keep the owner where allowed (root, or the same user)
        let _ = std::os::unix::fs::fchown(&f, Some(m.uid()), Some(m.gid()));
    }
    drop(f);
    match std::fs::rename(&tmp, &target) {
        Ok(()) => {
            if let Ok(d) = std::fs::File::open(config_dir(&target)) {
                let _ = d.sync_all();
            }
            Ok(())
        }
        Err(e) if matches!(errno_of(&e), libc::EBUSY | libc::EXDEV) => {
            let _ = std::fs::remove_file(&tmp);
            write_in_place(&target, bytes)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(fail(&e))
        }
    }
}

/// Rewrites `target` in place (the backup holds the old text).
fn write_in_place(target: &Path, bytes: &[u8]) -> Result<(), String> {
    let fail = |e: &std::io::Error| format!("can't write {}: {}", target.display(), plain_io(e));
    let mut f = std::fs::OpenOptions::new().write(true).open(target).map_err(|e| fail(&e))?;
    f.write_all(bytes)
        .and_then(|()| f.set_len(bytes.len() as u64))
        .and_then(|()| f.sync_all())
        .map_err(|e| fail(&e))
}

/// Puts a backup back: the current file is backed up first (so a restore
/// can be undone the same way), then replaced with the backup. `name` is the
/// backup's file name as [`list_backups`] gives it.
pub fn restore_backup(config: &Path, name: &str) -> Result<(), String> {
    restore_backup_in(config, name, &backup_dirs(config))
}

/// [`restore_backup`] looking in `dirs`.
pub fn restore_backup_in(config: &Path, name: &str, dirs: &[PathBuf]) -> Result<(), String> {
    let prefix = backup_prefix(config).ok_or("the configuration file has no name")?;
    if !is_backup_name(&prefix, name) {
        return Err(format!("\"{}\" is not a backup of this configuration file.", show(name)));
    }
    let source = list_backups_in(config, dirs)
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy() == name))
        .ok_or_else(|| format!("the backup {name} is not there any more"))?;
    let bytes = std::fs::read(&source).map_err(|e| format!("can't read the backup {name}: {}", plain_io(&e)))?;
    if let Err(why) = backup_config_in(config, dirs) {
        crate::log::log_active_bytes(1, format!("The settings could not be backed up before the restore: {why}").as_bytes());
    }
    write_config_atomically(config, &bytes)
}

// ------------------------------------------------------------------------
// RPC replies
// ------------------------------------------------------------------------

fn reply(is_json: bool, json: String) -> Vec<u8> {
    if is_json {
        json.into_bytes()
    } else {
        crate::sysmetrics::to_xml(&json).into_bytes()
    }
}

fn js(out: &mut String, s: &str) {
    crate::sysmetrics::json_str(out, s);
}

fn config_path() -> Option<PathBuf> {
    let opts = crate::options::active_options()?;
    let f = opts.str_bytes(crate::options::OptStr::ConfigFile);
    (!f.is_empty()).then(|| crate::cleanup::bytes_to_path_buf(f))
}

/// Can the configuration file be saved? The reason when not. (Backups go
/// elsewhere when its folder can't be written, so that is not required.)
pub fn config_writable(config: Option<&Path>) -> Result<(), String> {
    let Some(config) = config else {
        return Err("RustyGet is running without a configuration file, so settings can't be saved. \
Start it with -c <file> to name one."
            .to_owned());
    };
    match std::fs::metadata(config) {
        Ok(m) if m.is_dir() => Err(format!("{} is a folder, not a file.", config.display())),
        Ok(_) if !writable_file(config) => Err(format!(
            "RustyGet is not allowed to change {} (it is read-only). Change its permissions, or start RustyGet with -c <another file>.",
            config.display()
        )),
        Ok(_) => Ok(()),
        Err(e) => Err(format!("{} can't be used: {}.", config.display(), plain_io(&e))),
    }
}

#[cfg(unix)]
fn writable_file(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::PermissionsExt as _;
    // root passes access() for anything: a file nobody may write (0444) is
    // read-only on purpose, and is respected
    if std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o222 == 0) {
        return false;
    }
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: a NUL-terminated path.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

#[cfg(not(unix))]
fn writable_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| !m.permissions().readonly())
}

/// How many news servers are on and have a host.
fn usable_servers() -> usize {
    let Some(opts) = crate::options::active_options() else {
        return 0;
    };
    let mut hosts: Vec<(String, String)> = Vec::new();
    let mut inactive: Vec<String> = Vec::new();
    // the daemon uses a server only with a host, a port and connections
    let mut complete: Vec<String> = Vec::new();
    let mut ports: Vec<String> = Vec::new();
    opts.for_each_option(|n, v, _| {
        let name = String::from_utf8_lossy(n.to_bytes()).to_ascii_lowercase();
        let value = String::from_utf8_lossy(v.to_bytes()).trim().to_owned();
        if !name.starts_with("server") {
            return;
        }
        if let Some(prefix) = name.strip_suffix(".port") {
            ports.push(prefix.to_owned());
        } else if let Some(prefix) = name.strip_suffix(".connections") {
            complete.push(prefix.to_owned());
        } else if let Some(prefix) = name.strip_suffix(".host") {
            if !value.is_empty() {
                hosts.push((prefix.to_owned(), value));
            }
        } else if let Some(prefix) = name.strip_suffix(".active") {
            let off = crate::options::bool_value_names()
                .iter()
                .enumerate()
                .any(|(i, b)| i % 2 == 1 && b.to_bytes().eq_ignore_ascii_case(value.as_bytes()));
            if off {
                inactive.push(prefix.to_owned());
            }
        }
    });
    hosts
        .iter()
        .filter(|(p, _)| !inactive.contains(p) && ports.contains(p) && complete.contains(p))
        .count()
}

/// The paths warned about at start-up, with what is wrong with each now.
fn path_problems() -> Vec<(String, String, String)> {
    crate::options::path_warning_list()
        .into_iter()
        .filter_map(|(o, v)| {
            let option = String::from_utf8_lossy(&o).into_owned();
            let value = String::from_utf8_lossy(&v).into_owned();
            let why = if matches!(option.as_str(), "LogFile" | "LockFile") {
                match check_dir(Path::new(&value).parent().unwrap_or(Path::new(""))) {
                    DirState::Bad(why) => why,
                    _ => return None,
                }
            } else {
                match check_dir(Path::new(&value)) {
                    DirState::Bad(why) => why,
                    _ => return None,
                }
            };
            Some((option, value, why))
        })
        .collect()
}

/// No (host, active) pair is an active server with a host.
pub(crate) fn no_usable_server_in(hosts: Vec<(Vec<u8>, bool)>) -> bool {
    !hosts.iter().any(|(h, active)| *active && !h.trim_ascii().is_empty())
}

/// Is downloading blocked because no news server is set up? (The daemon
/// pauses downloading at start-up then.)
pub(crate) fn no_usable_server() -> bool {
    usable_servers() == 0
}

/// `setupstatus`: what still keeps RustyGet from downloading, for the setup
/// guide and the banner. Only a missing news server and unusable folders
/// block downloading (`DownloadsBlocked`); a stock or empty password is only
/// a hint. A restricted user is not told the user name or password state.
pub fn rpc_setup_status(is_json: bool, restricted: bool) -> Vec<u8> {
    let config = config_path();
    let password = running_option("ControlPassword").unwrap_or_default();
    let username = running_option("ControlUsername").unwrap_or_default();
    let servers = usable_servers();
    let paths = path_problems();
    let (notes, dropped, created, problem, instance) =
        with_state(|s| (s.notes.clone(), s.dropped, s.created, s.problem.clone(), s.instance));
    let writable = config_writable(config.as_deref());
    let mut o = String::with_capacity(1024);
    o.push_str("{\"ConfigFile\":");
    js(&mut o, &config.as_deref().map_or(String::new(), |p| p.display().to_string()));
    o.push_str(&format!(",\"Instance\":{instance}"));
    o.push_str(&format!(",\"ConfigCreated\":{created}"));
    o.push_str(&format!(",\"ConfigWritable\":{}", writable.is_ok()));
    o.push_str(",\"ConfigProblem\":");
    let problem = if problem.is_empty() { writable.err().unwrap_or_default() } else { problem };
    js(&mut o, &problem);
    if !restricted {
        o.push_str(&format!(",\"PasswordSet\":{}", !password.is_empty() && password != STOCK_PASSWORD));
        o.push_str(&format!(",\"PasswordIsStock\":{}", password == STOCK_PASSWORD));
        o.push_str(",\"ControlUsername\":");
        js(&mut o, &username);
    }
    o.push_str(&format!(",\"Servers\":{servers}"));
    o.push_str(",\"PathProblems\":[");
    for (i, (option, value, why)) in paths.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"Option\":");
        js(&mut o, option);
        o.push_str(",\"Value\":");
        js(&mut o, value);
        o.push_str(",\"Problem\":");
        js(&mut o, why);
        o.push('}');
    }
    o.push_str("],\"Warnings\":[");
    for (i, n) in notes.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        js(&mut o, n);
    }
    o.push_str(&format!("],\"MoreWarnings\":{dropped}"));
    let blocked = servers == 0 || !paths.is_empty();
    o.push_str(&format!(",\"DownloadsBlocked\":{blocked}"));
    o.push_str(&format!(
        ",\"SetupComplete\":{}}}",
        !blocked && !password.is_empty() && password != STOCK_PASSWORD
    ));
    reply(is_json, o)
}

/// `checkpath(path, maindir)`: can this folder be used? `maindir` is the
/// MainDir to read `${MainDir}` and relative paths with (the running one
/// when empty), so the setup guide can check folders before saving.
pub fn rpc_check_path(is_json: bool, path: &str, main_dir: &str) -> Vec<u8> {
    let main = if main_dir.trim().is_empty() {
        running_option("MainDir").unwrap_or_default()
    } else {
        main_dir.to_owned()
    };
    let main_expanded = expand_path(&main, "", &running_option);
    let full = expand_path(path, &main_expanded.to_string_lossy(), &running_option);
    let (state, message) = match check_dir(&full) {
        DirState::Ready => ("ok", "This folder exists and RustyGet can write to it.".to_owned()),
        DirState::WillCreate => ("create", "This folder doesn't exist yet. RustyGet will create it.".to_owned()),
        DirState::Bad(why) => ("bad", why),
    };
    let mut o = String::from("{\"Path\":");
    js(&mut o, &full.display().to_string());
    o.push_str(",\"State\":");
    js(&mut o, state);
    o.push_str(",\"Message\":");
    js(&mut o, &message);
    o.push('}');
    reply(is_json, o)
}

/// `configbackups()`: the backups of the configuration file, newest first.
pub fn rpc_config_backups(is_json: bool) -> Vec<u8> {
    let mut o = String::from("[");
    if let Some(config) = config_path() {
        for (i, p) in list_backups(&config).iter().rev().enumerate() {
            let meta = std::fs::metadata(p).ok();
            let time = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            if i > 0 {
                o.push(',');
            }
            o.push_str("{\"Name\":");
            js(&mut o, &p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned()));
            o.push_str(&format!(",\"Time\":{time},\"Size\":{}}}", meta.map_or(0, |m| m.len())));
        }
    }
    o.push(']');
    reply(is_json, o)
}

/// `restoreconfig(name)`: puts a backup back. The caller reloads afterwards.
/// `Err` holds the reason, in plain words.
pub fn rpc_restore_config(name: &str) -> Result<(), String> {
    let config = config_path().ok_or("RustyGet is running without a configuration file.")?;
    let _guard = crate::configfile::lock_config_writes();
    restore_backup(&config, name)?;
    crate::log::log_active_bytes(
        crate::log::MK_INFO,
        format!("Settings restored from the backup {name}; the settings before it were backed up too").as_bytes(),
    );
    Ok(())
}

/// The web interface's address, for messages.
pub fn web_address(ip: &str, port: i32) -> String {
    let host = match ip {
        "" | "0.0.0.0" | "127.0.0.1" => "127.0.0.1".to_owned(),
        "::" | "::1" => "[::1]".to_owned(),
        other if other.contains(':') => format!("[{other}]"),
        other => other.to_owned(),
    };
    format!("http://{host}:{port}/")
}

/// A `CStr` from text without NUL bytes (lossy).
pub(crate) fn cstring_lossy(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rg-firstrun-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn e(n: &str, v: &str) -> (Vec<u8>, Vec<u8>) {
        (n.as_bytes().to_vec(), v.as_bytes().to_vec())
    }

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn the_template_gives_choices_numbers_and_ranges() {
        let m = options_meta();
        assert_eq!(m["controlport"].kind(), Kind::Number { min: Some(1), max: Some(65535) });
        assert_eq!(m["server1.port"].kind(), Kind::Number { min: Some(1), max: Some(65535) });
        assert_eq!(m["articlecache"].kind(), Kind::Number { min: None, max: None });
        assert_eq!(m["umask"].kind(), Kind::Number { min: None, max: None });
        assert!(matches!(m["server1.encryption"].kind(), Kind::Choice(c) if c == ["yes", "no"]));
        assert!(matches!(m["outputmode"].kind(), Kind::Choice(_)));
        assert_eq!(m["maindir"].kind(), Kind::Text);
        assert_eq!(canonical_name("Server12.Host"), "server1.host");
        assert_eq!(canonical_name("Category3.DestDir"), "category1.destdir");
        assert_eq!(canonical_name("MainDir"), "maindir");
        assert_eq!(label("Server2.Port"), "News server 2: Port");
    }

    #[test]
    fn bad_values_are_refused_in_plain_words() {
        let p = validate(
            &[
                e("ControlPort", "abc"),
                e("Server1.Port", "70000"),
                e("Server1.Encryption", "maybe"),
                e("ArticleCache", ""),
                e("Bogus", "1"),
                e("Server1.Host", "http://news example"),
                e("NzbDirInterval", "5\nControlPassword=x"),
                e("bad=name", "1"),
            ],
            &none,
        );
        let all = p.join("\n");
        assert!(all.contains("ControlPort: \"abc\" is not a whole number"), "{all}");
        assert!(all.contains("News server 1: Port: 70000 is out of range. Enter a number from 1 to 65535."), "{all}");
        assert!(all.contains("News server 1: Encryption: \"maybe\" is not one of the choices. Pick one of: yes, no."), "{all}");
        assert!(all.contains("ArticleCache: enter a number (the default is 500)."), "{all}");
        assert!(all.contains("\"Bogus\" is not an option RustyGet knows"), "{all}");
        assert!(all.contains("is not a host name or IP address"), "{all}");
        assert!(all.contains("NzbDirInterval: the value has a line break"), "{all}");
        assert!(all.contains("\"bad=name\" is not an option name"), "{all}");
        assert_eq!(p.len(), 8, "{all}");
    }

    #[test]
    fn good_values_and_aliases_pass() {
        let d = tmp("good");
        let main = d.join("main");
        let p = validate(
            &[
                e("ControlPort", "6789"),
                e("Server1.Active", "yes"),
                e("Server1.Host", "news.example.com"),
                e("Server2.Host", "[2001:db8::1]"),
                e("Server3.Active", "no"),
                e("Server3.Host", ""),
                e("Server1.Encryption", "YES"),
                e("Server1.CertVerification", "strict"),
                e("OutputMode", "log"),
                e("DupeArticleFallback", "yes"),
                e("FlushQueue", "true"),
                e("UMask", "0022"),
                e("TimeCorrection", "-60"),
                e("VideoSort:MoviesFormat", "%t (%y)"),
                e("MainDir", main.to_str().unwrap()),
                e("DestDir", "${MainDir}/dst"),
                e("InterDir", ""),
                e("TempDir", "tmp"),
                e("NzbDirInterval", " 9 "),
            ],
            &none,
        );
        assert!(p.is_empty(), "{p:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_active_server_needs_a_host_and_folders_must_be_usable() {
        let d = tmp("dirs");
        std::fs::write(d.join("file"), b"x").unwrap();
        let p = validate(
            &[
                e("Server1.Host", ""),
                e("MainDir", d.join("file").join("sub").to_str().unwrap()),
                e("DestDir", ""),
                e("QueueDir", "relative/ok"),
            ],
            &none,
        );
        let all = p.join("\n");
        assert!(all.contains("News server 1: Host: a news server that is on needs a host name"), "{all}");
        assert!(all.contains("is a file, so no folder can be created inside it"), "{all}");
        assert!(all.contains("DestDir: enter a folder"), "{all}");
        assert_eq!(p.len(), 4, "the relative QueueDir lies in the bad MainDir too: {all}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn folders_are_checked() {
        let d = tmp("check");
        assert_eq!(check_dir(&d), DirState::Ready);
        assert_eq!(check_dir(&d.join("a/b/c")), DirState::WillCreate);
        std::fs::write(d.join("f"), b"").unwrap();
        assert!(matches!(check_dir(&d.join("f")), DirState::Bad(w) if w.contains("is a file, not a folder")));
        assert!(matches!(check_dir(Path::new("rel")), DirState::Bad(w) if w.contains("full path")));
        assert!(matches!(check_dir(Path::new("")), DirState::Bad(_)));
        #[cfg(unix)]
        if unsafe { libc::geteuid() } != 0 {
            use std::os::unix::fs::PermissionsExt as _;
            let ro = d.join("ro");
            std::fs::create_dir(&ro).unwrap();
            std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o500)).unwrap();
            assert!(matches!(check_dir(&ro), DirState::Bad(w) if w.contains("not allowed to write")));
            assert!(matches!(check_dir(&ro.join("x")), DirState::Bad(w) if w.contains("not allowed to create")));
            std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn paths_expand_like_the_loader() {
        let look = |n: &str| (n == "AppDir").then(|| "/opt/rg".to_owned());
        assert_eq!(expand_path("${MainDir}/dst", "/data", &look), PathBuf::from("/data/dst"));
        assert_eq!(expand_path("dst", "/data", &look), PathBuf::from("/data/dst"));
        assert_eq!(expand_path("${AppDir}/webui", "/data", &look), PathBuf::from("/opt/rg/webui"));
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(expand_path("~/x", "", &look), PathBuf::from(format!("{}/x", home.trim_end_matches('/'))));
        }
    }

    #[test]
    fn a_first_run_config_has_safe_defaults_and_loads() {
        let text = first_run_config(6790, "examplePassw0rd");
        let entries = crate::configfile::script_config_parse_bytes(text.as_bytes());
        let get = |n: &str| {
            entries
                .iter()
                .find(|(k, _)| k.to_bytes() == n.as_bytes())
                .map(|(_, v)| v.to_str().unwrap().to_owned())
        };
        assert_eq!(get("MainDir").as_deref(), Some("~/RustyGet"));
        assert_eq!(get("ControlIP").as_deref(), Some("127.0.0.1"));
        assert_eq!(get("ControlPort").as_deref(), Some("6790"));
        assert_eq!(get("ControlPassword").as_deref(), Some("examplePassw0rd"));
        assert_eq!(get("FormAuth").as_deref(), Some("yes"));
        assert_eq!(get("WebDir").as_deref(), Some(""));
        assert_eq!(get("Server1.Host").as_deref(), Some(""));
        assert_eq!(get("Server1.Password").as_deref(), Some(""));
        assert_eq!(get("Server1.Active").as_deref(), Some("no"));
        // the same number of options as the template
        assert_eq!(entries.len(), crate::configfile::script_config_parse_bytes(TEMPLATE.as_bytes()).len());
        // and every stock value passes the save checks (a description that
        // reads like a choice list must not reject its own default)
        let pairs: Vec<(Vec<u8>, Vec<u8>)> = entries
            .iter()
            .map(|(k, v)| (k.to_bytes().to_vec(), v.to_bytes().to_vec()))
            .filter(|(k, _)| !k.ends_with(b"Dir") && k != b"LogFile")
            .collect();
        let problems = validate(&pairs, &|_| None);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_new_config_file_is_private_and_never_overwrites() {
        let d = tmp("create");
        let f = d.join("deep/er/rustyget.conf");
        create_config_file(&f, "MainDir=/x\n").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "MainDir=/x\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(create_config_file(&f, "other").is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "MainDir=/x\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_free_port_is_picked() {
        let held = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = held.local_addr().unwrap().port();
        if port < 65000 {
            let got = pick_control_port("127.0.0.1", port);
            assert_ne!(got, port);
            assert!(got > port && got < port + 100);
        }
    }

    #[test]
    fn backups_are_made_kept_to_ten_plus_the_first_and_restored() {
        let d = tmp("backup");
        let conf = d.join("rustyget.conf");
        let dirs = vec![d.clone()];
        assert_eq!(backup_config_in(&conf, &dirs).unwrap(), None);
        // a copy made by hand is never pruned, listed or restored
        std::fs::write(d.join("rustyget.conf.bak-original"), b"mine").unwrap();
        for i in 0..13 {
            std::fs::write(&conf, format!("NzbDirInterval={i}\n")).unwrap();
            let b = backup_config_in(&conf, &dirs).unwrap().unwrap();
            assert!(is_backup_name("rustyget.conf.bak-", b.file_name().unwrap().to_str().unwrap()));
        }
        let all = list_backups_in(&conf, &dirs);
        assert_eq!(all.len(), BACKUPS_KEPT + 1);
        assert_eq!(std::fs::read_to_string(&all[0]).unwrap(), "NzbDirInterval=0\n", "the first one is kept");
        assert_eq!(std::fs::read_to_string(&all[1]).unwrap(), "NzbDirInterval=3\n");
        assert_eq!(std::fs::read_to_string(all.last().unwrap()).unwrap(), "NzbDirInterval=12\n");
        assert_eq!(std::fs::read(d.join("rustyget.conf.bak-original")).unwrap(), b"mine");
        let leftovers = std::fs::read_dir(&d).unwrap().filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("saving")).count();
        assert_eq!(leftovers, 0, "backups are written to a temporary file and renamed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(std::fs::metadata(&all[1]).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // restore one: the current text is backed up first
        let name = all[1].file_name().unwrap().to_str().unwrap().to_owned();
        std::fs::write(&conf, "NzbDirInterval=99\n").unwrap();
        restore_backup_in(&conf, &name, &dirs).unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), "NzbDirInterval=3\n");
        let after = list_backups_in(&conf, &dirs);
        assert_eq!(std::fs::read_to_string(after.last().unwrap()).unwrap(), "NzbDirInterval=99\n");
        assert!(restore_backup_in(&conf, "../etc/passwd", &dirs).is_err());
        assert!(restore_backup_in(&conf, "other.conf", &dirs).is_err());
        assert!(restore_backup_in(&conf, "rustyget.conf.bak-original", &dirs).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_temporary_write_never_touches_the_file() {
        // the temporary file can't be written (here: a folder is in the way,
        // as a full disk would fail the write): the old file stays whole, and
        // there is no fallback to writing in place
        let d = tmp("tmpfail");
        let conf = d.join("c.conf");
        std::fs::write(&conf, "NzbDirInterval=1\n").unwrap();
        FAIL_NEXT_TEMP_WRITE.store(true, std::sync::atomic::Ordering::SeqCst);
        let err = write_config_atomically(&conf, b"NzbDirInterval=2\n").unwrap_err();
        assert!(err.contains("No space left") || err.contains("space"), "{err}");
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), "NzbDirInterval=1\n");
        let left = std::fs::read_dir(&d).unwrap().filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("saving")).count();
        assert_eq!(left, 0, "the temporary file is removed");
        write_config_atomically(&conf, b"NzbDirInterval=2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), "NzbDirInterval=2\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn backup_names_are_recognized_exactly() {
        let p = "rustyget.conf.bak-";
        assert!(is_backup_name(p, "rustyget.conf.bak-20261123-101010"));
        assert!(is_backup_name(p, "rustyget.conf.bak-20261123-101010-12"));
        for bad in ["rustyget.conf.bak-original", "rustyget.conf.bak-2026112-101010", "rustyget.conf.bak-20261123-101010-",
            "rustyget.conf.bak-20261123-101010x", "other.conf.bak-20261123-101010", "rustyget.conf.bak-20261123_101010"] {
            assert!(!is_backup_name(p, bad), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn backups_go_elsewhere_and_saving_still_works_when_the_folder_is_read_only() {
        use std::os::unix::fs::PermissionsExt as _;
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let d = tmp("rodir");
        let cfgdir = d.join("cfg");
        let queue = d.join("queue");
        std::fs::create_dir_all(&cfgdir).unwrap();
        std::fs::create_dir_all(&queue).unwrap();
        let conf = cfgdir.join("rustyget.conf");
        std::fs::write(&conf, "NzbDirInterval=1\n").unwrap();
        std::fs::set_permissions(&cfgdir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let dirs = vec![cfgdir.clone(), queue.clone()];
        let b = backup_config_in(&conf, &dirs).unwrap().unwrap();
        assert!(b.starts_with(&queue), "{}", b.display());
        // the folder can't take a temporary file: the file is written in place
        write_config_atomically(&conf, b"NzbDirInterval=2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), "NzbDirInterval=2\n");
        // a read-only file is never replaced
        std::fs::set_permissions(&cfgdir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&conf, std::fs::Permissions::from_mode(0o400)).unwrap();
        let err = write_config_atomically(&conf, b"NzbDirInterval=3\n").unwrap_err();
        assert!(err.contains("read-only"), "{err}");
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), "NzbDirInterval=2\n");
        assert!(config_writable(Some(&conf)).unwrap_err().contains("read-only"));
        std::fs::set_permissions(&conf, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(config_writable(Some(&conf)).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unchanged_folder_is_not_checked_again() {
        let d = tmp("nas");
        std::fs::write(d.join("file"), b"x").unwrap();
        let unmounted = d.join("file").join("nas").to_str().unwrap().to_owned();
        let entries = vec![e("DestDir", &unmounted), e("NzbDirInterval", "7")];
        let same = |n: &str| (n == "DestDir").then(|| unmounted.clone());
        assert!(validate_detailed(&entries, &none, &same).is_empty(), "a NAS that is away blocks nothing");
        let p = validate_detailed(&entries, &none, &|_| None);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].0, "DestDir");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn random_passwords_are_long_and_different() {
        let a = random_password();
        let b = random_password();
        assert_eq!(a.len(), 20);
        assert!(a.bytes().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, b);
    }

    #[cfg(unix)]
    #[test]
    fn an_empty_mount_point_is_not_written_to() {
        let d = tmp("mnt");
        let root = format!("{}/", d.display());
        let empty = d.join("usb");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(looks_unmounted_dir(&empty, &[root.as_str()]).is_some());
        std::fs::write(empty.join("photo.jpg"), b"x").unwrap();
        assert!(looks_unmounted_dir(&empty, &[root.as_str()]).is_none(), "a folder with files is in use");
        assert!(looks_unmounted_dir(&d, &[root.as_str()]).is_none(), "the root itself");
        assert!(looks_unmounted(Path::new("/tmp/rg-not-a-mount/x.conf")).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn old_mac_line_ends_and_comments_with_cr_load() {
        let d = tmp("cr");
        let msgs = load_messages(&d, "mac.conf", b"# comment\rArticleTimeout=33\rNzbDirInterval=4\r");
        assert!(!msgs.iter().any(|m| m.contains("mac.conf:")), "{msgs:?}");
        let parsed = crate::configfile::script_config_parse_bytes(b"ArticleTimeout=33\rNzbDirInterval=4\r");
        assert_eq!(parsed.len(), 2);
        let merged = crate::configfile::script_config_merge_bytes(b"# a\rb comment\nArticleTimeout=1\n", &[(b"ArticleTimeout", b"2")]);
        assert_eq!(merged, b"# a\rb comment\nArticleTimeout=2\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn writing_replaces_the_file_and_keeps_its_mode() {
        let d = tmp("atomic");
        let conf = d.join("c.conf");
        std::fs::write(&conf, "old\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&conf, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        let link = d.join("link.conf");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&conf, &link).unwrap();
        #[cfg(unix)]
        write_config_atomically(&link, b"new\n").unwrap();
        #[cfg(not(unix))]
        write_config_atomically(&conf, b"new\n").unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), "new\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
            assert_eq!(std::fs::metadata(&conf).unwrap().permissions().mode() & 0o777, 0o640);
        }
        let leftovers: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("saving"))
            .collect();
        assert!(leftovers.is_empty());
        assert!(write_config_atomically(&d, b"x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Loads `text` as a configuration file (no disk access besides reading
    /// it) and returns every message the options reported.
    fn load_messages(dir: &Path, name: &str, text: &[u8]) -> Vec<String> {
        use crate::options::{OptMsg, OptionsListener, OptionsSource};
        struct Collect(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        impl OptionsListener for Collect {
            fn message(&mut self, _kind: OptMsg, text: &CStr) {
                self.0
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(text.to_bytes()).into_owned());
            }
        }
        let file = dir.join(name);
        std::fs::write(&file, text).unwrap();
        let path = CString::new(file.to_str().unwrap()).unwrap();
        let msgs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let opts = crate::options::new_options(
            OptionsSource::Given {
                exe_name: b"rustyget",
                config_file: Some(&path),
                no_config: false,
                cmd_opts: &[],
            },
            true,
            0,
            c"1.0",
            Some(Box::new(Collect(msgs.clone()))),
        );
        let out = msgs.lock().unwrap().clone();
        assert!(!opts.has_fatal_error(), "a readable file is never fatal: {out:?}");
        crate::options::release_options(opts);
        out
    }

    #[test]
    fn a_terrible_config_reports_each_bad_line_once_with_what_was_used() {
        let d = tmp("terrible");
        let mut text = Vec::new();
        text.extend_from_slice(b"ControlPort=abc\n"); // 1
        text.extend_from_slice(b"Bogus=1\n"); // 2
        text.extend_from_slice(b"garbage line\n"); // 3
        text.extend_from_slice(b"ParCheck=sometimes\n"); // 4
        text.extend_from_slice(b"ArticleTimeout=10\n"); // 5
        text.extend_from_slice(b"ArticleTimeout=20\n"); // 6: duplicate
        text.extend_from_slice(&vec![b'x'; 70_000]); // 7: too long
        text.extend_from_slice(b"\n\xff\xfe\x01junk\n"); // 8: binary
        text.extend_from_slice(b"# comment\n\n   \n"); // 9-11: fine
        let msgs = load_messages(&d, "terrible.conf", &text);
        let on = |n: usize| msgs.iter().filter(|m| m.contains(&format!("terrible.conf:{n}: "))).count();
        let all = msgs.join("\n");
        assert_eq!(on(1), 1, "{all}");
        assert!(all.contains("\"abc\" is not a whole number. Using the default \"6789\" instead"), "{all}");
        assert_eq!(on(2), 1, "{all}");
        assert!(all.contains("RustyGet has no such option"), "{all}");
        assert_eq!(on(3), 1, "{all}");
        assert_eq!(on(4), 1, "{all}");
        assert!(all.contains("Using \"auto\" instead; pick one of: auto, always, force, manual"), "{all}");
        assert_eq!(on(5), 0, "{all}");
        assert_eq!(on(6), 1, "{all}");
        assert!(all.contains("is set again (first on line 5)"), "{all}");
        assert_eq!(on(7), 1, "{all}");
        assert!(all.contains("Line is too long (70000 bytes"), "{all}");
        assert_eq!(on(8), 1, "{all}");
        assert!(!all.contains('\u{fffd}') && !all.contains('\u{1}'), "binary is shown as ?: {all}");
        for n in 9..=11 {
            assert_eq!(on(n), 0, "{all}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn folders_of_an_existing_config_are_never_moved() {
        // nzbget's rule stays: a default folder is expanded with the MainDir
        // of its time, and an explicit value is used as written
        let d = tmp("defaults");
        let file = d.join("partial.conf");
        std::fs::write(&file, "MainDir=/data/rg\nTempDir=/fast/tmp\nQueueDir=/data/rg/queue\n").unwrap();
        let path = CString::new(file.to_str().unwrap()).unwrap();
        let opts = crate::options::new_options(
            crate::options::OptionsSource::Given {
                exe_name: b"rustyget",
                config_file: Some(&path),
                no_config: false,
                cmd_opts: &[],
            },
            true,
            0,
            c"1.0",
            None,
        );
        use crate::options::OptStr;
        assert_eq!(opts.str_bytes(OptStr::QueueDir), b"/data/rg/queue");
        assert_eq!(opts.str_bytes(OptStr::TempDir), b"/fast/tmp");
        let dest = String::from_utf8_lossy(opts.str_bytes(OptStr::DestDir)).into_owned();
        assert!(dest.ends_with("downloads/dst") && !dest.starts_with("/data/rg"), "{dest}");
        crate::options::release_options(opts);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_empty_config_loads_quietly_and_floods_are_capped() {
        let d = tmp("empty");
        assert!(load_messages(&d, "empty.conf", b"").iter().all(|m| !m.contains("empty.conf:")));
        let flood: Vec<u8> = (0..500).flat_map(|i| format!("Junk{i}\n").into_bytes()).collect();
        let msgs = load_messages(&d, "flood.conf", &flood);
        let listed = msgs.iter().filter(|m| m.contains("flood.conf:") && m.contains("ignored")).count();
        assert_eq!(listed, 100, "{}", msgs.len());
        assert!(msgs.iter().any(|m| m.contains("400 more problems in the file were not listed")));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// No-panic property over random configuration files: the loader, the
    /// parser `loadconfig` uses, the merge `saveconfig` uses and the save
    /// check accept any bytes; a merged file loads without line problems.
    #[test]
    fn random_configs_never_panic() {
        let d = tmp("fuzz");
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let pieces: &[&[u8]] = &[
            b"ControlPort=", b"MainDir=", b"Server1.Host=", b"Server7.Port=", b"=", b"\n", b"\r\n", b"\0", b"#",
            b"${", b"}", b"${MainDir}", b"yes", b"no", b"abc", b"-1", b"99999999999999999999", b"\xff\xfe",
            b" ", b"\t", b"Server99999999999999.Port=", b"ParCheck=", b"Category1.Name=", b"Task1.Time=",
            b"Task1.Command=", b"Feed1.Url=", b"OutputMode=", b"[section]", "\u{e9}\u{4e2d}".as_bytes(), b"~/",
            b"DupeHealthPreset=", b"ArticleCache=", b"UMask=", b"ControlIP=", b"Server1.Encryption=",
        ];
        for round in 0..400 {
            let mut text = Vec::new();
            for _ in 0..next() % 80 {
                if next() % 4 == 0 {
                    for _ in 0..next() % 24 {
                        text.push((next() & 0xff) as u8);
                    }
                } else {
                    text.extend_from_slice(pieces[(next() % pieces.len() as u64) as usize]);
                }
            }
            if round % 50 == 0 {
                text.extend(std::iter::repeat_n(b'z', 70_000 + round));
            }
            let parsed = crate::configfile::script_config_parse_bytes(&text);
            let entries: Vec<(Vec<u8>, Vec<u8>)> =
                parsed.iter().map(|(n, v)| (n.to_bytes().to_vec(), v.to_bytes().to_vec())).collect();
            let _ = validate(&entries, &none);
            let pairs: Vec<(&[u8], &[u8])> = entries.iter().map(|(n, v)| (n.as_slice(), v.as_slice())).collect();
            let merged = crate::configfile::script_config_merge_bytes(&text, &pairs);
            let _ = load_messages(&d, "fuzz.conf", &text);
            // what a save writes has no junk lines left
            let again = load_messages(&d, "merged.conf", &merged);
            assert!(
                !again.iter().any(|m| m.contains("not a \"Name=value\" line") || m.contains("too long")),
                "round {round}: {again:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn web_addresses_read_well() {
        assert_eq!(web_address("0.0.0.0", 6789), "http://127.0.0.1:6789/");
        assert_eq!(web_address("192.168.1.5", 80), "http://192.168.1.5:80/");
        assert_eq!(web_address("fe80::1", 1), "http://[fe80::1]:1/");
    }
}
