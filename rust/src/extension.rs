//! End-to-end Rust implementation of `daemon/extension/` (`ScriptConfig`,
//! `ExtensionManager`, `NzbScriptController`, `ScanScriptController`,
//! `PostScriptController`, and `QueueScriptController`).

use std::ffi::{c_char, c_int, CStr, CString};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

fn cstring_lossy(bytes: &[u8]) -> CString {
    if let Some(nul) = bytes.iter().position(|&b| b == 0) {
        CString::new(&bytes[..nul]).unwrap_or_default()
    } else {
        CString::new(bytes).unwrap_or_default()
    }
}

fn io_err_msg(err: &std::io::Error) -> String {
    if let Some(code) = err.raw_os_error() {
        extern "C" {
            fn strerror(errnum: c_int) -> *const c_char;
        }
        let ptr = unsafe { strerror(code) };
        if !ptr.is_null() {
            return unsafe { CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned();
        }
    }
    err.to_string()
}

fn fs_remove_all(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Computes `ScriptConfig::CipherBaseline` from `(name, value)` pairs.
pub fn script_config_cipher_baseline(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut names = Vec::with_capacity(entries.len());
    let mut values = Vec::with_capacity(entries.len());
    for &(k, v) in entries {
        names.push(k);
        values.push(v);
    }
    crate::scripturlstat::build_cipher_baseline(&names, &values)
}

fn script_config_cipher_baseline_cstrings(entries: &[(CString, CString)]) -> Vec<u8> {
    let mut names = Vec::with_capacity(entries.len());
    let mut values = Vec::with_capacity(entries.len());
    for (k, v) in entries {
        names.push(k.to_bytes());
        values.push(v.to_bytes());
    }
    crate::scripturlstat::build_cipher_baseline(&names, &values)
}

/// Parses config file bytes into `(name, value)` entries, applying `ConvertOldOption`
/// and `ConvertOldOptions` (`ScriptConfig::LoadConfigUnlocked`).
pub fn script_config_parse_bytes(raw: &[u8]) -> Vec<(CString, CString)> {
    let mut entries = Vec::new();
    for raw_line in raw.split_inclusive(|&b| b == b'\n') {
        let nul_trimmed = match raw_line.iter().position(|&b| b == 0) {
            Some(pos) => &raw_line[..pos],
            None => raw_line,
        };
        let trimmed_len = crate::util::trim_right_line(nul_trimmed);
        let line = &nul_trimmed[..trimmed_len];
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        if let Some((raw_name, raw_val)) =
            crate::twinproberenhist::tpr_options_split_option_string(line)
        {
            let name_c = cstring_lossy(raw_name);
            let val_c = cstring_lossy(raw_val);
            let (conv_name, conv_val) = crate::options::convert_old_option(&name_c, &val_c);
            entries.push((conv_name, conv_val));
        }
    }
    crate::options::convert_old_options(&mut entries);
    entries
}

/// End-to-end implementation of `ScriptConfig::LoadConfigUnlocked`.
pub fn script_config_load(filename: &[u8]) -> Option<Vec<(CString, CString)>> {
    let path = bytes_to_path(filename);
    let mut file = std::fs::File::open(&path).ok()?;
    let meta = file.metadata().ok()?;
    if meta.is_dir() {
        return None;
    }
    if meta.len() == 0 {
        return Some(Vec::new());
    }
    let mut raw = Vec::with_capacity(meta.len() as usize);
    file.read_to_end(&mut raw).ok()?;
    Some(script_config_parse_bytes(&raw))
}

/// Merges `entries` into existing config file content `raw`, replacing existing options
/// in-place (using the first occurrence's option name casing in `entries` and the last
/// occurrence's value), dropping orphaned/duplicate option lines, preserving comments
/// with guaranteed trailing `\n`, and appending new options at the end.
pub fn script_config_merge_bytes(raw: &[u8], entries: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut values: Vec<(&[u8], &[u8])> = Vec::with_capacity(entries.len());
    for &(name, val) in entries {
        if let Some(existing) = values
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
        {
            existing.1 = val;
        } else {
            values.push((name, val));
        }
    }

    let mut written_options: Vec<Vec<u8>> = Vec::with_capacity(values.len());
    let mut out = Vec::with_capacity(raw.len() + 256);

    for raw_line in raw.split_inclusive(|&b| b == b'\n') {
        let line = match raw_line.iter().position(|&b| b == 0) {
            Some(pos) => &raw_line[..pos],
            None => raw_line,
        };
        if !line.is_empty() && line[0] != b'#' && line.contains(&b'=') {
            let mut trimmed = line;
            while trimmed
                .last()
                .is_some_and(|&b| matches!(b, b'\n' | b'\r' | b' '))
            {
                trimmed = &trimmed[..trimmed.len() - 1];
            }
            if let Some((raw_name, raw_val)) =
                crate::twinproberenhist::tpr_options_split_option_string(trimmed)
            {
                let name_c = cstring_lossy(raw_name);
                let val_c = cstring_lossy(raw_val);
                let (conv_name, _) = crate::options::convert_old_option(&name_c, &val_c);
                let opt_bytes = conv_name.to_bytes();
                if let Some(&(canon_name, latest_val)) = values
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(opt_bytes))
                {
                    if !written_options
                        .iter()
                        .any(|w| w.eq_ignore_ascii_case(canon_name))
                    {
                        written_options.push(canon_name.to_vec());
                        out.extend_from_slice(canon_name);
                        out.push(b'=');
                        out.extend_from_slice(latest_val);
                        out.push(b'\n');
                    }
                }
            }
        } else {
            out.extend_from_slice(line);
            if !line.ends_with(b"\n") {
                out.push(b'\n');
            }
        }
    }

    for &(name, val) in entries {
        if !written_options.iter().any(|w| w.eq_ignore_ascii_case(name)) {
            written_options.push(name.to_vec());
            let final_val = values
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map_or(val, |&(_, v)| v);
            out.extend_from_slice(name);
            out.push(b'=');
            out.extend_from_slice(final_val);
            out.push(b'\n');
        }
    }

    out
}

/// End-to-end implementation of `ScriptConfig::SaveConfig`.
/// Returns `(saved, conflict)`.
pub fn script_config_save(
    filename: &[u8],
    entries: &[(&[u8], &[u8])],
    cipher_baseline: Option<&[u8]>,
) -> (bool, bool) {
    if let Some(expected) = cipher_baseline {
        let Some(current) = script_config_load(filename) else {
            return (false, false);
        };
        let actual = script_config_cipher_baseline_cstrings(&current);
        if actual.as_slice() != expected {
            return (false, true);
        }
    }

    let path = bytes_to_path(filename);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
    else {
        return (false, false);
    };
    if file.metadata().map_or(true, |m| m.is_dir()) {
        return (false, false);
    }

    let mut raw = Vec::new();
    if file.read_to_end(&mut raw).is_err() {
        return (false, false);
    }

    let merged = script_config_merge_bytes(&raw, entries);
    if file.seek(SeekFrom::Start(0)).is_err() {
        return (false, false);
    }
    if file.write_all(&merged).is_err() || file.flush().is_err() {
        return (false, false);
    }
    let _ = file.set_len(merged.len() as u64);
    (true, false)
}

/// Checks whether `target_name` is in the `,;`-separated `extensions_list` (exact byte match),
/// matching `ExtensionManager::Manager::CreateTasks`.
pub fn ext_is_in_list(extensions_list: &[u8], target_name: &[u8]) -> bool {
    crate::frontexthealth::ext_is_in_list(extensions_list, target_name)
}

/// Phase 1 of `ExtensionManager::Manager::UpdateExtension`: validates lookup and
/// `use_count`, moves `target_path` aside to `<target_path>.update-backup`, and
/// cleans up `archive_path` if moving aside fails.
/// Returns `(backup_path, None)` on success or `(backup_path, Some(error))` on failure.
pub fn ext_mgr_prepare_update(
    ext_name: &[u8],
    found: bool,
    use_count: i64,
    target_path: &[u8],
    archive_path: &[u8],
) -> (Vec<u8>, Option<Vec<u8>>) {
    let ext_str = String::from_utf8_lossy(ext_name);
    if !found {
        return (
            Vec::new(),
            Some(format!("Update failed: Extension '{ext_str}' not found").into_bytes()),
        );
    }
    if use_count > 1 {
        return (
            Vec::new(),
            Some(format!("Update failed: Extension '{ext_str}' is currently in use").into_bytes()),
        );
    }

    let mut backup_bytes = Vec::with_capacity(target_path.len() + 14);
    backup_bytes.extend_from_slice(target_path);
    backup_bytes.extend_from_slice(b".update-backup");

    let target = bytes_to_path(target_path);
    let backup = bytes_to_path(&backup_bytes);
    let _ = fs_remove_all(&backup);

    if let Err(ec) = std::fs::rename(&target, &backup) {
        let target_str = String::from_utf8_lossy(target_path);
        let move_err = format!(
            "Cannot move old extension from '{}' ({})",
            target_str,
            io_err_msg(&ec)
        );
        let archive = bytes_to_path(archive_path);
        if let Err(ec2) = std::fs::remove_file(&archive) {
            let archive_str = String::from_utf8_lossy(archive_path);
            let compound = format!(
                "{move_err} and cannot delete downloaded archive '{archive_str}': {}",
                io_err_msg(&ec2)
            );
            return (backup_bytes, Some(compound.into_bytes()));
        }
        return (backup_bytes, Some(move_err.into_bytes()));
    }

    (backup_bytes, None)
}

/// Phase 2 of `ExtensionManager::Manager::UpdateExtension`: either cleans up
/// `backup_path` after a successful install, or restores `backup_path` back to
/// `target_path` when `install_err` is `Some`.
pub fn ext_mgr_finish_update(
    target_path: &[u8],
    backup_path: &[u8],
    archive_path: &[u8],
    install_err: Option<&[u8]>,
) -> Option<Vec<u8>> {
    let target = bytes_to_path(target_path);
    let backup = bytes_to_path(backup_path);
    if let Some(err_bytes) = install_err {
        let archive = bytes_to_path(archive_path);
        let _ = std::fs::remove_file(&archive);
        let _ = fs_remove_all(&target);
        if let Err(ec) = std::fs::rename(&backup, &target) {
            let err_str = String::from_utf8_lossy(err_bytes);
            let target_str = String::from_utf8_lossy(target_path);
            let backup_str = String::from_utf8_lossy(backup_path);
            return Some(
                format!(
                    "{err_str} and cannot restore previous extension to '{target_str}' from '{backup_str}': {}",
                    io_err_msg(&ec)
                )
                .into_bytes(),
            );
        }
        return Some(err_bytes.to_vec());
    }

    let _ = fs_remove_all(&backup);
    None
}

/// End-to-end validation and removal for `ExtensionManager::Manager::DeleteExtension`.
pub fn ext_mgr_delete_extension(
    ext_name: &[u8],
    found: bool,
    use_count: i64,
    target_path: &[u8],
) -> Option<Vec<u8>> {
    let ext_str = String::from_utf8_lossy(ext_name);
    if !found {
        return Some(format!("Deletion failed: Extension '{ext_str}' not found").into_bytes());
    }
    if use_count > 1 {
        return Some(
            format!("Deletion failed: Extension '{ext_str}' is currently in use").into_bytes(),
        );
    }

    let target = bytes_to_path(target_path);
    if let Err(ec) = fs_remove_all(&target) {
        let target_str = String::from_utf8_lossy(target_path);
        return Some(
            format!("Cannot delete directory '{target_str}': {}", io_err_msg(&ec)).into_bytes(),
        );
    }
    None
}

/// Formats error messages for `ExtensionManager::Manager::InstallExtension`.
pub fn ext_mgr_install_error(file_path: &[u8], os_err: Option<&[u8]>, kind: i32) -> Vec<u8> {
    let file_str = String::from_utf8_lossy(file_path);
    match kind {
        0 => format!("Unsupported archive type for file: {file_str}").into_bytes(),
        1 => format!("Failed to extract archive: {file_str}").into_bytes(),
        _ => {
            let err_str = String::from_utf8_lossy(os_err.unwrap_or(b""));
            format!("Cannot delete archive '{file_str}': {err_str}").into_bytes()
        }
    }
}

/// End-to-end implementation of `NzbScriptController::PrepareEnvScript` and
/// `NzbScriptController::PrepareEnvParameters`.
pub fn nzb_script_prepare_env(
    script_name: &[u8],
    has_parameters: bool,
    param_names: &[&CStr],
    param_values: &[&CStr],
    mut set_env_special: impl FnMut(&CStr, &CStr, &CStr),
    mut prepare_env_options: impl FnMut(&CStr),
) {
    if has_parameters {
        for (&name, &val) in param_names.iter().zip(param_values.iter()) {
            set_env_special(c"NZBPR", name, val);
        }
    }

    let mut prefix_bytes = Vec::with_capacity(script_name.len() + 2);
    if let Some(nul) = script_name.iter().position(|&b| b == 0) {
        prefix_bytes.extend_from_slice(&script_name[..nul]);
    } else {
        prefix_bytes.extend_from_slice(script_name);
    }
    prefix_bytes.push(b':');
    prefix_bytes.push(0);
    let prefix_cstr = CStr::from_bytes_with_nul(&prefix_bytes).expect("NUL-terminated");
    let prefix_slice = prefix_cstr.to_bytes();
    let prefix_len = prefix_slice.len();

    if has_parameters && prefix_len > 0 {
        for (&name, &val) in param_names.iter().zip(param_values.iter()) {
            let name_bytes = name.to_bytes();
            if name_bytes.len() > prefix_len && name_bytes.starts_with(prefix_slice) {
                let stripped = CStr::from_bytes_with_nul(&name.to_bytes_with_nul()[prefix_len..])
                    .expect("NUL-terminated suffix");
                set_env_special(c"NZBPR", stripped, val);
            }
        }
    }

    prepare_env_options(prefix_cstr);
}

fn dupe_mode_cstr(dupe_mode: i32) -> &'static CStr {
    match dupe_mode {
        1 => c"ALL",
        2 => c"FORCE",
        _ => c"SCORE",
    }
}

