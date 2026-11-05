//! System health configuration and filesystem validation engine
//! (`daemon/systemhealth/`).
//!
//! Replaces the C++ `SystemHealth` validators (`Validators.cpp`, `Status.cpp`,
//! `SectionValidator.cpp`, `SectionGroupValidator.cpp`, `PathsValidator.cpp`,
//! `CategoriesValidator.cpp`, `CategoryValidator.cpp`, `DisplayValidator.cpp`,
//! `LoggingValidator.cpp`, `SchedulerTasksValidator.cpp`, `SchedulerTaskValidator.cpp`,
//! `FeedsValidator.cpp`, `FeedValidator.cpp`, `IncomingNzbValidator.cpp`,
//! `UnpackValidator.cpp`, `CheckAndRepairValidator.cpp`, `DownloadQueueValidator.cpp`)
//! with native Rust implementations.

use std::ffi::CStr;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;

/// Mirrors `SystemHealth::Severity`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Ok = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
}

impl Severity {
    pub fn from_i32(val: i32) -> Self {
        match val {
            1 => Self::Info,
            2 => Self::Warning,
            3 => Self::Error,
            _ => Self::Ok,
        }
    }

    pub fn as_cstr(self) -> &'static CStr {
        match self {
            Self::Ok => c"Ok",
            Self::Info => c"Info",
            Self::Warning => c"Warning",
            Self::Error => c"Error",
        }
    }
}

/// Mirrors `SystemHealth::Status`, preserving raw byte messages without requiring UTF-8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthStatus {
    pub severity: Severity,
    pub message: Vec<u8>,
}

impl HealthStatus {
    pub fn ok() -> Self {
        Self {
            severity: Severity::Ok,
            message: Vec::new(),
        }
    }

    pub fn info(msg: impl Into<Vec<u8>>) -> Self {
        Self {
            severity: Severity::Info,
            message: msg.into(),
        }
    }

    pub fn warning(msg: impl Into<Vec<u8>>) -> Self {
        Self {
            severity: Severity::Warning,
            message: msg.into(),
        }
    }

    pub fn error(msg: impl Into<Vec<u8>>) -> Self {
        Self {
            severity: Severity::Error,
            message: msg.into(),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.severity == Severity::Ok
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub fn and_then(self, f: impl FnOnce() -> Self) -> Self {
        if self.is_ok() { f() } else { self }
    }
}

/// Helper to emit only non-Ok option statuses into a section report.
fn emit_if_not_ok(
    out: &mut impl FnMut(&str, Severity, &[u8]),
    name: &str,
    status: HealthStatus,
) {
    if !status.is_ok() {
        out(name, status.severity, &status.message);
    }
}

fn concat_bytes(parts: &[&[u8]]) -> Vec<u8> {
    let total: usize = parts.iter().map(|p| p.len()).sum();
    let mut out = Vec::with_capacity(total);
    for part in parts {
        out.extend_from_slice(part);
    }
    out
}

fn quoted_path_bytes(path: &Path) -> Vec<u8> {
    let raw = crate::cleanup::os_str_to_bytes(path.as_os_str());
    let mut out = Vec::with_capacity(raw.len() + 2);
    out.push(b'"');
    for &b in raw {
        if b == b'"' || b == b'\\' {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b'"');
    out
}

// ============================================================================
// Core Validators (`daemon/systemhealth/Validators.cpp`)
// ============================================================================

pub fn required_option(name: &[u8], value: &[u8]) -> HealthStatus {
    if value.is_empty() {
        HealthStatus::error(concat_bytes(&[
            b"'",
            name,
            b"' is required and cannot be empty",
        ]))
    } else {
        HealthStatus::ok()
    }
}

pub fn required_path_option(name: &[u8], value: &Path) -> HealthStatus {
    if value.as_os_str().is_empty() {
        HealthStatus::error(concat_bytes(&[
            b"'",
            name,
            b"' is required and cannot be empty",
        ]))
    } else {
        HealthStatus::ok()
    }
}

pub fn unique_path(name: &[u8], path: &Path, others: &[(&[u8], &Path)]) -> HealthStatus {
    if path.as_os_str().is_empty() {
        return HealthStatus::ok();
    }
    for &(other_name, other_path) in others {
        if other_path == path {
            return HealthStatus::warning(concat_bytes(&[
                b"'",
                name,
                b"' and '",
                other_name,
                b"' are identical that can lead to unexpected behavior",
            ]));
        }
    }
    HealthStatus::ok()
}

pub fn check_password(password: &[u8]) -> HealthStatus {
    if password.is_empty() {
        HealthStatus::warning("Password is set to empty")
    } else if password.len() < 8 {
        HealthStatus::info("Password is too short (recommended minimum 8 characters)")
    } else {
        HealthStatus::ok()
    }
}

pub fn check_positive_num(name: &[u8], value: i32) -> HealthStatus {
    if value < 0 {
        HealthStatus::error(concat_bytes(&[
            b"'",
            name,
            b"' option must not be negative",
        ]))
    } else {
        HealthStatus::ok()
    }
}

pub fn file_exists(path: &Path) -> HealthStatus {
    let qp = quoted_path_bytes(path);
    match std::fs::metadata(path) {
        Ok(meta) => {
            if !meta.is_file() {
                HealthStatus::error(concat_bytes(&[&qp, b" exists but is not a regular file"]))
            } else {
                HealthStatus::ok()
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            HealthStatus::error(concat_bytes(&[&qp, b" file doesn't exist"]))
        }
        Err(e) => {
            let err_str = e.to_string();
            HealthStatus::error(concat_bytes(&[
                b"Failed to check ",
                &qp,
                b": ",
                err_str.as_bytes(),
            ]))
        }
    }
}

pub fn file_readable(path: &Path) -> HealthStatus {
    match std::fs::File::open(path) {
        Ok(_) => HealthStatus::ok(),
        Err(e) => {
            let qp = quoted_path_bytes(path);
            let err_str = e.to_string();
            HealthStatus::error(concat_bytes(&[
                b"Failed to read ",
                &qp,
                b": ",
                err_str.as_bytes(),
            ]))
        }
    }
}

pub fn file_writable(path: &Path) -> HealthStatus {
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(_) => HealthStatus::ok(),
        Err(e) => {
            let qp = quoted_path_bytes(path);
            let err_str = e.to_string();
            HealthStatus::error(concat_bytes(&[
                b"Failed to write to ",
                &qp,
                b": ",
                err_str.as_bytes(),
            ]))
        }
    }
}

pub fn file_executable(path: &Path) -> HealthStatus {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        extern "C" {
            fn access(
                pathname: *const std::os::raw::c_char,
                mode: std::os::raw::c_int,
            ) -> std::os::raw::c_int;
        }
        const X_OK: std::os::raw::c_int = 1;
        let qp = quoted_path_bytes(path);
        if let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) {
            let rc = unsafe { access(c_path.as_ptr(), X_OK) };
            if rc == 0 {
                return HealthStatus::ok();
            }
            let err_str = std::io::Error::last_os_error().to_string();
            return HealthStatus::error(concat_bytes(&[
                &qp,
                b" is not executable: ",
                err_str.as_bytes(),
            ]));
        }
        HealthStatus::error(concat_bytes(&[&qp, b" is not executable"]))
    }
    #[cfg(not(unix))]
    {
        file_exists(path)
    }
}

pub fn file_null_or_console_device(path: &Path) -> bool {
    let raw = crate::cleanup::os_str_to_bytes(path.as_os_str());
    matches!(raw, b"/dev/null" | b"/dev/stdout" | b"/dev/stderr")
}

pub fn directory_exists(path: &Path) -> HealthStatus {
    let qp = quoted_path_bytes(path);
    match std::fs::metadata(path) {
        Ok(meta) => {
            if !meta.is_dir() {
                HealthStatus::error(concat_bytes(&[&qp, b" is not a directory"]))
            } else {
                HealthStatus::ok()
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            HealthStatus::error(concat_bytes(&[&qp, b" directory does not exist"]))
        }
        Err(e) => {
            let err_str = e.to_string();
            HealthStatus::error(concat_bytes(&[
                b"Failed to check ",
                &qp,
                b": ",
                err_str.as_bytes(),
            ]))
        }
    }
}

pub fn directory_readable(path: &Path) -> HealthStatus {
    match std::fs::read_dir(path) {
        Ok(_) => HealthStatus::ok(),
        Err(e) => {
            let qp = quoted_path_bytes(path);
            let err_str = e.to_string();
            HealthStatus::error(concat_bytes(&[
                b"Failed to read ",
                &qp,
                b" directory: ",
                err_str.as_bytes(),
            ]))
        }
    }
}

pub fn directory_writable(path: &Path) -> HealthStatus {
    let test_path = path.join("nzbget_write_test.tmp");
    let mut file = match std::fs::File::create(&test_path) {
        Ok(f) => f,
        Err(e) => {
            let qp = quoted_path_bytes(path);
            let err_str = e.to_string();
            return HealthStatus::error(concat_bytes(&[
                b"Failed to write to ",
                &qp,
                b": ",
                err_str.as_bytes(),
            ]));
        }
    };
    if let Err(e) = std::io::Write::write_all(&mut file, b"Write test") {
        drop(file);
        let _ = std::fs::remove_file(&test_path);
        let qp = quoted_path_bytes(path);
        let err_str = e.to_string();
        return HealthStatus::error(concat_bytes(&[
            b"Failed to write content to ",
            &qp,
            b": ",
            err_str.as_bytes(),
        ]));
    }
    drop(file);
    if let Err(e) = std::fs::remove_file(&test_path) {
        let qp = quoted_path_bytes(path);
        let err_str = e.to_string();
        return HealthStatus::warning(concat_bytes(&[
            b"Writable check failed during cleanup for ",
            &qp,
            b": ",
            err_str.as_bytes(),
        ]));
    }
    HealthStatus::ok()
}

fn is_valid_hostname_syntax(hostname: &[u8]) -> bool {
    if hostname.is_empty() {
        return false;
    }
    let is_end_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let is_mid_char = |b: u8| is_end_char(b) || b == b'-';
    for label in hostname.split(|&b| b == b'.') {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        if !is_end_char(label[0]) || !is_end_char(label[label.len() - 1]) {
            return false;
        }
        if label.len() > 2 && !label[1..label.len() - 1].iter().all(|&b| is_mid_char(b)) {
            return false;
        }
    }
    true
}

pub fn network_valid_host(hostname: &[u8], ipv: i32) -> HealthStatus {
    if hostname.is_empty() {
        return HealthStatus::error("Hostname or IP address is empty");
    }
    if hostname.len() > 255 {
        return HealthStatus::error("Hostname is too long (maximum 255 characters)");
    }
    let (has_brackets, ip_bytes) =
        if hostname.len() >= 2 && hostname[0] == b'[' && hostname[hostname.len() - 1] == b']' {
            (true, &hostname[1..hostname.len() - 1])
        } else {
            (false, hostname)
        };

    let cstr_slice = match ip_bytes.iter().position(|&b| b == 0) {
        Some(nul_pos) => &ip_bytes[..nul_pos],
        None => ip_bytes,
    };
    let is_v4 = std::str::from_utf8(cstr_slice)
        .ok()
        .and_then(|s| s.parse::<Ipv4Addr>().ok())
        .is_some();

    let v6_addr_slice = match cstr_slice.iter().position(|&b| b == b'%') {
        Some(pct_pos) => &cstr_slice[..pct_pos],
        None => cstr_slice,
    };
    let is_v6 = std::str::from_utf8(v6_addr_slice)
        .ok()
        .and_then(|s| s.parse::<Ipv6Addr>().ok())
        .is_some();

    if is_v4 || is_v6 {
        if ipv == 4 && is_v6 {
            return HealthStatus::error(concat_bytes(&[
                b"IPv6 address '",
                ip_bytes,
                b"' is provided, but connection mode is set to IPv4",
            ]));
        }
        if ipv == 6 && is_v4 {
            return HealthStatus::error(concat_bytes(&[
                b"IPv4 address '",
                ip_bytes,
                b"' is provided, but connection mode is set to IPv6",
            ]));
        }
        if has_brackets {
            return HealthStatus::error(concat_bytes(&[
                b"Remove brackets from the IP address, e.g., '",
                ip_bytes,
                b"'",
            ]));
        }
        return HealthStatus::ok();
    }

    if !has_brackets && is_valid_hostname_syntax(hostname) {
        return HealthStatus::ok();
    }
    HealthStatus::error(concat_bytes(&[
        b"Invalid hostname or IP address: '",
        hostname,
        b"'",
    ]))
}

pub fn network_valid_port(port: i32) -> HealthStatus {
    if (1..=65535).contains(&port) {
        HealthStatus::ok()
    } else {
        HealthStatus::error("Port must be between 1 and 65535")
    }
}

// ============================================================================
// PathsValidator (`daemon/systemhealth/PathsValidator.cpp`)
// ============================================================================

pub fn validate_required_writable_dir(name: &[u8], path: &Path) -> HealthStatus {
    required_path_option(name, path)
        .and_then(|| directory_exists(path))
        .and_then(|| directory_writable(path))
}

pub fn validate_inter_dir_path(path: &Path) -> HealthStatus {
    if path.as_os_str().is_empty() {
        return HealthStatus::warning(
            "'InterDir' is set to empty which is not recommended for optimal unpack performance",
        );
    }
    directory_exists(path).and_then(|| directory_writable(path))
}

pub fn validate_web_dir_path(path: &Path) -> HealthStatus {
    if path.as_os_str().is_empty() {
        return HealthStatus::ok();
    }
    directory_exists(path).and_then(|| directory_readable(path))
}

pub fn validate_log_file_path(path: &Path, write_log: i32) -> HealthStatus {
    // Options::EWriteLog: wlNone = 0, wlAppend = 1, wlReset = 2, wlRotate = 3
    if write_log == 0 {
        return HealthStatus::ok();
    }
    if path.as_os_str().is_empty() {
        return HealthStatus::warning("Logging is enabled, but 'LogFile' is set to empty");
    }
    if file_null_or_console_device(path) {
        return HealthStatus::ok();
    }
    if write_log == 2 || write_log == 3 {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                return directory_exists(parent).and_then(|| directory_writable(parent));
            }
        }
    }
    file_exists(path).and_then(|| file_writable(path))
}

