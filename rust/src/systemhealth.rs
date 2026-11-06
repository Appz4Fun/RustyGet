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

/// Returns true when `Maintenance::CheckUpdates` received non-empty update info output.
pub fn maintenance_has_update_info(len: i32) -> bool {
    len > 0
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
}