/// End-to-end implementation of `ScanScriptController::PrepareParams`.
#[allow(clippy::too_many_arguments)]
pub fn scan_script_prepare_env(
    nzb_filename: &CStr,
    base_filename: &CStr,
    url: &CStr,
    nzb_name: &CStr,
    category: &CStr,
    priority: i32,
    add_top: bool,
    add_paused: bool,
    dupe_key: &CStr,
    dupe_score: i32,
    dupe_mode: i32,
    directory: &[u8],
    path_sep: u8,
    mut set_env_var: impl FnMut(&CStr, &CStr),
    mut set_int_env_var: impl FnMut(&CStr, i32),
) {
    set_env_var(c"NZBNP_FILENAME", nzb_filename);
    set_env_var(c"NZBNP_URL", url);
    let effective_name = if !nzb_name.to_bytes().is_empty() {
        nzb_name
    } else {
        base_filename
    };
    set_env_var(c"NZBNP_NZBNAME", effective_name);
    set_env_var(c"NZBNP_CATEGORY", category);
    set_int_env_var(c"NZBNP_PRIORITY", priority);
    set_int_env_var(c"NZBNP_TOP", i32::from(add_top));
    set_int_env_var(c"NZBNP_PAUSED", i32::from(add_paused));
    set_env_var(c"NZBNP_DUPEKEY", dupe_key);
    set_int_env_var(c"NZBNP_DUPESCORE", dupe_score);
    set_env_var(c"NZBNP_DUPEMODE", dupe_mode_cstr(dupe_mode));

    let trimmed_dir = if directory.last() == Some(&path_sep) {
        &directory[..directory.len() - 1]
    } else {
        directory
    };
    let dir_c = cstring_lossy(trimmed_dir);
    set_env_var(c"NZBNP_DIRECTORY", &dir_c);
}