pub fn validate_cert_store_path(path: &Path, cert_check: bool) -> HealthStatus {
    if path.as_os_str().is_empty() {
        if !cert_check {
            return HealthStatus::ok();
        }
        return HealthStatus::error(
            "'CertCheck' requires proper configuration of option 'CertStore'",
        );
    }
    if !file_exists(path).is_error() {
        return file_readable(path);
    }
    if !directory_exists(path).is_error() {
        return directory_readable(path);
    }
    HealthStatus::error("'CertStore' must be a file or a directory")
}

pub fn validate_lock_file_path(path: &Path, daemon_mode: bool) -> HealthStatus {
    if !daemon_mode {
        return HealthStatus::ok();
    }
    if path.as_os_str().is_empty() {
        return HealthStatus::warning(
            "'LockFile' is set to empty. The check for another running instance is disabled",
        );
    }
    if file_exists(path).is_ok() {
        return file_writable(path);
    }
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => std::env::current_dir().unwrap_or_default(),
    };
    directory_exists(&parent).and_then(|| directory_writable(&parent))
}

pub struct PathsSectionInput<'a> {
    pub main_dir: &'a Path,
    pub dest_dir: &'a Path,
    pub inter_dir: &'a Path,
    pub nzb_dir: &'a Path,
    pub queue_dir: &'a Path,
    pub web_dir: &'a Path,
    pub temp_dir: &'a Path,
    pub script_dirs: &'a [&'a Path],
    pub config_template: &'a Path,
    pub log_file: &'a Path,
    pub cert_store: &'a Path,
    pub cert_store_path: &'a Path,
    pub config_file: &'a Path,
    pub lock_file: &'a Path,
    pub write_log: i32,
    pub cert_check: bool,
    pub daemon_mode: bool,
}

