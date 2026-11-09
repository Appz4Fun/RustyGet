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
        HealthStatus::ok()
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
    let _ = (unrar_path, sevenzip_path);
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
    } else if input.control_username == b"nzbget" || input.control_username == b"rustyget" {
        emit(
            "ControlUsername",
            Severity::Info,
            b"Using default username 'rustyget' is not recommended for security reasons",
        );
    }

    emit_if_not_ok(
        &mut emit,
        "ControlPassword",
        validate_control_password(input.control_password),
    );

    if !input.restricted_username.is_empty()
        && (input.control_username == b"nzbget" || input.control_username == b"rustyget")
    {
        emit(
            "RestrictedUsername",
            Severity::Warning,
            b"'RestrictedUsername' is enabled while 'ControlUsername' is still set to the default 'rustyget'",
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

    if !input.add_username.is_empty()
        && (input.control_username == b"nzbget" || input.control_username == b"rustyget")
    {
        emit(
            "AddUsername",
            Severity::Warning,
            b"'AddUsername' is enabled while 'ControlUsername' is still set to the default 'rustyget'",
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
    if n == 1 && hosts[0] == b"my.newsserver.com" && actives[0] == 0 {
        emit_issue(Severity::Info, b"No news servers are configured yet");
        return;
    }
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

/// Resolves `script` against `app_dir` if `script` does not start with `sep` for `Maintenance::StartUpdate`.
pub fn maintenance_resolve_script_path(app_dir: &[u8], sep: u8, script: &[u8]) -> Vec<u8> {
    if script.first().copied() != Some(sep) {
        let mut out = Vec::with_capacity(app_dir.len() + 1 + script.len());
        out.extend_from_slice(app_dir);
        out.push(sep);
        out.extend_from_slice(script);
        out
    } else {
        script.to_vec()
    }
}

/// Formats `"<web_dir><sep>package-info.json"` for `Maintenance::ReadPackageInfoStr`.
pub fn maintenance_format_package_info_path(web_dir: &[u8], sep: u8) -> Vec<u8> {
    const SUFFIX: &[u8] = b"package-info.json";
    let mut out = Vec::with_capacity(web_dir.len() + 1 + SUFFIX.len());
    out.extend_from_slice(web_dir);
    out.push(sep);
    out.extend_from_slice(SUFFIX);
    out
}

/// Formats `"update-script <base_name>"` or `"update-info-script <base_name>"`
/// for `UpdateScriptController::Run` and `UpdateInfoScriptController::ExecuteScript`.
pub fn maintenance_format_script_info_name(
    is_info_script: bool,
    base_name: &[u8],
) -> Vec<u8> {
    let prefix: &[u8] = if is_info_script {
        b"update-info-script "
    } else {
        b"update-script "
    };
    let mut out = Vec::with_capacity(prefix.len() + base_name.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(base_name);
    out
}

/// Returns `"STABLE"`, `"TESTING"`, or `"DEVEL"` for `UpdateScriptController::Run`.
pub fn maintenance_branch_name(branch: i32) -> &'static [u8] {
    match branch {
        1 => b"TESTING\0",
        2 => b"DEVEL\0",
        _ => b"STABLE\0",
    }
}

/// Returns `"DAEMON"` or `"SERVER"` for `UpdateScriptController::Run`.
pub fn maintenance_run_mode_name(daemon_mode: bool) -> &'static [u8] {
    if daemon_mode {
        b"DAEMON\0"
    } else {
        b"SERVER\0"
    }
}

/// Formats `"NZBUP_CMDLINE<index>"` for `UpdateScriptController::Run`.
pub fn maintenance_format_cmdline_env_name(index: i32) -> Vec<u8> {
    format!("NZBUP_CMDLINE{index}").into_bytes()
}

/// Returns `(log_prefix, prefix_len)` after bounding `base_name` to `max_len` bytes
/// and stripping the file extension for `UpdateScriptController::Run` and
/// `UpdateInfoScriptController::ExecuteScript`.
pub fn maintenance_strip_ext_and_calc_prefix_len(
    base_name: &[u8],
    max_len: usize,
) -> (Vec<u8>, usize) {
    let bounded = &base_name[..base_name.len().min(max_len)];
    let stem_len = bounded
        .iter()
        .rposition(|&b| b == b'.')
        .unwrap_or(bounded.len());
    (bounded[..stem_len].to_vec(), stem_len.saturating_add(2))
}

/// Formats `"\"RSA-SHA256(<base_name>)\" : \""` for `Signature::ReadSignature`.
pub fn maintenance_format_sig_title(base_name: &[u8]) -> Vec<u8> {
    const PREFIX: &[u8] = b"\"RSA-SHA256(";
    const SUFFIX: &[u8] = b")\" : \"";
    let mut out = Vec::with_capacity(PREFIX.len() + base_name.len() + SUFFIX.len());
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(base_name);
    out.extend_from_slice(SUFFIX);
    out
}

/// Formats `"<msg> (<base_filename>:<line_nr>:<funcname>)"` or `"<msg> (<base_filename>:<line_nr>)"` for `debug()`.
pub fn log_format_debug_line(
    msg: &[u8],
    base_filename: &[u8],
    line_nr: i32,
    funcname: Option<&[u8]>,
) -> Vec<u8> {
    let line_str = format!(":{line_nr}");
    let mut out = Vec::with_capacity(
        msg.len() + 3 + base_filename.len() + line_str.len() + funcname.map_or(0, |f| 1 + f.len()),
    );
    out.extend_from_slice(msg);
    out.extend_from_slice(b" (");
    out.extend_from_slice(base_filename);
    out.extend_from_slice(line_str.as_bytes());
    if let Some(fn_bytes) = funcname {
        out.push(b':');
        out.extend_from_slice(fn_bytes);
    }
    out.push(b')');
    out
}

/// Formats `"<base_name>-####-##-##<base_ext>"` for `Log::RotateLog`.
pub fn log_format_rotate_mask(base_name: &[u8], base_ext: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(base_name.len() + 11 + base_ext.len());
    out.extend_from_slice(base_name);
    out.extend_from_slice(b"-####-##-##");
    out.extend_from_slice(base_ext);
    out
}

/// Formats `"Deleting old log-file <filename>\n"` for `Log::RotateLog`.
pub fn log_format_deleting_old_msg(filename: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(22 + filename.len() + 1);
    out.extend_from_slice(b"Deleting old log-file ");
    out.extend_from_slice(filename);
    out.push(b'\n');
    out
}

/// Formats `"<directory><sep><base_name>-<year>-<month:02>-<mday:02><base_ext>"` for `Log::RotateLog`.
pub fn log_format_rotated_path(
    directory: &[u8],
    sep: u8,
    base_name: &[u8],
    year: i32,
    month: i32,
    mday: i32,
    base_ext: &[u8],
) -> Vec<u8> {
    let date_part = format!("-{year}-{month:02}-{mday:02}");
    let mut out = Vec::with_capacity(
        directory.len() + 1 + base_name.len() + date_part.len() + base_ext.len(),
    );
    out.extend_from_slice(directory);
    out.push(sep);
    out.extend_from_slice(base_name);
    out.extend_from_slice(date_part.as_bytes());
    out.extend_from_slice(base_ext);
    out
}

/// Formats `"<pid>\n"` for `NZBGet::Daemonize`.
pub fn nzbget_format_pid_line(pid: i32) -> Vec<u8> {
    format!("{pid}\n").into_bytes()
}

/// Formats `"<major>.<minor>.<patch>"` from a packed 24-bit version integer for `SystemInfo`.
pub fn sysinfo_format_rapidyenc_version(ver: i32) -> Vec<u8> {
    let major = (ver >> 16) & 0xFF;
    let minor = (ver >> 8) & 0xFF;
    let patch = ver & 0xFF;
    format!("{major}.{minor}.{patch}").into_bytes()
}

/// Returns `true` when `username == b"root"` for `NZBGet::Daemonize`.
pub fn nzbget_is_backward_compat_root_user(username: &[u8]) -> bool {
    username == b"root"
}

/// Splits `base_name` at the last `.` (when `pos > 0`) into `(stem, ext_with_dot)` for `Log::RotateLog`.
pub fn log_split_basename_and_ext(base_name: &[u8]) -> (Vec<u8>, Vec<u8>) {
    match base_name.iter().rposition(|&b| b == b'.') {
        Some(pos) if pos > 0 => (base_name[..pos].to_vec(), base_name[pos..].to_vec()),
        _ => (base_name.to_vec(), Vec::new()),
    }
}

/// Returns `true` when `line` contains `marker` as a contiguous byte slice for `SystemInfo::GetUnpackerVersion`.
pub fn sysinfo_line_contains_marker(line: &[u8], marker: &[u8]) -> bool {
    if marker.is_empty() {
        return true;
    }
    line.windows(marker.len()).any(|w| w == marker)
}

/// Returns `Some(sig_title.len())` when `line` starts with `sig_title`, or `None` otherwise,
/// for `Signature::ReadSignature`.
pub fn maintenance_match_sig_line_prefix(line: &[u8], sig_title: &[u8]) -> Option<usize> {
    if line.starts_with(sig_title) {
        Some(sig_title.len())
    } else {
        None
    }
}

/// Parses `(tm_year, tm_mon, tm_mday)` from a rotated log filename at the given match offsets
/// for `Log::RotateLog`.
pub fn log_parse_rotated_date_fields(
    filename: &[u8],
    year_off: usize,
    mon_off: usize,
    mday_off: usize,
) -> (i32, i32, i32) {
    let year = crate::util::parse_c_atoi(filename.get(year_off..).unwrap_or(b"")) - 1900;
    let mon = crate::util::parse_c_atoi(filename.get(mon_off..).unwrap_or(b"")) - 1;
    let mday = crate::util::parse_c_atoi(filename.get(mday_off..).unwrap_or(b""));
    (year, mon, mday)
}

/// Evaluates whether a rotated log file should be deleted based on the date fields parsed from
/// `filename` at `(year_off, mon_off, mday_off)`, `cur_time` (seconds since epoch), and `rotate_log`.
pub fn log_should_delete_rotated_file_by_name(
    filename: &[u8],
    year_off: usize,
    mon_off: usize,
    mday_off: usize,
    cur_time: i64,
    rotate_log: i32,
) -> bool {
    let (tm_year, tm_mon, tm_mday) =
        log_parse_rotated_date_fields(filename, year_off, mon_off, mday_off);
    let file_time = crate::util::util_internal_timegm(tm_year, tm_mon, tm_mday, 0, 0, 0);
    let file_day = (file_time / 86_400) as i32;
    let cur_day = (cur_time / 86_400) as i32;
    crate::util::log_should_delete_old_rotated(file_day, cur_day, rotate_log)
}

/// Formats `"<directory><sep><base_name>-<year>-<month:02>-<mday:02><base_ext>"` for `cur_time`
/// (seconds since epoch, UTC civil date) for `Log::RotateLog`.
pub fn log_format_rotated_path_for_time(
    directory: &[u8],
    sep: u8,
    base_name: &[u8],
    cur_time: i64,
    base_ext: &[u8],
) -> Vec<u8> {
    let (year, month, mday) = crate::statmeter::days_to_ymd(cur_time.div_euclid(86_400));
    log_format_rotated_path(directory, sep, base_name, year as i32, month, mday, base_ext)
}

/// Executes `Log::RotateLog`: scans the log directory for rotated log files matching
/// `<stem>-####-##-##<ext>`, deletes expired files while emitting `"Deleting old log-file <name>\n"`
/// via `on_info`, and returns the rotated log path for `cur_time`.
#[cfg(unix)]
pub fn log_rotate_log_file(
    log_file: &[u8],
    sep: u8,
    cur_time: i64,
    rotate_log: i32,
    mut on_info: impl FnMut(&[u8]),
) -> Vec<u8> {
    let pos = crate::paths::base_file_name(log_file);
    let raw_base = &log_file[pos..];
    let (stem, base_ext) = log_split_basename_and_ext(raw_base);
    let directory: &[u8] = if pos > 0 { &log_file[..pos - 1] } else { &stem };
    let file_mask = log_format_rotate_mask(&stem, &base_ext);
    let lower = crate::wildmask::Lower::Fold(&crate::util::wild_mask_fold_locale);
    let mut captures = Vec::new();
    let dir_cstr = std::ffi::CString::new(directory).ok();
    let mut dir = crate::fsops::PosixDirBrowser::new(dir_cstr.as_deref(), true);
    loop {
        let ptr = dir.next_ptr();
        if ptr.is_null() {
            break;
        }
        let filename = unsafe { std::ffi::CStr::from_ptr(ptr).to_bytes() };
        captures.clear();
        if crate::wildmask::wild_match(&lower, &file_mask, filename, Some(&mut captures)) {
            let full_filename = [directory, &[sep], filename].concat();
            let m0 = captures.first().map_or(0, |&(s, _)| s.max(0) as usize);
            let m1 = captures.get(1).map_or(0, |&(s, _)| s.max(0) as usize);
            let m2 = captures.get(2).map_or(0, |&(s, _)| s.max(0) as usize);
            if log_should_delete_rotated_file_by_name(
                filename, m0, m1, m2, cur_time, rotate_log,
            ) {
                let msg = log_format_deleting_old_msg(filename);
                on_info(&msg);
                if let Ok(full_cstr) = std::ffi::CString::new(full_filename) {
                    let _ = crate::fsops::delete_file(&full_cstr);
                }
            }
        }
    }
    log_format_rotated_path_for_time(directory, sep, &stem, cur_time, &base_ext)
}

/// Extracts the substring after the first ASCII space in `text` (or `text` if no space exists),
/// for `SystemInfo::InitLibraries`.
pub fn sysinfo_extract_version_after_space(text: &[u8]) -> Vec<u8> {
    match text.iter().position(|&b| b == b' ') {
        Some(idx) => text[idx + 1..].to_vec(),
        None => text.to_vec(),
    }
}

/// Classifies an `/etc/os-release` line for `OS::InitOSInfoFromOSRelease`:
/// `(1, 5)` for `NAME=`, `(2, 11)` for `VERSION_ID=`, `(3, 9)` for `BUILD_ID=`, or `(0, 0)`.
pub fn os_classify_os_release_line(
    line: &[u8],
    has_name: bool,
    has_version: bool,
) -> (i32, usize) {
    if !has_name && line.starts_with(b"NAME=") {
        (1, 5)
    } else if !has_version && line.starts_with(b"VERSION_ID=") {
        (2, 11)
    } else if !has_version && line.starts_with(b"BUILD_ID=") {
        (3, 9)
    } else {
        (0, 0)
    }
}

/// Returns true when `DiskService::ServiceWork` should check free disk space.
pub fn disk_service_should_check_disk_space(disk_space_mb: i32, is_downloading: bool) -> bool {
    disk_space_mb > 0 && is_downloading
}

/// Evaluates a required directory check in `DiskService::CheckRequiredDir`:
/// returns `(is_missing, should_log_waiting, next_waiting_reported)`.
pub fn disk_service_eval_missing_required_dir(
    file_exists: bool,
    dir_exists: bool,
    was_waiting_reported: bool,
    cur_waiting_reported: bool,
) -> (bool, bool, bool) {
    if !file_exists && !dir_exists {
        let should_log = !was_waiting_reported;
        let next_reported = if should_log {
            true
        } else {
            cur_waiting_reported
        };
        (true, should_log, next_reported)
    } else {
        (false, false, cur_waiting_reported)
    }
}

/// Returns true when `NZBGet::Phase1_Init` should run the system health check.
pub fn nzbget_should_run_system_health_check(
    server_mode: bool,
    system_health_check: bool,
) -> bool {
    server_mode && system_health_check
}

/// Evaluates whether `NZBGet::BootConfig` should print usage and exit, returning the exit code (`0` or `1`).
pub fn nzbget_classify_boot_usage_or_error(
    print_usage: bool,
    has_errors: bool,
    arg_count: i32,
) -> Option<i32> {
    if print_usage || has_errors || arg_count <= 1 {
        Some(if print_usage { 0 } else { 1 })
    } else {
        None
    }
}

/// Returns true when `NZBGet::StopFrontend` should immediately stop the frontend thread before waiting.
pub fn nzbget_should_stop_frontend_immediately(
    remote_client_mode: bool,
    stopped: bool,
) -> bool {
    !remote_client_mode || stopped
}

/// Classifies the log file initialization action in `Log::InitOptions`:
/// `0` = skip, `1` = update log path only, `2` = update log path and reset log.
pub fn log_classify_init_logfile_action(
    remote_client_mode: bool,
    direct_client_mode: bool,
    write_log: i32,
    wl_none: i32,
    wl_reset: i32,
    has_log_file: bool,
    server_mode: bool,
) -> i32 {
    if !remote_client_mode && !direct_client_mode && write_log != wl_none && has_log_file {
        if server_mode && write_log == wl_reset {
            2
        } else {
            1
        }
    } else {
        0
    }
}

// ============================================================================
// System (`SystemInfo`, `OS`, `CPU`) Helpers
// ============================================================================

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    find_subslice(haystack, needle).is_some()
}

/// Port of `SystemInfo::GetPythonVersion` string extraction after initial `Util::Trim`.
/// Given `trimmed_line`, locates `"Python"`, skips `"Python"` + 1 byte (matching
/// `pos + sizeof("Python")`), trims ASCII whitespace/control bytes (`<= b' '`),
/// and returns `Some((start_offset, len))`.
pub fn sys_extract_python_version(trimmed_line: &[u8]) -> Option<(usize, usize)> {
    const MARKER: &[u8] = b"Python";
    const SKIP_LEN: usize = MARKER.len() + 1; // sizeof("Python") == 7
    let pos = find_subslice(trimmed_line, MARKER)?;
    if pos + SKIP_LEN >= trimmed_line.len() {
        return Some((trimmed_line.len(), 0));
    }
    let mut start = pos + SKIP_LEN;
    let mut end = trimmed_line.len();
    while start < end && trimmed_line[start] <= b' ' {
        start += 1;
    }
    while end > start && trimmed_line[end - 1] <= b' ' {
        end -= 1;
    }
    Some((start, end - start))
}

/// Port of `SystemInfo::ParseUnpackerVersion`.
/// Finds the first `[0-9]+\.[0-9]+` span in `line` and returns `Some((start, len))`.
pub fn sys_parse_unpacker_version(line: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    let n = line.len();
    while i < n {
        if line[i].is_ascii_digit() {
            let start = i;
            while i < n && line[i].is_ascii_digit() {
                i += 1;
            }
            if i < n && line[i] == b'.' && i + 1 < n && line[i + 1].is_ascii_digit() {
                i += 1;
                while i < n && line[i].is_ascii_digit() {
                    i += 1;
                }
                return Some((start, i - start));
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Port of `SystemInfo::ParseParVersion`.
/// Returns the index of the last `' '` byte in `line` (matching `line.find_last_of(" ")`).
pub fn sys_parse_par_version(line: &[u8]) -> Option<usize> {
    line.iter().rposition(|&b| b == b' ')
}

/// Port of `OS::TrimQuotes`.
/// Strips at most one leading `"` and at most one trailing `"` and returns `(start, len)`.
pub fn os_trim_quotes(s: &[u8]) -> (usize, usize) {
    let mut start = 0;
    let mut end = s.len();
    if start < end && s[start] == b'"' {
        start += 1;
    }
    if start < end && s[end - 1] == b'"' {
        end -= 1;
    }
    (start, end - start)
}

/// Port of `OS::Init` (`__APPLE__`) field extraction from `sw_vers` output.
/// Finds `prefix` in `output` and returns `(val_start, val_len)` up to the next `'\n'`.
pub fn os_parse_sw_vers_field(output: &[u8], prefix: &[u8]) -> Option<(usize, usize)> {
    let pos = find_subslice(output, prefix)?;
    let val_start = pos + prefix.len();
    let end_pos = output[pos..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|rel| pos + rel)
        .unwrap_or(output.len());
    let val_len = end_pos.saturating_sub(val_start);
    Some((val_start, val_len))
}

/// Port of `CPU::GetCanonicalCPUArch`.
/// Returns `Some(canonical_cstr)` if `arch` maps to a canonical architecture, or `None`
/// if `arch` should be returned unchanged.
pub fn cpu_canonical_arch(arch: &[u8]) -> Option<&'static std::ffi::CStr> {
    if arch == b"i386" || arch == b"x86" {
        return Some(c"i686");
    }
    if arch == b"amd64" || arch == b"AMD64" {
        return Some(c"x86_64");
    }
    if contains_subslice(arch, b"armv5") || contains_subslice(arch, b"armv6") {
        return Some(c"armel");
    }
    if contains_subslice(arch, b"armv7") || contains_subslice(arch, b"armv8") {
        return Some(c"armhf");
    }
    None
}

/// Port of `CPU::GetCPUModelFromCPUInfo` line matching.
/// If `line` contains one of the recognized CPU model keys and a `':'`, returns
/// `Some(colon_pos + 1)`.
pub fn cpu_parse_cpuinfo_line(line: &[u8]) -> Option<usize> {
    let matched = contains_subslice(line, b"model name")
        || contains_subslice(line, b"Processor")
        || contains_subslice(line, b"cpu model")
        || contains_subslice(line, b"Hardware")
        || contains_subslice(line, b"system type");
    if !matched {
        return None;
    }
    let colon_pos = line.iter().position(|&b| b == b':')?;
    Some(colon_pos + 1)
}

fn trim_ascii_ctrl_ws(mut s: &[u8]) -> &[u8] {
    while let Some(&b) = s.first() {
        if b <= b' ' {
            s = &s[1..];
        } else {
            break;
        }
    }
    while let Some(&b) = s.last() {
        if b <= b' ' {
            s = &s[..s.len() - 1];
        } else {
            break;
        }
    }
    s
}

/// Parses `/proc/cpuinfo` bytes and extracts the first non-empty CPU model name.
pub fn cpu_parse_cpuinfo_bytes(content: &[u8]) -> Option<Vec<u8>> {
    for line in content.split(|&b| b == b'\n') {
        if let Some(val_off) = cpu_parse_cpuinfo_line(line) {
            let trimmed = trim_ascii_ctrl_ws(&line[val_off..]);
            if !trimmed.is_empty() {
                return Some(trimmed.to_vec());
            }
        }
    }
    None
}

/// Detects `(cpu_model, cpu_arch)` natively in Rust (`daemon/system/CPU.cpp`).
pub fn cpu_detect_info(raw_uts_machine: &[u8]) -> (Vec<u8>, Vec<u8>) {
    #[cfg(target_os = "macos")]
    extern "C" {
        fn sysctlbyname(
            name: *const std::ffi::c_char,
            oldp: *mut std::ffi::c_void,
            oldlenp: *mut usize,
            newp: *const std::ffi::c_void,
            newlen: usize,
        ) -> std::ffi::c_int;
    }

    #[cfg(any(
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    extern "C" {
        fn sysctl(
            name: *mut std::ffi::c_int,
            namelen: std::ffi::c_uint,
            oldp: *mut std::ffi::c_void,
            oldlenp: *mut usize,
            newp: *const std::ffi::c_void,
            newlen: usize,
        ) -> std::ffi::c_int;
    }

    let raw_arch: Vec<u8> = if !raw_uts_machine.is_empty() {
        raw_uts_machine.to_vec()
    } else {
        std::env::consts::ARCH.as_bytes().to_vec()
    };
    let arch = if let Some(canon) = cpu_canonical_arch(&raw_arch) {
        canon.to_bytes().to_vec()
    } else {
        raw_arch
    };

    #[allow(unused_mut)]
    let mut model = Vec::new();

    #[cfg(target_os = "linux")]
    {
        if let Ok(bytes) = std::fs::read("/proc/cpuinfo") {
            if let Some(m) = cpu_parse_cpuinfo_bytes(&bytes) {
                model = m;
            }
        }
    }

    #[cfg(target_os = "macos")]
    unsafe {
        let mut buf = [0u8; 256];
        let mut len = buf.len();
        if sysctlbyname(
            c"machdep.cpu.brand_string".as_ptr(),
            buf.as_mut_ptr().cast::<std::ffi::c_void>(),
            &mut len,
            std::ptr::null(),
            0,
        ) == 0
        {
            let slice = &buf[..len.min(buf.len())];
            let nul_pos = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
            model = trim_ascii_ctrl_ws(&slice[..nul_pos]).to_vec();
        }
    }

    #[cfg(any(
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    unsafe {
        // CTL_HW = 6, HW_MODEL = 2 across BSDs
        let mut mib: [std::ffi::c_int; 2] = [6, 2];
        let mut buf = [0u8; 256];
        let mut len = buf.len();
        if sysctl(
            mib.as_mut_ptr(),
            2,
            buf.as_mut_ptr().cast::<std::ffi::c_void>(),
            &mut len,
            std::ptr::null(),
            0,
        ) != -1
        {
            let slice = &buf[..len.min(buf.len())];
            let nul_pos = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
            model = trim_ascii_ctrl_ws(&slice[..nul_pos]).to_vec();
        }
    }

    (model, arch)
}

/// Parses `/etc/os-release` bytes into `(name, version)` matching `OS::InitOSInfoFromOSRelease`.
pub fn os_parse_os_release_bytes(content: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut name = Vec::new();
    let mut version = Vec::new();
    for raw_line in content.split(|&b| b == b'\n') {
        if !name.is_empty() && !version.is_empty() {
            break;
        }
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        let (kind, val_off) =
            os_classify_os_release_line(line, !name.is_empty(), !version.is_empty());
        if kind == 1 {
            let trimmed = trim_ascii_ctrl_ws(&line[val_off..]);
            let (qs, ql) = os_trim_quotes(trimmed);
            name = trimmed[qs..qs + ql].to_vec();
        } else if kind == 2 {
            let trimmed = trim_ascii_ctrl_ws(&line[val_off..]);
            let (qs, ql) = os_trim_quotes(trimmed);
            version = trimmed[qs..qs + ql].to_vec();
        } else if kind == 3 {
            version = trim_ascii_ctrl_ws(&line[val_off..]).to_vec();
        }
    }
    (name, version)
}

/// Parses `sw_vers` stdout into `(name, version)` matching `OS::Init` on macOS.
pub fn os_parse_sw_vers_output(output: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let name = os_parse_sw_vers_field(output, b"ProductName:")
        .map(|(s, l)| trim_ascii_ctrl_ws(&output[s..s + l]).to_vec())
        .unwrap_or_default();
    let version = os_parse_sw_vers_field(output, b"ProductVersion:")
        .map(|(s, l)| trim_ascii_ctrl_ws(&output[s..s + l]).to_vec())
        .unwrap_or_default();
    (name, version)
}

/// Detects `(os_name, os_version)` natively in Rust (`daemon/system/OS.cpp`).
pub fn os_detect_info(fallback_sysname: &[u8], fallback_release: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let _ = (fallback_sysname, fallback_release);
    #[cfg(target_os = "linux")]
    {
        let (mut name, mut version) = std::fs::read("/etc/os-release")
            .map(|b| os_parse_os_release_bytes(&b))
            .unwrap_or_default();
        if name.is_empty() {
            if !fallback_sysname.is_empty() {
                name = fallback_sysname.to_vec();
            } else if let Ok(b) = std::fs::read("/proc/sys/kernel/ostype") {
                name = trim_ascii_ctrl_ws(&b).to_vec();
            }
        }
        if version.is_empty() {
            if !fallback_release.is_empty() {
                version = fallback_release.to_vec();
            } else if let Ok(b) = std::fs::read("/proc/sys/kernel/osrelease") {
                version = trim_ascii_ctrl_ws(&b).to_vec();
            }
        }
        if Path::new("/.dockerenv").is_file() {
            version.extend_from_slice(b" (Running in Docker)");
        } else if std::env::var_os("container").is_some()
            || Path::new("/run/systemd/container").is_file()
            || Path::new("/run/.containerenv").is_file()
        {
            version.extend_from_slice(b" (Running in Container)");
        }
        return (name, version);
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("sw_vers").output() {
            return os_parse_sw_vers_output(&out.stdout);
        }
        return (Vec::new(), Vec::new());
    }

    #[cfg(any(
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    unsafe {
        extern "C" {
            fn sysctl(
                name: *mut std::ffi::c_int,
                namelen: std::ffi::c_uint,
                oldp: *mut std::ffi::c_void,
                oldlenp: *mut usize,
                newp: *const std::ffi::c_void,
                newlen: usize,
            ) -> std::ffi::c_int;
        }
        let mut name = Vec::new();
        let mut version = Vec::new();
        // CTL_KERN = 1, KERN_OSTYPE = 1, KERN_OSRELEASE = 2 across BSDs
        let mut mib: [std::ffi::c_int; 2] = [1, 1];
        let mut buf = [0u8; 128];
        let mut len = buf.len();
        if sysctl(
            mib.as_mut_ptr(),
            2,
            buf.as_mut_ptr().cast::<std::ffi::c_void>(),
            &mut len,
            std::ptr::null(),
            0,
        ) != -1
        {
            let slice = &buf[..len.min(buf.len())];
            let nul_pos = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
            name = trim_ascii_ctrl_ws(&slice[..nul_pos]).to_vec();
        }
        len = buf.len();
        mib[1] = 2;
        if sysctl(
            mib.as_mut_ptr(),
            2,
            buf.as_mut_ptr().cast::<std::ffi::c_void>(),
            &mut len,
            std::ptr::null(),
            0,
        ) != -1
        {
            let slice = &buf[..len.min(buf.len())];
            let nul_pos = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
            version = trim_ascii_ctrl_ws(&slice[..nul_pos]).to_vec();
        }
        return (name, version);
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )))]
    {
        (fallback_sysname.to_vec(), fallback_release.to_vec())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemToolView<'a> {
    pub name: &'a [u8],
    pub version: &'a [u8],
    pub path: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemLibraryView<'a> {
    pub name: &'a [u8],
    pub version: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInfoView<'a> {
    pub os_name: &'a [u8],
    pub os_version: &'a [u8],
    pub cpu_model: &'a [u8],
    pub cpu_arch: &'a [u8],
    pub public_ip: &'a [u8],
    pub private_ip: &'a [u8],
    pub tools: &'a [SystemToolView<'a>],
    pub libraries: &'a [SystemLibraryView<'a>],
}

fn write_boost_json_str(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'"');
    for &b in bytes {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\x08' => out.extend_from_slice(b"\\b"),
            b'\x0C' => out.extend_from_slice(b"\\f"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x00..=0x1F => {
                let hex = format!("\\u{b:04x}");
                out.extend_from_slice(hex.as_bytes());
            }
            _ => out.push(b),
        }
    }
    out.push(b'"');
}