pub struct PostScriptEnvInput<'a> {
    pub nzb_id: i32,
    pub nzb_name: &'a CStr,
    pub dest_dir: &'a CStr,
    pub filename: &'a CStr,
    pub queued_filename: &'a CStr,
    pub url: &'a CStr,
    pub final_dir: &'a CStr,
    pub category: &'a CStr,
    pub health: i32,
    pub critical_health: i32,
    pub dupe_key: &'a CStr,
    pub dupe_score: i32,
    pub dupe_mode: i32,
    pub status: &'a CStr,
    pub script_status: i32,
    pub par_status: i32,
    pub unpack_status: i32,
    pub delete_status: i32,
    pub mark_status: i32,
    pub total_articles: i32,
    pub success_articles: i32,
    pub failed_articles: i32,
    pub server_ids: &'a [i32],
    pub server_success_articles: &'a [i32],
    pub server_failed_articles: &'a [i32],
}

fn post_script_deprecated_status_code(status: i32) -> i32 {
    match status {
        0 | 1 => 0,
        2 => 1,
        3 => 2,
        4 => 3,
        5 => 4,
        _ => 0,
    }
}

/// End-to-end implementation of `PostScriptController::PrepareParams`.
pub fn post_script_prepare_env(
    inp: &PostScriptEnvInput<'_>,
    mut set_env_var: impl FnMut(&CStr, &CStr),
    mut set_int_env_var: impl FnMut(&CStr, i32),
) {
    set_int_env_var(c"NZBPP_NZBID", inp.nzb_id);
    set_env_var(c"NZBPP_NZBNAME", inp.nzb_name);
    set_env_var(c"NZBPP_DIRECTORY", inp.dest_dir);
    set_env_var(c"NZBPP_NZBFILENAME", inp.filename);
    set_env_var(c"NZBPP_QUEUEDFILE", inp.queued_filename);
    set_env_var(c"NZBPP_URL", inp.url);
    set_env_var(c"NZBPP_FINALDIR", inp.final_dir);
    set_env_var(c"NZBPP_CATEGORY", inp.category);
    set_int_env_var(c"NZBPP_HEALTH", inp.health);
    set_int_env_var(c"NZBPP_CRITICALHEALTH", inp.critical_health);
    set_env_var(c"NZBPP_DUPEKEY", inp.dupe_key);
    set_int_env_var(c"NZBPP_DUPESCORE", inp.dupe_score);
    set_env_var(c"NZBPP_DUPEMODE", dupe_mode_cstr(inp.dupe_mode));
    set_env_var(c"NZBPP_STATUS", inp.status);

    let status_bytes = inp.status.to_bytes();
    let total_len = status_bytes
        .iter()
        .position(|&b| b == b'/')
        .unwrap_or(status_bytes.len());
    let total_status = cstring_lossy(&status_bytes[..total_len]);
    set_env_var(c"NZBPP_TOTALSTATUS", &total_status);

    let script_status_name = match inp.script_status {
        1 => c"FAILURE",
        2 => c"SUCCESS",
        _ => c"NONE",
    };
    set_env_var(c"NZBPP_SCRIPTSTATUS", script_status_name);

    // psFailure = 2; dsNone = 0; ksBad = 1
    let compat_par_status = if inp.delete_status != 0 || inp.mark_status == 1 {
        2
    } else {
        inp.par_status
    };
    set_int_env_var(
        c"NZBPP_PARSTATUS",
        post_script_deprecated_status_code(compat_par_status),
    );
    set_int_env_var(
        c"NZBPP_UNPACKSTATUS",
        post_script_deprecated_status_code(inp.unpack_status),
    );
    // dsHealth = 2
    set_int_env_var(c"NZBPP_HEALTHDELETED", i32::from(inp.delete_status == 2));

    set_int_env_var(c"NZBPP_TOTALARTICLES", inp.total_articles);
    set_int_env_var(c"NZBPP_SUCCESSARTICLES", inp.success_articles);
    set_int_env_var(c"NZBPP_FAILEDARTICLES", inp.failed_articles);

    let count = inp
        .server_ids
        .len()
        .min(inp.server_success_articles.len())
        .min(inp.server_failed_articles.len());
    for i in 0..count {
        let srv_id = inp.server_ids[i];
        let succ_name =
            CString::new(format!("NZBPP_SERVER{srv_id}_SUCCESSARTICLES")).expect("no NUL");
        set_int_env_var(&succ_name, inp.server_success_articles[i]);
        let fail_name =
            CString::new(format!("NZBPP_SERVER{srv_id}_FAILEDARTICLES")).expect("no NUL");
        set_int_env_var(&fail_name, inp.server_failed_articles[i]);
    }
}