pub fn validate_paths_section(
    input: &PathsSectionInput<'_>,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    let main_status = validate_required_writable_dir(b"MainDir", input.main_dir);
    emit_if_not_ok(&mut emit, "MainDir", main_status);

    let dest_status = validate_required_writable_dir(b"DestDir", input.dest_dir).and_then(|| {
        unique_path(
            b"DestDir",
            input.dest_dir,
            &[(b"MainDir", input.main_dir)],
        )
    });
    emit_if_not_ok(&mut emit, "DestDir", dest_status);

    let inter_status = validate_inter_dir_path(input.inter_dir).and_then(|| {
        unique_path(
            b"InterDir",
            input.inter_dir,
            &[
                (b"MainDir", input.main_dir),
                (b"DestDir", input.dest_dir),
            ],
        )
    });
    emit_if_not_ok(&mut emit, "InterDir", inter_status);

    let nzb_status = validate_required_writable_dir(b"NzbDir", input.nzb_dir).and_then(|| {
        unique_path(
            b"NzbDir",
            input.nzb_dir,
            &[
                (b"MainDir", input.main_dir),
                (b"DestDir", input.dest_dir),
                (b"InterDir", input.inter_dir),
            ],
        )
    });
    emit_if_not_ok(&mut emit, "NzbDir", nzb_status);

    let queue_status = validate_required_writable_dir(b"QueueDir", input.queue_dir).and_then(|| {
        unique_path(
            b"QueueDir",
            input.queue_dir,
            &[
                (b"MainDir", input.main_dir),
                (b"DestDir", input.dest_dir),
                (b"InterDir", input.inter_dir),
                (b"NzbDir", input.nzb_dir),
            ],
        )
    });
    emit_if_not_ok(&mut emit, "QueueDir", queue_status);

    let web_status = validate_web_dir_path(input.web_dir).and_then(|| {
        if input.web_dir.as_os_str().is_empty() {
            return HealthStatus::ok();
        }
        unique_path(
            b"WebDir",
            input.web_dir,
            &[
                (b"MainDir", input.main_dir),
                (b"DestDir", input.dest_dir),
                (b"InterDir", input.inter_dir),
                (b"NzbDir", input.nzb_dir),
                (b"QueueDir", input.queue_dir),
            ],
        )
    });
    emit_if_not_ok(&mut emit, "WebDir", web_status);

    let temp_status = validate_required_writable_dir(b"TempDir", input.temp_dir).and_then(|| {
        unique_path(
            b"TempDir",
            input.temp_dir,
            &[
                (b"MainDir", input.main_dir),
                (b"DestDir", input.dest_dir),
                (b"InterDir", input.inter_dir),
                (b"NzbDir", input.nzb_dir),
                (b"QueueDir", input.queue_dir),
                (b"WebDir", input.web_dir),
            ],
        )
    });
    emit_if_not_ok(&mut emit, "TempDir", temp_status);

    let script_status = if input.script_dirs.is_empty() {
        HealthStatus::warning("'ScriptDir' is empty. Extensions cannot be found or installed")
    } else {
        let mut status = HealthStatus::ok();
        for &dir in input.script_dirs {
            let s = validate_required_writable_dir(b"ScriptDir", dir).and_then(|| {
                unique_path(
                    b"ScriptDir",
                    dir,
                    &[
                        (b"MainDir", input.main_dir),
                        (b"DestDir", input.dest_dir),
                        (b"InterDir", input.inter_dir),
                        (b"NzbDir", input.nzb_dir),
                        (b"QueueDir", input.queue_dir),
                        (b"WebDir", input.web_dir),
                        (b"TempDir", input.temp_dir),
                    ],
                )
            });
            if !s.is_ok() {
                status = s;
                break;
            }
        }
        status
    };
    emit_if_not_ok(&mut emit, "ScriptDir", script_status);

    let log_status = validate_log_file_path(input.log_file, input.write_log).and_then(|| {
        unique_path(
            b"LogFile",
            input.log_file,
            &[
                (b"ConfigTemplate", input.config_template),
                (b"ConfigFile", input.config_file),
            ],
        )
    });
    emit_if_not_ok(&mut emit, "LogFile", log_status);

    let cert_status =
        validate_cert_store_path(input.cert_store, input.cert_check).and_then(|| {
            unique_path(
                b"CertStore",
                input.cert_store_path,
                &[
                    (b"MainDir", input.main_dir),
                    (b"DestDir", input.dest_dir),
                    (b"InterDir", input.inter_dir),
                    (b"NzbDir", input.nzb_dir),
                    (b"QueueDir", input.queue_dir),
                    (b"WebDir", input.web_dir),
                    (b"TempDir", input.temp_dir),
                    (b"ConfigTemplate", input.config_template),
                    (b"LogFile", input.log_file),
                    (b"ConfigFile", input.config_file),
                ],
            )
        });
    emit_if_not_ok(&mut emit, "CertStore", cert_status);

    let lock_status =
        validate_lock_file_path(input.lock_file, input.daemon_mode).and_then(|| {
            unique_path(
                b"LockFile",
                input.lock_file,
                &[
                    (b"MainDir", input.main_dir),
                    (b"ConfigTemplate", input.config_template),
                    (b"LogFile", input.log_file),
                    (b"CertStore", input.cert_store_path),
                    (b"ConfigFile", input.config_file),
                ],
            )
        });
    emit_if_not_ok(&mut emit, "LockFile", lock_status);
}

// ============================================================================
// CategoryValidator & CategoriesValidator (`daemon/systemhealth/CategoriesValidator.cpp`)
// ============================================================================

pub fn validate_category_section(
    name: &[u8],
    global_unpack: bool,
    cat_unpack: bool,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    if name.is_empty() {
        emit(
            "Name",
            Severity::Warning,
            b"Category name is empty. Categories should have a name",
        );
    }
    if global_unpack && !cat_unpack {
        emit(
            "Unpack",
            Severity::Info,
            b"Unpack is disabled. Files will remain packed after download",
        );
    } else if !global_unpack && cat_unpack {
        emit(
            "Unpack",
            Severity::Info,
            b"Global Unpack is disabled, so this category setting has no effect",
        );
    }
}

pub fn validate_duplicate_category_names(names: &[&[u8]]) -> HealthStatus {
    for (i, &n) in names.iter().enumerate() {
        if n.is_empty() {
            continue;
        }
        if names[..i].contains(&n) {
            return HealthStatus::warning(concat_bytes(&[
                b"Duplicate category name detected: '",
                n,
                b"'",
            ]));
        }
    }
    HealthStatus::ok()
}

// ============================================================================
// DisplayValidator (`daemon/systemhealth/DisplayValidator.cpp`)
// ============================================================================

pub fn validate_display_section(
    output_mode: i32,
    update_interval: i32,
    curses_nzb_name: bool,
    curses_group: bool,
    curses_time: bool,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    // Options::EOutputMode: omLoggable = 0, omColored = 1, omNCurses = 2
    if !matches!(output_mode, 0..=2) {
        emit(
            "OutputMode",
            Severity::Error,
            b"'OutputMode' has an invalid value",
        );
    }
    if update_interval != 0 && update_interval < 25 {
        emit(
            "UpdateInterval",
            Severity::Error,
            b"'UpdateInterval' must be >= 25 ms.",
        );
    }
    if output_mode != 2 && curses_nzb_name {
        emit(
            "CursesNzbName",
            Severity::Info,
            b"'CursesNzbName' applies only when OutputMode=curses",
        );
    }
    if output_mode != 2 && curses_group {
        emit(
            "CursesGroup",
            Severity::Info,
            b"'CursesGroup' applies only when OutputMode=curses",
        );
    }
    if output_mode != 2 && curses_time {
        emit(
            "CursesTime",
            Severity::Info,
            b"'CursesTime' applies only when OutputMode=curses",
        );
    }
}

// ============================================================================
// LoggingValidator (`daemon/systemhealth/LoggingValidator.cpp`)
// ============================================================================

pub fn validate_write_log(write_log: i32, log_file_path: &Path) -> HealthStatus {
    // Options::EWriteLog: wlNone = 0, wlAppend = 1, wlReset = 2, wlRotate = 3
    if write_log == 1 && !file_null_or_console_device(log_file_path) {
        HealthStatus::warning("'WriteLog' is set to 'Append'. The log file may grow indefinitely")
    } else {
        HealthStatus::ok()
    }
}

pub fn validate_rotate_log(write_log: i32, rotate_log: i32) -> HealthStatus {
    if write_log != 3 {
        return HealthStatus::ok();
    }
    if rotate_log < 0 {
        return check_positive_num(b"RotateLog", rotate_log);
    }
    if rotate_log > 365 {
        return HealthStatus::warning(
            "'RotateLog' is very high (>365). This may consume significant disk space",
        );
    }
    HealthStatus::ok()
}

pub fn validate_logging_section(
    write_log: i32,
    log_file_path: &Path,
    rotate_log: i32,
    log_buffer: i32,
    crash_dump: bool,
    time_correction: i32,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    let _ = crash_dump;
    emit_if_not_ok(
        &mut emit,
        "WriteLog",
        validate_write_log(write_log, log_file_path),
    );
    emit_if_not_ok(
        &mut emit,
        "RotateLog",
        validate_rotate_log(write_log, rotate_log),
    );

    let buf_status = check_positive_num(b"LogBuffer", log_buffer).and_then(|| {
        if log_buffer < 100 {
            HealthStatus::info(
                "'LogBuffer' is very low. You might miss recent messages in the web UI",
            )
        } else {
            HealthStatus::ok()
        }
    });
    emit_if_not_ok(&mut emit, "LogBuffer", buf_status);

    #[cfg(target_os = "linux")]
    if crash_dump {
        emit(
            "CrashDump",
            Severity::Info,
            b"'CrashDump' is enabled. Memory dumps may contain sensitive data (passwords, keys)",
        );
    }

    let abs_tc = (time_correction as i64).abs();
    if abs_tc > 1440 && abs_tc < 1_000_000 {
        emit(
            "TimeCorrection",
            Severity::Warning,
            b"'TimeCorrection' value is very large (interpreted as minutes)",
        );
    }
}

// ============================================================================
// SchedulerTaskValidator (`daemon/systemhealth/SchedulerTaskValidator.cpp`)
// ============================================================================

pub fn validate_scheduler_task_section(
    hours: i32,
    minutes: i32,
    weekdays_bits: i32,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    if !(-1..=23).contains(&hours) {
        let msg = format!("Invalid hour: {hours}");
        emit("Time", Severity::Error, msg.as_bytes());
    } else if !(0..=59).contains(&minutes) {
        let msg = format!("Invalid minute: {minutes}");
        emit("Time", Severity::Error, msg.as_bytes());
    }

    if !(0..=127).contains(&weekdays_bits) {
        emit(
            "WeekDays",
            Severity::Error,
            b"Invalid weekdays configuration",
        );
    }
}

// ============================================================================
// FeedValidator & FeedsValidator (`daemon/systemhealth/FeedValidator.cpp`)
// ============================================================================

pub fn validate_feed_cert(url: &[u8], cert_level: u32) -> HealthStatus {
    // Options::ECertVerifLevel::cvNone = 0
    if cert_level == 0 && url.len() >= 8 && url[..8].eq_ignore_ascii_case(b"https://") {
        HealthStatus::warning(
            "Certificate verification is 'None', making the TLS connection insecure",
        )
    } else {
        HealthStatus::ok()
    }
}