fn write_libxml2_member(out: &mut Vec<u8>, name: &[u8], tag: &[u8], value: &[u8]) {
    out.extend_from_slice(b"<member><name>");
    write_libxml2_escaped(out, name);
    out.extend_from_slice(b"</name><value>");
    if value.is_empty() {
        out.push(b'<');
        out.extend_from_slice(tag);
        out.extend_from_slice(b"/>");
    } else {
        out.push(b'<');
        out.extend_from_slice(tag);
        out.push(b'>');
        write_libxml2_escaped(out, value);
        out.extend_from_slice(b"</");
        out.extend_from_slice(tag);
        out.push(b'>');
    }
    out.extend_from_slice(b"</value></member>");
}

fn write_libxml2_escaped(out: &mut Vec<u8>, bytes: &[u8]) {
    for &b in bytes {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'\r' => out.extend_from_slice(b"&#13;"),
            _ => out.push(b),
        }
    }
}

/// Formats `System::SystemInfo` as JSON matching `System::ToJsonStr` (`daemon/system/SystemInfo.cpp`).
pub fn system_info_to_json(view: &SystemInfoView<'_>) -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    out.extend_from_slice(b"{\"OS\":{\"Name\":");
    write_boost_json_str(&mut out, view.os_name);
    out.extend_from_slice(b",\"Version\":");
    write_boost_json_str(&mut out, view.os_version);
    out.extend_from_slice(b"},\"CPU\":{\"Model\":");
    write_boost_json_str(&mut out, view.cpu_model);
    out.extend_from_slice(b",\"Arch\":");
    write_boost_json_str(&mut out, view.cpu_arch);
    out.extend_from_slice(b"},\"Network\":{\"PublicIP\":");
    write_boost_json_str(&mut out, view.public_ip);
    out.extend_from_slice(b",\"PrivateIP\":");
    write_boost_json_str(&mut out, view.private_ip);
    out.extend_from_slice(b"},\"Tools\":[");
    for (i, tool) in view.tools.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(b"{\"Name\":");
        write_boost_json_str(&mut out, tool.name);
        out.extend_from_slice(b",\"Version\":");
        write_boost_json_str(&mut out, tool.version);
        out.extend_from_slice(b",\"Path\":");
        write_boost_json_str(&mut out, tool.path);
        out.push(b'}');
    }
    out.extend_from_slice(b"],\"Libraries\":[");
    for (i, lib) in view.libraries.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(b"{\"Name\":");
        write_boost_json_str(&mut out, lib.name);
        out.extend_from_slice(b",\"Version\":");
        write_boost_json_str(&mut out, lib.version);
        out.push(b'}');
    }
    out.extend_from_slice(b"]}");
    out
}

