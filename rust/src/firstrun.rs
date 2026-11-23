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

/// The built-in template with the defaults of a new configuration: MainDir
/// in the user's home, the web interface on this computer only, the given
/// port, no password yet (the setup guide asks for one) and no example news
/// server.
pub fn first_run_config(port: u16) -> String {
    let mut out = String::with_capacity(TEMPLATE.len() + 256);
    out.push_str(
        "# Created by RustyGet on first start. Change it in the web interface\n\
         # (Settings) rather than here: Settings checks every value.\n",
    );
    for line in TEMPLATE.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let replaced = match body.split_once('=') {
            Some(("MainDir", _)) => Some(format!("MainDir={DEFAULT_MAIN_DIR}")),
            Some(("ControlIP", _)) => Some("ControlIP=127.0.0.1".to_owned()),
            Some(("ControlPort", _)) => Some(format!("ControlPort={port}")),
            Some(("ControlPassword", _)) => Some("ControlPassword=".to_owned()),
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

/// Checks the options `saveconfig` was given. Each problem is one sentence
/// that names the option, says what is wrong and what to do. Options it does
/// not know the type of are only checked for characters that would break
/// the file. `lookup` gives the running value of an option the list does not
/// set (for `${...}` in paths).
pub fn validate(entries: &[(Vec<u8>, Vec<u8>)], lookup: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut problems = Vec::new();
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
        if name.trim().is_empty()
            || name.trim() != name
            || name.starts_with('#')
            || raw_name.iter().any(|&b| matches!(b, b'=' | b'\n' | b'\r' | 0))
        {
            problems.push(format!("\"{}\" is not an option name.", show(&name)));
            continue;
        }
        if raw_value.iter().any(|&b| matches!(b, b'\n' | b'\r' | 0)) {
            problems.push(format!("{nice}: the value has a line break in it. Enter it on one line."));
            continue;
        }
        if let Ok(c) = CString::new(raw_name.clone()) {
            let check = crate::options::validate_option_name(&c, &mut || predefined(&name));
            if check == crate::options::NameCheck::Invalid {
                problems.push(format!("\"{}\" is not an option RustyGet knows. Remove it.", show(&name)));
                continue;
            }
        }
        let Some(meta) = options_meta().get(&canonical_name(&name)) else {
            continue;
        };
        match meta.kind() {
            Kind::Choice(choices) => {
                if !choice_ok(&name, &choices, value.trim()) {
                    problems.push(format!(
                        "{nice}: \"{}\" is not one of the choices. Pick one of: {}.",
                        show(&value),
                        choices.join(", ")
                    ));
                }
            }
            Kind::Number { min, max } => match value.trim().parse::<i64>() {
                Err(_) if value.trim().is_empty() => {
                    problems.push(format!("{nice}: enter a number (the default is {}).", meta.default));
                }
                Err(_) => problems.push(format!(
                    "{nice}: \"{}\" is not a whole number. Enter digits only, for example {}.",
                    show(&value),
                    meta.default
                )),
                Ok(n) => {
                    if let (Some(lo), Some(hi)) = (min, max) {
                        if n < lo || n > hi {
                            problems.push(format!("{nice}: {n} is out of range. Enter a number from {lo} to {hi}."));
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
                problems.push(format!(
                    "{}: a news server that is on needs a host name, for example news.example.com. Enter it, or turn the server off.",
                    label(&name)
                ));
            }
        } else if !host_ok(host) {
            problems.push(format!(
                "{}: \"{}\" is not a host name or IP address. Enter only the name, for example news.example.com (no http:// and no spaces).",
                label(&name),
                show(host)
            ));
        }
    }

    // folders
    let main_dir = given("MainDir").or_else(|| lookup("MainDir")).unwrap_or_default();
    let resolve = |n: &str| given(n).or_else(|| lookup(n));
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
                problems.push(format!("{option}: enter a folder. It must not be empty."));
            }
            continue;
        }
        let path = expand_path(&value, if option == "MainDir" { "" } else { &main_dir }, &resolve);
        if let DirState::Bad(why) = check_dir(&path) {
            problems.push(format!("{option} ({}): {why}", path.display()));
        }
    }
    for (raw_name, raw_value) in entries {
        let name = text(raw_name);
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("category") && lower.ends_with(".destdir") && !raw_value.is_empty() {
            let path = expand_path(&text(raw_value), &main_dir, &resolve);
            if let DirState::Bad(why) = check_dir(&path) {
                problems.push(format!("{} ({}): {why}", label(&name), path.display()));
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
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("{secs}")
}

/// The backups of `config`, oldest first.
pub fn list_backups(config: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(prefix)) = (config.parent(), backup_prefix(config)) else {
        return Vec::new();
    };
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, String, PathBuf)> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let meta = e.metadata().ok()?;
            (name.starts_with(&prefix) && meta.is_file())
                .then(|| (meta.modified().unwrap_or(std::time::UNIX_EPOCH), name, e.path()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, _, p)| p).collect()
}

/// Copies `config` to `<config>.bak-<date>-<time>` (readable by its owner
/// only) and removes all but the newest [`BACKUPS_KEPT`]. `Ok(None)`: there
/// was nothing to copy.
pub fn backup_config(config: &Path) -> Result<Option<PathBuf>, String> {
    let bytes = match std::fs::read(config) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("can't read {} to back it up: {}", config.display(), plain_io(&e))),
    };
    let prefix = backup_prefix(config).ok_or("the configuration file has no name")?;
    let stamp = local_stamp();
    let mut target = config.with_file_name(format!("{prefix}{stamp}"));
    let mut n = 2;
    let file = loop {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            opts.mode(0o600);
        }
        match opts.open(&target) {
            Ok(f) => break f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => {
                target = config.with_file_name(format!("{prefix}{stamp}-{n}"));
                n += 1;
            }
            Err(e) => {
                return Err(format!(
                    "can't write the backup copy {}: {}",
                    target.display(),
                    plain_io(&e)
                ))
            }
        }
    };
    let mut file = file;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("can't write the backup copy {}: {}", target.display(), plain_io(&e)))?;
    let all = list_backups(config);
    if all.len() > BACKUPS_KEPT {
        for old in &all[..all.len() - BACKUPS_KEPT] {
            if old != &target {
                let _ = std::fs::remove_file(old);
            }
        }
    }
    Ok(Some(target))
}