pub fn validate_feed_section(
    name: &[u8],
    url: &[u8],
    interval: i32,
    cert_level: u32,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    if name.is_empty() {
        emit(
            "Name",
            Severity::Info,
            b"Name is recommended for clearer logs and troubleshooting",
        );
    }

    let has_http = url.windows(7).any(|w| w == b"http://");
    let has_https = url.windows(8).any(|w| w == b"https://");
    if url.is_empty() {
        emit("URL", Severity::Warning, b"URL is required");
    } else if !has_http && !has_https {
        emit(
            "URL",
            Severity::Warning,
            b"URL does not start with 'http://' or 'https://'",
        );
    }

    let interval_status = check_positive_num(b"Interval", interval).and_then(|| {
        if interval == 0 {
            HealthStatus::info("Automatic check is disabled")
        } else {
            HealthStatus::ok()
        }
    });
    emit_if_not_ok(&mut emit, "Interval", interval_status);

    emit_if_not_ok(
        &mut emit,
        "CertVerification",
        validate_feed_cert(url, cert_level),
    );
}

pub fn validate_duplicate_feed_urls(feeds: &[(&[u8], &[u8])]) -> HealthStatus {
    for (i, &(name_i, url_i)) in feeds.iter().enumerate() {
        if url_i.is_empty() {
            continue;
        }
        for &(name_j, url_j) in &feeds[..i] {
            if url_j == url_i {
                return HealthStatus::warning(concat_bytes(&[
                    b"Feeds '",
                    name_j,
                    b"' and '",
                    name_i,
                    b"' have the same URL",
                ]));
            }
        }
    }
    HealthStatus::ok()
}

// ============================================================================
// IncomingNzbValidator (`daemon/systemhealth/IncomingNzbValidator.cpp`)
// ============================================================================

pub fn validate_incoming_nzb_section(
    append_category_dir: bool,
    nzb_dir_interval: i32,
    nzb_dir_file_age: i32,
    dupe_check: bool,
    dupe_search: bool,
    health_check: i32,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    if append_category_dir {
        emit(
            "AppendCategoryDir",
            Severity::Info,
            b"Category subdirectories will be created automatically",
        );
    }

    let interval_status = if nzb_dir_interval < 0 {
        check_positive_num(b"NzbDirInterval", nzb_dir_interval)
    } else if nzb_dir_interval == 0 {
        HealthStatus::warning("Automatic NZB directory scanning is disabled")
    } else if nzb_dir_interval < 3 {
        HealthStatus::warning(format!(
            "'NzbDirInterval' is set to {nzb_dir_interval} seconds. Very frequent scanning may cause high CPU/Disk usage"
        ))
    } else {
        HealthStatus::ok()
    };
    emit_if_not_ok(&mut emit, "NzbDirInterval", interval_status);

    let age_status = if nzb_dir_file_age < 0 {
        check_positive_num(b"NzbDirFileAge", nzb_dir_file_age)
    } else if nzb_dir_file_age > 3600 {
        HealthStatus::warning(format!(
            "'NzbDirFileAge' is set to {nzb_dir_file_age} seconds (> 1 hour). New NZB files will not be picked up until they are at least this old"
        ))
    } else {
        HealthStatus::ok()
    };
    emit_if_not_ok(&mut emit, "NzbDirFileAge", age_status);

    // Options::EHealthCheck: hcPause = 0, hcDelete = 1, hcPark = 2, hcNone = 3, hcDupe = 4
    if dupe_check && health_check == 0 {
        emit(
            "DupeCheck",
            Severity::Info,
            b"'DupeCheck' is enabled while 'HealthCheck' is set to 'Pause'. This configuration is not recommended as it requires manual intervention to unpause backup downloads if the primary one fails. Consider using 'Delete', 'Park', or 'None' for 'HealthCheck'",
        );
    }

    if dupe_search {
        if !dupe_check {
            emit(
                "DupeSearch",
                Severity::Warning,
                b"'DupeSearch' is enabled while 'DupeCheck' is disabled. Nothing is searched: duplicates need duplicate handling",
            );
        } else if health_check != 4 {
            emit(
                "DupeSearch",
                Severity::Warning,
                b"'DupeSearch' is enabled while 'HealthCheck' isn't 'Dupe'. The duplicates it finds are only fetched once a download has failed, not as soon as it turns out dead",
            );
        }
    }
}

// ============================================================================
// UnpackValidator (`daemon/systemhealth/UnpackValidator.cpp`)
// ============================================================================

pub fn validate_unpack_section(
    unpack: bool,
    direct_unpack: bool,
    unpack_pause_queue: bool,
    unrar_path: &Path,
    sevenzip_path: &Path,
    ext_cleanup_disk: &[u8],
    unpack_pass_file: &Path,
    unpack_ignore_ext: &[u8],
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    if !unpack {
        emit(
            "Unpack",
            Severity::Warning,
            b"Unpacking is disabled - archives will not be extracted",
        );
    }
    if direct_unpack && !unpack {
        emit(
            "DirectUnpack",
            Severity::Warning,
            b"'DirectUnpack' is enabled, but 'Unpack' is disabled. Direct unpacking will not work",
        );
    }
    if !unpack {
        emit(
            "UnpackCleanupDisk",
            Severity::Info,
            b"'UnpackCleanupDisk' is configured but global Unpack is disabled; cleanup will not occur",
        );
    }
    if unpack_pause_queue && !unpack {
        emit(
            "UnpackPauseQueue",
            Severity::Info,
            b"'UnpackPauseQueue' has no effect because Unpack is disabled",
        );
    }
    if unpack {
        let unrar_status = if unrar_path.as_os_str().is_empty() {
            HealthStatus::warning("'UnrarCmd' is not configured. RAR archives cannot be unpacked")
        } else {
            file_exists(unrar_path).and_then(|| file_executable(unrar_path))
        };
        emit_if_not_ok(&mut emit, "UnrarCmd", unrar_status);

        let sevenzip_status = if sevenzip_path.as_os_str().is_empty() {
            HealthStatus::warning(
                "'SevenZipCmd' is not configured. This prevents unpacking 7z archives and installing extensions",
            )
        } else {
            file_exists(sevenzip_path).and_then(|| file_executable(sevenzip_path))
        };
        emit_if_not_ok(&mut emit, "SevenZipCmd", sevenzip_status);
    }
    if ext_cleanup_disk.len() > 512 {
        emit(
            "ExtCleanupDisk",
            Severity::Warning,
            b"'ExtCleanupDisk' value is unusually long",
        );
    }
    if !unpack_pass_file.as_os_str().is_empty() {
        emit_if_not_ok(&mut emit, "UnpackPassFile", file_exists(unpack_pass_file));
    }
    if unpack_ignore_ext.len() > 512 {
        emit(
            "UnpackIgnoreExt",
            Severity::Warning,
            b"'UnpackIgnoreExt' is unusually long",
        );
    }
}

// ============================================================================
// CheckAndRepairValidator (`daemon/systemhealth/CheckAndRepairValidator.cpp`)
// ============================================================================

#[allow(clippy::too_many_arguments)]
pub fn validate_check_and_repair_section(
    crc_check: bool,
    par_check: i32,
    par_repair: bool,
    par_quick: bool,
    par_buffer: i32,
    par_threads: i32,
    hw_threads: usize,
    par_rename: bool,
    rar_rename: bool,
    direct_rename: bool,
    hard_linking: bool,
    health_check: i32,
    par_time_limit: i32,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    if !crc_check {
        emit(
            "CrcCheck",
            Severity::Info,
            b"Normally, 'CrcCheck' should be enabled to better detect download errors and for quick par-verification",
        );
    }
    // Options::EParCheck: pcAuto = 0, pcAlways = 1, pcForce = 2, pcManual = 3
    if par_repair {
        match par_check {
            1 => emit(
                "ParCheck",
                Severity::Info,
                b"'ParCheck' is set to 'Always'. Verification runs on every download, even undamaged ones. Use 'Auto' to skip unnecessary checks and save CPU",
            ),
            2 => emit(
                "ParCheck",
                Severity::Info,
                b"'ParCheck' is set to 'Force'. All PAR2 files are downloaded immediately, wasting bandwidth and CPU. 'Auto' is recommended",
            ),
            3 => emit(
                "ParCheck",
                Severity::Warning,
                b"Automatic repair is disabled. Damaged downloads will require manual intervention",
            ),
            _ => {}
        }
    }
    if !par_repair {
        emit(
            "ParRepair",
            Severity::Warning,
            b"'ParRepair' is off. Corrupted files won't be automatically repaired",
        );
    }
    if !par_quick && par_repair {
        emit(
            "ParQuick",
            Severity::Info,
            b"'ParQuick' is off. Files will be verified by reading from disk, which is slower",
        );
    }
    if par_repair {
        let buf_status = if par_buffer < 0 {
            check_positive_num(b"ParBuffer", par_buffer)
        } else if par_buffer < 250 {
            HealthStatus::info(format!(
                "'ParBuffer' is set to {par_buffer} MB. Increasing to 250 MB or higher is recommended to speed up verification and repair"
            ))
        } else {
            HealthStatus::ok()
        };
        emit_if_not_ok(&mut emit, "ParBuffer", buf_status);

        let threads_status = if par_threads < 0 {
            check_positive_num(b"ParThreads", par_threads)
        } else if par_threads > 0 && hw_threads > 0 && (par_threads as usize) > hw_threads {
            HealthStatus::warning(format!(
                "'ParThreads' ({par_threads}) exceeds the number of CPU cores ({hw_threads}). This may slow down repair due to excessive context switching"
            ))
        } else {
            HealthStatus::ok()
        };
        emit_if_not_ok(&mut emit, "ParThreads", threads_status);
    }
    if !par_rename {
        emit(
            "ParRename",
            Severity::Info,
            b"ParRename is off. Original file names won't be restored from par2-files",
        );
    }
    if !rar_rename {
        emit(
            "RarRename",
            Severity::Info,
            b"'RarRename' is off. Original file names won't be restored from rar-files",
        );
    }
    if hard_linking && !direct_rename {
        emit(
            "HardLinking",
            Severity::Warning,
            b"'HardLinking' is enabled, but 'DirectRename' is disabled. Hard linking will not be performed",
        );
    }
    // Options::EHealthCheck::hcPause = 0
    if health_check == 0 {
        emit(
            "HealthCheck",
            Severity::Warning,
            b"'HealthCheck' is set to 'Pause' you will need to manually move another duplicate from history to queue",
        );
    }
    if par_repair {
        let limit_status = if par_time_limit < 0 {
            check_positive_num(b"ParTimeLimit", par_time_limit)
        } else if par_time_limit > 0 && par_time_limit < 5 {
            HealthStatus::info(format!(
                "'ParTimeLimit' is set to {par_time_limit} minutes. Large repairs often take longer, risking premature cancellation"
            ))
        } else {
            HealthStatus::ok()
        };
        emit_if_not_ok(&mut emit, "ParTimeLimit", limit_status);
    }
}