/// Formats `System::SystemInfo` as XML-RPC `<value><struct>...</struct></value>` matching `System::ToXmlStr`.
pub fn system_info_to_xml(view: &SystemInfoView<'_>) -> Vec<u8> {
    let mut out = Vec::with_capacity(1024);
    out.extend_from_slice(b"<value><struct><OS>");
    write_libxml2_member(&mut out, b"Name", b"string", view.os_name);
    write_libxml2_member(&mut out, b"Version", b"string", view.os_version);
    out.extend_from_slice(b"</OS><CPU>");
    write_libxml2_member(&mut out, b"Model", b"string", view.cpu_model);
    write_libxml2_member(&mut out, b"Arch", b"string", view.cpu_arch);
    out.extend_from_slice(b"</CPU><Network>");
    write_libxml2_member(&mut out, b"PublicIP", b"string", view.public_ip);
    write_libxml2_member(&mut out, b"PrivateIP", b"string", view.private_ip);
    out.extend_from_slice(b"</Network><Tools>");
    for tool in view.tools {
        write_libxml2_member(&mut out, b"Name", b"string", tool.name);
        write_libxml2_member(&mut out, b"Version", b"string", tool.version);
        write_libxml2_member(&mut out, b"Path", b"string", tool.path);
    }
    out.extend_from_slice(b"</Tools><Libraries>");
    for lib in view.libraries {
        write_libxml2_member(&mut out, b"Name", b"string", lib.name);
        write_libxml2_member(&mut out, b"Version", b"string", lib.version);
    }
    out.extend_from_slice(b"</Libraries></struct></value>");
    out
}

/// Formats `System::SystemInfo` human-readable text matching `operator<<`.
pub fn system_info_to_display_text(view: &SystemInfoView<'_>) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    out.extend_from_slice(b"OS: ");
    out.extend_from_slice(view.os_name);
    out.push(b' ');
    out.extend_from_slice(view.os_version);
    out.push(b'\n');
    out.extend_from_slice(b"CPU: ");
    out.extend_from_slice(view.cpu_model);
    out.push(b'\n');
    out.extend_from_slice(b"Arch: ");
    out.extend_from_slice(view.cpu_arch);
    out.push(b'\n');
    for tool in view.tools {
        out.extend_from_slice(tool.name);
        out.push(b' ');
        out.extend_from_slice(tool.version);
        out.push(b' ');
        out.extend_from_slice(tool.path);
        out.push(b'\n');
    }
    for lib in view.libraries {
        out.extend_from_slice(lib.name);
        out.push(b' ');
        out.extend_from_slice(lib.version);
        out.push(b'\n');
    }
    out
}