pub struct QueueScriptEnvInput<'a> {
    pub nzb_name: &'a CStr,
    pub nzb_id: i32,
    pub filename: &'a CStr,
    pub dest_dir: &'a CStr,
    pub queued_filename: &'a CStr,
    pub url: &'a CStr,
    pub category: &'a CStr,
    pub priority: i32,
    pub dupe_key: &'a CStr,
    pub dupe_score: i32,
    pub dupe_mode: i32,
    pub event: i32,
    pub delete_status: i32,
    pub url_status: i32,
    pub mark_status: i32,
}

/// End-to-end implementation of `QueueScriptController::PrepareParams`.
pub fn queue_script_prepare_env(
    inp: &QueueScriptEnvInput<'_>,
    mut set_env_var: impl FnMut(&CStr, &CStr),
    mut set_int_env_var: impl FnMut(&CStr, i32),
) {
    set_env_var(c"NZBNA_NZBNAME", inp.nzb_name);
    set_int_env_var(c"NZBNA_NZBID", inp.nzb_id);
    set_env_var(c"NZBNA_FILENAME", inp.filename);
    set_env_var(c"NZBNA_DIRECTORY", inp.dest_dir);
    set_env_var(c"NZBNA_QUEUEDFILE", inp.queued_filename);
    set_env_var(c"NZBNA_URL", inp.url);
    set_env_var(c"NZBNA_CATEGORY", inp.category);
    set_int_env_var(c"NZBNA_PRIORITY", inp.priority);
    set_int_env_var(c"NZBNA_LASTID", inp.nzb_id);
    set_env_var(c"NZBNA_DUPEKEY", inp.dupe_key);
    set_int_env_var(c"NZBNA_DUPESCORE", inp.dupe_score);
    set_env_var(c"NZBNA_DUPEMODE", dupe_mode_cstr(inp.dupe_mode));

    let event_name = match inp.event {
        0 => c"FILE_DOWNLOADED",
        1 => c"URL_COMPLETED",
        2 => c"NZB_MARKED",
        3 => c"NZB_ADDED",
        4 => c"NZB_NAMED",
        5 => c"NZB_DOWNLOADED",
        6 => c"NZB_DELETED",
        _ => c"NONE",
    };
    set_env_var(c"NZBNA_EVENT", event_name);

    let delete_status_name = match inp.delete_status {
        0 => c"NONE",
        1 => c"MANUAL",
        2 => c"HEALTH",
        3 => c"DUPE",
        4 => c"BAD",
        5 => c"GOOD",
        6 => c"COPY",
        7 => c"SCAN",
        _ => c"NONE",
    };
    set_env_var(c"NZBNA_DELETESTATUS", delete_status_name);

    let url_status_name = match inp.url_status {
        0 => c"NONE",
        1 => c"UNKNOWN",
        2 => c"SUCCESS",
        3 => c"FAILURE",
        4 => c"UNKNOWN",
        5 => c"SCAN_SKIPPED",
        6 => c"SCAN_FAILURE",
        _ => c"NONE",
    };
    set_env_var(c"NZBNA_URLSTATUS", url_status_name);

    let mark_status_name = match inp.mark_status {
        0 => c"NONE",
        1 => c"BAD",
        2 => c"GOOD",
        3 => c"SUCCESS",
        _ => c"NONE",
    };
    set_env_var(c"NZBNA_MARKSTATUS", mark_status_name);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_config_merge_and_parse() {
        let existing = b"# comment line\n\nServer1.Active=yes\nserver1.active=yes\nOrphan=1\n";
        let entries: &[(&[u8], &[u8])] = &[
            (b"Server1.Active", b"first"),
            (b"server1.active", b"no"),
            (b"NewOpt", b"val"),
        ];
        let merged = script_config_merge_bytes(existing, entries);
        assert_eq!(
            std::str::from_utf8(&merged).unwrap(),
            "# comment line\n\nServer1.Active=no\nNewOpt=val\n"
        );

        let parsed = script_config_parse_bytes(&merged);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0.as_bytes(), b"Server1.Active");
        assert_eq!(parsed[0].1.as_bytes(), b"no");
        assert_eq!(parsed[1].0.as_bytes(), b"NewOpt");
        assert_eq!(parsed[1].1.as_bytes(), b"val");
    }

    #[test]
    fn test_nzb_script_prepare_env() {
        let mut vars = Vec::new();
        let mut opt_prefix = String::new();
        nzb_script_prepare_env(
            b"Logger.py",
            true,
            &[
                c"*unpack:",
                c"Logger.py:",
                c"logger.py:Ignored",
                c"Logger.py:Verbose",
                c"Other",
            ],
            &[c"yes", c"empty", c"no", c"true", c"1"],
            |prefix, name, val| {
                vars.push((
                    prefix.to_string_lossy().into_owned(),
                    name.to_string_lossy().into_owned(),
                    val.to_string_lossy().into_owned(),
                ));
            },
            |prefix| {
                opt_prefix = prefix.to_string_lossy().into_owned();
            },
        );
        assert_eq!(opt_prefix, "Logger.py:");
        assert_eq!(vars.len(), 6);
        assert_eq!(
            vars[5],
            ("NZBPR".to_string(), "Verbose".to_string(), "true".to_string())
        );
    }

    #[test]
    fn test_post_and_queue_script_prepare_env() {
        let mut env = Vec::new();
        let mut int_env = Vec::new();
        let inp = PostScriptEnvInput {
            nzb_id: 7,
            nzb_name: c"My.Release",
            dest_dir: c"/dst",
            filename: c"/nzb/My.Release.nzb",
            queued_filename: c"/queue/7",
            url: c"",
            final_dir: c"/final",
            category: c"movies",
            health: 1000,
            critical_health: 900,
            dupe_key: c"dk",
            dupe_score: 10,
            dupe_mode: 2,
            status: c"SUCCESS/ALL",
            script_status: 2,
            par_status: 3,
            unpack_status: 3,
            delete_status: 2,
            mark_status: 0,
            total_articles: 100,
            success_articles: 98,
            failed_articles: 2,
            server_ids: &[1],
            server_success_articles: &[98],
            server_failed_articles: &[2],
        };
        post_script_prepare_env(
            &inp,
            |k, v| {
                env.push((
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                ))
            },
            |k, v| int_env.push((k.to_string_lossy().into_owned(), v)),
        );
        assert!(env.contains(&("NZBPP_TOTALSTATUS".to_string(), "SUCCESS".to_string())));
        assert!(env.contains(&("NZBPP_DUPEMODE".to_string(), "FORCE".to_string())));
        assert!(env.contains(&("NZBPP_SCRIPTSTATUS".to_string(), "SUCCESS".to_string())));
        assert!(int_env.contains(&("NZBPP_PARSTATUS".to_string(), 1)));
        assert!(int_env.contains(&("NZBPP_UNPACKSTATUS".to_string(), 2)));
        assert!(int_env.contains(&("NZBPP_HEALTHDELETED".to_string(), 1)));
        assert!(int_env.contains(&("NZBPP_SERVER1_SUCCESSARTICLES".to_string(), 98)));
        assert!(int_env.contains(&("NZBPP_SERVER1_FAILEDARTICLES".to_string(), 2)));

        let mut q_env = Vec::new();
        let q_inp = QueueScriptEnvInput {
            nzb_name: c"My.Release",
            nzb_id: 7,
            filename: c"/nzb/My.Release.nzb",
            dest_dir: c"/dst",
            queued_filename: c"/queue/7",
            url: c"https://example.com/a.nzb",
            category: c"movies",
            priority: 0,
            dupe_key: c"dk",
            dupe_score: 0,
            dupe_mode: 0,
            event: 1,
            delete_status: 0,
            url_status: 5,
            mark_status: 0,
        };
        queue_script_prepare_env(
            &q_inp,
            |k, v| {
                q_env.push((
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                ))
            },
            |_, _| {},
        );
        assert!(q_env.contains(&("NZBNA_URLSTATUS".to_string(), "SCAN_SKIPPED".to_string())));
        assert!(ext_is_in_list(b"ScriptA, ScriptB", b"ScriptB"));
        assert!(!ext_is_in_list(b"ScriptA, ScriptB", b"scriptb"));
    }
}