// ============================================================================
// DownloadQueueValidator (`daemon/systemhealth/DownloadQueueValidator.cpp`)
// ============================================================================

#[allow(clippy::too_many_arguments)]
pub fn validate_download_queue_section(
    flush_queue: bool,
    continue_partial: bool,
    propagation_delay: i32,
    article_cache: i32,
    direct_write: bool,
    write_buffer: i32,
    file_naming: i32,
    post_strategy: i32,
    disk_space: i32,
    nzb_cleanup_disk: bool,
    keep_history: i32,
    feed_history: i32,
    skip_write: bool,
    raw_article: bool,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    if flush_queue && skip_write {
        emit(
            "FlushQueue",
            Severity::Warning,
            b"'FlushQueue' is enabled while 'SkipWrite' is enabled: flushed data may not be written to disk",
        );
    }
    if continue_partial {
        emit(
            "ContinuePartial",
            Severity::Info,
            b"Disabling 'ContinuePartial' may slightly reduce disk access and is recommended on fast connections",
        );
    }
    emit_if_not_ok(
        &mut emit,
        "PropagationDelay",
        check_positive_num(b"PropagationDelay", propagation_delay),
    );

    let cache_status = if article_cache < 0 {
        HealthStatus::error("'ArticleCache' option must not be negative")
    } else if article_cache == 0 {
        HealthStatus::warning(
            "'ArticleCache' is disabled. Enabling it is recommended to reduce disk fragmentation",
        )
    } else if std::mem::size_of::<usize>() == 4 && article_cache > 1900 {
        HealthStatus::error("'ArticleCache' cannot exceed 1900 MB on 32-bit systems")
    } else if direct_write && article_cache < 50 {
        HealthStatus::warning("It's recommended to set 'ArticleCache' at least 50MB")
    } else if !direct_write && article_cache < 200 {
        HealthStatus::warning(
            "A cache under 200 MB is likely too small to hold complete files, forcing writes to the temporary directory and degrading performance",
        )
    } else {
        HealthStatus::ok()
    };
    emit_if_not_ok(&mut emit, "ArticleCache", cache_status);

    if !direct_write && !raw_article {
        emit(
            "DirectWrite",
            Severity::Warning,
            b"'DirectWrite' is disabled. Articles will be written to the temporary directory first, then copied to the destination. Enabling this option is usually recommended to reduce disk I/O usage",
        );
    }

    let wb_status = if write_buffer < 0 {
        HealthStatus::error("'WriteBuffer' option must not be negative")
    } else if write_buffer == 0 {
        HealthStatus::warning(
            "'WriteBuffer' is set to '0'. This uses the default system buffer, which is often too small and inefficient",
        )
    } else if write_buffer < 1024 {
        HealthStatus::warning(
            "'WriteBuffer' is very low. At least 1024 KB is recommended for systems with sufficient memory",
        )
    } else if write_buffer > 102400 {
        HealthStatus::warning(
            "'WriteBuffer' is very large (>100MB). This is per-connection and may exhaust memory",
        )
    } else {
        HealthStatus::ok()
    };
    emit_if_not_ok(&mut emit, "WriteBuffer", wb_status);

    // Options::EFileNaming: nfAuto = 0, nfArticle = 1, nfNzb = 2
    match file_naming {
        2 => emit(
            "FileNaming",
            Severity::Info,
            b"'FileNaming' is set to 'Nzb'. Files will be named strictly based on the NZB content. Obfuscated releases often result in meaningless filenames with this setting. 'Auto' is recommended",
        ),
        1 => emit(
            "FileNaming",
            Severity::Info,
            b"'FileNaming' is set to 'Article'. Files will be named using subject headers from the articles. If headers are malformed or missing, filenames will be incorrect. 'Auto' is recommended",
        ),
        _ => {}
    }

    // Options::EPostStrategy: ppSequential = 0, ppBalanced = 1, ppAggressive = 2, ppRocket = 3
    match post_strategy {
        0 => emit(
            "PostStrategy",
            Severity::Info,
            b"'PostStrategy' is set to 'Sequential'. Safe for low-end hardware, but may be slower.",
        ),
        2 => emit(
            "PostStrategy",
            Severity::Info,
            b"'PostStrategy' is set to 'Aggressive'. Ensure you have a multi-core CPU and fast storage (SSD) to prevent bottlenecks",
        ),
        3 => emit(
            "PostStrategy",
            Severity::Info,
            b"'PostStrategy' is set to 'Rocket'. This requires high-end hardware (NVMe SSD, many CPU cores)",
        ),
        _ => {}
    }

    let disk_status = check_positive_num(b"DiskSpace", disk_space).and_then(|| {
        if disk_space > 0 && disk_space < 50 {
            HealthStatus::warning(
                "'DiskSpace' is set very low (<50MB). Downloads may fill disk before pausing",
            )
        } else {
            HealthStatus::ok()
        }
    });
    emit_if_not_ok(&mut emit, "DiskSpace", disk_status);

    if !nzb_cleanup_disk {
        emit(
            "NzbCleanupDisk",
            Severity::Info,
            b"'NzbCleanupDisk' is disabled. Source NZB files will remain in the incoming directory after processing",
        );
    }

    emit_if_not_ok(
        &mut emit,
        "KeepHistory",
        check_positive_num(b"KeepHistory", keep_history),
    );
    emit_if_not_ok(
        &mut emit,
        "FeedHistory",
        check_positive_num(b"FeedHistory", feed_history),
    );

    if skip_write {
        emit(
            "SkipWrite",
            Severity::Warning,
            b"'SkipWrite' is enabled: downloaded data will NOT be saved to disk",
        );
    }
    if raw_article {
        emit(
            "RawArticle",
            Severity::Warning,
            b"'RawArticle' is enabled: articles will be saved in raw format (unusable for normal files)",
        );
    }
}

// ============================================================================
// ConnectionValidator (`daemon/systemhealth/ConnectionValidator.cpp`)
// ============================================================================

pub fn validate_article_retries(val: i32, hc_dupe: bool, dupe_search: bool) -> HealthStatus {
    if !(0..=99).contains(&val) {
        return HealthStatus::error("'ArticleRetries' must be between 0 and 99");
    }
    if val < 3 {
        let mut msg = format!(
            "'ArticleRetries' is set to {val}. This is very low; temporary connection drops may cause permanent download failures"
        );
        let decisions = match (hc_dupe, dupe_search) {
            (true, true) => "HealthCheck=dupe and DupeSearch",
            (true, false) => "HealthCheck=dupe",
            (false, true) => "DupeSearch",
            (false, false) => "",
        };
        if !decisions.is_empty() {
            msg.push_str(", and ");
            msg.push_str(decisions);
            msg.push_str(
                " may then take a good download for a bad one (fail over from it, rank its duplicates above it)",
            );
        }
        return HealthStatus::warning(msg);
    }
    HealthStatus::ok()
}