/// Formats `System::SystemInfo` as display text (`mode == 0`), JSON (`mode == 1`), or XML (`mode == 2`),
/// detecting OS and CPU info directly in Rust and building the default built-in tools and libraries lists.
pub fn system_info_format(
    mode: i32,
    uts_sysname: &[u8],
    uts_release: &[u8],
    uts_machine: &[u8],
    ssl_lib_name: &[u8],
    ssl_raw_version: &[u8],
    ssl_extract_after_space: bool,
    public_ip: &[u8],
    private_ip: &[u8],
) -> Vec<u8> {
    let mut uname_sys = Vec::new();
    let mut uname_rel = Vec::new();
    let mut uname_mach = Vec::new();
    let (uts_sysname, uts_release, uts_machine) =
        if uts_sysname.is_empty() && uts_release.is_empty() && uts_machine.is_empty() {
            #[cfg(unix)]
            unsafe {
                let mut uts: libc::utsname = std::mem::zeroed();
                if libc::uname(&mut uts) == 0 {
                    uname_sys = std::ffi::CStr::from_ptr(uts.sysname.as_ptr())
                        .to_bytes()
                        .to_vec();
                    uname_rel = std::ffi::CStr::from_ptr(uts.release.as_ptr())
                        .to_bytes()
                        .to_vec();
                    uname_mach = std::ffi::CStr::from_ptr(uts.machine.as_ptr())
                        .to_bytes()
                        .to_vec();
                }
            }
            (
                uname_sys.as_slice(),
                uname_rel.as_slice(),
                uname_mach.as_slice(),
            )
        } else {
            (uts_sysname, uts_release, uts_machine)
        };
    let (os_name, os_version) = os_detect_info(uts_sysname, uts_release);
    let (cpu_model, cpu_arch) = cpu_detect_info(uts_machine);
    let tools = [
        SystemToolView {
            name: b"UnRAR / 7-Zip / Archive Engine",
            version: b"Built-in",
            path: b"zipfast + zipview + native Rust decoders",
        },
        SystemToolView {
            name: b"Parity Verification & Repair",
            version: b"Built-in",
            path: b"par-rs (streaming + post-download)",
        },
        SystemToolView {
            name: b"Extended Unpacker & Permission Fixer",
            version: b"Built-in",
            path: b"First-class native pipeline",
        },
        SystemToolView {
            name: b"yEnc SIMD Decoder",
            version: b"Built-in",
            path: b"nntmux-yenc",
        },
    ];
    let ssl_ver_extracted;
    let mut libraries = vec![
        SystemLibraryView { name: b"zipfast", version: b"0.1.0" },
        SystemLibraryView { name: b"zipview", version: b"0.1.0" },
        SystemLibraryView { name: b"par-rs", version: b"0.2.0" },
        SystemLibraryView { name: b"nntmux-yenc", version: b"0.1.0 (SIMD)" },
        SystemLibraryView { name: b"flate2 (zlib-rs)", version: b"1.1.11" },
        SystemLibraryView { name: b"zstd", version: b"0.14.0" },
        SystemLibraryView { name: b"bzip2", version: b"0.6.1" },
        SystemLibraryView { name: b"crc32fast", version: b"1.5.2" },
        SystemLibraryView { name: b"quick-xml", version: b"0.41.0" },
        SystemLibraryView { name: b"md-5", version: b"0.11.0" },
    ];
    if !ssl_lib_name.is_empty() {
        let ver = if ssl_extract_after_space {
            ssl_ver_extracted = sysinfo_extract_version_after_space(ssl_raw_version);
            ssl_ver_extracted.as_slice()
        } else {
            ssl_raw_version
        };
        libraries.push(SystemLibraryView {
            name: ssl_lib_name,
            version: ver,
        });
    }
    let view = SystemInfoView {
        os_name: &os_name,
        os_version: &os_version,
        cpu_model: &cpu_model,
        cpu_arch: &cpu_arch,
        public_ip,
        private_ip,
        tools: &tools,
        libraries: &libraries,
    };
    match mode {
        1 => system_info_to_json(&view),
        2 => system_info_to_xml(&view),
        _ => system_info_to_display_text(&view),
    }
}

/// Connects to `host` (`port = 443` when `use_tls` else `80`) with a 10-second socket
/// timeout, queries the local socket IP (`getsockname`), exchanges the HTTP GET request
/// (either via `tls_exchange` on the connected fd or directly over the TCP stream),
/// and parses the HTTP 200 response body as `(public_ip, private_ip)`.
pub fn system_info_fetch_network<F>(
    host: &str,
    use_tls: bool,
    include_gzip: bool,
    version_revision: &[u8],
    tls_exchange: F,
) -> Result<(Vec<u8>, Vec<u8>), String>
where
    F: FnOnce(i32, &std::ffi::CStr, &[u8]) -> Result<Vec<u8>, String>,
{
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let port = if use_tls { 443u16 } else { 80u16 };
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|_| format!("Failed to resolve host: {host}"))?;

    let timeout = Duration::from_secs(10);
    let mut connected: Option<TcpStream> = None;
    for addr in addrs {
        if let Ok(stream) = TcpStream::connect_timeout(&addr, timeout) {
            let _ = stream.set_read_timeout(Some(timeout));
            let _ = stream.set_write_timeout(Some(timeout));
            connected = Some(stream);
            break;
        }
    }
    let mut stream = connected.ok_or_else(|| format!("Failed to connect to host: {host}"))?;
    let local_ip = stream
        .local_addr()
        .map(|a| a.ip().to_string().into_bytes())
        .unwrap_or_default();

    let req = crate::webdownload::http_client_build_headers(
        b"GET",
        host.as_bytes(),
        version_revision,
        include_gzip,
    );

    let raw_resp = if use_tls {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let fd = stream.as_raw_fd();
            let host_c = std::ffi::CString::new(host).unwrap_or_default();
            tls_exchange(fd, &host_c, &req)?
        }
        #[cfg(not(unix))]
        {
            let _ = tls_exchange;
            return Err("TLS not supported on non-unix".to_string());
        }
    } else {
        let _ = stream.write_all(&req);
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf);
        buf
    };

    let (status_code, body) = crate::webdownload::http_client_parse_response(&raw_resp)
        .ok_or_else(|| "Invalid HTTP response".to_string())?;
    if status_code == 200 {
        Ok((body, local_ip))
    } else {
        Ok((Vec::new(), Vec::new()))
    }
}

/// Formats a single `<struct><member>...</member></struct>` XML node matching `Xml::CreateStructNode` + `Xml::AddNewNode` + `Xml::Serialize`.
pub fn xml_serialize_struct_member(name: &[u8], tag: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::from(&b"<struct>"[..]);
    write_libxml2_member(&mut out, name, tag, value);
    out.extend_from_slice(b"</struct>");
    out
}

pub struct HealthServerInput<'a> {
    pub id: i32,
    pub section: NewsServerSectionInput<'a>,
}

pub struct HealthTaskInput {
    pub id: i32,
    pub hours: i32,
    pub minutes: i32,
    pub week_days_bits: i32,
}

pub struct HealthCategoryInput<'a> {
    pub name: &'a [u8],
    pub unpack: bool,
}

pub struct HealthFeedInput<'a> {
    pub id: i32,
    pub name: &'a [u8],
    pub url: &'a [u8],
    pub interval: i32,
    pub cert_level: u32,
}

pub struct SystemHealthDiagnoseInput<'a> {
    pub paths: PathsSectionInput<'a>,
    pub servers: &'a [HealthServerInput<'a>],
    pub output_mode: i32,
    pub update_interval: i32,
    pub curses_nzb_name: bool,
    pub curses_group: bool,
    pub curses_time: bool,
    pub rotate_log: i32,
    pub log_buffer: i32,
    pub crash_dump: bool,
    pub time_correction: i32,
    pub article_retries: i32,
    pub hc_dupe: bool,
    pub dupe_search: bool,
    pub article_interval: i32,
    pub article_timeout: i32,
    pub article_read_chunk_size: i32,
    pub url_retries: i32,
    pub url_interval: i32,
    pub url_timeout: i32,
    pub url_connections: i32,
    pub remote_timeout: i32,
    pub download_rate: i32,
    pub monthly_quota: i32,
    pub quota_start_day: i32,
    pub daily_quota: i32,
    pub flush_queue: bool,
    pub continue_partial: bool,
    pub propagation_delay: i32,
    pub article_cache: i32,
    pub direct_write: bool,
    pub write_buffer: i32,
    pub file_naming: i32,
    pub post_strategy: i32,
    pub disk_space: i32,
    pub nzb_cleanup_disk: bool,
    pub keep_history: i32,
    pub feed_history: i32,
    pub skip_write: bool,
    pub raw_article: bool,
    pub security: SecuritySectionInput<'a>,
    pub append_category_dir: bool,
    pub nzb_dir_interval: i32,
    pub nzb_dir_file_age: i32,
    pub dupe_check: bool,
    pub health_check: i32,
    pub crc_check: bool,
    pub par_check: i32,
    pub par_repair: bool,
    pub par_quick: bool,
    pub par_buffer: i32,
    pub par_threads: i32,
    pub hw_threads: usize,
    pub par_rename: bool,
    pub rar_rename: bool,
    pub direct_rename: bool,
    pub hard_linking: bool,
    pub par_time_limit: i32,
    pub unpack: bool,
    pub direct_unpack: bool,
    pub unpack_pause_queue: bool,
    pub unrar_path: &'a Path,
    pub seven_zip_path: &'a Path,
    pub ext_cleanup_disk: &'a [u8],
    pub unpack_pass_file: &'a Path,
    pub unpack_ignore_ext: &'a [u8],
    pub options_log_file: &'a Path,
    pub tasks: &'a [HealthTaskInput],
    pub categories: &'a [HealthCategoryInput<'a>],
    pub feeds: &'a [HealthFeedInput<'a>],
}

struct HealthOptionEntry {
    name: String,
    severity: Severity,
    message: Vec<u8>,
}

struct HealthSubsectionEntry {
    name: String,
    options: Vec<HealthOptionEntry>,
}

struct HealthSectionEntry {
    name: String,
    issues: Vec<HealthStatus>,
    options: Vec<HealthOptionEntry>,
    subsections: Vec<HealthSubsectionEntry>,
}

fn push_health_option(
    options: &mut Vec<HealthOptionEntry>,
    name: &str,
    sev: Severity,
    msg: &[u8],
) {
    if sev != Severity::Ok {
        options.push(HealthOptionEntry {
            name: name.to_string(),
            severity: sev,
            message: msg.to_vec(),
        });
    }
}