/// Replaces `config` with `bytes` in one step (a new file renamed over the
/// old one, so a crash leaves the old or the new file, never half of one).
/// A symbolic link is followed, the file's permissions are kept. Where the
/// rename is not possible (a file mounted on its own, as Docker does), the
/// file is rewritten in place.
pub fn write_config_atomically(config: &Path, bytes: &[u8]) -> Result<(), String> {
    let target = std::fs::canonicalize(config).unwrap_or_else(|_| config.to_path_buf());
    let meta = std::fs::metadata(&target).ok();
    if meta.as_ref().is_some_and(std::fs::Metadata::is_dir) {
        return Err(format!("{} is a folder", target.display()));
    }
    let name = target.file_name().map_or_else(|| "rustyget.conf".into(), |n| n.to_string_lossy().into_owned());
    let tmp = target.with_file_name(format!(".{name}.saving-{}", std::process::id()));
    let written = (|| -> std::io::Result<()> {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            use std::os::unix::fs::PermissionsExt as _;
            opts.mode(meta.as_ref().map_or(0o600, |m| m.permissions().mode() & 0o7777));
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        #[cfg(unix)]
        if let Some(m) = &meta {
            use std::os::unix::fs::PermissionsExt as _;
            f.set_permissions(std::fs::Permissions::from_mode(m.permissions().mode() & 0o7777))?;
        }
        Ok(())
    })();
    if written.is_ok() && std::fs::rename(&tmp, &target).is_ok() {
        return Ok(());
    }
    let _ = std::fs::remove_file(&tmp);
    // in place: the backup already holds the old text
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&target)
        .map_err(|e| format!("can't write {}: {}", target.display(), plain_io(&e)))?;
    f.write_all(bytes)
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("can't write {}: {}", target.display(), plain_io(&e)))
}