pub fn validate_article_read_chunk_size(bytes: i32) -> HealthStatus {
    let val = bytes / 1024;
    if val < 0 {
        HealthStatus::error("'ArticleReadChunkSize' option must not be negative")
    } else if val < 4 {
        HealthStatus::warning(format!(
            "'ArticleReadChunkSize' is set to {val} KB. This is very low and may reduce download speed due to network overhead"
        ))
    } else if val > 10240 {
        HealthStatus::info(format!(
            "'ArticleReadChunkSize' is set to {val} KB. This buffer is allocated per connection; ensure sufficient system RAM is available"
        ))
    } else {
        HealthStatus::ok()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn validate_connection_section(
    article_retries: i32,
    hc_dupe: bool,
    dupe_search: bool,
    article_interval: i32,
    article_timeout: i32,
    article_read_chunk_size: i32,
    url_retries: i32,
    url_interval: i32,
    url_timeout: i32,
    url_connections: i32,
    remote_timeout: i32,
    download_rate: i32,
    monthly_quota: i32,
    quota_start_day: i32,
    daily_quota: i32,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    emit_if_not_ok(
        &mut emit,
        "ArticleRetries",
        validate_article_retries(article_retries, hc_dupe, dupe_search),
    );
    emit_if_not_ok(
        &mut emit,
        "ArticleInterval",
        check_positive_num(b"ArticleInterval", article_interval),
    );

    let art_timeout_status = check_positive_num(b"ArticleTimeout", article_timeout).and_then(|| {
        if article_timeout < 30 {
            HealthStatus::warning(format!(
                "'ArticleTimeout' is set to {article_timeout} seconds. Slow server responses may cause valid connections to be dropped prematurely"
            ))
        } else {
            HealthStatus::ok()
        }
    });
    emit_if_not_ok(&mut emit, "ArticleTimeout", art_timeout_status);

    emit_if_not_ok(
        &mut emit,
        "ArticleReadChunkSize",
        validate_article_read_chunk_size(article_read_chunk_size),
    );

    if !(0..=99).contains(&url_retries) {
        emit(
            "UrlRetries",
            Severity::Error,
            b"'UrlRetries' must be between 0 and 99",
        );
    }

    emit_if_not_ok(
        &mut emit,
        "UrlInterval",
        check_positive_num(b"UrlInterval", url_interval),
    );

    let url_timeout_status = check_positive_num(b"UrlTimeout", url_timeout).and_then(|| {
        if url_timeout < 5 {
            HealthStatus::warning(format!(
                "'UrlTimeout' is set to {url_timeout} seconds. RSS feeds and external URL fetches may fail if the remote server is slow"
            ))
        } else {
            HealthStatus::ok()
        }
    });
    emit_if_not_ok(&mut emit, "UrlTimeout", url_timeout_status);

    if !(0..=999).contains(&url_connections) {
        emit(
            "UrlConnections",
            Severity::Error,
            b"'UrlConnections' must be between 0 and 999",
        );
    }

    emit_if_not_ok(
        &mut emit,
        "RemoteTimeout",
        check_positive_num(b"RemoteTimeout", remote_timeout),
    );

    let rate_status = check_positive_num(b"DownloadRate", download_rate).and_then(|| {
        if download_rate == 0 {
            HealthStatus::ok()
        } else {
            HealthStatus::info("Global download speed is restricted")
        }
    });
    emit_if_not_ok(&mut emit, "DownloadRate", rate_status);

    let monthly_status = check_positive_num(b"MonthlyQuota", monthly_quota).and_then(|| {
        if monthly_quota == 0 {
            HealthStatus::ok()
        } else {
            HealthStatus::info(format!(
                "'MonthlyQuota' is active ({monthly_quota} MB). Downloads will pause if this limit is reached"
            ))
        }
    });
    emit_if_not_ok(&mut emit, "MonthlyQuota", monthly_status);

    if !(1..=31).contains(&quota_start_day) {
        emit(
            "QuotaStartDay",
            Severity::Error,
            b"'QuotaStartDay' must be between 1 and 31",
        );
    }

    let daily_status = check_positive_num(b"DailyQuota", daily_quota).and_then(|| {
        if daily_quota == 0 {
            HealthStatus::ok()
        } else {
            HealthStatus::info(format!(
                "'DailyQuota' is active ({daily_quota} MB). Downloads will pause if this limit is reached"
            ))
        }
    });
    emit_if_not_ok(&mut emit, "DailyQuota", daily_status);
}

// ============================================================================
// SecurityValidator (`daemon/systemhealth/SecurityValidator.cpp`)
// ============================================================================

pub fn validate_control_password(password: &[u8]) -> HealthStatus {
    if password == b"tegbzn6789" {
        return HealthStatus::info("Using default password is not recommended for security");
    }
    check_password(password)
}

pub fn validate_form_auth(
    form_auth: bool,
    secure_control: bool,
    no_control_pass: bool,
    no_add_user: bool,
    no_restricted_user: bool,
) -> HealthStatus {
    if !form_auth {
        return HealthStatus::ok();
    }
    if !secure_control {
        return HealthStatus::warning(
            "'FormAuth' is enabled but 'SecureControl' is disabled. Form credentials may be transmitted in plaintext",
        );
    }
    if no_control_pass && no_add_user && no_restricted_user {
        return HealthStatus::warning(
            "'FormAuth' is enabled but no form users are configured. Users cannot log in via forms",
        );
    }
    HealthStatus::ok()
}

pub struct SecuritySectionInput<'a> {
    pub control_ip: &'a [u8],
    pub control_port: i32,
    pub control_username: &'a [u8],
    pub control_password: &'a [u8],
    pub restricted_username: &'a [u8],
    pub restricted_password: &'a [u8],
    pub add_username: &'a [u8],
    pub add_password: &'a [u8],
    pub form_auth: bool,
    pub secure_control: bool,
    pub secure_cert: &'a Path,
    pub secure_key: &'a Path,
    pub secure_port: i32,
    pub cert_check: bool,
    pub cert_store_path_empty: bool,
    pub daemon_mode: bool,
    pub daemon_username: &'a [u8],
    pub umask: i32,
}

pub fn validate_security_section(
    input: &SecuritySectionInput<'_>,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    let ip_status = required_option(b"ControlIP", input.control_ip).and_then(|| {
        if input.control_ip == b"0.0.0.0" {
            HealthStatus::info(
                "'ControlIP' is '0.0.0.0', allowing connections from any IP. Client mode will default to 127.0.0.1 to connect",
            )
        } else if input.control_ip.starts_with(b"/") || input.control_ip.starts_with(b".") {
            if cfg!(windows) {
                HealthStatus::error("Using Unix domain sockets is not supported on Windows")
            } else {
                HealthStatus::info(
                    "'ControlIP' is set to a path, activating Unix domain socket mode",
                )
            }
        } else {
            HealthStatus::ok()
        }
    });
    emit_if_not_ok(&mut emit, "ControlIP", ip_status);

    let port_status = network_valid_port(input.control_port).and_then(|| {
        if input.control_port < 1024 {
            HealthStatus::warning("'ControlPort' is below 1024 and may require root privileges")
        } else {
            HealthStatus::ok()
        }
    });
    emit_if_not_ok(&mut emit, "ControlPort", port_status);

    if input.control_username.is_empty() {
        emit(
            "ControlUsername",
            Severity::Info,
            b"Using an empty username is not recommended for security reasons",
        );
    } else if input.control_username == b"nzbget" {
        emit(
            "ControlUsername",
            Severity::Info,
            b"Using default username 'nzbget' is not recommended for security reasons",
        );
    }

    emit_if_not_ok(
        &mut emit,
        "ControlPassword",
        validate_control_password(input.control_password),
    );

    if !input.restricted_username.is_empty() && input.control_username == b"nzbget" {
        emit(
            "RestrictedUsername",
            Severity::Warning,
            b"'RestrictedUsername' is enabled while 'ControlUsername' is still set to the default 'nzbget'",
        );
    }

    if !input.restricted_username.is_empty() && input.restricted_password.is_empty() {
        emit(
            "RestrictedPassword",
            Severity::Warning,
            b"'RestrictedUsername' is enabled, but 'RestrictedPassword' is empty. This allows restricted access without a password",
        );
    } else if input.restricted_username.is_empty() && !input.restricted_password.is_empty() {
        emit(
            "RestrictedPassword",
            Severity::Info,
            b"'RestrictedPassword' is set, but 'RestrictedUsername' is empty. The restricted user is currently disabled",
        );
    }

    if !input.add_username.is_empty() && input.control_username == b"nzbget" {
        emit(
            "AddUsername",
            Severity::Warning,
            b"'AddUsername' is enabled while 'ControlUsername' is still set to the default 'nzbget'",
        );
    }

    if !input.add_username.is_empty() && input.add_password.is_empty() {
        emit(
            "AddPassword",
            Severity::Warning,
            b"'AddUsername' is enabled, but 'AddPassword' is empty. This allows add-only access without a password",
        );
    } else if input.add_username.is_empty() && !input.add_password.is_empty() {
        emit(
            "AddPassword",
            Severity::Info,
            b"'AddPassword' is set, but 'AddUsername' is empty. The add-user is currently disabled",
        );
    }

    emit_if_not_ok(
        &mut emit,
        "FormAuth",
        validate_form_auth(
            input.form_auth,
            input.secure_control,
            input.control_password.is_empty(),
            input.add_username.is_empty(),
            input.restricted_username.is_empty(),
        ),
    );

    if input.secure_control && input.secure_port == 0 {
        emit(
            "SecureControl",
            Severity::Error,
            b"'SecureControl' is enabled but 'SecurePort' is invalid",
        );
    }

    if input.secure_control {
        let cert_status = if input.secure_cert.as_os_str().is_empty() {
            HealthStatus::warning("'SecureControl' is enabled but 'SecureCert' is empty")
        } else {
            file_exists(input.secure_cert).and_then(|| file_readable(input.secure_cert))
        };
        emit_if_not_ok(&mut emit, "SecureCert", cert_status);

        let key_status = if input.secure_key.as_os_str().is_empty() {
            HealthStatus::warning("'SecureControl' is enabled but 'SecureKey' is empty")
        } else {
            file_exists(input.secure_key).and_then(|| file_readable(input.secure_key))
        };
        emit_if_not_ok(&mut emit, "SecureKey", key_status);

        let sec_port_status = network_valid_port(input.secure_port).and_then(|| {
            if input.secure_port == input.control_port {
                HealthStatus::error("'SecurePort' cannot be the same as 'ControlPort'")
            } else {
                HealthStatus::ok()
            }
        });
        emit_if_not_ok(&mut emit, "SecurePort", sec_port_status);
    }

    if !input.cert_check && !input.cert_store_path_empty {
        emit(
            "CertCheck",
            Severity::Warning,
            b"'CertCheck' is disabled. TLS connections may be insecure",
        );
    }

    if input.daemon_mode && input.daemon_username == b"root" {
        emit(
            "DaemonUsername",
            Severity::Warning,
            b"'DaemonUsername' is set to 'root', consider using a non-privileged user",
        );
    }

    if input.umask == 0 {
        emit(
            "UMask",
            Severity::Warning,
            b"'UMask' is set to 0, files will be created with full permissions",
        );
    }
}