fn build_health_sections(input: &SystemHealthDiagnoseInput<'_>) -> Vec<HealthSectionEntry> {
    let mut sections = Vec::with_capacity(13);

    // 1. Paths
    {
        let mut options = Vec::new();
        validate_paths_section(&input.paths, |name, sev, msg| {
            push_health_option(&mut options, name, sev, msg);
        });
        sections.push(HealthSectionEntry {
            name: "Paths".to_string(),
            issues: Vec::new(),
            options,
            subsections: Vec::new(),
        });
    }

    // 2. NewsServers
    {
        let mut subsections = Vec::with_capacity(input.servers.len());
        let mut hosts = Vec::with_capacity(input.servers.len());
        let mut actives = Vec::with_capacity(input.servers.len());
        let mut max_conns = Vec::with_capacity(input.servers.len());
        let mut levels = Vec::with_capacity(input.servers.len());

        for srv in input.servers {
            let mut sub_opts = Vec::new();
            validate_news_server_section(&srv.section, |name, sev, msg| {
                push_health_option(&mut sub_opts, name, sev, msg);
            });
            subsections.push(HealthSubsectionEntry {
                name: format!("Server{}", srv.id),
                options: sub_opts,
            });
            hosts.push(srv.section.host);
            actives.push(u8::from(srv.section.active));
            max_conns.push(srv.section.max_connections);
            levels.push(srv.section.level);
        }

        let mut issues = Vec::new();
        validate_news_servers_group(&hosts, &actives, &max_conns, &levels, |sev, msg| {
            if sev != Severity::Ok {
                issues.push(HealthStatus {
                    severity: sev,
                    message: msg.to_vec(),
                });
            }
        });

        sections.push(HealthSectionEntry {
            name: "NewsServers".to_string(),
            issues,
            options: Vec::new(),
            subsections,
        });
    }

    // 3. Display
    {
        let mut options = Vec::new();
        validate_display_section(
            input.output_mode,
            input.update_interval,
            input.curses_nzb_name,
            input.curses_group,
            input.curses_time,
            |name, sev, msg| push_health_option(&mut options, name, sev, msg),
        );
        sections.push(HealthSectionEntry {
            name: "Display".to_string(),
            issues: Vec::new(),
            options,
            subsections: Vec::new(),
        });
    }

    // 4. Logging
    {
        let mut options = Vec::new();
        validate_logging_section(
            input.paths.write_log,
            input.options_log_file,
            input.rotate_log,
            input.log_buffer,
            input.crash_dump,
            input.time_correction,
            |name, sev, msg| push_health_option(&mut options, name, sev, msg),
        );
        sections.push(HealthSectionEntry {
            name: "Logging".to_string(),
            issues: Vec::new(),
            options,
            subsections: Vec::new(),
        });
    }

    // 6. Connection
    {
        let mut options = Vec::new();
        validate_connection_section(
            input.article_retries,
            input.hc_dupe,
            input.dupe_search,
            input.article_interval,
            input.article_timeout,
            input.article_read_chunk_size,
            input.url_retries,
            input.url_interval,
            input.url_timeout,
            input.url_connections,
            input.remote_timeout,
            input.download_rate,
            input.monthly_quota,
            input.quota_start_day,
            input.daily_quota,
            |name, sev, msg| push_health_option(&mut options, name, sev, msg),
        );
        sections.push(HealthSectionEntry {
            name: "Connection".to_string(),
            issues: Vec::new(),
            options,
            subsections: Vec::new(),
        });
    }

    // 7. DownloadQueue
    {
        let mut options = Vec::new();
        validate_download_queue_section(
            input.flush_queue,
            input.continue_partial,
            input.propagation_delay,
            input.article_cache,
            input.direct_write,
            input.write_buffer,
            input.file_naming,
            input.post_strategy,
            input.disk_space,
            input.nzb_cleanup_disk,
            input.keep_history,
            input.feed_history,
            input.skip_write,
            input.raw_article,
            |name, sev, msg| push_health_option(&mut options, name, sev, msg),
        );
        sections.push(HealthSectionEntry {
            name: "DownloadQueue".to_string(),
            issues: Vec::new(),
            options,
            subsections: Vec::new(),
        });
    }

    // 8. Security
    {
        let mut options = Vec::new();
        validate_security_section(&input.security, |name, sev, msg| {
            push_health_option(&mut options, name, sev, msg);
        });
        sections.push(HealthSectionEntry {
            name: "Security".to_string(),
            issues: Vec::new(),
            options,
            subsections: Vec::new(),
        });
    }

    // 9. IncomingNzbs
    {
        let mut options = Vec::new();
        validate_incoming_nzb_section(
            input.append_category_dir,
            input.nzb_dir_interval,
            input.nzb_dir_file_age,
            input.dupe_check,
            input.dupe_search,
            input.health_check,
            |name, sev, msg| push_health_option(&mut options, name, sev, msg),
        );
        sections.push(HealthSectionEntry {
            name: "IncomingNzbs".to_string(),
            issues: Vec::new(),
            options,
            subsections: Vec::new(),
        });
    }

    // 10. CheckAndRepair
    {
        let mut options = Vec::new();
        validate_check_and_repair_section(
            input.crc_check,
            input.par_check,
            input.par_repair,
            input.par_quick,
            input.par_buffer,
            input.par_threads,
            input.hw_threads,
            input.par_rename,
            input.rar_rename,
            input.direct_rename,
            input.hard_linking,
            input.health_check,
            input.par_time_limit,
            |name, sev, msg| push_health_option(&mut options, name, sev, msg),
        );
        sections.push(HealthSectionEntry {
            name: "CheckAndRepair".to_string(),
            issues: Vec::new(),
            options,
            subsections: Vec::new(),
        });
    }

    // 11. Unpack
    {
        let mut options = Vec::new();
        validate_unpack_section(
            input.unpack,
            input.direct_unpack,
            input.unpack_pause_queue,
            input.unrar_path,
            input.seven_zip_path,
            input.ext_cleanup_disk,
            input.unpack_pass_file,
            input.unpack_ignore_ext,
            |name, sev, msg| push_health_option(&mut options, name, sev, msg),
        );
        sections.push(HealthSectionEntry {
            name: "Unpack".to_string(),
            issues: Vec::new(),
            options,
            subsections: Vec::new(),
        });
    }

    // 12. Scheduler
    {
        let mut subsections = Vec::with_capacity(input.tasks.len());
        for task in input.tasks {
            let mut sub_opts = Vec::new();
            validate_scheduler_task_section(
                task.hours,
                task.minutes,
                task.week_days_bits,
                |name, sev, msg| push_health_option(&mut sub_opts, name, sev, msg),
            );
            subsections.push(HealthSubsectionEntry {
                name: format!("Task{}", task.id),
                options: sub_opts,
            });
        }
        sections.push(HealthSectionEntry {
            name: "Scheduler".to_string(),
            issues: Vec::new(),
            options: Vec::new(),
            subsections,
        });
    }

    // 13. Categories
    {
        let mut subsections = Vec::with_capacity(input.categories.len());
        let mut cat_names = Vec::with_capacity(input.categories.len());
        for (i, cat) in input.categories.iter().enumerate() {
            cat_names.push(cat.name);
            let mut sub_opts = Vec::new();
            validate_category_section(cat.name, input.unpack, cat.unpack, |name, sev, msg| {
                push_health_option(&mut sub_opts, name, sev, msg);
            });
            subsections.push(HealthSubsectionEntry {
                name: format!("Category{}", i + 1),
                options: sub_opts,
            });
        }
        let mut issues = Vec::new();
        if !cat_names.is_empty() {
            let dup = validate_duplicate_category_names(&cat_names);
            if !dup.is_ok() {
                issues.push(dup);
            }
        }
        sections.push(HealthSectionEntry {
            name: "Categories".to_string(),
            issues,
            options: Vec::new(),
            subsections,
        });
    }

    // 14. Feeds
    {
        let mut subsections = Vec::with_capacity(input.feeds.len());
        let mut feed_pairs = Vec::with_capacity(input.feeds.len());
        for feed in input.feeds {
            feed_pairs.push((feed.name, feed.url));
            let mut sub_opts = Vec::new();
            validate_feed_section(
                feed.name,
                feed.url,
                feed.interval,
                feed.cert_level,
                |name, sev, msg| push_health_option(&mut sub_opts, name, sev, msg),
            );
            subsections.push(HealthSubsectionEntry {
                name: format!("Feed{}", feed.id),
                options: sub_opts,
            });
        }
        let mut issues = Vec::new();
        if !feed_pairs.is_empty() {
            let dup = validate_duplicate_feed_urls(&feed_pairs);
            if !dup.is_ok() {
                issues.push(dup);
            }
        }
        sections.push(HealthSectionEntry {
            name: "Feeds".to_string(),
            issues,
            options: Vec::new(),
            subsections,
        });
    }

    sections
}

fn write_xml_node_escaped(out: &mut Vec<u8>, raw: &[u8]) {
    let nul_pos = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    crate::escape::xml_encode(&raw[..nul_pos], out);
}

fn write_xml_node_member(out: &mut Vec<u8>, name: &[u8], tag: &[u8], value: &[u8]) {
    out.extend_from_slice(b"<member><name>");
    write_xml_node_escaped(out, name);
    out.extend_from_slice(b"</name><value><");
    out.extend_from_slice(tag);
    out.push(b'>');
    write_xml_node_escaped(out, value);
    out.extend_from_slice(b"</");
    out.extend_from_slice(tag);
    out.extend_from_slice(b"></value></member>");
}