/// Puts a backup back: the current file is backed up first (so a restore
/// can be undone the same way), then replaced with the backup. `name` is the
/// backup's file name as [`list_backups`] gives it.
pub fn restore_backup(config: &Path, name: &str) -> Result<(), String> {
    let prefix = backup_prefix(config).ok_or("the configuration file has no name")?;
    if !name.starts_with(&prefix) || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!("\"{}\" is not a backup of this configuration file.", show(name)));
    }
    let source = config.with_file_name(name);
    let bytes = std::fs::read(&source).map_err(|e| format!("can't read the backup {name}: {}", plain_io(&e)))?;
    backup_config(config)?;
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

/// Can the configuration file be saved (and backed up)? The reason when not.
pub fn config_writable(config: Option<&Path>) -> Result<(), String> {
    let Some(config) = config else {
        return Err("RustyGet is running without a configuration file, so settings can't be saved. \
Start it with -c <file> to name one."
            .to_owned());
    };
    match std::fs::metadata(config) {
        Ok(m) if m.is_dir() => return Err(format!("{} is a folder, not a file.", config.display())),
        Ok(_) => {
            if !writable_file(config) {
                return Err(format!(
                    "RustyGet is not allowed to change {}. Change its permissions, or start RustyGet with -c <another file>.",
                    config.display()
                ));
            }
        }
        Err(e) => return Err(format!("{} can't be used: {}.", config.display(), plain_io(&e))),
    }
    let dir = config.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if !writable(dir) {
        return Err(format!(
            "RustyGet is not allowed to write in {}, where it keeps a backup copy before every save. Change the folder's permissions.",
            dir.display()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn writable_file(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
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
/// guide and the banner.
pub fn rpc_setup_status(is_json: bool) -> Vec<u8> {
    let config = config_path();
    let password = running_option("ControlPassword").unwrap_or_default();
    let username = running_option("ControlUsername").unwrap_or_default();
    let servers = usable_servers();
    let paths = path_problems();
    let (notes, dropped, created, problem) =
        with_state(|s| (s.notes.clone(), s.dropped, s.created, s.problem.clone()));
    let writable = config_writable(config.as_deref());
    let mut o = String::with_capacity(1024);
    o.push_str("{\"ConfigFile\":");
    js(&mut o, &config.as_deref().map_or(String::new(), |p| p.display().to_string()));
    o.push_str(&format!(",\"ConfigCreated\":{created}"));
    o.push_str(&format!(",\"ConfigWritable\":{}", writable.is_ok()));
    o.push_str(",\"ConfigProblem\":");
    let problem = if problem.is_empty() { writable.err().unwrap_or_default() } else { problem };
    js(&mut o, &problem);
    o.push_str(&format!(",\"PasswordSet\":{}", !password.is_empty() && password != STOCK_PASSWORD));
    o.push_str(&format!(",\"PasswordIsStock\":{}", password == STOCK_PASSWORD));
    o.push_str(",\"ControlUsername\":");
    js(&mut o, &username);
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
    o.push_str(&format!(
        ",\"SetupComplete\":{}}}",
        servers > 0 && paths.is_empty() && !password.is_empty() && password != STOCK_PASSWORD
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
        let text = first_run_config(6790);
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
        assert_eq!(get("ControlPassword").as_deref(), Some(""));
        assert_eq!(get("WebDir").as_deref(), Some(""));
        assert_eq!(get("Server1.Host").as_deref(), Some(""));
        assert_eq!(get("Server1.Password").as_deref(), Some(""));
        assert_eq!(get("Server1.Active").as_deref(), Some("no"));
        // the same number of options as the template
        assert_eq!(entries.len(), crate::configfile::script_config_parse_bytes(TEMPLATE.as_bytes()).len());
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
    fn backups_are_made_kept_to_ten_and_restored() {
        let d = tmp("backup");
        let conf = d.join("rustyget.conf");
        assert_eq!(backup_config(&conf).unwrap(), None);
        for i in 0..13 {
            std::fs::write(&conf, format!("NzbDirInterval={i}\n")).unwrap();
            let b = backup_config(&conf).unwrap().unwrap();
            assert!(b.file_name().unwrap().to_str().unwrap().starts_with("rustyget.conf.bak-"));
        }
        let all = list_backups(&conf);
        assert_eq!(all.len(), BACKUPS_KEPT);
        assert_eq!(std::fs::read_to_string(all.last().unwrap()).unwrap(), "NzbDirInterval=12\n");
        assert_eq!(std::fs::read_to_string(&all[0]).unwrap(), "NzbDirInterval=3\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(std::fs::metadata(&all[0]).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // restore the oldest kept: the current text is backed up first
        let name = all[0].file_name().unwrap().to_str().unwrap().to_owned();
        std::fs::write(&conf, "NzbDirInterval=99\n").unwrap();
        restore_backup(&conf, &name).unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), "NzbDirInterval=3\n");
        let after = list_backups(&conf);
        assert_eq!(std::fs::read_to_string(after.last().unwrap()).unwrap(), "NzbDirInterval=99\n");
        assert!(restore_backup(&conf, "../etc/passwd").is_err());
        assert!(restore_backup(&conf, "other.conf").is_err());
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
        use std::ffi::{c_char, c_int, c_void};
        unsafe extern "C" fn cb(ctx: *mut c_void, op: c_int, _i: *const c_int, strs: *const *const c_char, n: usize) {
            if n > 0 && !strs.is_null() && !(*strs).is_null() && (0..=3).contains(&op) {
                let v = &mut *(ctx as *mut Vec<String>);
                v.push(String::from_utf8_lossy(CStr::from_ptr(*strs).to_bytes()).into_owned());
            }
        }
        let file = dir.join(name);
        std::fs::write(&file, text).unwrap();
        let path = CString::new(file.to_str().unwrap()).unwrap();
        let mut msgs: Vec<String> = Vec::new();
        let mut flags: c_int = 0;
        let handle = unsafe {
            crate::options::nzbget_rs_options_new(
                c"rustyget".as_ptr(),
                path.as_ptr(),
                0,
                1,
                std::ptr::null(),
                0,
                0,
                c"1.0".as_ptr(),
                (&mut msgs as *mut Vec<String>).cast(),
                Some(cb),
                std::ptr::null_mut(),
                &mut flags,
            )
        };
        unsafe { crate::options::nzbget_rs_options_free(handle) };
        assert_eq!(flags & 1, 0, "a readable file is never fatal: {msgs:?}");
        msgs
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
    fn default_folders_follow_the_main_dir_the_file_sets() {
        let d = tmp("defaults");
        let file = d.join("partial.conf");
        std::fs::write(&file, "MainDir=/data/rg\nTempDir=/fast/tmp\n").unwrap();
        let path = CString::new(file.to_str().unwrap()).unwrap();
        let mut flags = 0;
        let handle = unsafe {
            crate::options::nzbget_rs_options_new(
                c"rustyget".as_ptr(),
                path.as_ptr(),
                0,
                1,
                std::ptr::null(),
                0,
                0,
                c"1.0".as_ptr(),
                std::ptr::null_mut(),
                None,
                std::ptr::null_mut(),
                &mut flags,
            )
        };
        let opts = unsafe { &*handle };
        use crate::options::OptStr;
        assert_eq!(opts.str_bytes(OptStr::DestDir), b"/data/rg/dst");
        assert_eq!(opts.str_bytes(OptStr::QueueDir), b"/data/rg/queue");
        assert_eq!(opts.str_bytes(OptStr::TempDir), b"/fast/tmp");
        assert_eq!(opts.str_bytes(OptStr::LogFile), b"/data/rg/rustyget.log");
        unsafe { crate::options::nzbget_rs_options_free(handle) };
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