// ============================================================================
// NewsServerValidator & NewsServersValidator (`daemon/systemhealth/NewsServerValidator.cpp`)
// ============================================================================

pub fn validate_server_active(active: bool) -> HealthStatus {
    if !active {
        HealthStatus::info("Server is disabled")
    } else {
        HealthStatus::ok()
    }
}

pub fn validate_server_optional(active: bool, optional: bool, level: i32) -> HealthStatus {
    if !active {
        return HealthStatus::ok();
    }
    if optional && level == 0 {
        HealthStatus::info(
            "Server is marked as optional but assigned to level 0; this may affect primary server selection",
        )
    } else {
        HealthStatus::ok()
    }
}

pub fn validate_server_host(active: bool, host: &[u8], ip_version: i32) -> HealthStatus {
    if !active {
        return HealthStatus::ok();
    }
    required_option(b"Host", host).and_then(|| network_valid_host(host, ip_version))
}

pub fn validate_server_port(active: bool, port: i32, tls: bool) -> HealthStatus {
    if !active {
        return HealthStatus::ok();
    }
    if !(1..=65535).contains(&port) {
        return HealthStatus::error("Port must be between 1 and 65535");
    }
    if tls && port != 443 && port != 563 {
        return HealthStatus::info("Consider using a standard encrypted port (443 or 563)");
    }
    if !tls && port != 80 && port != 119 {
        return HealthStatus::info("Consider using a standard unencrypted port (80 or 119)");
    }
    HealthStatus::ok()
}

pub fn validate_server_username(active: bool, username: &[u8]) -> HealthStatus {
    if !active {
        return HealthStatus::ok();
    }
    if username.is_empty() {
        HealthStatus::warning("Username is set to empty")
    } else {
        HealthStatus::ok()
    }
}

pub fn validate_server_password(active: bool, password: &[u8]) -> HealthStatus {
    if !active {
        return HealthStatus::ok();
    }
    check_password(password)
}

pub fn validate_server_encryption(active: bool, tls: bool) -> HealthStatus {
    if !active {
        return HealthStatus::ok();
    }
    if !tls {
        HealthStatus::warning("TLS is disabled. Communication with this server will not be encrypted")
    } else {
        HealthStatus::ok()
    }
}

pub fn validate_server_cipher(active: bool, cipher: &[u8], tls: bool) -> HealthStatus {
    if !active || cipher.is_empty() {
        HealthStatus::ok()
    } else if !tls {
        HealthStatus::warning("Cipher specified but TLS is disabled")
    } else {
        HealthStatus::info("Using custom cipher suite")
    }
}

pub fn validate_server_connections(
    active: bool,
    max_connections: i32,
    dupe_search: bool,
) -> HealthStatus {
    if !active {
        return HealthStatus::ok();
    }
    if max_connections == 0 {
        return HealthStatus::warning("'Connections' is set to '0'. The Server is disabled");
    }
    if !(0..=999).contains(&max_connections) {
        return HealthStatus::error("'Connections' value is invalid. It must be between 0 and 999");
    }
    if max_connections < 4 && dupe_search {
        return HealthStatus::warning(
            "A low number of connections may impact download performance, and DupeSearch's health checks get only 1 connection to this server (4 or more give them 2)",
        );
    }
    if max_connections < 8 {
        return HealthStatus::warning("A low number of connections may impact download performance");
    }
    HealthStatus::ok()
}

pub fn validate_server_retention(active: bool, retention: i32) -> HealthStatus {
    if !active {
        return HealthStatus::ok();
    }
    if retention > 0 && retention < 100 {
        return HealthStatus::warning(format!(
            "Retention is set to {retention} days. This low value may cause downloads to fail"
        ));
    }
    if retention > 10000 {
        return HealthStatus::info(format!(
            "Retention ({retention}) is unrealistically high and will be limited by the server's actual retention"
        ));
    }
    HealthStatus::ok()
}

pub fn validate_server_cert_verification(
    active: bool,
    tls: bool,
    cert_level: u32,
) -> HealthStatus {
    if !active || !tls {
        return HealthStatus::ok();
    }
    // Options::ECertVerifLevel::cvNone = 0
    if cert_level == 0 {
        HealthStatus::warning(
            "Certificate verification is 'None', making the TLS connection insecure",
        )
    } else {
        HealthStatus::ok()
    }
}

pub fn validate_server_ip_version(ip_version: i32) -> HealthStatus {
    match ip_version {
        0 | 4 => HealthStatus::ok(),
        6 => HealthStatus::info("Using IPv6 - ensure your network supports IPv6"),
        _ => HealthStatus::error("Invalid value. Available options are: Auto, IpV4, IpV6"),
    }
}

pub struct NewsServerSectionInput<'a> {
    pub active: bool,
    pub name: &'a [u8],
    pub level: i32,
    pub optional: bool,
    pub group: i32,
    pub host: &'a [u8],
    pub port: i32,
    pub username: &'a [u8],
    pub password: &'a [u8],
    pub tls: bool,
    pub cipher: &'a [u8],
    pub max_connections: i32,
    pub retention: i32,
    pub cert_level: u32,
    pub ip_version: i32,
    pub dupe_search: bool,
}

pub fn validate_news_server_section(
    input: &NewsServerSectionInput<'_>,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    emit_if_not_ok(&mut emit, "Active", validate_server_active(input.active));

    if input.active && input.name.is_empty() {
        emit(
            "Name",
            Severity::Info,
            b"Name is recommended for clearer logs and troubleshooting",
        );
    }

    if input.active && !(0..=99).contains(&input.level) {
        emit(
            "Level",
            Severity::Error,
            b"Level must be between 0 and 99",
        );
    }

    emit_if_not_ok(
        &mut emit,
        "Optional",
        validate_server_optional(input.active, input.optional, input.level),
    );

    if input.active && !(0..=99).contains(&input.group) {
        emit(
            "Group",
            Severity::Error,
            b"Group must be between 0 and 99",
        );
    }

    emit_if_not_ok(
        &mut emit,
        "Host",
        validate_server_host(input.active, input.host, input.ip_version),
    );
    emit_if_not_ok(
        &mut emit,
        "Port",
        validate_server_port(input.active, input.port, input.tls),
    );
    emit_if_not_ok(
        &mut emit,
        "Username",
        validate_server_username(input.active, input.username),
    );
    emit_if_not_ok(
        &mut emit,
        "Password",
        validate_server_password(input.active, input.password),
    );
    emit_if_not_ok(
        &mut emit,
        "Encryption",
        validate_server_encryption(input.active, input.tls),
    );
    emit_if_not_ok(
        &mut emit,
        "Cipher",
        validate_server_cipher(input.active, input.cipher, input.tls),
    );
    emit_if_not_ok(
        &mut emit,
        "Connections",
        validate_server_connections(input.active, input.max_connections, input.dupe_search),
    );
    emit_if_not_ok(
        &mut emit,
        "Retention",
        validate_server_retention(input.active, input.retention),
    );
    emit_if_not_ok(
        &mut emit,
        "CertVerification",
        validate_server_cert_verification(input.active, input.tls, input.cert_level),
    );
    emit_if_not_ok(
        &mut emit,
        "IpVersion",
        validate_server_ip_version(input.ip_version),
    );
}

pub fn validate_primary_server_exists(levels: &[i32], actives: &[u8]) -> HealthStatus {
    let n = levels.len().min(actives.len());
    if n == 0 {
        return HealthStatus::error("At least one server must be configured");
    }
    let mut min_level = levels[0];
    for &lvl in &levels[..n] {
        if lvl < min_level {
            min_level = lvl;
        }
    }
    if (0..n).any(|i| levels[i] == min_level && actives[i] != 0) {
        HealthStatus::ok()
    } else {
        HealthStatus::error("At least one primary server must be active")
    }
}