/// Runs all 13 `SystemHealth` section validators:
/// - `format_mode == 0`: logs warnings (`1`) and errors (`2`) via `on_log`, returns empty `Vec<u8>`.
/// - `format_mode == 1`: returns JSON string (`SystemHealth::ToJsonStr`).
/// - `format_mode == 2`: returns XML string (`SystemHealth::ToXmlStr`).
pub fn diagnose_system_health<L>(
    input: &SystemHealthDiagnoseInput<'_>,
    format_mode: i32,
    mut on_log: L,
) -> Vec<u8>
where
    L: FnMut(i32, &[u8]),
{
    let sections = build_health_sections(input);

    if format_mode == 0 {
        for sec in &sections {
            for issue in &sec.issues {
                if issue.severity == Severity::Error {
                    on_log(2, &issue.message);
                } else if issue.severity == Severity::Warning {
                    on_log(1, &issue.message);
                }
            }
            for opt in &sec.options {
                if opt.severity == Severity::Error || opt.severity == Severity::Warning {
                    let mut line = format!("[{}][{}]: ", sec.name, opt.name).into_bytes();
                    line.extend_from_slice(&opt.message);
                    on_log(if opt.severity == Severity::Error { 2 } else { 1 }, &line);
                }
            }
            for sub in &sec.subsections {
                for opt in &sub.options {
                    if opt.severity == Severity::Error || opt.severity == Severity::Warning {
                        let mut line =
                            format!("[{}][{}.{}]: ", sec.name, sub.name, opt.name).into_bytes();
                        line.extend_from_slice(&opt.message);
                        on_log(if opt.severity == Severity::Error { 2 } else { 1 }, &line);
                    }
                }
            }
        }
        return Vec::new();
    }

    if format_mode == 1 {
        let mut out = Vec::with_capacity(2048);
        out.extend_from_slice(b"{\"Alerts\":[],\"Sections\":[");
        for (sec_idx, sec) in sections.iter().enumerate() {
            if sec_idx > 0 {
                out.push(b',');
            }
            out.extend_from_slice(b"{\"Name\":");
            write_boost_json_str(&mut out, sec.name.as_bytes());
            out.extend_from_slice(b",\"Issues\":[");
            for (i, issue) in sec.issues.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(b"{\"Severity\":");
                write_boost_json_str(&mut out, issue.severity.as_cstr().to_bytes());
                out.extend_from_slice(b",\"Message\":");
                write_boost_json_str(&mut out, &issue.message);
                out.push(b'}');
            }
            out.extend_from_slice(b"],\"Options\":[");
            for (i, opt) in sec.options.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(b"{\"Name\":");
                write_boost_json_str(&mut out, opt.name.as_bytes());
                out.extend_from_slice(b",\"Severity\":");
                write_boost_json_str(&mut out, opt.severity.as_cstr().to_bytes());
                out.extend_from_slice(b",\"Message\":");
                write_boost_json_str(&mut out, &opt.message);
                out.push(b'}');
            }
            out.extend_from_slice(b"],\"Subsections\":[");
            for (i, sub) in sec.subsections.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(b"{\"Name\":");
                write_boost_json_str(&mut out, sub.name.as_bytes());
                out.extend_from_slice(b",\"Options\":[");
                for (j, opt) in sub.options.iter().enumerate() {
                    if j > 0 {
                        out.push(b',');
                    }
                    out.extend_from_slice(b"{\"Name\":");
                    write_boost_json_str(&mut out, opt.name.as_bytes());
                    out.extend_from_slice(b",\"Severity\":");
                    write_boost_json_str(&mut out, opt.severity.as_cstr().to_bytes());
                    out.extend_from_slice(b",\"Message\":");
                    write_boost_json_str(&mut out, &opt.message);
                    out.push(b'}');
                }
                out.extend_from_slice(b"]}");
            }
            out.extend_from_slice(b"]}");
        }
        out.extend_from_slice(b"]}");
        return out;
    }

    let mut out = Vec::with_capacity(4096);
    out.extend_from_slice(b"<value><struct><member><name>Alerts</name><value><array><data></data></array></value></member><member><name>Sections</name><value><array><data>");
    for sec in &sections {
        out.extend_from_slice(b"<value><struct>");
        write_xml_node_member(&mut out, b"Name", b"string", sec.name.as_bytes());
        out.extend_from_slice(b"<member><name>Issues</name><value><array><data>");
        for issue in &sec.issues {
            out.extend_from_slice(b"<value><struct>");
            write_xml_node_member(
                &mut out,
                b"Severity",
                b"string",
                issue.severity.as_cstr().to_bytes(),
            );
            write_xml_node_member(&mut out, b"Message", b"string", &issue.message);
            out.extend_from_slice(b"</struct></value>");
        }
        out.extend_from_slice(b"</data></array></value></member><member><name>Options</name><value><array><data>");
        for opt in &sec.options {
            out.extend_from_slice(b"<value><struct>");
            write_xml_node_member(&mut out, b"Name", b"string", opt.name.as_bytes());
            write_xml_node_member(
                &mut out,
                b"Severity",
                b"string",
                opt.severity.as_cstr().to_bytes(),
            );
            write_xml_node_member(&mut out, b"Message", b"string", &opt.message);
            out.extend_from_slice(b"</struct></value>");
        }
        out.extend_from_slice(b"</data></array></value></member><member><name>Subsections</name><value><array><data>");
        for sub in &sec.subsections {
            out.extend_from_slice(b"<value><struct>");
            write_xml_node_member(&mut out, b"Name", b"string", sub.name.as_bytes());
            out.extend_from_slice(b"<member><name>Options</name><value><array><data>");
            for opt in &sub.options {
                out.extend_from_slice(b"<value><struct>");
                write_xml_node_member(&mut out, b"Name", b"string", opt.name.as_bytes());
                write_xml_node_member(
                    &mut out,
                    b"Severity",
                    b"string",
                    opt.severity.as_cstr().to_bytes(),
                );
                write_xml_node_member(&mut out, b"Message", b"string", &opt.message);
                out.extend_from_slice(b"</struct></value>");
            }
            out.extend_from_slice(b"</data></array></value></member></struct></value>");
        }
        out.extend_from_slice(b"</data></array></value></member></struct></value>");
    }
    out.extend_from_slice(b"</data></array></value></member></struct></value>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let unique = format!(
                "nzbget_rs_health_{}_{}_{}",
                tag,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_core_validators() {
        assert!(required_option(b"Username", b"admin").is_ok());
        let s2 = required_option(b"Username", b"");
        assert!(s2.is_error());
        assert!(String::from_utf8_lossy(&s2.message).contains("is required"));
        assert!(required_option(b"MainDir", &[0xff, 0xfe]).is_ok());

        assert!(required_path_option(b"OutputDir", Path::new("/tmp")).is_ok());
        assert!(required_path_option(b"OutputDir", Path::new("")).is_error());

        let pw_empty = check_password(b"");
        assert_eq!(pw_empty.severity, Severity::Warning);
        assert!(String::from_utf8_lossy(&pw_empty.message).contains("empty"));
        let pw_short = check_password(b"12345");
        assert_eq!(pw_short.severity, Severity::Info);
        assert!(String::from_utf8_lossy(&pw_short.message).contains("too short"));
        assert!(check_password(b"correcthorsebatterystaple").is_ok());
        assert!(check_password(&[0xff; 8]).is_ok());

        assert!(check_positive_num(b"Port", 8080).is_ok());
        assert!(check_positive_num(b"Zero", 0).is_ok());
        let neg = check_positive_num(b"Limit", -5);
        assert!(neg.is_error());
        assert!(String::from_utf8_lossy(&neg.message).contains("negative"));

        let p1 = Path::new("/data/downloads");
        let p2 = Path::new("/data/movies");
        let others: [(&[u8], &Path); 2] = [(b"DownloadDir", p1), (b"MovieDir", p2)];
        assert!(unique_path(b"MusicDir", Path::new("/data/music"), &others).is_ok());
        assert!(unique_path(b"EmptyDir", Path::new(""), &others).is_ok());
        let dup = unique_path(b"InterimDir", Path::new("/data/downloads"), &others);
        assert_eq!(dup.severity, Severity::Warning);
        assert!(
            String::from_utf8_lossy(&dup.message)
                .contains("'InterimDir' and 'DownloadDir' are identical")
        );
    }

    #[test]
    fn test_network_validators() {
        for host in [
            b"localhost".as_slice(),
            b"google.com",
            b"my-server-1",
            b"sub.domain.example.com",
            b"my_server_1",
            b"_leading",
            b"trailing_",
            b"foo_bar.baz",
            b"my-server_1",
            b"a",
            &[b'a'; 63],
            b"192.168.0.1",
            b"10.0.0.1",
            b"::1",
            b"2001:db8:1234:12:321::1",
            b"::ffff:192.168.0.1",
            b"fe80::1%3",
        ] {
            assert!(network_valid_host(host, 0).is_ok(), "{host:?}");
        }

        for bad in [
            b"".as_slice(),
            b"invalid char!",
            b"-foo",
            b"foo-",
            b"example..com",
            b"[foo]",
            b"[::1]",
            b"2001:db8::1::1",
            &[b'a'; 64],
            &[b'a'; 300],
            b"[fe80::1%3]",
        ] {
            assert!(network_valid_host(bad, 0).is_error(), "{bad:?}");
        }

        let bracketed = network_valid_host(b"[2001:db8:1234:12:321::1]", 0);
        assert!(bracketed.is_error());
        assert!(String::from_utf8_lossy(&bracketed.message).contains("2001:db8:1234:12:321::1"));

        assert!(network_valid_host(b"192.168.0.1", 4).is_ok());
        assert!(network_valid_host(b"2001:db8:1234:12:321::1", 6).is_ok());
        assert!(network_valid_host(b"fe80::1%eth0", 6).is_ok());
        assert!(network_valid_host(b"news.valid.com", 4).is_ok());
        assert!(network_valid_host(b"news.valid.com", 6).is_ok());
        assert!(network_valid_host(b"192.168.0.1", 6).is_error());
        assert!(network_valid_host(b"2001:db8:1234:12:321::1", 4).is_error());
        assert!(network_valid_host(b"::ffff:192.168.0.1", 4).is_error());
        assert!(network_valid_host(b"fe80::1%3", 4).is_error());

        let ipv4_mismatch = network_valid_host(b"2001:db8:1234:12:321::1", 4);
        assert!(String::from_utf8_lossy(&ipv4_mismatch.message).contains("IPv4"));
        let ipv6_mismatch = network_valid_host(b"192.168.0.1", 6);
        assert!(String::from_utf8_lossy(&ipv6_mismatch.message).contains("IPv6"));

        assert!(network_valid_port(80).is_ok());
        assert!(network_valid_port(443).is_ok());
        assert!(network_valid_port(65535).is_ok());
        assert!(network_valid_port(0).is_error());
        assert!(network_valid_port(-1).is_error());
        assert!(network_valid_port(70000).is_error());
    }

    #[test]
    fn test_filesystem_and_paths_validators() {
        let tmp = TempDir::new("fs_paths");
        let f = tmp.path.join("testfile.txt");

        let s1 = file_exists(&f);
        assert!(s1.is_error());
        assert!(String::from_utf8_lossy(&s1.message).contains("doesn't exist"));
        assert!(!file_null_or_console_device(&f));
        assert!(!file_null_or_console_device(Path::new("")));

        std::fs::write(&f, b"content").unwrap();
        assert!(file_exists(&f).is_ok());
        assert!(!file_null_or_console_device(&f));
        assert!(file_readable(&f).is_ok());

        let subdir = tmp.path.join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        let s2 = file_exists(&subdir);
        assert!(s2.is_error());
        assert!(String::from_utf8_lossy(&s2.message).contains("not a regular file"));
        assert!(!file_null_or_console_device(&subdir));

        assert!(file_null_or_console_device(Path::new("/dev/null")));
        assert!(file_null_or_console_device(Path::new("/dev/stdout")));
        assert!(file_null_or_console_device(Path::new("/dev/stderr")));

        let writable = tmp.path.join("writable.txt");
        assert!(file_writable(&writable).is_ok());
        assert!(file_writable(&subdir).is_error());

        let script = tmp.path.join("script.sh");
        std::fs::write(&script, b"#!/bin/bash").unwrap();
        assert!(file_executable(&script).is_error());
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(perms.mode() | 0o100);
        std::fs::set_permissions(&script, perms).unwrap();
        assert!(file_executable(&script).is_ok());

        let mydir = tmp.path.join("mydir");
        assert!(directory_exists(&mydir).is_error());
        std::fs::create_dir(&mydir).unwrap();
        assert!(directory_exists(&mydir).is_ok());
        let dir_err = directory_exists(&f);
        assert!(dir_err.is_error());
        assert!(String::from_utf8_lossy(&dir_err.message).contains("not a directory"));

        let writedir = tmp.path.join("writedir");
        std::fs::create_dir(&writedir).unwrap();
        assert!(directory_writable(&writedir).is_ok());
        assert!(!writedir.join("nzbget_write_test.tmp").exists());
        let mut dperms = std::fs::metadata(&writedir).unwrap().permissions();
        dperms.set_mode(dperms.mode() & !0o200);
        std::fs::set_permissions(&writedir, dperms.clone()).unwrap();
        assert!(directory_writable(&writedir).is_error());
        dperms.set_mode(dperms.mode() | 0o200);
        std::fs::set_permissions(&writedir, dperms).unwrap();

        let readdir = tmp.path.join("readdir");
        std::fs::create_dir(&readdir).unwrap();
        assert!(directory_readable(&readdir).is_ok());
        let mut rperms = std::fs::metadata(&readdir).unwrap().permissions();
        rperms.set_mode(rperms.mode() & !0o400);
        std::fs::set_permissions(&readdir, rperms.clone()).unwrap();
        assert!(directory_readable(&readdir).is_error());
        rperms.set_mode(rperms.mode() | 0o400);
        std::fs::set_permissions(&readdir, rperms).unwrap();

        // PathsValidator individual helpers
        let main_dir = tmp.path.join("main");
        assert!(validate_required_writable_dir(b"MainDir", &main_dir).is_error());
        std::fs::create_dir(&main_dir).unwrap();
        assert!(validate_required_writable_dir(b"MainDir", &main_dir).is_ok());
        assert!(validate_required_writable_dir(b"MainDir", Path::new("")).is_error());

        let inter_empty = validate_inter_dir_path(Path::new(""));
        assert_eq!(inter_empty.severity, Severity::Warning);
        assert!(String::from_utf8_lossy(&inter_empty.message).contains("not recommended"));
        assert!(validate_inter_dir_path(&main_dir).is_ok());
        assert!(validate_inter_dir_path(&tmp.path.join("phantom")).is_error());

        assert!(validate_web_dir_path(Path::new("")).is_ok());
        assert!(validate_web_dir_path(&main_dir).is_ok());
        assert!(validate_web_dir_path(&tmp.path.join("noweb")).is_error());

        let log_path = tmp.path.join("nzbget.log");
        assert_eq!(validate_log_file_path(Path::new(""), 1).severity, Severity::Warning);
        let log_none = validate_log_file_path(Path::new(""), 0);
        assert!(log_none.is_ok());
        assert!(log_none.message.is_empty());
        assert!(validate_log_file_path(&tmp.path.join("ghost.log"), 1).is_error());
        std::fs::write(&log_path, b"log").unwrap();
        assert!(validate_log_file_path(&log_path, 1).is_ok());
        for dev in ["/dev/null", "/dev/stdout", "/dev/stderr"] {
            assert!(validate_log_file_path(Path::new(dev), 1).is_ok());
        }
        assert!(validate_log_file_path(Path::new("/dev/null"), 2).is_ok());
        assert!(validate_log_file_path(Path::new("/dev/null"), 3).is_ok());

        let cert_file = tmp.path.join("cacert.pem");
        let cert_dir = tmp.path.join("certs");
        assert!(validate_cert_store_path(Path::new(""), false).is_ok());
        assert!(validate_cert_store_path(Path::new(""), true).is_error());
        std::fs::write(&cert_file, b"CERT").unwrap();
        assert!(validate_cert_store_path(&cert_file, true).is_ok());
        std::fs::create_dir(&cert_dir).unwrap();
        assert!(validate_cert_store_path(&cert_dir, true).is_ok());
        assert!(validate_cert_store_path(&tmp.path.join("missing.pem"), true).is_error());

        let lock_path = tmp.path.join("nzbget.lock");
        let lock_empty = validate_lock_file_path(Path::new(""), true);
        assert_eq!(lock_empty.severity, Severity::Warning);
        assert!(
            String::from_utf8_lossy(&lock_empty.message)
                .contains("check for another running instance is disabled")
        );
        assert!(validate_lock_file_path(Path::new(""), false).is_ok());
        assert!(validate_lock_file_path(Path::new("random/path"), false).is_ok());
        assert!(validate_lock_file_path(&tmp.path.join("not_yet_created.lock"), true).is_ok());
        std::fs::write(&lock_path, b"pid").unwrap();
        assert!(validate_lock_file_path(&lock_path, true).is_ok());
        assert!(validate_lock_file_path(&tmp.path.join("missing_dir").join("nzbget.lock"), true).is_error());
    }

    #[test]
    fn test_sections_and_news_server_validators() {
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

        // LoggingValidator
        assert!(validate_write_log(0, Path::new("")).is_ok());
        let append_status = validate_write_log(1, Path::new("/tmp/nzbget.log"));
        assert_eq!(append_status.severity, Severity::Warning);
        assert!(String::from_utf8_lossy(&append_status.message).contains("may grow indefinitely"));
        for wl in [1, 2, 3] {
            assert!(validate_write_log(wl, Path::new("/dev/null")).is_ok());
        }
        let rot_high = validate_rotate_log(3, 366);
        assert_eq!(rot_high.severity, Severity::Warning);
        assert!(String::from_utf8_lossy(&rot_high.message).contains("'RotateLog' is very high"));

        // FeedValidator
        assert!(validate_feed_cert(b"https://my.feed.com", 2).is_ok());
        assert!(validate_feed_cert(b"", 0).is_ok());
        assert_eq!(validate_feed_cert(b"https://my.feed.com", 0).severity, Severity::Warning);
        assert!(validate_feed_cert(b"https://my.feed.com", 1).is_ok());
        assert!(validate_feed_cert(b"http://my.feed.com", 0).is_ok());

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

        // ConnectionValidator
        assert!(validate_article_retries(3, false, false).is_ok());
        let plain = validate_article_retries(0, false, false);
        assert_eq!(plain.severity, Severity::Warning);
        assert!(!String::from_utf8_lossy(&plain.message).contains("HealthCheck"));
        let dupe = validate_article_retries(0, true, false);
        assert_eq!(dupe.severity, Severity::Warning);
        assert!(String::from_utf8_lossy(&dupe.message).contains("HealthCheck=dupe"));
        let search = validate_article_retries(1, false, true);
        assert_eq!(search.severity, Severity::Warning);
        assert!(String::from_utf8_lossy(&search.message).contains("DupeSearch"));
        assert!(validate_article_retries(100, false, false).is_error());

        assert!(validate_article_read_chunk_size(4 * 1024).is_ok());
        assert!(validate_article_read_chunk_size(1025 * 1024).is_ok());
        assert!(validate_article_read_chunk_size(10240 * 1024).is_ok());
        let chunk_low = validate_article_read_chunk_size(3 * 1024);
        assert_eq!(chunk_low.severity, Severity::Warning);
        assert!(String::from_utf8_lossy(&chunk_low.message).contains("3 KB."));
        let chunk_high = validate_article_read_chunk_size(10241 * 1024);
        assert_eq!(chunk_high.severity, Severity::Info);
        assert!(String::from_utf8_lossy(&chunk_high.message).contains("10241 KB."));

        // SecurityValidator
        assert!(!validate_control_password(b"").is_ok());
        assert_eq!(validate_control_password(b"tegbzn6789").severity, Severity::Info);
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

        // NewsServerValidator & NewsServersValidator
        assert!(validate_server_active(true).is_ok());
        let inactive = validate_server_active(false);
        assert_eq!(inactive.severity, Severity::Info);
        assert_eq!(inactive.message, b"Server is disabled");

        assert!(validate_server_host(true, b"news.valid.com", 0).is_ok());
        assert!(validate_server_host(true, b"", 0).is_error());
        assert!(validate_server_host(true, b"bad@host!", 0).is_error());
        assert!(validate_server_host(true, b"192.168.0.1", 0).is_ok());
        assert!(validate_server_host(true, b"2001:db8:1234:12:321::1", 0).is_ok());
        assert!(validate_server_host(true, b"[2001:db8:1234:12:321::1]", 0).is_error());
        assert!(validate_server_host(true, b"[foo]", 0).is_error());
        assert!(validate_server_host(true, b"192.168.0.1", 4).is_ok());
        assert!(validate_server_host(true, b"2001:db8:1234:12:321::1", 6).is_ok());
        assert!(validate_server_host(true, b"2001:db8:1234:12:321::1", 4).is_error());
        assert!(validate_server_host(true, b"192.168.0.1", 6).is_error());
        assert!(validate_server_host(true, b"news.valid.com", 6).is_ok());

        assert!(validate_server_port(true, 563, true).is_ok());
        assert!(validate_server_port(true, 443, true).is_ok());
        assert_eq!(validate_server_port(true, 9000, true).severity, Severity::Info);
        assert!(validate_server_port(true, 119, false).is_ok());
        assert_eq!(validate_server_port(true, 8080, false).severity, Severity::Info);

        assert!(validate_server_username(true, b"user").is_ok());
        assert_eq!(validate_server_username(true, b"").severity, Severity::Warning);
        assert_eq!(validate_server_password(true, b"pass").severity, Severity::Info);
        assert_eq!(validate_server_password(true, b"").severity, Severity::Warning);
        assert_eq!(validate_server_password(true, b"123").severity, Severity::Info);

        assert!(validate_server_connections(true, 50, false).is_ok());
        assert_eq!(validate_server_connections(true, 4, false).severity, Severity::Warning);
        assert_eq!(validate_server_connections(true, 0, false).severity, Severity::Warning);
        assert!(validate_server_connections(true, 1000, false).is_error());
        assert_eq!(validate_server_connections(true, 2, true).severity, Severity::Warning);

        assert!(validate_server_encryption(true, true).is_ok());
        assert_eq!(validate_server_encryption(true, false).severity, Severity::Warning);
        assert!(validate_server_cipher(true, b"", true).is_ok());
        assert_eq!(validate_server_cipher(true, b"AES", true).severity, Severity::Info);
        assert_eq!(validate_server_cipher(true, b"AES", false).severity, Severity::Warning);

        assert!(validate_server_retention(true, 0).is_ok());
        assert!(validate_server_retention(true, 3000).is_ok());
        assert_eq!(validate_server_retention(true, 50).severity, Severity::Warning);
        assert_eq!(validate_server_retention(true, 20000).severity, Severity::Info);

        assert!(validate_server_optional(true, false, 0).is_ok());
        assert!(validate_server_optional(true, true, 1).is_ok());
        assert_eq!(validate_server_optional(true, true, 0).severity, Severity::Info);

        assert!(validate_server_cert_verification(true, true, 2).is_ok());
        assert_eq!(validate_server_cert_verification(true, true, 0).severity, Severity::Warning);
        assert!(validate_server_cert_verification(true, false, 0).is_ok());

        let no_srv = validate_primary_server_exists(&[], &[]);
        assert!(no_srv.is_error());
        assert!(
            String::from_utf8_lossy(&no_srv.message)
                .contains("At least one server must be configured")
        );
        let no_active = validate_primary_server_exists(&[0, 1], &[0, 0]);
        assert!(no_active.is_error());
        assert!(
            String::from_utf8_lossy(&no_active.message)
                .contains("At least one primary server must be active")
        );
        assert!(validate_primary_server_exists(&[0, 1], &[1, 0]).is_ok());
        assert!(validate_primary_server_exists(&[0, 0], &[1, 1]).is_ok());
        let higher_only = validate_primary_server_exists(&[0, 1], &[0, 1]);
        assert!(higher_only.is_error());
        assert!(
            String::from_utf8_lossy(&higher_only.message)
                .contains("At least one primary server must be active")
        );

        // ExtensionScriptsValidator
        let existing_script = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let loaded: [(&[u8], &Path); 2] = [
            (b"QueueSort", existing_script.as_path()),
            (b"Completion", existing_script.as_path()),
        ];
        assert!(validate_extension_list(b"QueueSort,Completion", &loaded).is_ok());
        assert!(validate_extension_list(b"QueueSort.py,Completion.py", &loaded).is_ok());
        assert_eq!(
            validate_extension_list(b"Missing.py", &loaded).severity,
            Severity::Warning
        );
    }

    #[test]
    fn system_info_full_suite_port_from_cpp() {
        let (os_name, os_ver) = os_detect_info(b"", b"");
        let (cpu_model, cpu_arch) = cpu_detect_info(b"");

        assert!(!os_name.is_empty());
        assert!(!os_ver.is_empty());
        assert!(!cpu_arch.is_empty());

        let tools = [
            SystemToolView {
                name: b"Python",
                version: b"3.12.3",
                path: b"/usr/bin/python3",
            },
            SystemToolView {
                name: b"7-Zip",
                version: b"",
                path: b"",
            },
            SystemToolView {
                name: b"UnRAR",
                version: b"7.01",
                path: b"/usr/local/bin/unrar",
            },
        ];
        let libs = [
            SystemLibraryView {
                name: b"LibXML2",
                version: b"2.9.13",
            },
            SystemLibraryView {
                name: b"OpenSSL",
                version: b"3.3.1",
            },
        ];

        let view = SystemInfoView {
            os_name: &os_name,
            os_version: &os_ver,
            cpu_model: &cpu_model,
            cpu_arch: &cpu_arch,
            public_ip: b"1.2.3.4",
            private_ip: b"192.168.1.5",
            tools: &tools,
            libraries: &libs,
        };

        let json_res = system_info_to_json(&view);
        let xml_res = system_info_to_xml(&view);

        let expected_json = format!(
            concat!(
                "{{\"OS\":{{\"Name\":\"{}\",\"Version\":\"{}\"}},",
                "\"CPU\":{{\"Model\":\"{}\",\"Arch\":\"{}\"}},",
                "\"Network\":{{\"PublicIP\":\"1.2.3.4\",\"PrivateIP\":\"192.168.1.5\"}},",
                "\"Tools\":[",
                "{{\"Name\":\"Python\",\"Version\":\"3.12.3\",\"Path\":\"/usr/bin/python3\"}},",
                "{{\"Name\":\"7-Zip\",\"Version\":\"\",\"Path\":\"\"}},",
                "{{\"Name\":\"UnRAR\",\"Version\":\"7.01\",\"Path\":\"/usr/local/bin/unrar\"}}",
                "],",
                "\"Libraries\":[",
                "{{\"Name\":\"LibXML2\",\"Version\":\"2.9.13\"}},",
                "{{\"Name\":\"OpenSSL\",\"Version\":\"3.3.1\"}}",
                "]}}"
            ),
            String::from_utf8_lossy(&os_name),
            String::from_utf8_lossy(&os_ver),
            String::from_utf8_lossy(&cpu_model),
            String::from_utf8_lossy(&cpu_arch),
        );
        assert_eq!(json_res, expected_json.as_bytes());

        let expected_xml = format!(
            concat!(
                "<value><struct>",
                "<OS>",
                "<member><name>Name</name><value><string>{}</string></value></member>",
                "<member><name>Version</name><value><string>{}</string></value></member>",
                "</OS>",
                "<CPU>",
                "<member><name>Model</name><value>{}</value></member>",
                "<member><name>Arch</name><value><string>{}</string></value></member>",
                "</CPU>",
                "<Network>",
                "<member><name>PublicIP</name><value><string>1.2.3.4</string></value></member>",
                "<member><name>PrivateIP</name><value><string>192.168.1.5</string></value></member>",
                "</Network>",
                "<Tools>",
                "<member><name>Name</name><value><string>Python</string></value></member>",
                "<member><name>Version</name><value><string>3.12.3</string></value></member>",
                "<member><name>Path</name><value><string>/usr/bin/python3</string></value></member>",
                "<member><name>Name</name><value><string>7-Zip</string></value></member>",
                "<member><name>Version</name><value><string/></value></member>",
                "<member><name>Path</name><value><string/></value></member>",
                "<member><name>Name</name><value><string>UnRAR</string></value></member>",
                "<member><name>Version</name><value><string>7.01</string></value></member>",
                "<member><name>Path</name><value><string>/usr/local/bin/unrar</string></value></member>",
                "</Tools>",
                "<Libraries>",
                "<member><name>Name</name><value><string>LibXML2</string></value></member>",
                "<member><name>Version</name><value><string>2.9.13</string></value></member>",
                "<member><name>Name</name><value><string>OpenSSL</string></value></member>",
                "<member><name>Version</name><value><string>3.3.1</string></value></member>",
                "</Libraries>",
                "</struct></value>"
            ),
            String::from_utf8_lossy(&os_name),
            String::from_utf8_lossy(&os_ver),
            if cpu_model.is_empty() {
                "<string/>".to_string()
            } else {
                format!("<string>{}</string>", String::from_utf8_lossy(&cpu_model))
            },
            String::from_utf8_lossy(&cpu_arch),
        );
        assert_eq!(xml_res, expected_xml.as_bytes());

        // XmlValueWithAmpersandTest and non-ASCII UTF-8 preservation
        let xml_amp = xml_serialize_struct_member(b"Path", b"string", "/opt/R&D/café/unrar".as_bytes());
        assert_eq!(
            std::str::from_utf8(&xml_amp).unwrap(),
            "<struct><member><name>Path</name><value><string>/opt/R&amp;D/café/unrar</string></value></member></struct>"
        );
        let utf8_tools = [SystemToolView {
            name: b"Python",
            version: b"3.12",
            path: "/opt/café/python3".as_bytes(),
        }];
        let utf8_view = SystemInfoView {
            os_name: b"Linux",
            os_version: b"6.8",
            cpu_model: b"ARM",
            cpu_arch: b"aarch64",
            public_ip: b"",
            private_ip: b"",
            tools: &utf8_tools,
            libraries: &[],
        };
        let utf8_json = system_info_to_json(&utf8_view);
        assert!(std::str::from_utf8(&utf8_json).unwrap().contains("\"/opt/café/python3\""));
        let utf8_xml = system_info_to_xml(&utf8_view);
        assert!(std::str::from_utf8(&utf8_xml).unwrap().contains("<string>/opt/café/python3</string>"));
    }

    #[test]
    fn test_diagnose_system_health_modes() {
        let tmp = TempDir::new("diagnose");
        let main_dir = tmp.path.join("main");
        let dest_dir = tmp.path.join("dest");
        let inter_dir = tmp.path.join("inter");
        let nzb_dir = tmp.path.join("nzb");
        let queue_dir = tmp.path.join("queue");
        let temp_dir = tmp.path.join("temp");
        let cfg_file = tmp.path.join("rustyget.conf");
        for d in [&main_dir, &dest_dir, &inter_dir, &nzb_dir, &queue_dir, &temp_dir] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(&cfg_file, b"MainDir=/tmp\n").unwrap();

        let servers = [HealthServerInput {
            id: 1,
            section: NewsServerSectionInput {
                active: true,
                name: b"Primary",
                level: 0,
                optional: false,
                group: 0,
                host: b"news.example.com",
                port: 563,
                username: b"user",
                password: b"strongpassword",
                tls: true,
                cipher: b"",
                max_connections: 20,
                retention: 1000,
                cert_level: 2,
                ip_version: 0,
                dupe_search: false,
            },
        }];
        let tasks = [HealthTaskInput {
            id: 1,
            hours: 25,
            minutes: 0,
            week_days_bits: 127,
        }];
        let categories = [HealthCategoryInput {
            name: b"",
            unpack: true,
        }];
        let feeds = [HealthFeedInput {
            id: 1,
            name: b"Feed1",
            url: b"https://example.com/rss&a=1",
            interval: 15,
            cert_level: 0,
        }];

        let input = SystemHealthDiagnoseInput {
            paths: PathsSectionInput {
                main_dir: &main_dir,
                dest_dir: &dest_dir,
                inter_dir: &inter_dir,
                nzb_dir: &nzb_dir,
                queue_dir: &queue_dir,
                web_dir: Path::new(""),
                temp_dir: &temp_dir,
                script_dirs: &[],
                config_template: Path::new(""),
                log_file: Path::new("/dev/null"),
                cert_store: Path::new(""),
                cert_store_path: Path::new(""),
                config_file: &cfg_file,
                lock_file: Path::new(""),
                write_log: 0,
                cert_check: false,
                daemon_mode: false,
            },
            servers: &servers,
            output_mode: 0,
            update_interval: 100,
            curses_nzb_name: false,
            curses_group: false,
            curses_time: false,
            rotate_log: 3,
            log_buffer: 1000,
            crash_dump: false,
            time_correction: 0,
            article_retries: 3,
            hc_dupe: false,
            dupe_search: false,
            article_interval: 10,
            article_timeout: 60,
            article_read_chunk_size: 4,
            url_retries: 3,
            url_interval: 10,
            url_timeout: 60,
            url_connections: 4,
            remote_timeout: 90,
            download_rate: 0,
            monthly_quota: 0,
            quota_start_day: 1,
            daily_quota: 0,
            flush_queue: true,
            continue_partial: true,
            propagation_delay: 0,
            article_cache: 250,
            direct_write: true,
            write_buffer: 1024,
            file_naming: 0,
            post_strategy: 1,
            disk_space: 250,
            nzb_cleanup_disk: true,
            keep_history: 30,
            feed_history: 7,
            skip_write: false,
            raw_article: false,
            security: SecuritySectionInput {
                control_ip: b"127.0.0.1",
                control_port: 6789,
                control_username: b"admin",
                control_password: b"strongpassword",
                restricted_username: b"",
                restricted_password: b"",
                add_username: b"",
                add_password: b"",
                form_auth: false,
                secure_control: false,
                secure_cert: Path::new(""),
                secure_key: Path::new(""),
                secure_port: 6791,
                cert_check: false,
                cert_store_path_empty: true,
                daemon_mode: false,
                daemon_username: b"",
                umask: 0o022,
            },
            append_category_dir: false,
            nzb_dir_interval: 5,
            nzb_dir_file_age: 60,
            dupe_check: true,
            health_check: 1,
            crc_check: true,
            par_check: 0,
            par_repair: true,
            par_quick: true,
            par_buffer: 250,
            par_threads: 1,
            hw_threads: 4,
            par_rename: true,
            rar_rename: true,
            direct_rename: false,
            hard_linking: false,
            par_time_limit: 0,
            unpack: true,
            direct_unpack: false,
            unpack_pause_queue: false,
            unrar_path: Path::new(""),
            seven_zip_path: Path::new(""),
            ext_cleanup_disk: b"",
            unpack_pass_file: Path::new(""),
            unpack_ignore_ext: b"",
            options_log_file: Path::new("/dev/null"),
            tasks: &tasks,
            categories: &categories,
            feeds: &feeds,
        };

        let mut logs = Vec::new();
        let out_log = diagnose_system_health(&input, 0, |sev, msg| {
            logs.push((sev, String::from_utf8_lossy(msg).into_owned()));
        });
        assert!(out_log.is_empty());
        assert!(logs.iter().any(|(sev, m)| *sev == 2 && m.contains("[Scheduler][Task1.Time]: Invalid hour: 25")));
        assert!(logs.iter().any(|(sev, m)| *sev == 1 && m.contains("[Categories][Category1.Name]: Category name is empty")));

        let json_out = String::from_utf8(diagnose_system_health(&input, 1, |_, _| {})).unwrap();
        assert!(json_out.starts_with("{\"Alerts\":[],\"Sections\":["));
        assert!(json_out.contains("\"Name\":\"Scheduler\""));
        assert!(json_out.contains("\"Name\":\"Task1\""));

        let xml_out = String::from_utf8(diagnose_system_health(&input, 2, |_, _| {})).unwrap();
        assert!(xml_out.starts_with("<value><struct><member><name>Alerts</name>"));
        assert!(xml_out.contains("<string>Invalid hour: 25</string>"));
    }

    #[test]
    fn test_system_info_format() {
        let disp = String::from_utf8(system_info_format(
            0,
            b"Darwin",
            b"24.0.0",
            b"arm64",
            b"OpenSSL",
            b"OpenSSL 3.3.1 4 Jun 2024",
            true,
            b"",
            b"",
        ))
        .unwrap();
        assert!(disp.contains("UnRAR / 7-Zip / Archive Engine Built-in"));
        assert!(disp.contains("OpenSSL 3.3.1"));

        let json = String::from_utf8(system_info_format(
            1,
            b"Darwin",
            b"24.0.0",
            b"arm64",
            b"OpenSSL",
            b"3.3.1",
            false,
            b"1.2.3.4",
            b"192.168.1.10",
        ))
        .unwrap();
        assert!(json.contains("\"PublicIP\":\"1.2.3.4\""));
        assert!(json.contains("\"PrivateIP\":\"192.168.1.10\""));
        assert!(json.contains("\"Name\":\"OpenSSL\",\"Version\":\"3.3.1\""));

        let xml = String::from_utf8(system_info_format(
            2,
            b"Darwin",
            b"24.0.0",
            b"arm64",
            b"",
            b"",
            false,
            b"1.2.3.4",
            b"192.168.1.10",
        ))
        .unwrap();
        assert!(xml.contains("<Network><member><name>PublicIP</name><value><string>1.2.3.4</string></value></member>"));
    }

    #[test]
    fn test_log_rotate_time_helpers() {
        // 2026-03-10 00:00:00 UTC
        let cur_time = crate::util::util_internal_timegm(2026 - 1900, 2, 10, 0, 0, 0);
        let path = log_format_rotated_path_for_time(b"/var/log", b'/', b"rustyget", cur_time, b".log");
        assert_eq!(path, b"/var/log/rustyget-2026-03-10.log");

        // rustyget-2026-03-05.log -> 5 days older than 2026-03-10
        let fname = b"rustyget-2026-03-05.log";
        assert!(log_should_delete_rotated_file_by_name(fname, 9, 14, 17, cur_time, 3));
        assert!(!log_should_delete_rotated_file_by_name(fname, 9, 14, 17, cur_time, 10));

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let tmp = TempDir::new("log_rotate");
            let old_log = tmp.path.join("rustyget-2026-03-05.log");
            let recent_log = tmp.path.join("rustyget-2026-03-09.log");
            std::fs::write(&old_log, b"old").unwrap();
            std::fs::write(&recent_log, b"recent").unwrap();
            let base_log = tmp.path.join("rustyget.log");
            let mut info_msgs = Vec::new();
            let rotated = log_rotate_log_file(
                base_log.as_os_str().as_bytes(),
                b'/',
                cur_time,
                3,
                |msg| info_msgs.push(msg.to_vec()),
            );
            assert_eq!(
                rotated,
                tmp.path.join("rustyget-2026-03-10.log").as_os_str().as_bytes()
            );
            assert!(!old_log.exists());
            assert!(recent_log.exists());
            assert_eq!(info_msgs, vec![b"Deleting old log-file rustyget-2026-03-05.log\n".to_vec()]);
        }
    }
}