pub fn validate_news_servers_group(
    hosts: &[&[u8]],
    actives: &[u8],
    max_conns: &[i32],
    levels: &[i32],
    mut emit_issue: impl FnMut(Severity, &[u8]),
) {
    let n = hosts
        .len()
        .min(actives.len())
        .min(max_conns.len())
        .min(levels.len());
    if n == 0 || (n == 1 && hosts[0] == b"my.newsserver.com") {
        emit_issue(Severity::Error, b"No servers are configured");
    }
    if !(0..n).any(|i| actives[i] != 0 && max_conns[i] > 0) {
        emit_issue(Severity::Error, b"At least one server must be active");
    }
    let prim = validate_primary_server_exists(&levels[..n], &actives[..n]);
    if !prim.is_ok() {
        emit_issue(prim.severity, &prim.message);
    }
}

// ============================================================================
// ExtensionScriptsValidator (`daemon/systemhealth/ExtensionScriptsValidator.cpp`)
// ============================================================================

fn trim_tokenizer_ws(mut s: &[u8]) -> &[u8] {
    while let Some(&b) = s.first() {
        if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
            s = &s[1..];
        } else {
            break;
        }
    }
    while let Some(&b) = s.last() {
        if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
            s = &s[..s.len() - 1];
        } else {
            break;
        }
    }
    s
}

fn strip_script_ext(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b'.') {
        Some(pos) if pos > 0 => &name[..pos],
        _ => name,
    }
}

pub fn validate_extension_list(
    extensions_opt: &[u8],
    known_extensions: &[(&[u8], &Path)],
) -> HealthStatus {
    if extensions_opt.is_empty() {
        return HealthStatus::ok();
    }
    let mut message = Vec::new();
    for raw_tok in extensions_opt.split(|&b| b == b',' || b == b';') {
        let script_name_raw = trim_tokenizer_ws(raw_tok);
        if script_name_raw.is_empty() {
            continue;
        }
        let script_name = strip_script_ext(script_name_raw);
        let found = known_extensions
            .iter()
            .find(|&&(ext_name, _)| ext_name == script_name);
        let Some(&(_, entry_path)) = found else {
            if !message.is_empty() {
                message.extend_from_slice(b"; ");
            }
            message.push(b'\'');
            message.extend_from_slice(script_name_raw);
            message.extend_from_slice(b"' doesn't exist");
            continue;
        };
        let exists = file_exists(entry_path);
        if !exists.is_ok() {
            if !message.is_empty() {
                message.extend_from_slice(b"; ");
            }
            message.extend_from_slice(&exists.message);
            message.push(b' ');
        }
    }
    if message.is_empty() {
        HealthStatus::ok()
    } else {
        HealthStatus::warning(message)
    }
}

pub fn validate_shell_override(shell_override: &[u8]) -> HealthStatus {
    if shell_override.is_empty() {
        return HealthStatus::ok();
    }
    let mut message = Vec::new();
    for raw_tok in shell_override.split(|&b| b == b',' || b == b';') {
        let tok = trim_tokenizer_ws(raw_tok);
        if tok.is_empty() {
            continue;
        }
        if let Some(eq_pos) = tok.iter().position(|&b| b == b'=') {
            let actual_cmd_bytes = &tok[eq_pos + 1..];
            let actual_path = bytes_to_path(actual_cmd_bytes);
            let exists = file_exists(&actual_path);
            if !exists.is_ok() {
                if !message.is_empty() {
                    message.extend_from_slice(b"; ");
                }
                message.extend_from_slice(&exists.message);
                message.push(b' ');
                continue;
            }
            let exe = file_executable(&actual_path);
            if !exe.is_ok() {
                if !message.is_empty() {
                    message.extend_from_slice(b"; ");
                }
                message.extend_from_slice(&exe.message);
                message.push(b' ');
            }
        }
    }
    if message.is_empty() {
        HealthStatus::ok()
    } else {
        HealthStatus::warning(message)
    }
}

pub fn validate_extension_scripts_section(
    extensions_opt: &[u8],
    known_extensions: &[(&[u8], &Path)],
    shell_override: &[u8],
    event_interval: i32,
    mut emit: impl FnMut(&str, Severity, &[u8]),
) {
    emit_if_not_ok(
        &mut emit,
        "Extensions",
        validate_extension_list(extensions_opt, known_extensions),
    );
    emit_if_not_ok(
        &mut emit,
        "ShellOverride",
        validate_shell_override(shell_override),
    );
    if event_interval < -1 {
        emit(
            "EventInterval",
            Severity::Error,
            b"'EventInterval' cannot be less than -1",
        );
    }
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::path::PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> std::path::PathBuf {
    std::path::PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_validators() {
        assert!(required_option(b"User", b"admin").is_ok());
        assert!(required_option(b"User", b"").is_error());
        assert!(required_option(b"MainDir", &[0xff, 0xfe]).is_ok());
        assert!(check_password(b"").severity == Severity::Warning);
        assert!(check_password(b"short").severity == Severity::Info);
        assert!(check_password(&[0xff; 8]).is_ok());
        assert!(check_positive_num(b"Port", 0).is_ok());
        assert!(check_positive_num(b"Port", -1).is_error());
        assert!(network_valid_port(443).is_ok());
        assert!(network_valid_port(0).is_error());
        assert!(network_valid_host(b"news.example.com", 0).is_ok());
        assert!(network_valid_host(b"192.168.1.1", 4).is_ok());
        assert!(network_valid_host(b"192.168.1.1", 6).is_error());
        assert!(network_valid_host(b"2001:db8::1", 6).is_ok());
        assert!(network_valid_host(b"2001:db8::1", 4).is_error());
        assert!(network_valid_host(b"[2001:db8::1]", 0).is_error());
        assert!(network_valid_host(b"fe80::1%3", 0).is_ok());
        assert!(network_valid_host(b"fe80::1%eth0", 6).is_ok());
        assert!(network_valid_host(b"fe80::1%3", 4).is_error());
        assert!(network_valid_host(b"[fe80::1%3]", 0).is_error());
    }

    #[test]
    fn test_sections() {
        let mut opts = Vec::new();
        validate_display_section(0, 10, true, false, false, |name, sev, msg| {
            opts.push((name.to_string(), sev, msg.to_vec()));
        });
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].0, "UpdateInterval");
        assert_eq!(opts[0].1, Severity::Error);
        assert_eq!(opts[1].0, "CursesNzbName");
        assert_eq!(opts[1].1, Severity::Info);

        assert_eq!(
            validate_duplicate_category_names(&[b"Movies", b"TV", b"Movies"]).severity,
            Severity::Warning
        );
        assert!(validate_duplicate_category_names(&[b"Movies", b"TV"]).is_ok());

        let non_utf8_cat = &[b'C', b'a', b't', 0xff];
        let dup_status = validate_duplicate_category_names(&[non_utf8_cat, non_utf8_cat]);
        assert_eq!(dup_status.severity, Severity::Warning);
        assert!(dup_status.message.ends_with(b"Cat\xff'"));

        let mut feed_opts = Vec::new();
        validate_feed_section(b"MyFeed", b"", 15, 0, |name, sev, _| {
            feed_opts.push((name.to_string(), sev));
        });
        assert_eq!(feed_opts, vec![("URL".to_string(), Severity::Warning)]);

        feed_opts.clear();
        validate_feed_section(b"MyFeed", b"https://example.com/rss", 15, 0, |name, sev, _| {
            feed_opts.push((name.to_string(), sev));
        });
        assert_eq!(
            feed_opts,
            vec![("CertVerification".to_string(), Severity::Warning)]
        );

        assert_eq!(
            validate_article_retries(1, true, false).severity,
            Severity::Warning
        );
        assert!(validate_article_retries(3, true, true).is_ok());
        assert!(validate_article_retries(100, false, false).is_error());
        assert_eq!(
            validate_article_read_chunk_size(2048).severity,
            Severity::Warning
        );
        assert!(validate_article_read_chunk_size(4096).is_ok());

        assert_eq!(
            validate_control_password(b"tegbzn6789").severity,
            Severity::Info
        );
        assert!(validate_control_password(b"a-long-password").is_ok());
        assert_eq!(
            validate_form_auth(true, false, false, true, true).severity,
            Severity::Warning
        );
        assert_eq!(
            validate_form_auth(true, true, true, true, true).severity,
            Severity::Warning
        );
        assert!(validate_form_auth(true, true, false, true, true).is_ok());

        assert!(validate_server_port(true, 563, true).is_ok());
        assert_eq!(validate_server_port(true, 119, true).severity, Severity::Info);
        assert!(validate_server_connections(true, 1000, false).is_error());
        assert_eq!(
            validate_server_connections(true, 2, true).severity,
            Severity::Warning
        );
        assert_eq!(
            validate_server_retention(true, 50).severity,
            Severity::Warning
        );
        assert!(validate_primary_server_exists(&[1, 0], &[1, 0]).is_error());
        assert!(validate_primary_server_exists(&[1, 0], &[0, 1]).is_ok());

        assert_eq!(
            validate_extension_list(b"Missing.py", &[]).severity,
            Severity::Warning
        );
    }
}
